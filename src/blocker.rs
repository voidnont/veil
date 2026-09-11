use std::collections::HashMap;
use url::Url;

const DEFAULT_FILTERS: &str = include_str!("../filters/veil-default.txt");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Document,
    Image,
    Script,
    Stylesheet,
    Font,
    Media,
    Other,
}

#[derive(Debug, Clone)]
pub struct BlockContext<'a> {
    pub url: &'a Url,
    pub top_level: &'a Url,
    pub resource_type: ResourceType,
    pub third_party: bool,
}

#[derive(Debug, Clone)]
pub struct BlockDecision {
    pub blocked: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FilterStats {
    pub network_rules: usize,
    pub exception_rules: usize,
    pub cosmetic_rules: usize,
    pub cosmetic_exceptions: usize,
    pub ignored_rules: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuleAction {
    Block,
    Allow,
}

#[derive(Debug, Clone)]
struct NetworkRule {
    action: RuleAction,
    pattern: String,
    kinds: Vec<ResourceType>,
    excluded_kinds: Vec<ResourceType>,
    third_party: Option<bool>,
    include_domains: Vec<String>,
    exclude_domains: Vec<String>,
    raw: String,
}

#[derive(Debug, Clone)]
struct CosmeticRule {
    domains: Vec<String>,
    excluded_domains: Vec<String>,
    selector: SimpleSelector,
    exception: bool,
}

#[derive(Debug, Clone, Default)]
struct SimpleSelector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attr: Option<AttrSelector>,
}

#[derive(Debug, Clone)]
struct AttrSelector {
    name: String,
    op: AttrOp,
    value: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum AttrOp {
    Exists,
    Equals,
    Prefix,
    Suffix,
    Contains,
}

pub struct Blocker {
    network_rules: Vec<NetworkRule>,
    cosmetic_rules: Vec<CosmeticRule>,
    stats: FilterStats,
    custom_text: String,
}

impl Default for Blocker {
    fn default() -> Self {
        let mut blocker = Self {
            network_rules: Vec::new(),
            cosmetic_rules: Vec::new(),
            stats: FilterStats::default(),
            custom_text: String::new(),
        };
        blocker.load_filter_text(DEFAULT_FILTERS);
        blocker
    }
}

impl Blocker {
    pub fn stats(&self) -> FilterStats {
        self.stats
    }

    pub fn custom_text(&self) -> &str {
        &self.custom_text
    }

    pub fn replace_custom_filters(&mut self, text: String) {
        let mut fresh = Self::default();
        fresh.custom_text = text.clone();
        fresh.load_filter_text(&text);
        *self = fresh;
    }

    pub fn check(&self, context: &BlockContext<'_>) -> BlockDecision {
        for rule in self.network_rules.iter().filter(|rule| rule.action == RuleAction::Allow) {
            if rule_matches(rule, context) {
                return BlockDecision {
                    blocked: false,
                    reason: format!("allow rule: {}", rule.raw),
                };
            }
        }

        for rule in self.network_rules.iter().filter(|rule| rule.action == RuleAction::Block) {
            if rule_matches(rule, context) {
                return BlockDecision {
                    blocked: true,
                    reason: format!("filter: {}", rule.raw),
                };
            }
        }

        BlockDecision {
            blocked: false,
            reason: "allowed".into(),
        }
    }

    pub fn should_hide_element(
        &self,
        page_url: &Url,
        tag: &str,
        attrs: &HashMap<String, String>,
    ) -> bool {
        let host = page_url.host_str().unwrap_or_default().to_ascii_lowercase();
        let mut hidden = false;

        for rule in &self.cosmetic_rules {
            if !domain_scope_matches(&rule.domains, &rule.excluded_domains, &host) {
                continue;
            }
            if !selector_matches(&rule.selector, tag, attrs) {
                continue;
            }
            if rule.exception {
                return false;
            }
            hidden = true;
        }

        hidden
    }

    pub fn load_filter_text(&mut self, text: &str) {
        for line in text.lines() {
            let raw = line.trim();
            if raw.is_empty()
                || raw.starts_with('!')
                || raw.starts_with('[')
                || (raw.starts_with('#')
                    && !raw.starts_with("##")
                    && !raw.starts_with("#@#"))
            {
                continue;
            }

            if let Some((domains, selector)) = raw.split_once("#@#") {
                if !self.add_cosmetic_rule(domains, selector, true) {
                    self.stats.ignored_rules += 1;
                }
                continue;
            }
            if let Some((domains, selector)) = raw.split_once("##") {
                if !self.add_cosmetic_rule(domains, selector, false) {
                    self.stats.ignored_rules += 1;
                }
                continue;
            }

            match parse_network_rule(raw) {
                Some(rule) => {
                    if rule.action == RuleAction::Allow {
                        self.stats.exception_rules += 1;
                    } else {
                        self.stats.network_rules += 1;
                    }
                    self.network_rules.push(rule);
                }
                None => self.stats.ignored_rules += 1,
            }
        }
    }

    fn add_cosmetic_rule(&mut self, domains_raw: &str, selector_raw: &str, exception: bool) -> bool {
        let (domains, excluded_domains) = parse_domain_scope(domains_raw);
        let mut added = false;

        for part in selector_raw.split(',') {
            let selector_text = part.trim();
            if selector_text.is_empty() {
                continue;
            }
            let Some(selector) = parse_simple_selector(selector_text) else {
                continue;
            };
            self.cosmetic_rules.push(CosmeticRule {
                domains: domains.clone(),
                excluded_domains: excluded_domains.clone(),
                selector,
                exception,
            });
            if exception {
                self.stats.cosmetic_exceptions += 1;
            } else {
                self.stats.cosmetic_rules += 1;
            }
            added = true;
        }

        added
    }
}

fn parse_network_rule(raw: &str) -> Option<NetworkRule> {
    let mut body = raw.trim();
    let action = if let Some(rest) = body.strip_prefix("@@") {
        body = rest;
        RuleAction::Allow
    } else {
        RuleAction::Block
    };

    if body.is_empty() || body.starts_with('/') && body.ends_with('/') {
        // Regex rules are intentionally ignored in 0.5.
        return None;
    }

    let (pattern, options) = body.split_once('$').unwrap_or((body, ""));
    if pattern.trim().is_empty() {
        return None;
    }

    let mut kinds = Vec::new();
    let mut excluded_kinds = Vec::new();
    let mut third_party = None;
    let mut include_domains = Vec::new();
    let mut exclude_domains = Vec::new();

    for option in options.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match option {
            "image" => kinds.push(ResourceType::Image),
            "script" => kinds.push(ResourceType::Script),
            "stylesheet" => kinds.push(ResourceType::Stylesheet),
            "document" => kinds.push(ResourceType::Document),
            "font" => kinds.push(ResourceType::Font),
            "media" => kinds.push(ResourceType::Media),
            "other" => kinds.push(ResourceType::Other),
            "~image" => excluded_kinds.push(ResourceType::Image),
            "~script" => excluded_kinds.push(ResourceType::Script),
            "~stylesheet" => excluded_kinds.push(ResourceType::Stylesheet),
            "~document" => excluded_kinds.push(ResourceType::Document),
            "~font" => excluded_kinds.push(ResourceType::Font),
            "~media" => excluded_kinds.push(ResourceType::Media),
            "~other" => excluded_kinds.push(ResourceType::Other),
            "third-party" => third_party = Some(true),
            "~third-party" => third_party = Some(false),
            _ if option.starts_with("domain=") => {
                let scope = &option[7..];
                for domain in scope.split('|').map(str::trim).filter(|d| !d.is_empty()) {
                    if let Some(excluded) = domain.strip_prefix('~') {
                        exclude_domains.push(excluded.to_ascii_lowercase());
                    } else {
                        include_domains.push(domain.to_ascii_lowercase());
                    }
                }
            }
            // "important" affects rule priority, not whether the request matches.
            "important" => {},
            // Options that change request rewriting, casing, content policy, or target
            // resource classes we do not implement are safer to ignore as a whole rule.
            "redirect" | "rewrite" | "csp" | "removeparam" | "badfilter" | "match-case"
            | "popup" | "object" | "object-subrequest"
            | "xmlhttprequest" | "subdocument" | "ping" | "websocket" | "webrtc" => {
                return None
            }
            _ => return None,
        }
    }

    Some(NetworkRule {
        action,
        pattern: pattern.to_ascii_lowercase(),
        kinds,
        excluded_kinds,
        third_party,
        include_domains,
        exclude_domains,
        raw: raw.to_owned(),
    })
}

fn rule_matches(rule: &NetworkRule, context: &BlockContext<'_>) -> bool {
    if !rule.kinds.is_empty() && !rule.kinds.contains(&context.resource_type) {
        return false;
    }
    if rule.excluded_kinds.contains(&context.resource_type) {
        return false;
    }
    if let Some(required) = rule.third_party {
        if context.third_party != required {
            return false;
        }
    }

    let page_host = context
        .top_level
        .host_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !domain_scope_matches(&rule.include_domains, &rule.exclude_domains, &page_host) {
        return false;
    }

    pattern_matches(&rule.pattern, context.url)
}

fn domain_scope_matches(included: &[String], excluded: &[String], host: &str) -> bool {
    if excluded.iter().any(|domain| host_matches_domain(host, domain)) {
        return false;
    }
    included.is_empty() || included.iter().any(|domain| host_matches_domain(host, domain))
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn pattern_matches(pattern: &str, url: &Url) -> bool {
    let url_text = url.as_str().to_ascii_lowercase();
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();

    if let Some(rest) = pattern.strip_prefix("||") {
        let host_pattern = rest
            .split(&['^', '/', '*'][..])
            .next()
            .unwrap_or_default()
            .trim_matches('.');
        if host_pattern.is_empty() || !host_matches_domain(&host, host_pattern) {
            return false;
        }

        let after_host = &rest[host_pattern.len()..];
        if after_host.is_empty() || after_host == "^" {
            return true;
        }
        return simplified_glob_matches(after_host, &url_text, false, false);
    }

    let anchored_start = pattern.starts_with('|');
    let anchored_end = pattern.ends_with('|') && pattern.len() > 1;
    let mut body = pattern;
    if anchored_start {
        body = &body[1..];
    }
    if anchored_end {
        body = &body[..body.len() - 1];
    }

    simplified_glob_matches(body, &url_text, anchored_start, anchored_end)
}

fn simplified_glob_matches(pattern: &str, text: &str, anchor_start: bool, anchor_end: bool) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();

    if anchor_start {
        return glob_from(p, t, 0, 0, anchor_end, &mut HashMap::new());
    }

    for start in 0..=t.len() {
        if glob_from(p, t, 0, start, anchor_end, &mut HashMap::new()) {
            return true;
        }
    }
    false
}

fn glob_from(
    pattern: &[u8],
    text: &[u8],
    pi: usize,
    ti: usize,
    anchor_end: bool,
    memo: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if let Some(value) = memo.get(&(pi, ti)) {
        return *value;
    }

    let result = if pi == pattern.len() {
        !anchor_end || ti == text.len()
    } else {
        match pattern[pi] {
            b'*' => {
                glob_from(pattern, text, pi + 1, ti, anchor_end, memo)
                    || (ti < text.len() && glob_from(pattern, text, pi, ti + 1, anchor_end, memo))
            }
            b'^' => {
                (ti == text.len() && glob_from(pattern, text, pi + 1, ti, anchor_end, memo))
                    || (ti < text.len()
                        && is_separator(text[ti])
                        && glob_from(pattern, text, pi + 1, ti + 1, anchor_end, memo))
            }
            byte => {
                ti < text.len()
                    && byte == text[ti]
                    && glob_from(pattern, text, pi + 1, ti + 1, anchor_end, memo)
            }
        }
    };

    memo.insert((pi, ti), result);
    result
}

fn is_separator(byte: u8) -> bool {
    !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'-' | b'.' | b'%')
}

fn parse_domain_scope(raw: &str) -> (Vec<String>, Vec<String>) {
    let mut included = Vec::new();
    let mut excluded = Vec::new();

    for domain in raw.split(',').map(str::trim).filter(|d| !d.is_empty()) {
        if let Some(excluded_domain) = domain.strip_prefix('~') {
            excluded.push(excluded_domain.to_ascii_lowercase());
        } else {
            included.push(domain.to_ascii_lowercase());
        }
    }

    (included, excluded)
}

fn parse_simple_selector(raw: &str) -> Option<SimpleSelector> {
    let raw = raw.trim();
    if raw.is_empty()
        || raw.contains(' ')
        || raw.contains('>')
        || raw.contains('+')
        || raw.contains('~')
        || raw.contains(':')
    {
        return None;
    }

    let mut selector = SimpleSelector::default();
    let mut cursor = 0usize;
    let bytes = raw.as_bytes();

    if cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
        let start = cursor;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'-')
        {
            cursor += 1;
        }
        selector.tag = Some(raw[start..cursor].to_ascii_lowercase());
    }

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'#' => {
                cursor += 1;
                let start = cursor;
                while cursor < bytes.len() && is_ident_byte(bytes[cursor]) {
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
                while cursor < bytes.len() && is_ident_byte(bytes[cursor]) {
                    cursor += 1;
                }
                if start == cursor {
                    return None;
                }
                selector.classes.push(raw[start..cursor].to_owned());
            }
            b'[' => {
                let end_rel = raw[cursor + 1..].find(']')?;
                let end = cursor + 1 + end_rel;
                if selector.attr.is_some() {
                    return None;
                }
                selector.attr = parse_attr_selector(&raw[cursor + 1..end]);
                if selector.attr.is_none() {
                    return None;
                }
                cursor = end + 1;
            }
            _ => return None,
        }
    }

    if selector.tag.is_none()
        && selector.id.is_none()
        && selector.classes.is_empty()
        && selector.attr.is_none()
    {
        None
    } else {
        Some(selector)
    }
}

fn parse_attr_selector(raw: &str) -> Option<AttrSelector> {
    let raw = raw.trim();
    for (token, op) in [
        ("^=", AttrOp::Prefix),
        ("$=", AttrOp::Suffix),
        ("*=", AttrOp::Contains),
        ("=", AttrOp::Equals),
    ] {
        if let Some((name, value)) = raw.split_once(token) {
            let name = name.trim().to_ascii_lowercase();
            if name.is_empty() {
                return None;
            }
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_owned();
            return Some(AttrSelector {
                name,
                op,
                value: Some(value),
            });
        }
    }

    if raw.is_empty() {
        None
    } else {
        Some(AttrSelector {
            name: raw.to_ascii_lowercase(),
            op: AttrOp::Exists,
            value: None,
        })
    }
}

fn selector_matches(selector: &SimpleSelector, tag: &str, attrs: &HashMap<String, String>) -> bool {
    if let Some(expected_tag) = &selector.tag {
        if !tag.eq_ignore_ascii_case(expected_tag) {
            return false;
        }
    }

    if let Some(expected_id) = &selector.id {
        if attrs.get("id") != Some(expected_id) {
            return false;
        }
    }

    if !selector.classes.is_empty() {
        let classes: Vec<&str> = attrs
            .get("class")
            .map(|value| value.split_whitespace().collect())
            .unwrap_or_default();
        if selector
            .classes
            .iter()
            .any(|expected| !classes.iter().any(|actual| *actual == expected))
        {
            return false;
        }
    }

    if let Some(attr) = &selector.attr {
        let Some(actual) = attrs.get(&attr.name) else {
            return false;
        };
        let matched = match attr.op {
            AttrOp::Exists => true,
            AttrOp::Equals => attr.value.as_deref() == Some(actual.as_str()),
            AttrOp::Prefix => attr
                .value
                .as_deref()
                .map(|value| actual.starts_with(value))
                .unwrap_or(false),
            AttrOp::Suffix => attr
                .value
                .as_deref()
                .map(|value| actual.ends_with(value))
                .unwrap_or(false),
            AttrOp::Contains => attr
                .value
                .as_deref()
                .map(|value| actual.contains(value))
                .unwrap_or(false),
        };
        if !matched {
            return false;
        }
    }

    true
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_rule_blocks_subdomain() {
        let blocker = Blocker::default();
        let top = Url::parse("https://example.com/").unwrap();
        let request = Url::parse("https://ads.doubleclick.net/x").unwrap();
        let decision = blocker.check(&BlockContext {
            url: &request,
            top_level: &top,
            resource_type: ResourceType::Image,
            third_party: true,
        });
        assert!(decision.blocked);
    }

    #[test]
    fn exception_rule_overrides_block() {
        let mut blocker = Blocker::default();
        blocker.load_filter_text("||ads.example^\n@@||ads.example/safe^");
        let top = Url::parse("https://site.example/").unwrap();
        let request = Url::parse("https://ads.example/safe/banner.png").unwrap();
        let decision = blocker.check(&BlockContext {
            url: &request,
            top_level: &top,
            resource_type: ResourceType::Image,
            third_party: true,
        });
        assert!(!decision.blocked);
    }

    #[test]
    fn generic_cosmetic_exception_is_parsed() {
        let mut blocker = Blocker::default();
        blocker.load_filter_text("##.sponsor\n#@#.sponsor");
        let page = Url::parse("https://example.com/").unwrap();
        let attrs = HashMap::from([("class".to_owned(), "sponsor".to_owned())]);
        assert!(!blocker.should_hide_element(&page, "div", &attrs));
    }

    #[test]
    fn simple_cosmetic_selector_matches() {
        let selector = parse_simple_selector("div.ad-container[data-ad]").unwrap();
        let attrs = HashMap::from([
            ("class".to_owned(), "card ad-container".to_owned()),
            ("data-ad".to_owned(), "1".to_owned()),
        ]);
        assert!(selector_matches(&selector, "div", &attrs));
    }
}
