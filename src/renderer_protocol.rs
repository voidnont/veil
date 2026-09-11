use serde::{Deserialize, Serialize};

use crate::engine::DocumentView;
use crate::privacy::SitePrivacy;
use crate::storage::ScriptStorageSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderRequest {
    pub session_id: String,
    pub url: String,
    pub html: String,
    pub privacy: SitePrivacy,
    pub custom_filters: String,
    pub external_css: Vec<String>,
    pub external_scripts: Vec<String>,
    pub external_script_count: usize,
    pub storage: ScriptStorageSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomEventRequest {
    pub node_id: usize,
    pub event_type: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeUpdate {
    pub view: DocumentView,
    pub default_prevented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload")]
pub enum RendererCommand {
    Render(RenderRequest),
    Event { session_id: String, event: DomEventRequest },
    Tick { session_id: String, elapsed_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reply", content = "payload")]
pub enum RendererReply {
    Render(Result<DocumentView, String>),
    Runtime(Result<RuntimeUpdate, String>),
}
