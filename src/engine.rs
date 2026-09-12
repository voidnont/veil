use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::blocker::Blocker;
use crate::dom::{Dom, NodeKind};
use crate::privacy::SitePrivacy;
use crate::script::{CanvasCommand, DomMutation, JavascriptSandbox, ScriptReport};
use crate::storage::ScriptStorageSnapshot;
use crate::style::{ComputedStyle, FontKind, StyleSheet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub href: Option<String>,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub muted: bool,
    pub color: Option<[u8; 4]>,
    pub font_family: FontKind,
    pub font_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormControlKind {
    Text,
    Search,
    Email,
    Url,
    Password,
    Hidden,
    Checkbox,
    File,
    Submit,
    Button,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormControl {
    pub node_id: usize,
    pub kind: FormControlKind,
    pub name: String,
    pub value: String,
    pub placeholder: String,
    pub label: String,
    pub checked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenderBlock {
    Heading {
        level: u8,
        runs: Vec<TextRun>,
        style: ComputedStyle,
    },
    Paragraph {
        runs: Vec<TextRun>,
        style: ComputedStyle,
    },
    Container {
        children: Vec<RenderBlock>,
        style: ComputedStyle,
    },
    Image {
        src: String,
        alt: String,
        width: Option<f32>,
        height: Option<f32>,
        style: ComputedStyle,
    },
    Canvas {
        id: String,
        width: f32,
        height: f32,
        commands: Vec<CanvasCommand>,
        style: ComputedStyle,
    },
    Media {
        kind: String,
        src: String,
        poster: Option<String>,
        controls: bool,
        muted: bool,
        autoplay: bool,
        width: Option<f32>,
        height: Option<f32>,
        style: ComputedStyle,
    },
    Form {
        action: String,
        method: String,
        enctype: String,
        controls: Vec<FormControl>,
        style: ComputedStyle,
    },
    Rule {
        style: ComputedStyle,
    },
    Code {
        text: String,
        style: ComputedStyle,
    },
    Notice(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFontResource {
    pub family: String,
    pub url: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct WebFontSource {
    pub family: String,
    pub url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentView {
    pub url: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub blocks: Vec<RenderBlock>,
    pub cosmetic_hidden: usize,
    pub script_report: ScriptReport,
    pub external_stylesheets: usize,
    pub external_scripts: usize,
    pub web_fonts: Vec<WebFontResource>,
}

impl DocumentView {
    pub fn home() -> Self {
        let base = ComputedStyle::default();
        let mut h1 = base.clone();
        h1.font_size = 40.0;
        h1.bold = true;
        h1.margin.bottom = 10.0;

        let mut h2 = base.clone();
        h2.font_size = 24.0;
        h2.bold = true;
        h2.margin.top = 18.0;
        h2.margin.bottom = 8.0;

        Self {
            url: "veil://home".into(),
            title: "New Tab".into(),
            icon_url: None,
            blocks: vec![
                RenderBlock::Heading {
                    level: 1,
                    runs: vec![run("Veil", &h1, None)],
                    style: h1,
                },
                RenderBlock::Notice(
                    "Private by default · built-in ad/tracker blocking · independent rendering engine".into(),
                ),
                RenderBlock::Paragraph {
                    runs: vec![run(
                        "Veil Browser is a privacy-first browser with its own Veil Engine. Tabs live in the vertical command center, the URL bar floats above the page, and the glass shell stays translucent so your desktop remains visible.",
                        &base,
                        None,
                    )],
                    style: base.clone(),
                },
                RenderBlock::Heading {
                    level: 2,
                    runs: vec![run("Privacy model", &h2, None)],
                    style: h2,
                },
                RenderBlock::Paragraph {
                    runs: vec![run(
                        "Ad/tracker requests are filtered before they are sent. GPC and DNT are enabled, referrers are suppressed, tracking query parameters are removed, and cookies/history are not persisted. Strict third-party blocking remains available per site.",
                        &base,
                        None,
                    )],
                    style: base,
                },
            ],
            cosmetic_hidden: 0,
            script_report: ScriptReport::default(),
            external_stylesheets: 0,
            external_scripts: 0,
            web_fonts: Vec::new(),
        }
    }

    pub fn error(url: &str, error: &str) -> Self {
        let base = ComputedStyle::default();
        let mut heading = base.clone();
        heading.font_size = 32.0;
        heading.bold = true;
        Self {
            url: url.into(),
            title: "Navigation error".into(),
            icon_url: None,
            blocks: vec![
                RenderBlock::Heading {
                    level: 1,
                    runs: vec![run("Couldn’t open this page", &heading, None)],
                    style: heading,
                },
                RenderBlock::Notice(error.into()),
                RenderBlock::Paragraph {
                    runs: vec![TextRun {
                        text: url.into(),
                        href: None,
                        size: 14.0,
                        bold: false,
                        italic: false,
                        muted: true,
                        color: None,
                        font_family: FontKind::Sans,
                        font_name: None,
                    }],
                    style: base,
                },
            ],
            cosmetic_hidden: 0,
            script_report: ScriptReport::default(),
            external_stylesheets: 0,
            external_scripts: 0,
            web_fonts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvalidationKind {
    None,
    Metadata,
    Paint,
    Layout,
}

impl InvalidationKind {
    pub fn needs_repaint(self) -> bool {
        self != Self::None
    }

    pub fn needs_layout(self) -> bool {
        self == Self::Layout
    }
}

/// Retained DOM/style state for one live page session. Gecko keeps style/layout
/// state alive and invalidates it from mutations instead of reparsing the whole
/// document on every refresh tick. Veil follows that model at a smaller scale:
/// the parsed DOM and stylesheet survive for the navigation lifetime, and only
/// newly reported JS mutations are applied between paints.
pub struct RetainedDocument {
    url: String,
    original_dom: Dom,
    live_dom: Dom,
    sheet: StyleSheet,
    page_url: Option<Url>,
    privacy: SitePrivacy,
    external_script_count: usize,
    icon_url: Option<String>,
    applied_mutations: usize,
    applied_canvas_commands: usize,
    last_body_html_override: Option<String>,
    last_title_override: Option<String>,
}

impl RetainedDocument {
    pub fn new(
        url: &str,
        html: &str,
        privacy: SitePrivacy,
        external_css: &[String],
        external_script_count: usize,
        report: &ScriptReport,
    ) -> Self {
        let original_dom = Dom::parse(html);
        let page_url = Url::parse(url).ok();
        let icon_url = find_icon_url(&original_dom, page_url.as_ref());
        let mut sheet = StyleSheet::from_dom(&original_dom);
        for css in external_css {
            sheet.parse_and_append(css);
        }
        let mut live_dom = original_dom.clone();
        apply_script_mutations(&mut live_dom, report);
        Self {
            url: url.to_owned(),
            original_dom,
            live_dom,
            sheet,
            page_url,
            privacy,
            external_script_count,
            icon_url,
            applied_mutations: report.dom_mutations.len(),
            applied_canvas_commands: report.canvas_commands.len(),
            last_body_html_override: report.body_html_override.clone(),
            last_title_override: report.title_override.clone(),
        }
    }

    pub fn update_from_report(&mut self, report: &ScriptReport) -> InvalidationKind {
        let mut invalidation = InvalidationKind::None;

        if report.dom_mutations.len() < self.applied_mutations {
            self.live_dom = self.original_dom.clone();
            apply_script_mutations(&mut self.live_dom, report);
            self.applied_mutations = report.dom_mutations.len();
            invalidation = InvalidationKind::Layout;
        } else if report.dom_mutations.len() > self.applied_mutations {
            for mutation in &report.dom_mutations[self.applied_mutations..] {
                apply_single_script_mutation(&mut self.live_dom, mutation);
            }
            self.applied_mutations = report.dom_mutations.len();
            invalidation = InvalidationKind::Layout;
        }

        if report.body_html_override != self.last_body_html_override {
            if let Some(body) = report
                .body_html_override
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                if let Some(idx) = self.live_dom.find_first_tag("body") {
                    self.live_dom.replace_inner_html(idx, body);
                }
            } else {
                self.live_dom = self.original_dom.clone();
                apply_script_mutations(&mut self.live_dom, report);
                self.applied_mutations = report.dom_mutations.len();
            }
            self.last_body_html_override = report.body_html_override.clone();
            invalidation = invalidation.max(InvalidationKind::Layout);
        }

        if report.canvas_commands.len() != self.applied_canvas_commands {
            self.applied_canvas_commands = report.canvas_commands.len();
            invalidation = invalidation.max(InvalidationKind::Paint);
        }

        if report.title_override != self.last_title_override {
            self.last_title_override = report.title_override.clone();
            invalidation = invalidation.max(InvalidationKind::Metadata);
        }

        invalidation
    }

    pub fn render(&self, blocker: &Blocker, report: &ScriptReport) -> DocumentView {
        let title = report
            .title_override
            .clone()
            .or_else(|| find_title(&self.original_dom))
            .unwrap_or_else(|| self.url.clone());
        let mut cosmetic_hidden = 0usize;
        let base = ComputedStyle::default();
        let mut blocks = build_children(
            &self.live_dom,
            self.live_dom.root,
            &base,
            &self.sheet,
            blocker,
            self.page_url.as_ref(),
            self.privacy.shields,
            false,
            &mut cosmetic_hidden,
        );
        attach_canvas_commands(&mut blocks, &report.canvas_commands);

        if blocks.is_empty() {
            let mut recovery_hidden = 0usize;
            let mut recovered = build_children(
                &self.live_dom,
                self.live_dom.root,
                &base,
                &self.sheet,
                blocker,
                self.page_url.as_ref(),
                false,
                true,
                &mut recovery_hidden,
            );
            attach_canvas_commands(&mut recovered, &report.canvas_commands);
            if !recovered.is_empty() {
                blocks = recovered;
            }
        }

        if blocks.is_empty() {
            let fallback = build_compatibility_fallback(&self.live_dom, &base);
            if fallback.is_empty() {
                blocks.push(RenderBlock::Notice(
                    "This document has no visible content Veil Browser 0.8.7 can currently paint. It may depend on unsupported Web APIs, canvas/WebGL, iframes, or a newer layout feature.".into(),
                ));
            } else {
                blocks.push(RenderBlock::Notice(
                    "Compatibility view: Veil Engine simplified this page because its normal layout produced no paintable blocks.".into(),
                ));
                blocks.extend(fallback);
            }
        }

        DocumentView {
            url: self.url.clone(),
            title,
            icon_url: self.icon_url.clone(),
            blocks,
            cosmetic_hidden,
            script_report: report.clone(),
            external_stylesheets: 0,
            external_scripts: self.external_script_count,
            web_fonts: Vec::new(),
        }
    }

    pub fn update_cached_view(
        &self,
        view: &mut DocumentView,
        report: &ScriptReport,
        invalidation: InvalidationKind,
    ) {
        view.title = report
            .title_override
            .clone()
            .or_else(|| find_title(&self.original_dom))
            .unwrap_or_else(|| self.url.clone());
        view.icon_url = self.icon_url.clone();
        view.script_report = report.clone();
        if invalidation >= InvalidationKind::Paint {
            clear_canvas_commands(&mut view.blocks);
            attach_canvas_commands(&mut view.blocks, &report.canvas_commands);
        }
    }
}

pub fn paint_fingerprint(view: &DocumentView) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(&view.blocks) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}

fn clear_canvas_commands(blocks: &mut [RenderBlock]) {
    for block in blocks {
        match block {
            RenderBlock::Canvas { commands, .. } => commands.clear(),
            RenderBlock::Container { children, .. } => clear_canvas_commands(children),
            _ => {}
        }
    }
}

#[derive(Default)]
pub struct Engine {
    sandbox: JavascriptSandbox,
}

impl Engine {
    pub fn discover_stylesheets(&self, html: &str, base: &Url) -> Vec<Url> {
        discover_resources(html, base, ResourceDiscovery::Stylesheet)
    }

    pub fn discover_external_scripts(&self, html: &str, base: &Url) -> Vec<Url> {
        discover_resources(html, base, ResourceDiscovery::Script)
    }

    pub fn discover_web_fonts(&self, css: &str, base: &Url) -> Vec<WebFontSource> {
        discover_web_fonts(css, base)
    }

    pub fn parse(
        &self,
        url: &str,
        html: &str,
        blocker: &Blocker,
        privacy: SitePrivacy,
    ) -> DocumentView {
        self.parse_with_resources(
            url,
            html,
            blocker,
            privacy,
            &[],
            &[],
            0,
            &ScriptStorageSnapshot::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn parse_with_resources(
        &self,
        url: &str,
        html: &str,
        blocker: &Blocker,
        privacy: SitePrivacy,
        external_css: &[String],
        external_scripts: &[String],
        external_script_count: usize,
        storage: &ScriptStorageSnapshot,
    ) -> DocumentView {
        let original_dom = Dom::parse(html);
        let script_report = self.sandbox.run(
            &original_dom,
            privacy.javascript,
            external_scripts,
            external_script_count,
            storage,
        );
        self.render_with_script_report(
            url,
            html,
            blocker,
            privacy,
            external_css,
            external_script_count,
            script_report,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_with_script_report(
        &self,
        url: &str,
        html: &str,
        blocker: &Blocker,
        privacy: SitePrivacy,
        external_css: &[String],
        external_script_count: usize,
        script_report: ScriptReport,
    ) -> DocumentView {
        let original_dom = Dom::parse(html);
        let page_url = Url::parse(url).ok();
        let mut sheet = StyleSheet::from_dom(&original_dom);
        for css in external_css {
            sheet.parse_and_append(css);
        }

        let mut live_dom = original_dom.clone();
        apply_script_mutations(&mut live_dom, &script_report);
        let dom = &live_dom;

        let title = script_report
            .title_override
            .clone()
            .or_else(|| find_title(&original_dom))
            .unwrap_or_else(|| url.to_owned());

        let icon_url = find_icon_url(&original_dom, page_url.as_ref());

        let mut cosmetic_hidden = 0usize;
        let base = ComputedStyle::default();
        let mut blocks = build_children(
            dom,
            dom.root,
            &base,
            &sheet,
            blocker,
            page_url.as_ref(),
            privacy.shields,
            false,
            &mut cosmetic_hidden,
        );

        attach_canvas_commands(&mut blocks, &script_report.canvas_commands);

        // Many modern apps intentionally hide their root until hydration. If Veil's first
        // paint is completely empty, retry the same structural renderer while relaxing only
        // visibility/display suppression and cosmetic blocking. This keeps flex/grid,
        // dimensions, backgrounds, images, forms, and text instead of flattening the page.
        if blocks.is_empty() {
            let mut recovery_hidden = 0usize;
            let mut recovered = build_children(
                dom,
                dom.root,
                &base,
                &sheet,
                blocker,
                page_url.as_ref(),
                false,
                true,
                &mut recovery_hidden,
            );
            attach_canvas_commands(&mut recovered, &script_report.canvas_commands);
            if !recovered.is_empty() {
                blocks = recovered;
            }
        }

        if blocks.is_empty() {
            let fallback = build_compatibility_fallback(dom, &base);
            if fallback.is_empty() {
                blocks.push(RenderBlock::Notice(
                    "This document has no visible content Veil Browser 0.8.0 can currently paint. It may depend on unsupported Web APIs, canvas/WebGL, iframes, or a newer layout feature.".into(),
                ));
            } else {
                blocks.push(RenderBlock::Notice(
                    "Compatibility view: Veil Engine simplified this page because its normal layout produced no paintable blocks.".into(),
                ));
                blocks.extend(fallback);
            }
        }

        DocumentView {
            url: url.into(),
            title,
            icon_url,
            blocks,
            cosmetic_hidden,
            script_report,
            external_stylesheets: external_css.len(),
            external_scripts: external_script_count,
            web_fonts: Vec::new(),
        }
    }
}

fn find_icon_url(dom: &Dom, base: Option<&Url>) -> Option<String> {
    let base = base?;
    for node in &dom.nodes {
        let NodeKind::Element(el) = &node.kind else {
            continue;
        };
        if el.tag != "link" {
            continue;
        }
        let rel = el
            .attrs
            .get("rel")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        if !rel
            .split_whitespace()
            .any(|token| token == "icon" || token == "shortcut")
        {
            continue;
        }
        let Some(href) = el
            .attrs
            .get("href")
            .map(String::as_str)
            .filter(|v| !v.trim().is_empty())
        else {
            continue;
        };
        if let Ok(url) = base.join(href) {
            if matches!(url.scheme(), "http" | "https") {
                return Some(url.to_string());
            }
        }
    }

    let host = base.host_str()?;
    let mut fallback = base.clone();
    fallback.set_path("/favicon.ico");
    fallback.set_query(None);
    fallback.set_fragment(None);
    if !host.is_empty() {
        Some(fallback.to_string())
    } else {
        None
    }
}

fn apply_single_script_mutation(dom: &mut Dom, mutation: &DomMutation) {
    let target = if mutation.target_id == "__body__" {
        dom.find_first_tag("body")
    } else if let Some(raw) = mutation.target_id.strip_prefix("@node:n") {
        raw.parse::<usize>()
            .ok()
            .filter(|idx| *idx < dom.nodes.len())
    } else {
        dom.find_element_by_id(&mutation.target_id)
    };
    let Some(idx) = target else {
        return;
    };
    match mutation.kind.as_str() {
        "text" => dom.replace_text_content(idx, &mutation.value),
        "html" => dom.replace_inner_html(idx, &mutation.value),
        "append-html" => dom.append_inner_html(idx, &mutation.value, false),
        "prepend-html" => dom.append_inner_html(idx, &mutation.value, true),
        "attr-set" => {
            if let Some((name, value)) = mutation.value.split_once(' ') {
                dom.set_attribute(idx, name, value);
            }
        }
        "attr-remove" => dom.remove_attribute(idx, &mutation.value),
        "style-set" => {
            if let Some((name, value)) = mutation.value.split_once(':') {
                dom.set_style_property(idx, name, value);
            }
        }
        "remove" => dom.remove_node(idx),
        _ => {}
    }
}

fn apply_script_mutations(dom: &mut Dom, report: &ScriptReport) {
    for mutation in &report.dom_mutations {
        apply_single_script_mutation(dom, mutation);
    }
    if let Some(body) = report
        .body_html_override
        .as_ref()
        .filter(|body| !body.trim().is_empty())
    {
        if let Some(idx) = dom.find_first_tag("body") {
            dom.replace_inner_html(idx, body);
        }
    }
}

fn attach_canvas_commands(blocks: &mut [RenderBlock], commands: &[CanvasCommand]) {
    for block in blocks {
        match block {
            RenderBlock::Canvas {
                id,
                commands: target,
                ..
            } => {
                target.extend(
                    commands
                        .iter()
                        .filter(|command| command.canvas_id == *id)
                        .cloned(),
                );
            }
            RenderBlock::Container { children, .. } => attach_canvas_commands(children, commands),
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
enum ResourceDiscovery {
    Stylesheet,
    Script,
}

fn discover_resources(html: &str, base: &Url, kind: ResourceDiscovery) -> Vec<Url> {
    let dom = Dom::parse(html);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for node in &dom.nodes {
        let NodeKind::Element(el) = &node.kind else {
            continue;
        };
        let href = match kind {
            ResourceDiscovery::Stylesheet if el.tag == "link" => {
                let rel = el
                    .attrs
                    .get("rel")
                    .map(|v| v.to_ascii_lowercase())
                    .unwrap_or_default();
                if !rel.split_whitespace().any(|token| token == "stylesheet") {
                    continue;
                }
                el.attrs.get("href")
            }
            ResourceDiscovery::Script if el.tag == "script" => {
                if !script_type_is_executable(el.attrs.get("type").map(String::as_str)) {
                    continue;
                }
                el.attrs.get("src")
            }
            _ => continue,
        };
        let Some(href) = href.map(String::as_str).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        let Ok(url) = base.join(href) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        if seen.insert(url.to_string()) {
            out.push(url);
        }
    }
    out
}

fn script_type_is_executable(kind: Option<&str>) -> bool {
    let Some(kind) = kind.map(str::trim).filter(|v| !v.is_empty()) else {
        return true;
    };
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "text/javascript"
            | "application/javascript"
            | "application/ecmascript"
            | "text/ecmascript"
            | "module"
    )
}

fn discover_web_fonts(css: &str, base: &Url) -> Vec<WebFontSource> {
    let lower = css.to_ascii_lowercase();
    let mut cursor = 0usize;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    while let Some(relative) = lower[cursor..].find("@font-face") {
        let start = cursor + relative;
        let Some(open_rel) = lower[start..].find('{') else {
            break;
        };
        let open = start + open_rel + 1;
        let Some(close_rel) = lower[open..].find('}') else {
            break;
        };
        let close = open + close_rel;
        let block = &css[open..close];
        let mut family = None;
        let mut source = None;
        for decl in block.split(';') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            match name.trim().to_ascii_lowercase().as_str() {
                "font-family" => {
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    if !value.is_empty() {
                        family = Some(value.to_owned());
                    }
                }
                "src" => {
                    let lower_value = value.to_ascii_lowercase();
                    if let Some(url_pos) = lower_value.find("url(") {
                        let tail = &value[url_pos + 4..];
                        if let Some(end) = tail.find(')') {
                            let raw = tail[..end].trim().trim_matches('"').trim_matches('\'');
                            if !raw.is_empty() && !raw.starts_with("data:") {
                                source = Some(raw.to_owned());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let (Some(family), Some(source)) = (family, source) {
            if let Ok(url) = base.join(&source) {
                if matches!(url.scheme(), "http" | "https") && seen.insert(url.to_string()) {
                    out.push(WebFontSource { family, url });
                }
            }
        }
        cursor = close + 1;
    }
    out
}

fn build_children(
    dom: &Dom,
    parent_idx: usize,
    parent_style: &ComputedStyle,
    sheet: &StyleSheet,
    blocker: &Blocker,
    page_url: Option<&Url>,
    cosmetic_enabled: bool,
    recover_visibility: bool,
    hidden: &mut usize,
) -> Vec<RenderBlock> {
    let mut blocks = Vec::new();
    let mut pending_runs = Vec::new();

    for &child in &dom.nodes[parent_idx].children {
        match &dom.nodes[child].kind {
            NodeKind::Text(_) => collect_runs(
                dom,
                child,
                parent_style,
                None,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
                &mut pending_runs,
            ),
            NodeKind::Element(el) if is_ignored_tag(&el.tag) => {}
            NodeKind::Element(el) if is_block_tag(&el.tag) => {
                flush_pending(&mut pending_runs, parent_style, &mut blocks);
                if let Some(block) = build_block(
                    dom,
                    child,
                    parent_style,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                ) {
                    blocks.push(block);
                }
            }
            NodeKind::Element(_) => {
                collect_runs(
                    dom,
                    child,
                    parent_style,
                    None,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    &mut pending_runs,
                );
                let mut embedded = Vec::new();
                collect_embedded_blocks(
                    dom,
                    child,
                    parent_style,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    &mut embedded,
                );
                if !embedded.is_empty() {
                    flush_pending(&mut pending_runs, parent_style, &mut blocks);
                    blocks.extend(embedded);
                }
            }
        }
    }

    flush_pending(&mut pending_runs, parent_style, &mut blocks);
    blocks
}

fn build_block(
    dom: &Dom,
    idx: usize,
    parent_style: &ComputedStyle,
    sheet: &StyleSheet,
    blocker: &Blocker,
    page_url: Option<&Url>,
    cosmetic_enabled: bool,
    recover_visibility: bool,
    hidden: &mut usize,
) -> Option<RenderBlock> {
    let NodeKind::Element(el) = &dom.nodes[idx].kind else {
        return None;
    };
    if is_ignored_tag(&el.tag) {
        return None;
    }
    if cosmetic_enabled {
        if let Some(url) = page_url {
            if blocker.should_hide_element(url, &el.tag, &el.attrs) {
                *hidden += 1;
                return None;
            }
        }
    }

    let mut style = sheet.compute_node(dom, idx, parent_style);
    if recover_visibility {
        style.display_none = false;
    }
    if style.display_none {
        return None;
    }

    match el.tag.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let mut runs = Vec::new();
            collect_runs(
                dom,
                idx,
                &style,
                None,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
                &mut runs,
            );
            compact_runs(&mut runs);
            (!runs.is_empty()).then(|| RenderBlock::Heading {
                level: el.tag[1..].parse::<u8>().unwrap_or(2),
                runs,
                style,
            })
        }
        "p" | "blockquote" | "li" => {
            let mut runs = Vec::new();
            collect_runs(
                dom,
                idx,
                &style,
                None,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
                &mut runs,
            );
            compact_runs(&mut runs);
            if el.tag == "li" && !runs.is_empty() {
                runs.insert(0, run("• ", &style, None));
            }
            (!runs.is_empty()).then(|| RenderBlock::Paragraph { runs, style })
        }
        "pre" => {
            let text = dom.text_content(idx);
            (!text.trim().is_empty()).then(|| RenderBlock::Code { text, style })
        }
        "hr" => Some(RenderBlock::Rule { style }),
        "img" => {
            let src = image_source(dom, idx)?;
            let width = el
                .attrs
                .get("width")
                .and_then(|v| parse_dimension(v))
                .or(style.width);
            let height = el.attrs.get("height").and_then(|v| parse_dimension(v));
            Some(RenderBlock::Image {
                src,
                alt: el.attrs.get("alt").cloned().unwrap_or_default(),
                width,
                height,
                style,
            })
        }
        "canvas" => {
            let id = el
                .attrs
                .get("id")
                .cloned()
                .unwrap_or_else(|| format!("__vv_canvas_{idx}"));
            let width = el
                .attrs
                .get("width")
                .and_then(|v| parse_dimension(v))
                .or(style.width)
                .unwrap_or(300.0)
                .clamp(1.0, 4096.0);
            let height = el
                .attrs
                .get("height")
                .and_then(|v| parse_dimension(v))
                .or(style.height)
                .unwrap_or(150.0)
                .clamp(1.0, 4096.0);
            Some(RenderBlock::Canvas {
                id,
                width,
                height,
                commands: Vec::new(),
                style,
            })
        }
        "video" | "audio" => {
            let src = media_source(dom, idx).unwrap_or_default();
            let width = el
                .attrs
                .get("width")
                .and_then(|v| parse_dimension(v))
                .or(style.width);
            let height = el
                .attrs
                .get("height")
                .and_then(|v| parse_dimension(v))
                .or(style.height);
            Some(RenderBlock::Media {
                kind: el.tag.clone(),
                src,
                poster: el.attrs.get("poster").cloned(),
                controls: el.attrs.contains_key("controls"),
                muted: el.attrs.contains_key("muted"),
                autoplay: el.attrs.contains_key("autoplay"),
                width,
                height,
                style,
            })
        }
        "form" => {
            let mut controls = Vec::new();
            collect_form_controls(dom, idx, &mut controls);
            if controls.is_empty() {
                return None;
            }
            Some(RenderBlock::Form {
                action: el.attrs.get("action").cloned().unwrap_or_default(),
                method: el
                    .attrs
                    .get("method")
                    .map(|v| v.to_ascii_lowercase())
                    .unwrap_or_else(|| "get".into()),
                enctype: el
                    .attrs
                    .get("enctype")
                    .map(|v| v.to_ascii_lowercase())
                    .unwrap_or_else(|| "application/x-www-form-urlencoded".into()),
                controls,
                style,
            })
        }
        "input" | "textarea" | "button" => {
            let mut controls = Vec::new();
            collect_form_controls(dom, idx, &mut controls);
            (!controls.is_empty()).then(|| RenderBlock::Form {
                action: String::new(),
                method: "get".into(),
                enctype: "application/x-www-form-urlencoded".into(),
                controls,
                style,
            })
        }
        _ => {
            let children = build_children(
                dom,
                idx,
                &style,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
            );
            (!children.is_empty()).then(|| RenderBlock::Container { children, style })
        }
    }
}

fn collect_form_controls(dom: &Dom, idx: usize, out: &mut Vec<FormControl>) {
    if let NodeKind::Element(el) = &dom.nodes[idx].kind {
        match el.tag.as_str() {
            "input" => {
                let raw_type = el
                    .attrs
                    .get("type")
                    .map(|v| v.to_ascii_lowercase())
                    .unwrap_or_else(|| "text".into());
                let kind = match raw_type.as_str() {
                    "search" => FormControlKind::Search,
                    "email" => FormControlKind::Email,
                    "url" => FormControlKind::Url,
                    "password" => FormControlKind::Password,
                    "hidden" => FormControlKind::Hidden,
                    "checkbox" => FormControlKind::Checkbox,
                    "file" => FormControlKind::File,
                    "submit" => FormControlKind::Submit,
                    "button" => FormControlKind::Button,
                    "text" | "tel" | "number" => FormControlKind::Text,
                    _ => FormControlKind::Text,
                };
                out.push(FormControl {
                    node_id: idx,
                    kind,
                    name: el.attrs.get("name").cloned().unwrap_or_default(),
                    value: el.attrs.get("value").cloned().unwrap_or_default(),
                    placeholder: el.attrs.get("placeholder").cloned().unwrap_or_default(),
                    label: el
                        .attrs
                        .get("value")
                        .cloned()
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| "Submit".into()),
                    checked: el.attrs.contains_key("checked"),
                });
                return;
            }
            "textarea" => {
                out.push(FormControl {
                    node_id: idx,
                    kind: FormControlKind::Text,
                    name: el.attrs.get("name").cloned().unwrap_or_default(),
                    value: dom.text_content(idx),
                    placeholder: el.attrs.get("placeholder").cloned().unwrap_or_default(),
                    label: String::new(),
                    checked: false,
                });
                return;
            }
            "button" => {
                let is_submit = el
                    .attrs
                    .get("type")
                    .map(|v| !v.eq_ignore_ascii_case("button"))
                    .unwrap_or(true);
                out.push(FormControl {
                    node_id: idx,
                    kind: if is_submit {
                        FormControlKind::Submit
                    } else {
                        FormControlKind::Button
                    },
                    name: el.attrs.get("name").cloned().unwrap_or_default(),
                    value: el.attrs.get("value").cloned().unwrap_or_default(),
                    placeholder: String::new(),
                    label: normalize_text(&dom.text_content(idx)).trim().to_owned(),
                    checked: false,
                });
                return;
            }
            _ => {}
        }
    }
    for &child in &dom.nodes[idx].children {
        collect_form_controls(dom, child, out);
    }
}

fn image_source(dom: &Dom, idx: usize) -> Option<String> {
    let NodeKind::Element(el) = &dom.nodes[idx].kind else {
        return None;
    };

    // Static recovery for common lazy-loading libraries. These attributes contain
    // the real image URL even when the site's JavaScript/IntersectionObserver has
    // not run yet.
    for key in [
        "data-src",
        "data-original",
        "data-lazy-src",
        "data-original-src",
        "data-image-src",
    ] {
        if let Some(src) = nonempty_attr(&el.attrs, key) {
            return Some(src.to_owned());
        }
    }
    if let Some(set) = nonempty_attr(&el.attrs, "data-srcset") {
        if let Some(src) = best_srcset_candidate(set) {
            return Some(src);
        }
    }

    // Respect <picture> by selecting a source Veil can actually decode. AVIF and
    // other unsupported formats are deliberately skipped so the <img> fallback
    // remains usable.
    if let Some(parent) = dom.nodes[idx].parent {
        if let NodeKind::Element(parent_el) = &dom.nodes[parent].kind {
            if parent_el.tag == "picture" {
                for &sibling in &dom.nodes[parent].children {
                    let NodeKind::Element(source) = &dom.nodes[sibling].kind else {
                        continue;
                    };
                    if source.tag != "source" || !supported_picture_type(source.attrs.get("type")) {
                        continue;
                    }
                    if let Some(set) = nonempty_attr(&source.attrs, "srcset")
                        .or_else(|| nonempty_attr(&source.attrs, "data-srcset"))
                    {
                        if let Some(src) = best_srcset_candidate(set) {
                            return Some(src);
                        }
                    }
                    if let Some(src) = nonempty_attr(&source.attrs, "src") {
                        return Some(src.to_owned());
                    }
                }
            }
        }
    }

    if let Some(src) = nonempty_attr(&el.attrs, "src") {
        return Some(src.to_owned());
    }
    nonempty_attr(&el.attrs, "srcset").and_then(best_srcset_candidate)
}

fn nonempty_attr<'a>(
    attrs: &'a std::collections::HashMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    attrs
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

fn supported_picture_type(kind: Option<&String>) -> bool {
    let Some(kind) = kind.map(|value| value.trim().to_ascii_lowercase()) else {
        return true;
    };
    matches!(
        kind.as_str(),
        "image/webp"
            | "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
            | "image/bmp"
            | "image/svg+xml"
    )
}

fn best_srcset_candidate(set: &str) -> Option<String> {
    #[derive(Clone)]
    struct Candidate {
        url: String,
        width: Option<f32>,
        density: Option<f32>,
    }

    let mut candidates = Vec::new();
    for raw in set.split(',') {
        let mut parts = raw.split_whitespace();
        let Some(url) = parts.next().map(str::trim).filter(|url| !url.is_empty()) else {
            continue;
        };
        let descriptor = parts.next().unwrap_or_default();
        let (width, density) = if let Some(raw_width) = descriptor.strip_suffix('w') {
            (
                raw_width.parse::<f32>().ok().filter(|value| *value > 0.0),
                None,
            )
        } else if let Some(raw_density) = descriptor.strip_suffix('x') {
            (
                None,
                raw_density.parse::<f32>().ok().filter(|value| *value > 0.0),
            )
        } else {
            (None, Some(1.0))
        };
        candidates.push(Candidate {
            url: url.to_owned(),
            width,
            density,
        });
    }
    if candidates.is_empty() {
        return None;
    }

    // Gecko's ResponsiveImageSelector prefers the lowest density greater than
    // or equal to the display density, otherwise the greatest available below
    // it. Veil does not yet have the layout viewport inside Engine, so use a
    // conservative 1x / 1280 CSS-pixel target rather than always downloading
    // the largest candidate.
    if candidates.iter().any(|candidate| candidate.width.is_some()) {
        let target = 1280.0_f32;
        let mut above: Option<&Candidate> = None;
        let mut below: Option<&Candidate> = None;
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.width.is_some())
        {
            let width = candidate.width.unwrap();
            if width >= target {
                if above
                    .and_then(|current| current.width)
                    .map(|current| width < current)
                    .unwrap_or(true)
                {
                    above = Some(candidate);
                }
            } else if below
                .and_then(|current| current.width)
                .map(|current| width > current)
                .unwrap_or(true)
            {
                below = Some(candidate);
            }
        }
        return above.or(below).map(|candidate| candidate.url.clone());
    }

    let target = 1.0_f32;
    let mut above: Option<&Candidate> = None;
    let mut below: Option<&Candidate> = None;
    for candidate in &candidates {
        let density = candidate.density.unwrap_or(1.0);
        if density >= target {
            if above
                .and_then(|current| current.density)
                .map(|current| density < current)
                .unwrap_or(true)
            {
                above = Some(candidate);
            }
        } else if below
            .and_then(|current| current.density)
            .map(|current| density > current)
            .unwrap_or(true)
        {
            below = Some(candidate);
        }
    }
    above.or(below).map(|candidate| candidate.url.clone())
}

#[allow(clippy::too_many_arguments)]
fn collect_embedded_blocks(
    dom: &Dom,
    idx: usize,
    parent_style: &ComputedStyle,
    sheet: &StyleSheet,
    blocker: &Blocker,
    page_url: Option<&Url>,
    cosmetic_enabled: bool,
    recover_visibility: bool,
    hidden: &mut usize,
    out: &mut Vec<RenderBlock>,
) {
    for &child in &dom.nodes[idx].children {
        let NodeKind::Element(el) = &dom.nodes[child].kind else {
            continue;
        };
        if is_ignored_tag(&el.tag) {
            continue;
        }
        if is_block_tag(&el.tag) {
            if let Some(block) = build_block(
                dom,
                child,
                parent_style,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
            ) {
                out.push(block);
            }
        } else {
            let style = sheet.compute_node(dom, child, parent_style);
            if !style.display_none || recover_visibility {
                collect_embedded_blocks(
                    dom,
                    child,
                    &style,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    out,
                );
            }
        }
    }
}

fn media_source(dom: &Dom, idx: usize) -> Option<String> {
    if let NodeKind::Element(el) = &dom.nodes[idx].kind {
        if let Some(src) = el
            .attrs
            .get("src")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return Some(src.to_owned());
        }
    }
    for &child in &dom.nodes[idx].children {
        if let NodeKind::Element(el) = &dom.nodes[child].kind {
            if el.tag == "source" {
                if let Some(src) = el
                    .attrs
                    .get("src")
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    return Some(src.to_owned());
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn collect_runs(
    dom: &Dom,
    idx: usize,
    inherited: &ComputedStyle,
    inherited_href: Option<String>,
    sheet: &StyleSheet,
    blocker: &Blocker,
    page_url: Option<&Url>,
    cosmetic_enabled: bool,
    recover_visibility: bool,
    hidden: &mut usize,
    out: &mut Vec<TextRun>,
) {
    match &dom.nodes[idx].kind {
        NodeKind::Text(text) => {
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                out.push(TextRun {
                    text: normalized,
                    href: inherited_href,
                    size: inherited.font_size,
                    bold: inherited.bold,
                    italic: inherited.italic,
                    muted: inherited.muted,
                    color: inherited.color,
                    font_family: inherited.font_family,
                    font_name: inherited.font_name.clone(),
                });
            }
        }
        NodeKind::Element(el) => {
            if is_ignored_tag(&el.tag) || (is_block_tag(&el.tag) && el.tag != "br") {
                if !matches!(
                    el.tag.as_str(),
                    "p" | "li" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                ) {
                    return;
                }
            }
            if cosmetic_enabled {
                if let Some(url) = page_url {
                    if blocker.should_hide_element(url, &el.tag, &el.attrs) {
                        *hidden += 1;
                        return;
                    }
                }
            }
            let mut styled = sheet.compute_node(dom, idx, inherited);
            if recover_visibility {
                styled.display_none = false;
            }
            if styled.display_none {
                return;
            }
            if el.tag == "br" {
                out.push(TextRun {
                    text: "\n".into(),
                    href: inherited_href,
                    size: styled.font_size,
                    bold: styled.bold,
                    italic: styled.italic,
                    muted: styled.muted,
                    color: styled.color,
                    font_family: styled.font_family,
                    font_name: styled.font_name.clone(),
                });
                return;
            }
            let href = if el.tag == "a" {
                el.attrs.get("href").cloned().or(inherited_href)
            } else {
                inherited_href
            };
            for &child in &dom.nodes[idx].children {
                collect_runs(
                    dom,
                    child,
                    &styled,
                    href.clone(),
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    out,
                );
            }
        }
    }
}

fn build_compatibility_fallback(dom: &Dom, base: &ComputedStyle) -> Vec<RenderBlock> {
    let mut runs = Vec::new();
    collect_fallback_runs(dom, dom.root, None, base, &mut runs);
    compact_runs(&mut runs);
    if runs.is_empty() {
        Vec::new()
    } else {
        vec![RenderBlock::Paragraph {
            runs,
            style: base.clone(),
        }]
    }
}

fn collect_fallback_runs(
    dom: &Dom,
    idx: usize,
    inherited_href: Option<String>,
    base: &ComputedStyle,
    out: &mut Vec<TextRun>,
) {
    match &dom.nodes[idx].kind {
        NodeKind::Text(text) => {
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                out.push(TextRun {
                    text: normalized,
                    href: inherited_href,
                    size: base.font_size,
                    bold: false,
                    italic: false,
                    muted: false,
                    color: base.color,
                    font_family: base.font_family,
                    font_name: base.font_name.clone(),
                });
            }
        }
        NodeKind::Element(el) => {
            if matches!(
                el.tag.as_str(),
                "script" | "style" | "noscript" | "svg" | "head" | "template"
            ) {
                return;
            }
            let href = if el.tag == "a" {
                el.attrs.get("href").cloned().or(inherited_href)
            } else {
                inherited_href
            };
            if el.tag == "img" {
                if let Some(alt) = el.attrs.get("alt") {
                    let alt = normalize_text(alt);
                    if !alt.is_empty() {
                        out.push(TextRun {
                            text: alt,
                            href: href.clone(),
                            size: base.font_size,
                            bold: false,
                            italic: true,
                            muted: true,
                            color: base.color,
                            font_family: base.font_family,
                            font_name: base.font_name.clone(),
                        });
                    }
                }
            }
            for &child in &dom.nodes[idx].children {
                collect_fallback_runs(dom, child, href.clone(), base, out);
            }
            if is_block_tag(&el.tag) && !out.is_empty() {
                out.push(TextRun {
                    text: "\n".into(),
                    href: None,
                    size: base.font_size,
                    bold: false,
                    italic: false,
                    muted: false,
                    color: base.color,
                    font_family: base.font_family,
                    font_name: base.font_name.clone(),
                });
            }
        }
    }
}

fn flush_pending(runs: &mut Vec<TextRun>, style: &ComputedStyle, blocks: &mut Vec<RenderBlock>) {
    compact_runs(runs);
    if !runs.is_empty() {
        blocks.push(RenderBlock::Paragraph {
            runs: std::mem::take(runs),
            style: style.clone(),
        });
    }
}

fn compact_runs(runs: &mut Vec<TextRun>) {
    runs.retain(|run| !run.text.is_empty());
    for index in 0..runs.len().saturating_sub(1) {
        let current_ends_ws = runs[index]
            .text
            .chars()
            .last()
            .map(char::is_whitespace)
            .unwrap_or(false);
        let next_starts_ws = runs[index + 1]
            .text
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false);
        if !current_ends_ws && !next_starts_ws {
            runs[index].text.push(' ');
        }
    }
}

fn normalize_text(text: &str) -> String {
    let had_leading = text
        .chars()
        .next()
        .map(char::is_whitespace)
        .unwrap_or(false);
    let had_trailing = text
        .chars()
        .last()
        .map(char::is_whitespace)
        .unwrap_or(false);
    let core = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if core.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if had_leading {
        out.push(' ');
    }
    out.push_str(&core);
    if had_trailing {
        out.push(' ');
    }
    out
}

fn is_block_tag(tag: &str) -> bool {
    matches!(
        tag,
        "html"
            | "body"
            | "main"
            | "article"
            | "section"
            | "div"
            | "nav"
            | "header"
            | "footer"
            | "aside"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "pre"
            | "hr"
            | "img"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "figure"
            | "figcaption"
            | "picture"
            | "form"
            | "input"
            | "textarea"
            | "button"
            | "details"
            | "summary"
            | "canvas"
            | "video"
            | "audio"
    )
}

fn is_ignored_tag(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "noscript" | "svg" | "head" | "template"
    )
}

fn parse_dimension(value: &str) -> Option<f32> {
    value
        .trim()
        .trim_end_matches("px")
        .parse::<f32>()
        .ok()
        .map(|v| v.clamp(1.0, 4096.0))
}

fn find_title(dom: &Dom) -> Option<String> {
    for (idx, node) in dom.nodes.iter().enumerate() {
        if let NodeKind::Element(el) = &node.kind {
            if el.tag == "title" {
                let text = dom
                    .text_content(idx)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn run(text: &str, style: &ComputedStyle, href: Option<String>) -> TextRun {
    TextRun {
        text: text.to_owned(),
        href,
        size: style.font_size,
        bold: style.bold,
        italic: style.italic,
        muted: style.muted,
        color: style.color,
        font_family: style.font_family,
        font_name: style.font_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_pass_restores_hidden_app_shell_before_text_fallback() {
        let engine = Engine::default();
        let blocker = Blocker::default();
        let view = engine.parse(
            "https://example.com/",
            "<html><head><style>body{display:none}.card{display:flex;padding:12px}</style></head><body><div class='card'><a href='/result'>Visible result</a></div></body></html>",
            &blocker,
            SitePrivacy::default(),
        );
        let rendered_text = format!("{:?}", view.blocks);
        assert!(!rendered_text.contains("Compatibility view"));
        assert!(rendered_text.contains("Visible result"));
        assert!(rendered_text.contains("FlexRow"));
    }

    #[test]
    fn discovers_linked_css_and_scripts() {
        let engine = Engine::default();
        let base = Url::parse("https://example.com/path/").unwrap();
        let html = "<link rel='stylesheet' href='/app.css'><script src='app.js'></script>";
        assert_eq!(
            engine.discover_stylesheets(html, &base)[0].as_str(),
            "https://example.com/app.css"
        );
        assert_eq!(
            engine.discover_external_scripts(html, &base)[0].as_str(),
            "https://example.com/path/app.js"
        );
    }

    #[test]
    fn discovers_font_face_resources() {
        let engine = Engine::default();
        let base = Url::parse("https://example.com/css/app.css").unwrap();
        let fonts = engine.discover_web_fonts(
            "@font-face{font-family:'Veil Sans';src:url('../fonts/vv.ttf') format('truetype')}",
            &base,
        );
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].family, "Veil Sans");
        assert_eq!(fonts[0].url.as_str(), "https://example.com/fonts/vv.ttf");
    }

    #[test]
    fn renders_images_inside_inline_links() {
        let engine = Engine::default();
        let blocker = Blocker::default();
        let view = engine.parse(
            "https://example.com/",
            "<div><a href='/story'><img src='/photo.jpg' alt='Photo'></a></div>",
            &blocker,
            SitePrivacy::default(),
        );
        let rendered = format!("{:?}", view.blocks);
        assert!(rendered.contains("Image"));
        assert!(rendered.contains("/photo.jpg"));
    }

    #[test]
    fn resolves_lazy_and_picture_images() {
        let engine = Engine::default();
        let blocker = Blocker::default();
        let lazy = engine.parse(
            "https://example.com/",
            "<img src='placeholder.gif' data-src='/real-photo.jpg'>",
            &blocker,
            SitePrivacy::default(),
        );
        assert!(format!("{:?}", lazy.blocks).contains("/real-photo.jpg"));

        let picture = engine.parse(
            "https://example.com/",
            "<picture><source type='image/avif' srcset='/photo.avif 1200w'><source type='image/webp' srcset='/small.webp 320w, /large.webp 1280w'><img src='/fallback.jpg'></picture>",
            &blocker,
            SitePrivacy::default(),
        );
        let rendered = format!("{:?}", picture.blocks);
        assert!(rendered.contains("/large.webp"));
        assert!(!rendered.contains("/photo.avif"));
    }

    #[test]
    fn srcset_prefers_one_x_instead_of_largest_density() {
        assert_eq!(
            best_srcset_candidate("small.jpg 1x, medium.jpg 2x, huge.jpg 3x").as_deref(),
            Some("small.jpg")
        );
    }

    #[test]
    fn srcset_prefers_reasonable_width_instead_of_largest_asset() {
        assert_eq!(
            best_srcset_candidate("a.jpg 320w, b.jpg 640w, c.jpg 1280w, d.jpg 4096w").as_deref(),
            Some("c.jpg")
        );
    }

    #[test]
    fn parses_simple_get_form() {
        let engine = Engine::default();
        let blocker = Blocker::default();
        let view = engine.parse(
            "https://example.com/",
            "<form action='/search'><input name='q' placeholder='Search'><button>Go</button></form>",
            &blocker,
            SitePrivacy::default(),
        );
        assert!(matches!(
            view.blocks.first(),
            Some(RenderBlock::Container { .. }) | Some(RenderBlock::Form { .. })
        ));
        assert!(format!("{:?}", view.blocks).contains("Search"));
    }
}

#[cfg(test)]
mod retained_render_tests {
    use super::*;

    #[test]
    fn retained_document_ignores_noop_runtime_snapshots() {
        let report = ScriptReport::default();
        let mut retained = RetainedDocument::new(
            "https://example.com/",
            "<html><body><p id='x'>Hello</p></body></html>",
            SitePrivacy::default(),
            &[],
            0,
            &report,
        );
        assert_eq!(retained.update_from_report(&report), InvalidationKind::None);
    }

    #[test]
    fn retained_document_marks_dom_mutations_as_layout_damage() {
        let mut report = ScriptReport::default();
        let mut retained = RetainedDocument::new(
            "https://example.com/",
            "<html><body><p id='x'>Hello</p></body></html>",
            SitePrivacy::default(),
            &[],
            0,
            &report,
        );
        report.dom_mutations.push(DomMutation {
            target_id: "x".into(),
            kind: "text".into(),
            value: "Updated".into(),
        });
        assert_eq!(
            retained.update_from_report(&report),
            InvalidationKind::Layout
        );
        let view = retained.render(&Blocker::default(), &report);
        assert!(format!("{:?}", view.blocks).contains("Updated"));
    }
}
