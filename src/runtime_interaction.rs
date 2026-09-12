use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use url::Url;

use crate::engine::DocumentView;
use crate::renderer_host::{RendererHost, RendererMode};
use crate::renderer_protocol::{DomEventRequest, RuntimeDamage};
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
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
}

pub struct RuntimeInteractionResult {
    pub request_id: u64,
    pub tab_id: u64,
    pub generation: u64,
    pub mode: Option<RendererMode>,
    pub default_prevented: bool,
    pub result: Result<RuntimePageUpdate, String>,
}

pub struct RuntimeInteractionLoader {
    sender: Sender<RuntimeInteractionResult>,
    receiver: Receiver<RuntimeInteractionResult>,
}

impl RuntimeInteractionLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    pub fn start(&self, request: RuntimeInteractionRequest) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let host = RendererHost::default();
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
                            script_report: update.script_report,
                            damage: update.damage,
                        }),
                    )
                }
                Err(error) => (None, false, Err(error)),
            };
            let _ = sender.send(RuntimeInteractionResult {
                request_id: request.request_id,
                tab_id: request.tab_id,
                generation: request.generation,
                mode,
                default_prevented,
                result,
            });
        });
    }

    pub fn try_recv(&self) -> Option<RuntimeInteractionResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}
