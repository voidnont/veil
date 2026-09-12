use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use url::Url;

use crate::engine::DocumentView;
use crate::renderer_host::{RendererHost, RendererMode};
use crate::renderer_protocol::{DisplayListPatch, DomEventRequest, RuntimeDamage};
use crate::script::ScriptReport;
use crate::storage::SharedBrowserStorage;

#[derive(Debug, Clone)]
pub enum RuntimeInteractionKind {
    Event(DomEventRequest),
    Tick(u64),
}

pub struct RuntimeInteractionRequest {
    pub request_id: u64,
    pub tab_id: u64,
    pub generation: u64,
    pub page_url: String,
    pub session_id: String,
    pub storage: SharedBrowserStorage,
    pub kind: RuntimeInteractionKind,
}

pub struct RuntimePageUpdate {
    pub view: Option<DocumentView>,
    pub patch: Option<DisplayListPatch>,
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
    pub title: String,
    pub icon_url: Option<String>,
    pub cosmetic_hidden: usize,
    pub external_stylesheets: usize,
    pub external_scripts: usize,
    pub reused_blocks: usize,
}

pub struct RuntimeInteractionResult {
    pub request_id: u64,
    pub tab_id: u64,
    pub generation: u64,
    pub mode: Option<RendererMode>,
    pub default_prevented: bool,
    pub result: Result<RuntimePageUpdate, String>,
}

fn process_request(
    host: &RendererHost,
    request: RuntimeInteractionRequest,
) -> RuntimeInteractionResult {
    let update = match request.kind {
        RuntimeInteractionKind::Event(event) => {
            host.dispatch_event(&request.page_url, &request.session_id, event)
        }
        RuntimeInteractionKind::Tick(elapsed) => {
            host.tick(&request.page_url, &request.session_id, elapsed)
        }
    };
    let (mode, default_prevented, result) = match update {
        Ok(update) => {
            if let Ok(url) = Url::parse(&request.page_url) {
                request
                    .storage
                    .apply_script_snapshot(&url, &update.script_report.storage);
                for cookie in &update.script_report.cookie_writes {
                    request.storage.store_set_cookie(&url, &url, cookie);
                }
            }
            (
                Some(update.mode),
                update.default_prevented,
                Ok(RuntimePageUpdate {
                    view: update.view,
                    patch: update.patch,
                    script_report: update.script_report,
                    damage: update.damage,
                    title: update.title,
                    icon_url: update.icon_url,
                    cosmetic_hidden: update.cosmetic_hidden,
                    external_stylesheets: update.external_stylesheets,
                    external_scripts: update.external_scripts,
                    reused_blocks: update.reused_blocks,
                }),
            )
        }
        Err(error) => (None, false, Err(error)),
    };
    RuntimeInteractionResult {
        request_id: request.request_id,
        tab_id: request.tab_id,
        generation: request.generation,
        mode,
        default_prevented,
        result,
    }
}

pub struct RuntimeInteractionLoader {
    request_sender: Sender<RuntimeInteractionRequest>,
    receiver: Receiver<RuntimeInteractionResult>,
}

impl RuntimeInteractionLoader {
    pub fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<RuntimeInteractionRequest>();
        let (result_sender, receiver) = mpsc::channel::<RuntimeInteractionResult>();
        thread::spawn(move || {
            let host = RendererHost::default();
            while let Ok(request) = request_receiver.recv() {
                let result = process_request(&host, request);
                if result_sender.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            request_sender,
            receiver,
        }
    }

    pub fn start(&self, request: RuntimeInteractionRequest) {
        let _ = self.request_sender.send(request);
    }

    pub fn try_recv(&self) -> Option<RuntimeInteractionResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}
