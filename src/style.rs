use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dom::{Dom, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeSizes {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Default for EdgeSizes {
    fn default() -> Self {
        Self {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutMode {
    Block,
    FlexRow,
    FlexColumn,
    Grid,
}
impl Default for LayoutMode {
    fn default() -> Self {
        Self::Block
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontKind {
    Sans,
    Serif,
    Mono,
}
impl Default for FontKind {
    fn default() -> Self {
        Self::Sans
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JustifyContent {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}
impl Default for JustifyContent {
    fn default() -> Self {
        Self::Start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlignItems {
    Start,
    Center,
    End,
    Stretch,
}
impl Default for AlignItems {
    fn default() -> Self {
        Self::Stretch
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedStyle {
    pub font_size: f32,
    pub font_family: FontKind,
    pub font_name: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub muted: bool,
    pub display_none: bool,
    pub layout: LayoutMode,
    pub gap: f32,
    pub grid_columns: usize,
    pub flex_wrap: bool,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Option<f32>,
    pub order: i32,
    pub grid_column_span: usize,
    pub custom_properties: HashMap<String, String>,
    pub margin: EdgeSizes,
    pub padding: EdgeSizes,
    pub width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_width: Option<f32>,
    pub height: Option<f32>,
    pub max_height: Option<f32>,
    pub min_height: Option<f32>,
    pub color: Option<[u8; 4]>,
    pub background: Option<[u8; 4]>,
    pub border_width: f32,
    pub border_radius: f32,
    pub text_align: TextAlign,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            font_family: FontKind::Sans,
            font_name: None,
            bold: false,
            italic: false,
            muted: false,
            display_none: false,
            layout: LayoutMode::Block,
            gap: 0.0,
            grid_columns: 1,
            flex_wrap: false,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: None,
            order: 0,
            grid_column_span: 1,
            custom_properties: HashMap::new(),
            margin: EdgeSizes::default(),
            padding: EdgeSizes::default(),
            width: None,
            max_width: None,
            min_width: None,
            height: None,
            max_height: None,
            min_height: None,
            color: None,
            background: None,
            border_width: 0.0,
            border_radius: 0.0,
            text_align: TextAlign::Left,
        }
    }
}

impl ComputedStyle {
    pub fn inherited_text(&self) -> Self {
        Self {
            font_size: self.font_size,
            font_family: self.font_family,
            font_name: self.font_name.clone(),
            bold: self.bold,
            italic: self.italic,
            muted: self.muted,
            color: self.color,
            text_align: self.text_align,
            custom_properties: self.custom_properties.clone(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    rules: Vec<CssRule>,
}

#[derive(Debug, Clone)]
struct CssRule {
    selectors: Vec<CssSelector>,
    declarations: Vec<(String, String)>,
    order: usize,
}
#[derive(Debug, Clone, Default)]
struct CssSelector {
    parts: Vec<SimpleSelector>,
}
#[derive(Debug, Clone, Default)]
struct SimpleSelector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
}

impl StyleSheet {
    pub fn from_dom(dom: &Dom) -> Self {
        let mut sheet = Self::default();
        for (idx, node) in dom.nodes.iter().enumerate() {
            let NodeKind::Element(el) = &node.kind else {
                continue;
            };
            if el.tag == "style" {
                sheet.parse_and_append(&dom.text_content(idx));
            }
        }
        sheet
    }

    pub fn parse_and_append(&mut self, css: &str) {
        let cleaned = strip_css_comments(css);
        for chunk in cleaned.split('}') {
            let Some((selector_text, declarations_text)) = chunk.split_once('{') else {
                continue;
            };
            if selector_text.trim().starts_with('@') {
                continue;
            }
            let selectors: Vec<CssSelector> = selector_text
                .split(',')
                .filter_map(parse_css_selector)
                .collect();
            if selectors.is_empty() {
                continue;
            }
            let declarations = parse_declarations(declarations_text);
            if declarations.is_empty() {
                continue;
            }
            let order = self.rules.len();
            self.rules.push(CssRule {
                selectors,
                declarations,
                order,
            });
        }
    }

    pub fn compute_node(&self, dom: &Dom, idx: usize, parent: &ComputedStyle) -> ComputedStyle {
        let NodeKind::Element(el) = &dom.nodes[idx].kind else {
            return parent.inherited_text();
        };
        let mut style = parent.inherited_text();
        apply_tag_defaults(&el.tag, &mut style);

        let mut matched: Vec<(u32, usize, &[(String, String)])> = Vec::new();
        for rule in &self.rules {
            let mut best_specificity = None;
            for selector in &rule.selectors {
                if selector_matches_dom(selector, dom, idx) {
                    let spec = selector_specificity(selector);
                    best_specificity =
                        Some(best_specificity.map_or(spec, |old: u32| old.max(spec)));
                }
            }
            if let Some(spec) = best_specificity {
                matched.push((spec, rule.order, &rule.declarations));
            }
        }
        matched.sort_by_key(|(spec, order, _)| (*spec, *order));
        for (_, _, declarations) in matched {
            for (name, value) in declarations {
                apply_declaration(&mut style, name, value);
            }
        }
        if let Some(inline) = el.attrs.get("style") {
            for (name, value) in parse_declarations(inline) {
                apply_declaration(&mut style, &name, &value);
            }
        }
        style
    }
}

fn apply_tag_defaults(tag: &str, style: &mut ComputedStyle) {
    match tag {
        "h1" => {
            style.font_size = 34.0;
            style.bold = true;
            style.margin.top = 18.0;
            style.margin.bottom = 10.0;
        }
        "h2" => {
            style.font_size = 28.0;
            style.bold = true;
            style.margin.top = 16.0;
            style.margin.bottom = 9.0;
        }
        "h3" => {
            style.font_size = 23.0;
            style.bold = true;
            style.margin.top = 14.0;
            style.margin.bottom = 8.0;
        }
        "h4" | "h5" | "h6" => {
            style.font_size = 19.0;
            style.bold = true;
            style.margin.top = 12.0;
            style.margin.bottom = 7.0;
        }
        "p" => {
            style.margin.top = 4.0;
            style.margin.bottom = 12.0;
        }
        "blockquote" => {
            style.margin.left = 22.0;
            style.margin.top = 8.0;
            style.margin.bottom = 12.0;
            style.padding.left = 12.0;
            style.muted = true;
        }
        "pre" => {
            style.font_size = 14.0;
            style.font_family = FontKind::Mono;
            style.margin.top = 8.0;
            style.margin.bottom = 12.0;
            style.padding = EdgeSizes {
                top: 10.0,
                right: 10.0,
                bottom: 10.0,
                left: 10.0,
            };
        }
        "small" => style.font_size = (style.font_size * 0.82).max(9.0),
        "strong" | "b" => style.bold = true,
        "em" | "i" => style.italic = true,
        "code" => {
            style.font_size = (style.font_size * 0.9).max(10.0);
            style.font_family = FontKind::Mono;
        }
        "img" | "canvas" | "video" | "audio" => {
            style.margin.top = 6.0;
            style.margin.bottom = 8.0;
        }
        "form" => {
            style.margin.top = 6.0;
            style.margin.bottom = 10.0;
            style.gap = 6.0;
        }
        "div" | "section" | "article" | "main" | "nav" | "header" | "footer" => {
            style.margin.bottom = 4.0
        }
        _ => {}
    }
}

fn apply_declaration(style: &mut ComputedStyle, name: &str, value: &str) {
    let name = name.trim().to_ascii_lowercase();
    let raw_value = value.trim();
    if name.starts_with("--") {
        style.custom_properties.insert(name, raw_value.to_owned());
        return;
    }
    let resolved = resolve_css_var(raw_value, &style.custom_properties);
    let value = resolved.trim().to_ascii_lowercase();
    match name.as_str() {
        "display" if value == "none" => style.display_none = true,
        "display" if value == "flex" || value == "inline-flex" => {
            style.display_none = false;
            style.layout = LayoutMode::FlexRow;
        }
        "display" if value == "grid" || value == "inline-grid" => {
            style.display_none = false;
            style.layout = LayoutMode::Grid;
        }
        "display" => style.display_none = false,
        "flex-direction" if value.starts_with("column") => style.layout = LayoutMode::FlexColumn,
        "flex-direction" if value.starts_with("row") => style.layout = LayoutMode::FlexRow,
        "flex-wrap" => style.flex_wrap = value != "nowrap",
        "justify-content" => {
            style.justify_content = match value.as_str() {
                "center" => JustifyContent::Center,
                "end" | "flex-end" => JustifyContent::End,
                "space-between" => JustifyContent::SpaceBetween,
                "space-around" | "space-evenly" => JustifyContent::SpaceAround,
                _ => JustifyContent::Start,
            }
        }
        "align-items" => {
            style.align_items = match value.as_str() {
                "center" => AlignItems::Center,
                "end" | "flex-end" => AlignItems::End,
                "start" | "flex-start" => AlignItems::Start,
                _ => AlignItems::Stretch,
            }
        }
        "flex-grow" => style.flex_grow = value.parse::<f32>().unwrap_or(0.0).clamp(0.0, 64.0),
        "flex-shrink" => style.flex_shrink = value.parse::<f32>().unwrap_or(1.0).clamp(0.0, 64.0),
        "flex-basis" => {
            style.flex_basis = parse_length(&value, style.font_size);
            if style.width.is_none() {
                style.width = style.flex_basis;
            }
        }
        "flex" => apply_flex_shorthand(style, &value),
        "order" => style.order = value.parse::<i32>().unwrap_or(0).clamp(-128, 128),
        "grid-template-columns" => style.grid_columns = parse_grid_columns(&value).clamp(1, 12),
        "grid-column" | "grid-column-end" => {
            style.grid_column_span = parse_grid_span(&value).clamp(1, 12)
        }
        "gap" | "column-gap" | "row-gap" => {
            if let Some(v) = parse_length(&value, style.font_size) {
                style.gap = v.clamp(0.0, 96.0);
            }
        }
        "visibility" if value == "hidden" || value == "collapse" => style.display_none = true,
        "font-weight" => {
            style.bold = value == "bold"
                || value == "bolder"
                || value.parse::<u32>().map(|n| n >= 600).unwrap_or(false)
        }
        "font-style" => style.italic = value == "italic" || value == "oblique",
        "font-family" => {
            style.font_family = parse_font_family(&value);
            style.font_name = parse_font_name(resolved.trim());
        }
        "font-size" => {
            if let Some(size) = parse_length(&value, style.font_size) {
                style.font_size = size.clamp(8.0, 144.0);
            }
        }
        "opacity" => {
            if value.parse::<f32>().unwrap_or(1.0) < 0.55 {
                style.muted = true;
            }
        }
        "color" => style.color = parse_color(&value),
        "background" | "background-color" => style.background = parse_color(&value),
        "width" => style.width = parse_length(&value, style.font_size),
        "min-width" => style.min_width = parse_length(&value, style.font_size),
        "max-width" => style.max_width = parse_length(&value, style.font_size),
        "height" => style.height = parse_length(&value, style.font_size),
        "min-height" => style.min_height = parse_length(&value, style.font_size),
        "max-height" => style.max_height = parse_length(&value, style.font_size),
        "margin" => {
            if let Some(edges) = parse_edge_shorthand(&value, style.font_size) {
                style.margin = edges;
            }
        }
        "padding" => {
            if let Some(edges) = parse_edge_shorthand(&value, style.font_size) {
                style.padding = edges;
            }
        }
        "margin-top" => set_edge(&mut style.margin.top, &value, style.font_size),
        "margin-right" => set_edge(&mut style.margin.right, &value, style.font_size),
        "margin-bottom" => set_edge(&mut style.margin.bottom, &value, style.font_size),
        "margin-left" => set_edge(&mut style.margin.left, &value, style.font_size),
        "padding-top" => set_edge(&mut style.padding.top, &value, style.font_size),
        "padding-right" => set_edge(&mut style.padding.right, &value, style.font_size),
        "padding-bottom" => set_edge(&mut style.padding.bottom, &value, style.font_size),
        "padding-left" => set_edge(&mut style.padding.left, &value, style.font_size),
        "border-width" => {
            if let Some(width) = parse_length(&value, style.font_size) {
                style.border_width = width.clamp(0.0, 12.0);
            }
        }
        "border-radius" => {
            if let Some(radius) = parse_length(
                value.split_whitespace().next().unwrap_or("0"),
                style.font_size,
            ) {
                style.border_radius = radius.clamp(0.0, 255.0);
            }
        }
        "border" => {
            if let Some(first) = value.split_whitespace().next() {
                if let Some(width) = parse_length(first, style.font_size) {
                    style.border_width = width.clamp(0.0, 12.0);
                }
            }
        }
        "text-align" => {
            style.text_align = match value.as_str() {
                "center" => TextAlign::Center,
                "right" | "end" => TextAlign::Right,
                _ => TextAlign::Left,
            }
        }
        _ => {}
    }
}

fn resolve_css_var(value: &str, vars: &HashMap<String, String>) -> String {
    let Some(start) = value.find("var(") else {
        return value.to_owned();
    };
    let tail = &value[start + 4..];
    let Some(end) = tail.find(')') else {
        return value.to_owned();
    };
    let inside = &tail[..end];
    let mut pieces = inside.splitn(2, ',');
    let name = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
    let fallback = pieces.next().unwrap_or("").trim();
    let replacement = vars.get(&name).map(String::as_str).unwrap_or(fallback);
    format!("{}{}{}", &value[..start], replacement, &tail[end + 1..])
}

fn apply_flex_shorthand(style: &mut ComputedStyle, value: &str) {
    if value == "none" {
        style.flex_grow = 0.0;
        style.flex_shrink = 0.0;
        style.flex_basis = None;
        return;
    }
    if value == "auto" {
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        return;
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    if let Some(grow) = parts.first().and_then(|part| part.parse::<f32>().ok()) {
        style.flex_grow = grow.clamp(0.0, 64.0);
    }
    if let Some(shrink) = parts.get(1).and_then(|part| part.parse::<f32>().ok()) {
        style.flex_shrink = shrink.clamp(0.0, 64.0);
    }
    if let Some(basis) = parts
        .get(2)
        .and_then(|part| parse_length(part, style.font_size))
    {
        style.flex_basis = Some(basis);
        if style.width.is_none() {
            style.width = Some(basis);
        }
    }
}

fn parse_grid_span(value: &str) -> usize {
    let lower = value.to_ascii_lowercase();
    if let Some(pos) = lower.find("span") {
        return lower[pos + 4..]
            .trim()
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1);
    }
    1
}

fn parse_font_family(value: &str) -> FontKind {
    let lower = value.to_ascii_lowercase();
    if lower.contains("mono") || lower.contains("consolas") || lower.contains("courier") {
        FontKind::Mono
    } else if lower.contains("serif") || lower.contains("times") || lower.contains("georgia") {
        FontKind::Serif
    } else {
        FontKind::Sans
    }
}

fn parse_font_name(value: &str) -> Option<String> {
    let first = value
        .split(',')
        .next()?
        .trim()
        .trim_matches('\"')
        .trim_matches('\'');
    if first.is_empty() {
        return None;
    }
    let lower = first.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "serif" | "sans-serif" | "monospace" | "system-ui" | "cursive" | "fantasy"
    ) {
        None
    } else {
        Some(first.to_owned())
    }
}

fn parse_grid_columns(value: &str) -> usize {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix("repeat(") {
        if let Some((count, tail)) = rest.split_once(',') {
            if let Ok(n) = count.trim().parse::<usize>() {
                return n;
            }
            // auto-fit/auto-fill cannot be resolved without a concrete viewport;
            // choose a useful responsive default and let the UI clamp it.
            if matches!(count.trim(), "auto-fit" | "auto-fill") {
                if tail.contains("minmax") {
                    return 3;
                }
                return 2;
            }
        }
    }
    let mut depth = 0i32;
    let mut tokens = 0usize;
    let mut in_token = false;
    for ch in v.chars() {
        match ch {
            '(' => {
                depth += 1;
                in_token = true;
            }
            ')' => {
                depth = (depth - 1).max(0);
            }
            c if c.is_whitespace() && depth == 0 => {
                if in_token {
                    tokens += 1;
                    in_token = false;
                }
            }
            _ => in_token = true,
        }
    }
    if in_token {
        tokens += 1;
    }
    tokens.max(1)
}

fn parse_declarations(input: &str) -> Vec<(String, String)> {
    input
        .split(';')
        .filter_map(|d| {
            let (name, value) = d.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                None
            } else {
                Some((
                    name.to_owned(),
                    value.trim_end_matches("!important").trim().to_owned(),
                ))
            }
        })
        .collect()
}

fn parse_css_selector(raw: &str) -> Option<CssSelector> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('+') || raw.contains('~') || raw.contains(':') {
        return None;
    }
    // 0.5 treats the child combinator as a stricter-looking descendant
    // combinator. The matcher remains conservative but accepts far more
    // production stylesheets than earlier releases.
    let normalized = raw.replace('>', " ");
    let parts: Vec<SimpleSelector> = normalized
        .split_whitespace()
        .filter_map(parse_simple_selector)
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(CssSelector { parts })
    }
}

fn parse_simple_selector(raw: &str) -> Option<SimpleSelector> {
    let mut selector = SimpleSelector::default();
    let bytes = raw.as_bytes();
    let mut cursor = 0usize;
    if cursor < bytes.len() && (bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'*') {
        let start = cursor;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'-' | b'*'))
        {
            cursor += 1;
        }
        if &raw[start..cursor] != "*" {
            selector.tag = Some(raw[start..cursor].to_ascii_lowercase());
        }
    }
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'#' => {
                cursor += 1;
                let start = cursor;
                while cursor < bytes.len() && is_ident(bytes[cursor]) {
                    cursor += 1;
                }
                if start == cursor {
                    return None;
                }
                selector.id = Some(raw[start..cursor].to_owned());
            }
            b'.' => {
                cursor += 1;
                let start = cursor;
                while cursor < bytes.len() && is_ident(bytes[cursor]) {
                    cursor += 1;
                }
                if start == cursor {
                    return None;
                }
                selector.classes.push(raw[start..cursor].to_owned());
            }
            b'[' => {
                let rest = &raw[cursor + 1..];
                let close = rest.find(']')?;
                let inner = rest[..close].trim();
                if let Some((name, value)) = inner.split_once('=') {
                    let value = value.trim().trim_matches('"').trim_matches('\'').to_owned();
                    selector
                        .attrs
                        .push((name.trim().to_ascii_lowercase(), Some(value)));
                } else if !inner.is_empty() {
                    selector.attrs.push((inner.to_ascii_lowercase(), None));
                }
                cursor += close + 2;
            }
            _ => return None,
        }
    }
    Some(selector)
}

fn selector_matches_dom(selector: &CssSelector, dom: &Dom, idx: usize) -> bool {
    let Some(last) = selector.parts.last() else {
        return false;
    };
    if !simple_matches_node(last, dom, idx) {
        return false;
    }
    let mut ancestor = dom.nodes[idx].parent;
    for part in selector.parts[..selector.parts.len() - 1].iter().rev() {
        let mut found = None;
        while let Some(a) = ancestor {
            if simple_matches_node(part, dom, a) {
                found = Some(a);
                ancestor = dom.nodes[a].parent;
                break;
            }
            ancestor = dom.nodes[a].parent;
        }
        if found.is_none() {
            return false;
        }
    }
    true
}

fn simple_matches_node(selector: &SimpleSelector, dom: &Dom, idx: usize) -> bool {
    let NodeKind::Element(el) = &dom.nodes[idx].kind else {
        return false;
    };
    if let Some(expected) = &selector.tag {
        if !el.tag.eq_ignore_ascii_case(expected) {
            return false;
        }
    }
    if let Some(expected) = &selector.id {
        if el.attrs.get("id") != Some(expected) {
            return false;
        }
    }
    if !selector.classes.is_empty() {
        let classes: Vec<&str> = el
            .attrs
            .get("class")
            .map(|v| v.split_whitespace().collect())
            .unwrap_or_default();
        if selector
            .classes
            .iter()
            .any(|expected| !classes.iter().any(|actual| *actual == expected))
        {
            return false;
        }
    }
    for (name, expected) in &selector.attrs {
        let Some(actual) = el.attrs.get(name) else {
            return false;
        };
        if let Some(expected) = expected {
            if actual != expected {
                return false;
            }
        }
    }
    true
}

fn selector_specificity(selector: &CssSelector) -> u32 {
    selector
        .parts
        .iter()
        .map(|p| {
            (p.id.is_some() as u32) * 100
                + (p.classes.len() + p.attrs.len()) as u32 * 10
                + p.tag.is_some() as u32
        })
        .sum()
}

fn parse_edge_shorthand(value: &str, em: f32) -> Option<EdgeSizes> {
    let values: Vec<f32> = value
        .split_whitespace()
        .filter_map(|p| parse_length(p, em))
        .collect();
    match values.as_slice() {
        [all] => Some(EdgeSizes {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [v, h] => Some(EdgeSizes {
            top: *v,
            right: *h,
            bottom: *v,
            left: *h,
        }),
        [t, h, b] => Some(EdgeSizes {
            top: *t,
            right: *h,
            bottom: *b,
            left: *h,
        }),
        [t, r, b, l] => Some(EdgeSizes {
            top: *t,
            right: *r,
            bottom: *b,
            left: *l,
        }),
        _ => None,
    }
}

fn set_edge(edge: &mut f32, value: &str, em: f32) {
    if let Some(v) = parse_length(value, em) {
        *edge = v;
    }
}

fn parse_length(value: &str, em: f32) -> Option<f32> {
    let value = value.trim();
    if value == "0" || value == "auto" {
        return Some(0.0);
    }
    if let Some(px) = value.strip_suffix("px") {
        return px.trim().parse().ok();
    }
    if let Some(rem) = value.strip_suffix("rem") {
        return rem.trim().parse::<f32>().ok().map(|n| n * 16.0);
    }
    if let Some(rel) = value.strip_suffix("em") {
        return rel.trim().parse::<f32>().ok().map(|n| n * em);
    }
    value.parse().ok()
}

pub fn parse_color(value: &str) -> Option<[u8; 4]> {
    match value.trim().to_ascii_lowercase().as_str() {
        "black" => Some([0, 0, 0, 255]),
        "white" => Some([255, 255, 255, 255]),
        "gray" | "grey" => Some([128, 128, 128, 255]),
        "red" => Some([255, 0, 0, 255]),
        "green" => Some([0, 128, 0, 255]),
        "blue" => Some([0, 0, 255, 255]),
        "transparent" => Some([0, 0, 0, 0]),
        other => {
            let hex = other.strip_prefix('#')?;
            match hex.len() {
                3 => Some([
                    u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?,
                    u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?,
                    u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?,
                    255,
                ]),
                6 => Some([
                    u8::from_str_radix(&hex[0..2], 16).ok()?,
                    u8::from_str_radix(&hex[2..4], 16).ok()?,
                    u8::from_str_radix(&hex[4..6], 16).ok()?,
                    255,
                ]),
                8 => Some([
                    u8::from_str_radix(&hex[0..2], 16).ok()?,
                    u8::from_str_radix(&hex[2..4], 16).ok()?,
                    u8::from_str_radix(&hex[4..6], 16).ok()?,
                    u8::from_str_radix(&hex[6..8], 16).ok()?,
                ]),
                _ => None,
            }
        }
    }
}

fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    loop {
        let Some(start) = rest.find("/*") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("*/") else {
            break;
        };
        rest = &after[end + 2..];
    }
    out
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_prefers_specific_descendant_selector() {
        let dom = Dom::parse("<main class='page'><p id='hero' class='copy'>Hello</p></main>");
        let p = dom
            .nodes
            .iter()
            .enumerate()
            .find_map(|(i, n)| match &n.kind {
                NodeKind::Element(el) if el.tag == "p" => Some(i),
                _ => None,
            })
            .unwrap();
        let mut sheet = StyleSheet::default();
        sheet.parse_and_append("p{font-size:12px}.page .copy{font-size:18px}#hero{font-size:22px}");
        let style = sheet.compute_node(&dom, p, &ComputedStyle::default());
        assert_eq!(style.font_size, 22.0);
    }

    #[test]
    fn variables_attributes_and_flex_shorthand_work() {
        let dom = Dom::parse("<div id='x' data-kind='hero' style='--size:21px;font-size:var(--size);flex:1 0 120px'></div>");
        let idx = dom.find_element_by_id("x").unwrap();
        let mut sheet = StyleSheet::default();
        sheet.parse_and_append("[data-kind=hero]{font-weight:700}");
        let style = sheet.compute_node(&dom, idx, &ComputedStyle::default());
        assert_eq!(style.font_size, 21.0);
        assert!(style.bold);
        assert_eq!(style.flex_grow, 1.0);
        assert_eq!(style.flex_basis, Some(120.0));
    }

    #[test]
    fn flex_and_grid_layouts_are_recognized() {
        let dom = Dom::parse("<div id='f' style='display:flex;flex-direction:column;gap:8px'>x</div><div id='g' style='display:grid;grid-template-columns:repeat(3,1fr)'>y</div>");
        let f = dom
            .nodes
            .iter()
            .enumerate()
            .find_map(|(i, n)| match &n.kind {
                NodeKind::Element(el) if el.attrs.get("id").map(String::as_str) == Some("f") => {
                    Some(i)
                }
                _ => None,
            })
            .unwrap();
        let g = dom
            .nodes
            .iter()
            .enumerate()
            .find_map(|(i, n)| match &n.kind {
                NodeKind::Element(el) if el.attrs.get("id").map(String::as_str) == Some("g") => {
                    Some(i)
                }
                _ => None,
            })
            .unwrap();
        let sheet = StyleSheet::default();
        let fs = sheet.compute_node(&dom, f, &ComputedStyle::default());
        let gs = sheet.compute_node(&dom, g, &ComputedStyle::default());
        assert_eq!(fs.layout, LayoutMode::FlexColumn);
        assert_eq!(fs.gap, 8.0);
        assert_eq!(gs.layout, LayoutMode::Grid);
        assert_eq!(gs.grid_columns, 3);
    }
}
