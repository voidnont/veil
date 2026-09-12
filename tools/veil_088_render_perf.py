from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


# Version.
replace_once("Cargo.toml", 'version = "0.8.7"', 'version = "0.8.8"')

# Shared retained display-list / virtualization helpers.
Path("src/display_list.rs").write_text(r'''use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use crate::engine::{RenderBlock, TextRun};
use crate::style::{ComputedStyle, LayoutMode};

/// Only virtualize long vertical sequences. Small groups are cheaper and safer to paint normally.
pub const VIRTUALIZE_MIN_ITEMS: usize = 24;
/// Keep a generous band around the visible clip so fast wheel/touchpad scrolling never reveals blanks.
pub const VIRTUALIZE_OVERSCAN: f32 = 900.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisplayDiff {
    pub changed: usize,
    pub reused: usize,
    pub start: usize,
    pub old_remove_count: usize,
    pub new_end: usize,
}

impl DisplayDiff {
    pub fn is_empty(self) -> bool {
        self.changed == 0
    }
}

#[derive(Debug, Clone)]
struct RetainedPaintItem {
    fingerprint: u64,
    height: f32,
    measured: bool,
}

/// Browser-side retained paint metadata. The engine owns DOM/style retention; this list owns
/// stable paint fingerprints and measured heights so egui can skip off-screen work without
/// losing scroll geometry.
#[derive(Debug, Clone, Default)]
pub struct RetainedDisplayList {
    items: Vec<RetainedPaintItem>,
    revision: u64,
}

impl RetainedDisplayList {
    pub fn reconcile(&mut self, blocks: &[RenderBlock], width: f32) -> DisplayDiff {
        let old_fingerprints: Vec<u64> = self.items.iter().map(|item| item.fingerprint).collect();
        let new_fingerprints = block_fingerprints(blocks);
        let diff = diff_fingerprints(&old_fingerprints, &new_fingerprints);

        // Reuse measured geometry even when an insertion shifts an unchanged block's index.
        let mut old_heights: HashMap<u64, VecDeque<(f32, bool)>> = HashMap::new();
        for item in self.items.drain(..) {
            old_heights
                .entry(item.fingerprint)
                .or_default()
                .push_back((item.height, item.measured));
        }

        self.items = blocks
            .iter()
            .zip(new_fingerprints)
            .map(|(block, fingerprint)| {
                let reused = old_heights
                    .get_mut(&fingerprint)
                    .and_then(VecDeque::pop_front);
                let (height, measured) = reused.unwrap_or_else(|| {
                    (estimate_block_height(block, width).max(1.0), false)
                });
                RetainedPaintItem {
                    fingerprint,
                    height,
                    measured,
                }
            })
            .collect();

        if !diff.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        diff
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn height_for(&self, index: usize, block: &RenderBlock, width: f32) -> f32 {
        self.items
            .get(index)
            .map(|item| item.height)
            .unwrap_or_else(|| estimate_block_height(block, width))
            .max(1.0)
    }

    pub fn observe_height(&mut self, index: usize, height: f32) {
        let Some(item) = self.items.get_mut(index) else {
            return;
        };
        let height = height.clamp(1.0, 12_000.0);
        item.height = if item.measured {
            // Smooth tiny font/layout jitter so the virtual scroll geometry stays stable.
            item.height * 0.75 + height * 0.25
        } else {
            height
        };
        item.measured = true;
    }
}

pub fn block_fingerprint(block: &RenderBlock) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(block) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn block_fingerprints(blocks: &[RenderBlock]) -> Vec<u64> {
    blocks.iter().map(block_fingerprint).collect()
}

/// Find a single contiguous replacement range by preserving the common prefix and suffix.
/// This is the same broad retained-list idea Gecko uses: unchanged display items stay retained,
/// and only the damaged middle span needs to cross the renderer IPC boundary.
pub fn diff_fingerprints(old: &[u64], new: &[u64]) -> DisplayDiff {
    let mut prefix = 0usize;
    let common = old.len().min(new.len());
    while prefix < common && old[prefix] == new[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix < old.len().saturating_sub(prefix)
        && suffix < new.len().saturating_sub(prefix)
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_remove_count = old.len().saturating_sub(prefix + suffix);
    let new_end = new.len().saturating_sub(suffix);
    let inserted = new_end.saturating_sub(prefix);
    let changed = old_remove_count.max(inserted);
    DisplayDiff {
        changed,
        reused: prefix + suffix,
        start: prefix,
        old_remove_count,
        new_end,
    }
}

pub fn estimate_block_height(block: &RenderBlock, available_width: f32) -> f32 {
    let width = available_width.max(220.0);
    let raw = match block {
        RenderBlock::Heading { runs, style, .. } | RenderBlock::Paragraph { runs, style } => {
            estimate_text_height(runs, style, width)
        }
        RenderBlock::Container { children, style } => {
            let child_width = style
                .width
                .or(style.max_width)
                .unwrap_or(width)
                .min(width)
                .max(160.0);
            let body = match style.layout {
                LayoutMode::Block | LayoutMode::FlexColumn => {
                    let sum: f32 = children
                        .iter()
                        .map(|child| estimate_block_height(child, child_width))
                        .sum();
                    sum + style.gap.max(0.0) * children.len().saturating_sub(1) as f32
                }
                LayoutMode::FlexRow => {
                    if children.is_empty() {
                        1.0
                    } else if style.flex_wrap {
                        let columns = ((child_width / 220.0).floor() as usize).max(1);
                        let rows = children.len().div_ceil(columns);
                        let row_height = children
                            .iter()
                            .map(|child| estimate_block_height(child, child_width / columns as f32))
                            .fold(1.0_f32, f32::max);
                        rows as f32 * row_height + rows.saturating_sub(1) as f32 * style.gap.max(0.0)
                    } else {
                        children
                            .iter()
                            .map(|child| estimate_block_height(child, child_width / children.len() as f32))
                            .fold(1.0_f32, f32::max)
                    }
                }
                LayoutMode::Grid => {
                    let columns = style.grid_columns.max(1).min(((child_width / 180.0).floor() as usize).max(1));
                    let rows = children.len().div_ceil(columns);
                    let row_height = children
                        .iter()
                        .map(|child| estimate_block_height(child, child_width / columns as f32))
                        .fold(1.0_f32, f32::max);
                    rows as f32 * row_height + rows.saturating_sub(1) as f32 * style.gap.max(0.0)
                }
            };
            body + box_vertical(style)
        }
        RenderBlock::Image {
            width,
            height,
            style,
            ..
        } => {
            height
                .or(style.height)
                .unwrap_or_else(|| width.or(style.width).unwrap_or(360.0).min(width_value(width, style, available_width)) * 0.62)
                .clamp(24.0, 1800.0)
                + box_vertical(style)
        }
        RenderBlock::Canvas { height, style, .. } => height.clamp(1.0, 1400.0) + box_vertical(style),
        RenderBlock::Media {
            height,
            width,
            style,
            ..
        } => height
            .or(style.height)
            .unwrap_or_else(|| width.or(style.width).unwrap_or(420.0) * 0.56 + 86.0)
            .clamp(60.0, 1800.0)
            + box_vertical(style),
        RenderBlock::Form { controls, style, .. } => {
            controls.len().max(1) as f32 * 46.0 + 18.0 + box_vertical(style)
        }
        RenderBlock::Rule { style } => 18.0 + box_vertical(style),
        RenderBlock::Code { text, style } => {
            let lines = text.lines().count().max(1) as f32;
            lines.min(80.0) * (style.font_size.max(12.0) * 1.35) + box_vertical(style)
        }
        RenderBlock::Notice(text) => 42.0 + (text.len() as f32 / 90.0).floor() * 20.0,
    };
    raw.clamp(1.0, 12_000.0)
}

fn width_value(width: &Option<f32>, style: &ComputedStyle, available_width: f32) -> f32 {
    width
        .or(style.width)
        .unwrap_or(available_width)
        .min(available_width)
        .max(1.0)
}

fn estimate_text_height(runs: &[TextRun], style: &ComputedStyle, available_width: f32) -> f32 {
    let chars: usize = runs.iter().map(|run| run.text.chars().count()).sum();
    let font = runs
        .iter()
        .map(|run| run.size)
        .fold(style.font_size.max(12.0), f32::max)
        .max(10.0);
    let chars_per_line = (available_width / (font * 0.54)).max(8.0);
    let lines = ((chars.max(1) as f32 / chars_per_line).ceil()).clamp(1.0, 120.0);
    lines * font * 1.42 + box_vertical(style)
}

fn box_vertical(style: &ComputedStyle) -> f32 {
    style.margin.top.max(0.0)
        + style.margin.bottom.max(0.0)
        + style.padding.top.max(0.0)
        + style.padding.bottom.max(0.0)
        + style.border_width.max(0.0) * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ComputedStyle;

    fn paragraph(text: &str) -> RenderBlock {
        let style = ComputedStyle::default();
        RenderBlock::Paragraph {
            runs: vec![TextRun {
                text: text.to_owned(),
                href: None,
                size: 16.0,
                bold: false,
                italic: false,
                muted: false,
                color: None,
                font_family: Default::default(),
                font_name: None,
            }],
            style,
        }
    }

    #[test]
    fn diff_keeps_common_prefix_and_suffix() {
        let old = vec![1, 2, 3, 4];
        let new = vec![1, 9, 3, 4];
        let diff = diff_fingerprints(&old, &new);
        assert_eq!(diff.start, 1);
        assert_eq!(diff.old_remove_count, 1);
        assert_eq!(diff.new_end, 2);
        assert_eq!(diff.reused, 3);
    }

    #[test]
    fn retained_list_reuses_measured_height() {
        let mut list = RetainedDisplayList::default();
        let blocks = vec![paragraph("hello"), paragraph("world")];
        list.reconcile(&blocks, 800.0);
        list.observe_height(1, 123.0);
        let diff = list.reconcile(&blocks, 800.0);
        assert!(diff.is_empty());
        assert!((list.height_for(1, &blocks[1], 800.0) - 123.0).abs() < 0.01);
    }

    #[test]
    fn text_estimate_grows_with_content() {
        let short = paragraph("small");
        let long = paragraph(&"long content ".repeat(80));
        assert!(estimate_block_height(&long, 500.0) > estimate_block_height(&short, 500.0));
    }
}
''')

replace_once(
    "src/lib.rs",
    "pub mod dom;\n",
    "pub mod dom;\npub mod display_list;\n",
)

# Engine: finer mutation damage so metadata-only mutations don't force layout, and subtree
# mutations are distinguishable from full-document rebuilds.
replace_once(
    "src/engine.rs",
    '''pub enum InvalidationKind {
    None,
    Metadata,
    Paint,
    Layout,
}''',
    '''pub enum InvalidationKind {
    None,
    Metadata,
    Paint,
    LayoutSubtree,
    Layout,
}''',
)
replace_once(
    "src/engine.rs",
    '''    pub fn needs_layout(self) -> bool {
        self == Self::Layout
    }''',
    '''    pub fn needs_layout(self) -> bool {
        self >= Self::LayoutSubtree
    }''',
)
replace_once(
    "src/engine.rs",
    '''        } else if report.dom_mutations.len() > self.applied_mutations {
            for mutation in &report.dom_mutations[self.applied_mutations..] {
                apply_single_script_mutation(&mut self.live_dom, mutation);
            }
            self.applied_mutations = report.dom_mutations.len();
            invalidation = InvalidationKind::Layout;
        }''',
    '''        } else if report.dom_mutations.len() > self.applied_mutations {
            for mutation in &report.dom_mutations[self.applied_mutations..] {
                invalidation = invalidation.max(mutation_invalidation(mutation));
                apply_single_script_mutation(&mut self.live_dom, mutation);
            }
            self.applied_mutations = report.dom_mutations.len();
        }''',
)
# Version string in compatibility notice.
text = Path("src/engine.rs").read_text().replace("Veil Browser 0.8.7 can currently paint", "Veil Browser 0.8.8 can currently paint")
Path("src/engine.rs").write_text(text)

# Add mutation classifier immediately before apply_script_mutations.
replace_once(
    "src/engine.rs",
    "fn apply_script_mutations(dom: &mut Dom, report: &ScriptReport) {",
    r'''fn mutation_invalidation(mutation: &DomMutation) -> InvalidationKind {
    match mutation.kind.as_str() {
        // These attributes do not participate in Veil's current layout/paint model.
        "attr-set" => {
            let name = mutation.value.split_once('\0').map(|(name, _)| name).unwrap_or("");
            if name.starts_with("data-") || name.starts_with("aria-") || name == "title" {
                InvalidationKind::None
            } else {
                InvalidationKind::LayoutSubtree
            }
        }
        "attr-remove" => {
            let name = mutation.value.as_str();
            if name.starts_with("data-") || name.starts_with("aria-") || name == "title" {
                InvalidationKind::None
            } else {
                InvalidationKind::LayoutSubtree
            }
        }
        "text" | "style-set" | "html" | "append-html" | "prepend-html" | "remove" => {
            InvalidationKind::LayoutSubtree
        }
        _ => InvalidationKind::LayoutSubtree,
    }
}

fn apply_script_mutations(dom: &mut Dom, report: &ScriptReport) {''',
)

# Renderer protocol: small retained display-list patches can cross IPC instead of a full page.
replace_once(
    "src/renderer_protocol.rs",
    "use crate::engine::DocumentView;",
    "use crate::engine::{DocumentView, RenderBlock};",
)
replace_once(
    "src/renderer_protocol.rs",
    '''pub enum RuntimeDamage {
    None,
    Metadata,
    Paint,
    Layout,
}''',
    '''pub enum RuntimeDamage {
    None,
    Metadata,
    Paint,
    LayoutSubtree,
    Layout,
}''',
)
replace_once(
    "src/renderer_protocol.rs",
    '''#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeUpdate {
    /// Present only when retained paint/layout output actually changed.
    pub view: Option<DocumentView>,
    /// Always returned so timer/rAF/storage state can advance without repainting.
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
    pub default_prevented: bool,
}''',
    '''#[derive(Debug, Clone, Serialize, Deserialize)]
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
}''',
)

# Engine process: compare per-display-item fingerprints, preserve common prefix/suffix, and send
# only a compact middle replacement when the damage is localized.
replace_once(
    "src/bin/veil_engine.rs",
    '''use veil_engine::engine::{paint_fingerprint, DocumentView, InvalidationKind, RetainedDocument};
use veil_engine::renderer_protocol::{
    RenderRequest, RendererCommand, RendererReply, RuntimeDamage, RuntimeUpdate,
};''',
    '''use veil_engine::display_list::{block_fingerprints, diff_fingerprints};
use veil_engine::engine::{DocumentView, InvalidationKind, RetainedDocument};
use veil_engine::renderer_protocol::{
    DisplayListPatch, RenderRequest, RendererCommand, RendererReply, RuntimeDamage, RuntimeUpdate,
};''',
)
replace_once(
    "src/bin/veil_engine.rs",
    "    last_paint_fingerprint: u64,",
    "    last_paint_fingerprints: Vec<u64>,",
)
replace_once(
    "src/bin/veil_engine.rs",
    '''    let fingerprint = paint_fingerprint(&view);
    let session = LiveSession {
        runtime,
        report,
        retained,
        blocker,
        last_view: view.clone(),
        last_paint_fingerprint: fingerprint,
    };''',
    '''    let fingerprints = block_fingerprints(&view.blocks);
    let session = LiveSession {
        runtime,
        report,
        retained,
        blocker,
        last_view: view.clone(),
        last_paint_fingerprints: fingerprints,
    };''',
)
# Replace entire update_session function (it's the final function in this small binary).
p = Path("src/bin/veil_engine.rs")
text = p.read_text()
start = text.index("fn update_session(session: &mut LiveSession, default_prevented: bool) -> RuntimeUpdate {")
text = text[:start] + r'''fn update_session(session: &mut LiveSession, default_prevented: bool) -> RuntimeUpdate {
    let requested_damage = session.retained.update_from_report(&session.report);
    let old_title = session.last_view.title.clone();
    let old_icon = session.last_view.icon_url.clone();

    let mut candidate = if requested_damage.needs_layout() {
        session.retained.render(&session.blocker, &session.report)
    } else {
        let mut view = session.last_view.clone();
        session
            .retained
            .update_cached_view(&mut view, &session.report, requested_damage);
        view
    };
    candidate.external_stylesheets = session.last_view.external_stylesheets;
    candidate.external_scripts = session.last_view.external_scripts;
    candidate.web_fonts = session.last_view.web_fonts.clone();

    let fingerprints = block_fingerprints(&candidate.blocks);
    let diff = diff_fingerprints(&session.last_paint_fingerprints, &fingerprints);
    let paint_changed = !diff.is_empty();
    let metadata_changed = candidate.title != old_title || candidate.icon_url != old_icon;
    let actual_damage = if paint_changed {
        match requested_damage {
            InvalidationKind::Layout => RuntimeDamage::Layout,
            InvalidationKind::LayoutSubtree => RuntimeDamage::LayoutSubtree,
            _ => RuntimeDamage::Paint,
        }
    } else if metadata_changed {
        RuntimeDamage::Metadata
    } else {
        RuntimeDamage::None
    };

    let mut full_view = None;
    let mut patch = None;
    if actual_damage != RuntimeDamage::None {
        let inserted = diff.new_end.saturating_sub(diff.start);
        let localized = actual_damage != RuntimeDamage::Layout
            && diff.changed > 0
            && diff.changed <= candidate.blocks.len().max(1).div_ceil(2)
            && inserted <= 96;
        if localized {
            patch = Some(DisplayListPatch {
                start: diff.start,
                remove_count: diff.old_remove_count,
                blocks: candidate.blocks[diff.start..diff.new_end].to_vec(),
            });
        } else if paint_changed {
            full_view = Some(candidate.clone());
        }
        session.last_paint_fingerprints = fingerprints;
        session.last_view = candidate.clone();
    } else {
        session.last_view.script_report = session.report.clone();
    }

    RuntimeUpdate {
        view: full_view,
        patch,
        script_report: session.report.clone(),
        damage: actual_damage,
        title: candidate.title,
        icon_url: candidate.icon_url,
        cosmetic_hidden: candidate.cosmetic_hidden,
        external_stylesheets: candidate.external_stylesheets,
        external_scripts: candidate.external_scripts,
        reused_blocks: diff.reused,
        default_prevented,
    }
}
'''
p.write_text(text)

# Propagate retained patches through renderer host.
replace_once(
    "src/renderer_host.rs",
    '''use crate::renderer_protocol::{
    DomEventRequest, RenderRequest, RendererCommand, RendererReply, RuntimeDamage,
};''',
    '''use crate::renderer_protocol::{
    DisplayListPatch, DomEventRequest, RenderRequest, RendererCommand, RendererReply, RuntimeDamage,
};''',
)
replace_once(
    "src/renderer_host.rs",
    '''pub struct RuntimeHostUpdate {
    pub view: Option<DocumentView>,
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
    pub mode: RendererMode,
    pub default_prevented: bool,
}''',
    '''pub struct RuntimeHostUpdate {
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
    pub mode: RendererMode,
    pub default_prevented: bool,
}''',
)
old_map = '''RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                script_report: update.script_report,
                damage: update.damage,
                mode,
                default_prevented: update.default_prevented,
            }),'''
new_map = '''RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
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
                mode,
                default_prevented: update.default_prevented,
            }),'''
p = Path("src/renderer_host.rs")
text = p.read_text()
count = text.count(old_map)
if count != 2:
    raise SystemExit(f"src/renderer_host.rs: expected two runtime maps, found {count}")
p.write_text(text.replace(old_map, new_map))

# Runtime worker propagation.
replace_once(
    "src/runtime_interaction.rs",
    "use crate::renderer_protocol::{DomEventRequest, RuntimeDamage};",
    "use crate::renderer_protocol::{DisplayListPatch, DomEventRequest, RuntimeDamage};",
)
replace_once(
    "src/runtime_interaction.rs",
    '''pub struct RuntimePageUpdate {
    pub view: Option<DocumentView>,
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
}''',
    '''pub struct RuntimePageUpdate {
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
}''',
)
replace_once(
    "src/runtime_interaction.rs",
    '''                        Ok(RuntimePageUpdate {
                            view: update.view,
                            script_report: update.script_report,
                            damage: update.damage,
                        }),''',
    '''                        Ok(RuntimePageUpdate {
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
                        }),''',
)

# Browser UI retained lists + viewport virtualization.
replace_once(
    "src/main.rs",
    '''use veil_engine::engine::{
    DocumentView, FormControl, FormControlKind, RenderBlock, TextRun, WebFontResource,
};''',
    '''use veil_engine::display_list::{
    estimate_block_height, RetainedDisplayList, VIRTUALIZE_MIN_ITEMS, VIRTUALIZE_OVERSCAN,
};
use veil_engine::engine::{
    DocumentView, FormControl, FormControlKind, RenderBlock, TextRun, WebFontResource,
};''',
)
replace_once(
    "src/main.rs",
    "    media_cache: HashMap<String, CachedMedia>,\n    web_font_registry: HashMap<String, Vec<u8>>,",
    "    media_cache: HashMap<String, CachedMedia>,\n    display_lists: HashMap<u64, RetainedDisplayList>,\n    web_font_registry: HashMap<String, Vec<u8>>,",
)
replace_once(
    "src/main.rs",
    "            media_cache: HashMap::new(),\n            web_font_registry: HashMap::new(),",
    "            media_cache: HashMap::new(),\n            display_lists: HashMap::new(),\n            web_font_registry: HashMap::new(),",
)
replace_once(
    "src/main.rs",
    "        self.home_queries.remove(&closed_id);",
    "        self.home_queries.remove(&closed_id);\n        self.display_lists.remove(&closed_id);",
)
replace_once(
    "src/main.rs",
    '''            tab.address = HOME.into();
            tab.page = DocumentView::home();
            tab.status = "Private new tab".into();''',
    '''            tab.address = HOME.into();
            tab.page = DocumentView::home();
            tab.status = "Private new tab".into();
            self.display_lists.remove(&tab.id);''',
)

# Reconcile a retained paint list when a navigation result arrives.
replace_once(
    "src/main.rs",
    '''                Ok(view) => {
                    install_web_fonts(ctx, &view.web_fonts, &mut self.web_font_registry);
                    tab.address = view.url.clone();
                    tab.status = format!(
                        "{} ms · {} blocked · {} CSS · {} scripts · {} · retained paint",
                        result.elapsed_ms,
                        result.blocked_count,
                        view.external_stylesheets,
                        view.external_scripts,
                        result.renderer_mode.label(),
                    );
                    tab.page = view;
                }''',
    '''                Ok(view) => {
                    install_web_fonts(ctx, &view.web_fonts, &mut self.web_font_registry);
                    let tab_id = tab.id;
                    let diff = self
                        .display_lists
                        .entry(tab_id)
                        .or_default()
                        .reconcile(&view.blocks, 1100.0);
                    tab.address = view.url.clone();
                    tab.status = format!(
                        "{} ms · {} blocked · {} CSS · {} scripts · {} · {} retained / {} changed",
                        result.elapsed_ms,
                        result.blocked_count,
                        view.external_stylesheets,
                        view.external_scripts,
                        result.renderer_mode.label(),
                        diff.reused,
                        diff.changed,
                    );
                    tab.page = view;
                }''',
)

# Replace runtime-success arm with patch-aware application.
old_runtime = '''                Ok(update) => {
                    let damage = update.damage;
                    if let Some(mut view) = update.view {
                        // Fonts are loaded by the navigation broker, not the engine process.
                        // Preserve them while applying retained layout/paint damage.
                        view.web_fonts = self.tabs[index].page.web_fonts.clone();
                        install_web_fonts(ctx, &view.web_fonts, &mut self.web_font_registry);
                        self.tabs[index].page = view;
                    } else {
                        // RefreshDriver-style no-op tick: advance timers/rAF state without
                        // replacing or repainting the retained page.
                        self.tabs[index].page.script_report = update.script_report;
                    }
                    if damage != RuntimeDamage::None {
                        self.tabs[index].status = result
                            .mode
                            .map(|mode| format!("Live {:?} update · {}", damage, mode.label()))
                            .unwrap_or_else(|| format!("Live {:?} update", damage));
                        ctx.request_repaint();
                    }
                }'''
new_runtime = '''                Ok(update) => {
                    let damage = update.damage;
                    let retained_count = update.reused_blocks;
                    if let Some(mut view) = update.view {
                        // Fonts are loaded by the navigation broker, not the engine process.
                        // Preserve them while applying a full retained-layout replacement.
                        view.web_fonts = self.tabs[index].page.web_fonts.clone();
                        install_web_fonts(ctx, &view.web_fonts, &mut self.web_font_registry);
                        self.tabs[index].page = view;
                    } else if let Some(patch) = update.patch {
                        let page = &mut self.tabs[index].page;
                        let start = patch.start.min(page.blocks.len());
                        let end = start.saturating_add(patch.remove_count).min(page.blocks.len());
                        page.blocks.splice(start..end, patch.blocks);
                        page.title = update.title;
                        page.icon_url = update.icon_url;
                        page.cosmetic_hidden = update.cosmetic_hidden;
                        page.external_stylesheets = update.external_stylesheets;
                        page.external_scripts = update.external_scripts;
                        page.script_report = update.script_report;
                    } else {
                        // RefreshDriver-style no-op or metadata-only tick: advance runtime state
                        // without replacing the retained display list.
                        let page = &mut self.tabs[index].page;
                        page.title = update.title;
                        page.icon_url = update.icon_url;
                        page.cosmetic_hidden = update.cosmetic_hidden;
                        page.external_stylesheets = update.external_stylesheets;
                        page.external_scripts = update.external_scripts;
                        page.script_report = update.script_report;
                    }
                    if damage != RuntimeDamage::None {
                        let tab_id = self.tabs[index].id;
                        let diff = self
                            .display_lists
                            .entry(tab_id)
                            .or_default()
                            .reconcile(&self.tabs[index].page.blocks, 1100.0);
                        self.tabs[index].status = result
                            .mode
                            .map(|mode| {
                                format!(
                                    "Live {:?} · {} retained · {} changed · {}",
                                    damage,
                                    retained_count.max(diff.reused),
                                    diff.changed,
                                    mode.label()
                                )
                            })
                            .unwrap_or_else(|| format!("Live {:?} update", damage));
                        ctx.request_repaint();
                    }
                }'''
replace_once("src/main.rs", old_runtime, new_runtime)

# Replace page loop with retained measured-height virtualization.
old_page_loop = '''        ScrollArea::vertical()
            .id_salt(("page", self.tabs[tab_index].id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(if split { 8.0 } else { 4.0 });
                ui.horizontal(|ui| {
                    ui.add_space(if split { 8.0 } else { 6.0 });
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 20.0).max(280.0).min(1260.0));
                        for block in &page.blocks {
                            self.render_block(
                                ctx,
                                ui,
                                tab_index,
                                block,
                                base_url.as_ref(),
                                privacy,
                                &mut navigation,
                            );
                        }
                        ui.add_space(56.0);
                    });
                });
            });'''
new_page_loop = '''        let tab_id = self.tabs[tab_index].id;
        let page_width = ui.available_width().max(280.0).min(1260.0);
        self.display_lists
            .entry(tab_id)
            .or_default()
            .reconcile(&page.blocks, page_width);
        let heights: Vec<f32> = page
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                self.display_lists
                    .get(&tab_id)
                    .map(|list| list.height_for(index, block, page_width))
                    .unwrap_or_else(|| estimate_block_height(block, page_width))
            })
            .collect();
        let virtualize = page.blocks.len() >= VIRTUALIZE_MIN_ITEMS;
        let mut observed_heights = Vec::new();

        ScrollArea::vertical()
            .id_salt(("page", tab_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(if split { 8.0 } else { 4.0 });
                ui.horizontal(|ui| {
                    ui.add_space(if split { 8.0 } else { 6.0 });
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 20.0).max(280.0).min(1260.0));
                        let expanded_clip = ui.clip_rect().expand(VIRTUALIZE_OVERSCAN);
                        for (block_index, block) in page.blocks.iter().enumerate() {
                            let estimated = heights
                                .get(block_index)
                                .copied()
                                .unwrap_or_else(|| estimate_block_height(block, ui.available_width()));
                            let predicted = egui::Rect::from_min_size(
                                egui::pos2(ui.min_rect().left(), ui.next_widget_position().y),
                                egui::vec2(ui.available_width().max(1.0), estimated.max(1.0)),
                            );
                            if virtualize && !expanded_clip.intersects(predicted) {
                                ui.allocate_space(egui::vec2(
                                    ui.available_width().max(1.0),
                                    estimated.max(1.0),
                                ));
                                continue;
                            }

                            let before = ui.next_widget_position().y;
                            self.render_block(
                                ctx,
                                ui,
                                tab_index,
                                block,
                                base_url.as_ref(),
                                privacy,
                                &mut navigation,
                            );
                            let measured = (ui.next_widget_position().y - before).abs().max(1.0);
                            observed_heights.push((block_index, measured));
                        }
                        ui.add_space(56.0);
                    });
                });
            });

        if let Some(list) = self.display_lists.get_mut(&tab_id) {
            for (index, height) in observed_heights {
                list.observe_height(index, height);
            }
        }'''
replace_once("src/main.rs", old_page_loop, new_page_loop)

# Virtualize large nested vertical containers too; row/grid layouts keep normal layout semantics.
old_container = '''            RenderBlock::Container { children, style } => {
                with_box(ui, style, |ui| match style.layout {
                    LayoutMode::Block => {
                        for child in children {
                            self.render_block(
                                ctx, ui, tab_index, child, base_url, privacy, navigation,
                            );
                        }
                    }
                    LayoutMode::FlexColumn => {
                        let mut ordered: Vec<&RenderBlock> = children.iter().collect();
                        ordered.sort_by_key(|child| block_order(child));
                        for (index, child) in ordered.iter().enumerate() {
                            self.render_block(
                                ctx, ui, tab_index, child, base_url, privacy, navigation,
                            );
                            if index + 1 < ordered.len() {
                                ui.add_space(style.gap);
                            }
                        }
                    }'''
new_container = '''            RenderBlock::Container { children, style } => {
                with_box(ui, style, |ui| match style.layout {
                    LayoutMode::Block => {
                        let ordered: Vec<&RenderBlock> = children.iter().collect();
                        self.render_vertical_children_virtualized(
                            ctx,
                            ui,
                            tab_index,
                            &ordered,
                            0.0,
                            base_url,
                            privacy,
                            navigation,
                        );
                    }
                    LayoutMode::FlexColumn => {
                        let mut ordered: Vec<&RenderBlock> = children.iter().collect();
                        ordered.sort_by_key(|child| block_order(child));
                        self.render_vertical_children_virtualized(
                            ctx,
                            ui,
                            tab_index,
                            &ordered,
                            style.gap,
                            base_url,
                            privacy,
                            navigation,
                        );
                    }'''
replace_once("src/main.rs", old_container, new_container)

# Insert nested vertical virtualization helper immediately before render_form.
replace_once(
    "src/main.rs",
    "    fn render_form(\n",
    r'''    fn render_vertical_children_virtualized(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        tab_index: usize,
        children: &[&RenderBlock],
        gap: f32,
        base_url: Option<&Url>,
        privacy: SitePrivacy,
        navigation: &mut Option<PendingNavigation>,
    ) {
        let virtualize = children.len() >= VIRTUALIZE_MIN_ITEMS;
        let expanded_clip = ui.clip_rect().expand(VIRTUALIZE_OVERSCAN);
        for (index, child) in children.iter().enumerate() {
            let estimated = estimate_block_height(child, ui.available_width()).max(1.0);
            let predicted = egui::Rect::from_min_size(
                egui::pos2(ui.min_rect().left(), ui.next_widget_position().y),
                egui::vec2(ui.available_width().max(1.0), estimated),
            );
            if virtualize && !expanded_clip.intersects(predicted) {
                ui.allocate_space(egui::vec2(ui.available_width().max(1.0), estimated));
            } else {
                self.render_block(
                    ctx,
                    ui,
                    tab_index,
                    child,
                    base_url,
                    privacy,
                    navigation,
                );
            }
            if index + 1 < children.len() && gap > 0.0 {
                ui.add_space(gap);
            }
        }
    }

    fn render_form(
''',
)

# UA version.
p = Path("src/net.rs")
text = p.read_text().replace("VeilBrowser/0.8.7 VeilEngine/0.8.7", "VeilBrowser/0.8.8 VeilEngine/0.8.8")
p.write_text(text)

print("Veil 0.8.8 render-performance patch applied")
