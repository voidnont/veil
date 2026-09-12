use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Instant;

use url::Url;

use crate::engine::{DocumentView, Engine, WebFontResource};
use crate::net::{MultipartPart, PrivacyNetwork};
use crate::privacy::SitePrivacy;
use crate::renderer_host::{RendererHost, RendererMode};
use crate::renderer_protocol::RenderRequest;
use crate::storage::SharedBrowserStorage;

const MAX_STYLESHEETS_PER_PAGE: usize = 24;
const MAX_SCRIPTS_PER_PAGE: usize = 12;
const MAX_WEB_FONTS_PER_PAGE: usize = 8;
const MAX_TOTAL_STYLESHEET_BYTES: usize = 6 * 1024 * 1024;
const MAX_TOTAL_SCRIPT_BYTES: usize = 8 * 1024 * 1024;
const HEAVY_DOCUMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub enum NavigationMethod {
    Get,
    PostForm(String),
    PostMultipart(Vec<MultipartPart>),
}

pub struct LoadRequest {
    pub tab_id: u64,
    pub generation: u64,
    pub url: String,
    pub privacy: SitePrivacy,
    pub custom_filters: String,
    pub storage: SharedBrowserStorage,
    pub method: NavigationMethod,
}

pub struct LoadResult {
    pub tab_id: u64,
    pub generation: u64,
    pub elapsed_ms: u128,
    pub blocked_count: usize,
    pub blocked_events: Vec<String>,
    pub renderer_mode: RendererMode,
    pub result: Result<DocumentView, String>,
}

pub struct PageLoader {
    sender: Sender<LoadResult>,
    receiver: Receiver<LoadResult>,
}

impl PageLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    pub fn start(&self, request: LoadRequest) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let mut network = PrivacyNetwork::new_with_storage(request.storage.clone());
            if !request.custom_filters.trim().is_empty() {
                network
                    .blocker_mut()
                    .replace_custom_filters(request.custom_filters.clone());
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_document(&mut network, &request)
            }))
            .unwrap_or_else(|_| Err("Veil recovered from an internal page-load panic.".into()));
            let blocked_count = network.blocked_count();
            let blocked_events = network.take_blocked_events();
            let (renderer_mode, result) = match result {
                Ok((view, mode)) => (mode, Ok(view)),
                Err(error) => (RendererMode::InProcessFallback, Err(error)),
            };

            let _ = sender.send(LoadResult {
                tab_id: request.tab_id,
                generation: request.generation,
                elapsed_ms: started.elapsed().as_millis(),
                blocked_count,
                blocked_events,
                renderer_mode,
                result,
            });
        });
    }

    pub fn try_recv(&self) -> Option<LoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn load_document(
    network: &mut PrivacyNetwork,
    request: &LoadRequest,
) -> Result<(DocumentView, RendererMode), String> {
    let url = Url::parse(&request.url).map_err(|e| format!("Invalid URL: {e}"))?;
    let response = match &request.method {
        NavigationMethod::Get => network.get_document(&url, request.privacy)?,
        NavigationMethod::PostForm(body) => {
            network.post_form_document(&url, request.privacy, body)?
        }
        NavigationMethod::PostMultipart(parts) => {
            network.post_multipart_document(&url, request.privacy, parts.clone())?
        }
    };
    let final_url = response.final_url.clone();

    // Resource discovery remains deliberately bounded. Full HTML/CSS/JS parsing
    // and render-tree creation happens inside veil-engine when available.
    let discovery = Engine::default();
    let stylesheet_urls = discovery.discover_stylesheets(&response.body, &final_url);
    let mut external_css = Vec::new();
    let mut external_css_bytes = 0usize;
    let mut web_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();
    for stylesheet in stylesheet_urls.into_iter().take(MAX_STYLESHEETS_PER_PAGE) {
        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, request.privacy) {
            if external_css_bytes.saturating_add(resource.body.len()) > MAX_TOTAL_STYLESHEET_BYTES {
                break;
            }
            external_css_bytes = external_css_bytes.saturating_add(resource.body.len());
            if web_fonts.len() < MAX_WEB_FONTS_PER_PAGE {
                for source in discovery.discover_web_fonts(&resource.body, &resource.final_url) {
                    if web_fonts.len() >= MAX_WEB_FONTS_PER_PAGE {
                        break;
                    }
                    if !seen_fonts.insert(source.url.to_string()) {
                        continue;
                    }
                    if let Ok(font) = network.get_font(&final_url, &source.url, request.privacy) {
                        web_fonts.push(WebFontResource {
                            family: source.family,
                            url: font.final_url.to_string(),
                            bytes: font.bytes,
                        });
                    }
                }
            }
            external_css.push(resource.body);
        }
    }

    let script_urls = discovery.discover_external_scripts(&response.body, &final_url);
    let heavy_document = response.body.len() >= HEAVY_DOCUMENT_BYTES || script_urls.len() > 16;
    let script_limit = if heavy_document {
        4
    } else {
        MAX_SCRIPTS_PER_PAGE
    };
    let script_byte_limit = if heavy_document {
        2 * 1024 * 1024
    } else {
        MAX_TOTAL_SCRIPT_BYTES
    };
    let mut external_scripts = Vec::new();
    let mut external_script_bytes = 0usize;
    if request.privacy.javascript {
        for script in script_urls.into_iter().take(script_limit) {
            if let Ok(resource) = network.get_script(&final_url, &script, request.privacy) {
                if external_script_bytes.saturating_add(resource.body.len()) > script_byte_limit {
                    break;
                }
                external_script_bytes = external_script_bytes.saturating_add(resource.body.len());
                external_scripts.push(resource.body);
            }
        }
    }
    let external_script_count = external_scripts.len();

    let storage_snapshot = request.storage.script_snapshot(&final_url);
    let render_request = RenderRequest {
        session_id: format!("tab-{}-{}", request.tab_id, request.generation),
        url: final_url.to_string(),
        html: response.body,
        privacy: request.privacy,
        custom_filters: request.custom_filters.clone(),
        external_css,
        external_scripts,
        external_script_count,
        storage: storage_snapshot,
    };

    let (mut view, mode) = RendererHost::default().render(render_request)?;
    request
        .storage
        .apply_script_snapshot(&final_url, &view.script_report.storage);
    for cookie in &view.script_report.cookie_writes {
        request
            .storage
            .store_set_cookie(&final_url, &final_url, cookie);
    }
    view.web_fonts = web_fonts;
    Ok((view, mode))
}
