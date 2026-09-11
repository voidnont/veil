use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum NodeKind {
    Element(ElementData),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct ElementData {
    pub tag: String,
    pub attrs: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Dom {
    pub nodes: Vec<Node>,
    pub root: usize,
}

impl Dom {
    pub fn parse(html: &str) -> Self {
        let mut nodes = vec![Node {
            kind: NodeKind::Element(ElementData { tag: "document".into(), attrs: HashMap::new() }),
            children: Vec::new(),
            parent: None,
        }];
        let root = 0;
        let mut stack = vec![root];
        let bytes = html.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            if bytes[i] == b'<' {
                if html[i..].starts_with("<!--") {
                    if let Some(end) = html[i + 4..].find("-->") { i += 4 + end + 3; } else { break; }
                    continue;
                }
                if html[i..].starts_with("<!") {
                    if let Some(end) = html[i..].find('>') { i += end + 1; } else { break; }
                    continue;
                }
                let Some(end_rel) = html[i..].find('>') else { break; };
                let end = i + end_rel;
                let inside = html[i + 1..end].trim();
                if inside.starts_with('/') {
                    let closing = inside[1..].split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                    if let Some(pos) = stack.iter().rposition(|idx| match &nodes[*idx].kind {
                        NodeKind::Element(el) => el.tag == closing,
                        NodeKind::Text(_) => false,
                    }) {
                        stack.truncate(pos);
                        if stack.is_empty() { stack.push(root); }
                    }
                } else {
                    let self_closing = inside.ends_with('/');
                    let content = inside.trim_end_matches('/').trim();
                    let (tag, attrs) = parse_tag(content);
                    if !tag.is_empty() {
                        let idx = nodes.len();
                        let parent = stack.last().copied();
                        nodes.push(Node { kind: NodeKind::Element(ElementData { tag: tag.clone(), attrs }), children: Vec::new(), parent });
                        if let Some(parent) = parent { nodes[parent].children.push(idx); }

                        if !self_closing && matches!(tag.as_str(), "script" | "style") {
                            let after_start = end + 1;
                            let tail = &html[after_start..];
                            let tail_lower = tail.to_ascii_lowercase();
                            let closing = format!("</{tag}>");
                            if let Some(close_rel) = tail_lower.find(&closing) {
                                let raw_text = &tail[..close_rel];
                                if !raw_text.is_empty() {
                                    let text_idx = nodes.len();
                                    nodes.push(Node { kind: NodeKind::Text(raw_text.to_owned()), children: Vec::new(), parent: Some(idx) });
                                    nodes[idx].children.push(text_idx);
                                }
                                i = after_start + close_rel + closing.len();
                                continue;
                            }
                        }

                        if !self_closing && !is_void(&tag) { stack.push(idx); }
                    }
                }
                i = end + 1;
            } else {
                let end = html[i..].find('<').map(|x| i + x).unwrap_or(bytes.len());
                let text = decode_entities(&html[i..end]);
                if !text.trim().is_empty() {
                    let idx = nodes.len();
                    let parent = stack.last().copied();
                    nodes.push(Node { kind: NodeKind::Text(text), children: Vec::new(), parent });
                    if let Some(parent) = parent { nodes[parent].children.push(idx); }
                }
                i = end;
            }
        }

        Self { nodes, root }
    }

    pub fn text_content(&self, idx: usize) -> String {
        let node = &self.nodes[idx];
        match &node.kind {
            NodeKind::Text(t) => t.clone(),
            NodeKind::Element(_) => {
                let mut out = String::new();
                for &child in &node.children {
                    let part = self.text_content(child);
                    if !out.is_empty() && !part.chars().next().map(|c| c.is_whitespace()).unwrap_or(false) { out.push(' '); }
                    out.push_str(&part);
                }
                out
            }
        }
    }

    pub fn find_element_by_id(&self, id: &str) -> Option<usize> {
        self.nodes.iter().enumerate().find_map(|(idx, node)| match &node.kind {
            NodeKind::Element(el) if el.attrs.get("id").map(String::as_str) == Some(id) => Some(idx),
            _ => None,
        })
    }

    pub fn find_first_tag(&self, tag: &str) -> Option<usize> {
        self.nodes.iter().enumerate().find_map(|(idx, node)| match &node.kind {
            NodeKind::Element(el) if el.tag.eq_ignore_ascii_case(tag) => Some(idx),
            _ => None,
        })
    }

    pub fn replace_text_content(&mut self, idx: usize, text: &str) {
        if idx >= self.nodes.len() { return; }
        self.nodes[idx].children.clear();
        if text.is_empty() { return; }
        let text_idx = self.nodes.len();
        self.nodes.push(Node { kind: NodeKind::Text(text.to_owned()), children: Vec::new(), parent: Some(idx) });
        self.nodes[idx].children.push(text_idx);
    }

    pub fn replace_inner_html(&mut self, idx: usize, html: &str) {
        if idx >= self.nodes.len() { return; }
        let fragment = Dom::parse(html);
        self.nodes[idx].children.clear();
        let roots = fragment.nodes[fragment.root].children.clone();
        for child in roots {
            let cloned = self.clone_subtree_from(&fragment, child, Some(idx));
            self.nodes[idx].children.push(cloned);
        }
    }

    pub fn append_inner_html(&mut self, idx: usize, html: &str, prepend: bool) {
        if idx >= self.nodes.len() { return; }
        let fragment = Dom::parse(html);
        let roots = fragment.nodes[fragment.root].children.clone();
        let mut added = Vec::new();
        for child in roots {
            added.push(self.clone_subtree_from(&fragment, child, Some(idx)));
        }
        if prepend {
            added.extend(self.nodes[idx].children.iter().copied());
            self.nodes[idx].children = added;
        } else {
            self.nodes[idx].children.extend(added);
        }
    }

    pub fn set_attribute(&mut self, idx: usize, name: &str, value: &str) {
        if let Some(Node { kind: NodeKind::Element(el), .. }) = self.nodes.get_mut(idx) {
            el.attrs.insert(name.to_ascii_lowercase(), value.to_owned());
        }
    }

    pub fn remove_attribute(&mut self, idx: usize, name: &str) {
        if let Some(Node { kind: NodeKind::Element(el), .. }) = self.nodes.get_mut(idx) {
            el.attrs.remove(&name.to_ascii_lowercase());
        }
    }

    pub fn set_style_property(&mut self, idx: usize, name: &str, value: &str) {
        let Some(Node { kind: NodeKind::Element(el), .. }) = self.nodes.get_mut(idx) else { return; };
        let mut declarations: Vec<(String, String)> = el.attrs.get("style")
            .map(|style| style.split(';').filter_map(|decl| {
                let (key, value) = decl.split_once(':')?;
                Some((key.trim().to_ascii_lowercase(), value.trim().to_owned()))
            }).collect())
            .unwrap_or_default();
        let name = name.trim().to_ascii_lowercase();
        declarations.retain(|(key, _)| key != &name);
        declarations.push((name, value.trim().to_owned()));
        let serialized = declarations.into_iter().map(|(key, value)| format!("{key}:{value}")).collect::<Vec<_>>().join(";");
        el.attrs.insert("style".into(), serialized);
    }

    pub fn remove_node(&mut self, idx: usize) {
        if idx == self.root || idx >= self.nodes.len() { return; }
        let parent = self.nodes[idx].parent;
        if let Some(parent) = parent {
            self.nodes[parent].children.retain(|child| *child != idx);
        }
        self.nodes[idx].parent = None;
    }

    fn clone_subtree_from(&mut self, source: &Dom, src_idx: usize, parent: Option<usize>) -> usize {
        let new_idx = self.nodes.len();
        self.nodes.push(Node {
            kind: source.nodes[src_idx].kind.clone(),
            children: Vec::new(),
            parent,
        });
        let children = source.nodes[src_idx].children.clone();
        for child in children {
            let cloned = self.clone_subtree_from(source, child, Some(new_idx));
            self.nodes[new_idx].children.push(cloned);
        }
        new_idx
    }
}

fn parse_tag(content: &str) -> (String, HashMap<String, String>) {
    let mut chars = content.char_indices().peekable();
    let mut tag_end = content.len();
    while let Some((idx, ch)) = chars.next() {
        if ch.is_whitespace() { tag_end = idx; break; }
    }
    let tag = content[..tag_end].to_ascii_lowercase();
    let mut attrs = HashMap::new();
    let rest = content[tag_end..].trim();
    let mut pos = 0usize;
    let rb = rest.as_bytes();

    while pos < rb.len() {
        while pos < rb.len() && rb[pos].is_ascii_whitespace() { pos += 1; }
        if pos >= rb.len() { break; }
        let key_start = pos;
        while pos < rb.len() && !rb[pos].is_ascii_whitespace() && rb[pos] != b'=' { pos += 1; }
        let key = rest[key_start..pos].to_ascii_lowercase();
        while pos < rb.len() && rb[pos].is_ascii_whitespace() { pos += 1; }
        let mut value = String::new();
        if pos < rb.len() && rb[pos] == b'=' {
            pos += 1;
            while pos < rb.len() && rb[pos].is_ascii_whitespace() { pos += 1; }
            if pos < rb.len() && (rb[pos] == b'\'' || rb[pos] == b'\"') {
                let quote = rb[pos]; pos += 1; let start = pos;
                while pos < rb.len() && rb[pos] != quote { pos += 1; }
                value = rest[start..pos].to_owned();
                if pos < rb.len() { pos += 1; }
            } else {
                let start = pos;
                while pos < rb.len() && !rb[pos].is_ascii_whitespace() { pos += 1; }
                value = rest[start..pos].to_owned();
            }
        }
        if !key.is_empty() { attrs.insert(key, decode_entities(&value)); }
    }

    (tag, attrs)
}

fn is_void(tag: &str) -> bool {
    matches!(tag, "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr")
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_script_text_is_not_parsed_as_html() {
        let dom = Dom::parse("<script>if (a < b) console.log('x');</script><p>Hello</p>");
        let script = dom
            .nodes
            .iter()
            .enumerate()
            .find_map(|(index, node)| match &node.kind {
                NodeKind::Element(el) if el.tag == "script" => Some(index),
                _ => None,
            })
            .unwrap();
        assert!(dom.text_content(script).contains("a < b"));
    }
}
