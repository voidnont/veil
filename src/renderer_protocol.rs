use serde::{Deserialize, Serialize};

use crate::engine::DocumentView;
use crate::privacy::SitePrivacy;
use crate::script::ScriptReport;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeDamage {
    None,
    Metadata,
    Paint,
    Layout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeUpdate {
    /// Present only when retained paint/layout output actually changed.
    pub view: Option<DocumentView>,
    /// Always returned so timer/rAF/storage state can advance without repainting.
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
    pub default_prevented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload")]
pub enum RendererCommand {
    Render(RenderRequest),
    Event {
        session_id: String,
        event: DomEventRequest,
    },
    Tick {
        session_id: String,
        elapsed_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reply", content = "payload")]
pub enum RendererReply {
    Render(Result<DocumentView, String>),
    Runtime(Result<RuntimeUpdate, String>),
}
