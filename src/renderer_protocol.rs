use serde::{Deserialize, Serialize};

use crate::engine::{DocumentView, RenderBlock};
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
    LayoutSubtree,
    Layout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayListPatch {
    pub start: usize,
    pub remove_count: usize,
    pub blocks: Vec<RenderBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeUpdate {
    /// Used for full-layout changes or when a retained patch would be larger than the full view.
    pub view: Option<DocumentView>,
    /// Used for localized retained display-list changes.
    pub patch: Option<DisplayListPatch>,
    /// Always returned so timer/rAF/storage state can advance without repainting.
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
    pub title: String,
    pub icon_url: Option<String>,
    pub cosmetic_hidden: usize,
    pub external_stylesheets: usize,
    pub external_scripts: usize,
    pub reused_blocks: usize,
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
