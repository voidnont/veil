from pathlib import Path
import re


def replace_once(path, old, new):
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


def regex_once(path, pattern, replacement):
    p = Path(path)
    text = p.read_text()
    new, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{path}: regex expected one match, found {count}: {pattern[:100]!r}")
    p.write_text(new)


# Version bump.
replace_once("Cargo.toml", 'version = "0.8.6"', 'version = "0.8.7"')
lock = Path("Cargo.lock")
lock_text = lock.read_text()
lock_text, count = re.subn(r'(name = "veil-browser"\nversion = ")0\.8\.6("\n)', r'\g<1>0.8.7\2', lock_text, count=1)
if count != 1:
    raise SystemExit("Cargo.lock: veil-browser 0.8.6 entry not found")
lock.write_text(lock_text)

# ---------------------------------------------------------------------------
# Network cache + priority scheduler, following the same broad ideas as
# Gecko/Necko: shared connection pools, partitioned memory cache, bounded
# resource concurrency, and urgency hints.
# ---------------------------------------------------------------------------
replace_once(
    "src/net.rs",
    "use std::io::Read;\nuse std::sync::{Arc, Mutex, OnceLock};\nuse std::time::{Duration, SystemTime, UNIX_EPOCH};",
    "use std::collections::{HashMap, VecDeque};\nuse std::io::Read;\nuse std::sync::{Arc, Condvar, Mutex, OnceLock};\nuse std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};",
)
replace_once(
    "src/net.rs",
    "HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, DNT, LOCATION,\n    SET_COOKIE, USER_AGENT,",
    "HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, CONTENT_TYPE, COOKIE, DNT,\n    EXPIRES, LOCATION, SET_COOKIE, USER_AGENT, VARY,",
)
replace_once(
    "src/net.rs",
    "use crate::privacy::{is_third_party, SitePrivacy};",
    "use crate::privacy::{is_third_party, site_key_for_url, SitePrivacy};",
)
replace_once(
    "src/net.rs",
    'HeaderValue::from_static("Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.6 VeilEngine/0.8.6"),',
    'HeaderValue::from_static("Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.7 VeilEngine/0.8.7"),',
)

net_anchor = '''static SHARED_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
'''
net_insert = r'''static SHARED_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static RESOURCE_CACHE: OnceLock<Mutex<MemoryResourceCache>> = OnceLock::new();
static NETWORK_SCHEDULER: OnceLock<NetworkScheduler> = OnceLock::new();

const MEMORY_CACHE_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const MEMORY_CACHE_MAX_ENTRIES: usize = 512;
const MEMORY_CACHE_MAX_ENTRY_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_RESOURCE_TTL: Duration = Duration::from_secs(60);
const MAX_NETWORK_REQUESTS: usize = 12;
const MAX_NORMAL_REQUESTS: usize = 10;
const MAX_LOW_REQUESTS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestPriority {
    High,
    Normal,
    Low,
}

impl RequestPriority {
    fn for_resource(kind: ResourceType) -> Self {
        match kind {
            ResourceType::Document | ResourceType::Stylesheet | ResourceType::Script => Self::High,
            ResourceType::Font | ResourceType::Other => Self::Normal,
            ResourceType::Image | ResourceType::Media => Self::Low,
        }
    }

    fn header_value(self) -> &'static str {
        match self {
            Self::High => "u=0, i",
            Self::Normal => "u=3",
            Self::Low => "u=5",
        }
    }
}

#[derive(Default)]
struct SchedulerState {
    active_total: usize,
    active_normal: usize,
    active_low: usize,
}

struct NetworkScheduler {
    state: Mutex<SchedulerState>,
    wake: Condvar,
}

impl NetworkScheduler {
    fn new() -> Self {
        Self {
            state: Mutex::new(SchedulerState::default()),
            wake: Condvar::new(),
        }
    }

    fn acquire(&'static self, priority: RequestPriority) -> RequestPermit {
        let mut state = self.state.lock().expect("network scheduler lock poisoned");
        loop {
            let allowed = match priority {
                RequestPriority::High => state.active_total < MAX_NETWORK_REQUESTS,
                RequestPriority::Normal => {
                    state.active_total < MAX_NORMAL_REQUESTS
                        && state.active_normal + state.active_low < MAX_NORMAL_REQUESTS
                }
                RequestPriority::Low => {
                    state.active_total < MAX_LOW_REQUESTS && state.active_low < MAX_LOW_REQUESTS
                }
            };
            if allowed {
                state.active_total += 1;
                match priority {
                    RequestPriority::High => {}
                    RequestPriority::Normal => state.active_normal += 1,
                    RequestPriority::Low => state.active_low += 1,
                }
                break;
            }
            state = self
                .wake
                .wait(state)
                .expect("network scheduler lock poisoned while waiting");
        }
        RequestPermit {
            scheduler: self,
            priority,
        }
    }
}

struct RequestPermit {
    scheduler: &'static NetworkScheduler,
    priority: RequestPriority,
}

impl Drop for RequestPermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.scheduler.state.lock() {
            state.active_total = state.active_total.saturating_sub(1);
            match self.priority {
                RequestPriority::High => {}
                RequestPriority::Normal => {
                    state.active_normal = state.active_normal.saturating_sub(1)
                }
                RequestPriority::Low => state.active_low = state.active_low.saturating_sub(1),
            }
            self.scheduler.wake.notify_all();
        }
    }
}

fn network_scheduler() -> &'static NetworkScheduler {
    NETWORK_SCHEDULER.get_or_init(NetworkScheduler::new)
}

#[derive(Clone)]
struct CachedResource {
    final_url: Url,
    bytes: Vec<u8>,
    content_type: String,
    expires_at: Instant,
    weight: usize,
}

#[derive(Default)]
struct MemoryResourceCache {
    entries: HashMap<String, CachedResource>,
    lru: VecDeque<String>,
    bytes: usize,
}

impl MemoryResourceCache {
    fn get(&mut self, key: &str) -> Option<BinaryResponse> {
        let entry = self.entries.get(key)?.clone();
        if entry.expires_at <= Instant::now() {
            self.remove(key);
            return None;
        }
        self.touch(key);
        Some(BinaryResponse {
            final_url: entry.final_url,
            bytes: entry.bytes,
            content_type: entry.content_type,
        })
    }

    fn insert(
        &mut self,
        key: String,
        final_url: Url,
        bytes: Vec<u8>,
        content_type: String,
        ttl: Duration,
    ) {
        if bytes.is_empty() || bytes.len() > MEMORY_CACHE_MAX_ENTRY_BYTES || ttl.is_zero() {
            return;
        }
        self.remove(&key);
        let weight = bytes
            .len()
            .saturating_add(content_type.len())
            .saturating_add(final_url.as_str().len())
            .saturating_add(key.len());
        self.bytes = self.bytes.saturating_add(weight);
        self.entries.insert(
            key.clone(),
            CachedResource {
                final_url,
                bytes,
                content_type,
                expires_at: Instant::now() + ttl,
                weight,
            },
        );
        self.lru.push_back(key);
        self.evict();
    }

    fn touch(&mut self, key: &str) {
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(key.to_owned());
    }

    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.weight);
        }
        self.lru.retain(|candidate| candidate != key);
    }

    fn evict(&mut self) {
        while self.bytes > MEMORY_CACHE_LIMIT_BYTES
            || self.entries.len() > MEMORY_CACHE_MAX_ENTRIES
        {
            let Some(key) = self.lru.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.weight);
            }
        }
    }
}

fn resource_cache() -> &'static Mutex<MemoryResourceCache> {
    RESOURCE_CACHE.get_or_init(|| Mutex::new(MemoryResourceCache::default()))
}

fn cacheable_resource(kind: ResourceType) -> bool {
    matches!(
        kind,
        ResourceType::Image | ResourceType::Stylesheet | ResourceType::Script | ResourceType::Font
    )
}

fn resource_cache_key(top_level: &Url, url: &Url, kind: ResourceType) -> String {
    format!("{}|{:?}|{}", site_key_for_url(top_level), kind, url)
}

fn cache_ttl(headers: &HeaderMap) -> Option<Duration> {
    let cache_control = headers
        .get(CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if cache_control
        .split(',')
        .map(str::trim)
        .any(|directive| directive == "no-store" || directive == "no-cache")
    {
        return None;
    }
    for directive in cache_control.split(',').map(str::trim) {
        if let Some(raw) = directive.strip_prefix("max-age=") {
            if let Ok(seconds) = raw.trim_matches('"').parse::<u64>() {
                return (seconds > 0).then(|| Duration::from_secs(seconds.min(24 * 60 * 60)));
            }
        }
    }
    if let Some(expires) = headers
        .get(EXPIRES)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
    {
        if let Ok(ttl) = expires.duration_since(SystemTime::now()) {
            if !ttl.is_zero() {
                return Some(ttl.min(Duration::from_secs(24 * 60 * 60)));
            }
        }
        return None;
    }
    Some(DEFAULT_RESOURCE_TTL)
}

fn response_cacheable(headers: &HeaderMap, request_had_cookie: bool) -> Option<Duration> {
    if request_had_cookie || headers.contains_key(SET_COOKIE) {
        return None;
    }
    let vary = headers
        .get(VARY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if vary.split(',').map(str::trim).any(|value| value == "cookie" || value == "*") {
        return None;
    }
    cache_ttl(headers)
}
'''
replace_once("src/net.rs", net_anchor, net_insert)

# Document requests get high urgency and a scheduler permit.
replace_once(
    "src/net.rs",
    '''            if let Some(cookie) =
                self.storage
                    .cookie_header_for(&initial_top_level, &current, true, safe_method)
            {
                request = request.header(COOKIE, cookie);
            }
            let response = request.send().map_err(|e| format!("Network error: {e}"))?;''',
    '''            if let Some(cookie) =
                self.storage
                    .cookie_header_for(&initial_top_level, &current, true, safe_method)
            {
                request = request.header(COOKIE, cookie);
            }
            request = request.header("priority", RequestPriority::High.header_value());
            let _permit = network_scheduler().acquire(RequestPriority::High);
            let response = request.send().map_err(|e| format!("Network error: {e}"))?;''',
)

# Replace the binary subresource broker with a partitioned cache + priority path.
regex_once(
    "src/net.rs",
    r'''    fn get_binary_subresource\(\n        &mut self,\n        top_level: &Url,\n        url: &Url,\n        privacy: SitePrivacy,\n        resource_type: ResourceType,\n        accept: &str,\n        limit: usize,\n    \) -> Result<BinaryResponse, String> \{.*?\n    \}\n\n    pub fn get_image''',
    r'''    fn get_binary_subresource(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
        resource_type: ResourceType,
        accept: &str,
        limit: usize,
    ) -> Result<BinaryResponse, String> {
        validate_http_url(url)?;
        self.enforce(url, top_level, resource_type, privacy)?;

        let cache_key = resource_cache_key(top_level, url, resource_type);
        let initial_cookie = self
            .storage
            .cookie_header_for(top_level, url, false, true);
        if cacheable_resource(resource_type) && initial_cookie.is_none() {
            if let Ok(mut cache) = resource_cache().lock() {
                if let Some(hit) = cache.get(&cache_key) {
                    if hit.final_url != *url {
                        self.enforce(&hit.final_url, top_level, resource_type, privacy)?;
                    }
                    return Ok(hit);
                }
            }
        }

        let priority = RequestPriority::for_resource(resource_type);
        let mut current = url.clone();
        for _ in 0..=MAX_REDIRECTS {
            self.enforce(&current, top_level, resource_type, privacy)?;
            let mut request = self
                .client
                .get(current.clone())
                .header(ACCEPT, accept)
                .header("priority", priority.header_value());
            let cookie_header = self
                .storage
                .cookie_header_for(top_level, &current, false, true);
            let request_had_cookie = cookie_header.is_some();
            if let Some(cookie) = cookie_header {
                request = request.header(COOKIE, cookie);
            }

            let _permit = network_scheduler().acquire(priority);
            let response = request
                .send()
                .map_err(|e| format!("Subresource request failed: {e}"))?;
            self.store_response_cookies(top_level, &current, &response);
            if is_redirect(response.status()) {
                current = resolve_redirect(&current, &response)?;
                continue;
            }
            let final_url = response.url().clone();
            let status = response.status();
            if !status.is_success() {
                return Err(format!("Subresource returned HTTP {status}"));
            }
            let response_type = content_type(&response);
            let response_headers = response.headers().clone();
            let bytes = read_limited(response, limit)
                .map_err(|e| format!("Failed to read subresource: {e}"))?;

            if cacheable_resource(resource_type)
                && bytes.len() <= MEMORY_CACHE_MAX_ENTRY_BYTES
            {
                if let Some(ttl) = response_cacheable(&response_headers, request_had_cookie) {
                    if let Ok(mut cache) = resource_cache().lock() {
                        cache.insert(
                            cache_key.clone(),
                            final_url.clone(),
                            bytes.clone(),
                            response_type.clone(),
                            ttl,
                        );
                    }
                }
            }

            return Ok(BinaryResponse {
                final_url,
                bytes,
                content_type: response_type,
            });
        }
        Err("Too many subresource redirects.".into())
    }

    pub fn get_image''',
)

# Add network/cache tests.
replace_once(
    "src/net.rs",
    '''    #[test]
    fn multipart_encoder_emits_file_and_text_parts() {''',
    '''    #[test]
    fn cache_keys_are_partitioned_by_top_level_site() {
        let a = Url::parse("https://a.example/page").unwrap();
        let b = Url::parse("https://b.example.net/page").unwrap();
        let resource = Url::parse("https://cdn.example.org/app.js").unwrap();
        assert_ne!(
            resource_cache_key(&a, &resource, ResourceType::Script),
            resource_cache_key(&b, &resource, ResourceType::Script)
        );
    }

    #[test]
    fn no_store_responses_are_not_cached() {
        let mut headers = HeaderMap::new();
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("public, no-store"));
        assert!(cache_ttl(&headers).is_none());
    }

    #[test]
    fn priorities_reserve_capacity_for_critical_resources() {
        assert_eq!(RequestPriority::for_resource(ResourceType::Document), RequestPriority::High);
        assert_eq!(RequestPriority::for_resource(ResourceType::Stylesheet), RequestPriority::High);
        assert_eq!(RequestPriority::for_resource(ResourceType::Image), RequestPriority::Low);
        assert!(MAX_LOW_REQUESTS < MAX_NETWORK_REQUESTS);
    }

    #[test]
    fn multipart_encoder_emits_file_and_text_parts() {''',
)

# ---------------------------------------------------------------------------
# Retained live document / mutation-scoped invalidation.
# ---------------------------------------------------------------------------
replace_once(
    "src/engine.rs",
    "use std::collections::HashSet;",
    "use std::collections::HashSet;\nuse std::hash::{Hash, Hasher};",
)
replace_once(
    "src/engine.rs",
    "use crate::script::{CanvasCommand, JavascriptSandbox, ScriptReport};",
    "use crate::script::{CanvasCommand, DomMutation, JavascriptSandbox, ScriptReport};",
)

retained_anchor = '''#[derive(Default)]
pub struct Engine {
    sandbox: JavascriptSandbox,
}
'''
retained_code = r'''#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
'''
replace_once("src/engine.rs", retained_anchor, retained_code)

# Refactor mutation application into a single-mutation primitive that retained
# sessions can reuse without replaying every previous mutation.
regex_once(
    "src/engine.rs",
    r'''fn apply_script_mutations\(dom: &mut Dom, report: &ScriptReport\) \{.*?\n\}\n\nfn attach_canvas_commands''',
    r'''fn apply_single_script_mutation(dom: &mut Dom, mutation: &DomMutation) {
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
            if let Some((name, value)) = mutation.value.split_once('\0') {
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

fn attach_canvas_commands''',
)

# Retained invalidation tests are intentionally small and deterministic.
engine_test_append = r'''

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
        assert_eq!(retained.update_from_report(&report), InvalidationKind::Layout);
        let view = retained.render(&Blocker::default(), &report);
        assert!(format!("{:?}", view.blocks).contains("Updated"));
    }
}
'''
Path("src/engine.rs").write_text(Path("src/engine.rs").read_text() + engine_test_append)

# ---------------------------------------------------------------------------
# Renderer protocol: live updates may now carry no full view when there is no
# paint damage. Script/timer state still travels back so refresh scheduling is
# correct without forcing a page replacement.
# ---------------------------------------------------------------------------
Path("src/renderer_protocol.rs").write_text(r'''use serde::{Deserialize, Serialize};

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
''')

# ---------------------------------------------------------------------------
# Site engine process: keep retained DOM/style state and paint fingerprint.
# ---------------------------------------------------------------------------
Path("src/bin/veil_engine.rs").write_text(r'''use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use veil_engine::blocker::Blocker;
use veil_engine::dom::Dom;
use veil_engine::engine::{
    paint_fingerprint, DocumentView, InvalidationKind, RetainedDocument,
};
use veil_engine::renderer_protocol::{
    RenderRequest, RendererCommand, RendererReply, RuntimeDamage, RuntimeUpdate,
};
use veil_engine::script::{JavascriptSandbox, LiveJavascriptRuntime, ScriptReport};

const MAX_REQUEST_LINE: usize = 24 * 1024 * 1024;
const MAX_SESSIONS: usize = 32;

struct LiveSession {
    runtime: Option<LiveJavascriptRuntime>,
    report: ScriptReport,
    retained: RetainedDocument,
    blocker: Blocker,
    last_view: DocumentView,
    last_paint_fingerprint: u64,
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut sessions: HashMap<String, LiveSession> = HashMap::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.is_empty() {
            continue;
        }

        let reply = if line.len() > MAX_REQUEST_LINE {
            RendererReply::Render(Err("engine request exceeded 24 MiB safety limit".into()))
        } else {
            match serde_json::from_str::<RendererCommand>(&line) {
                Ok(RendererCommand::Render(request)) => {
                    let session_id = request.session_id.clone();
                    let (session, result) = create_session(request);
                    if sessions.len() >= MAX_SESSIONS && !sessions.contains_key(&session_id) {
                        if let Some(key) = sessions.keys().next().cloned() {
                            sessions.remove(&key);
                        }
                    }
                    sessions.insert(session_id, session);
                    RendererReply::Render(result)
                }
                Ok(RendererCommand::Event { session_id, event }) => {
                    let result = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| "live page session not found".to_owned())
                        .and_then(|session| {
                            let Some(runtime) = session.runtime.as_mut() else {
                                return Err("JavaScript is disabled for this page".into());
                            };
                            let (default_prevented, report) = runtime.dispatch_event(
                                event.node_id,
                                &event.event_type,
                                event.value.as_deref(),
                                &session.report.storage,
                            );
                            session.report = report;
                            Ok(update_session(session, default_prevented))
                        });
                    RendererReply::Runtime(result)
                }
                Ok(RendererCommand::Tick {
                    session_id,
                    elapsed_ms,
                }) => {
                    let result = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| "live page session not found".to_owned())
                        .and_then(|session| {
                            let Some(runtime) = session.runtime.as_mut() else {
                                return Err("JavaScript is disabled for this page".into());
                            };
                            session.report = runtime.tick(elapsed_ms, &session.report.storage);
                            Ok(update_session(session, false))
                        });
                    RendererReply::Runtime(result)
                }
                Err(err) => RendererReply::Render(Err(format!("invalid engine request: {err}"))),
            }
        };

        if serde_json::to_writer(&mut stdout, &reply).is_err() {
            break;
        }
        if stdout.write_all(b"\n").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

fn create_session(request: RenderRequest) -> (LiveSession, Result<DocumentView, String>) {
    let dom = Dom::parse(&request.html);
    let (runtime, report) = if request.privacy.javascript {
        let (runtime, report) = LiveJavascriptRuntime::new(
            &dom,
            &request.external_scripts,
            request.external_script_count,
            &request.storage,
        );
        (Some(runtime), report)
    } else {
        let report = JavascriptSandbox::default().run(
            &dom,
            false,
            &request.external_scripts,
            request.external_script_count,
            &request.storage,
        );
        (None, report)
    };

    let mut blocker = Blocker::default();
    if !request.custom_filters.trim().is_empty() {
        blocker.replace_custom_filters(request.custom_filters.clone());
    }
    let retained = RetainedDocument::new(
        &request.url,
        &request.html,
        request.privacy,
        &request.external_css,
        request.external_script_count,
        &report,
    );
    let mut view = retained.render(&blocker, &report);
    view.external_stylesheets = request.external_css.len();
    let fingerprint = paint_fingerprint(&view);
    let session = LiveSession {
        runtime,
        report,
        retained,
        blocker,
        last_view: view.clone(),
        last_paint_fingerprint: fingerprint,
    };
    (session, Ok(view))
}

fn update_session(session: &mut LiveSession, default_prevented: bool) -> RuntimeUpdate {
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

    let fingerprint = paint_fingerprint(&candidate);
    let paint_changed = fingerprint != session.last_paint_fingerprint;
    let metadata_changed = candidate.title != old_title || candidate.icon_url != old_icon;
    let actual_damage = if paint_changed {
        if requested_damage == InvalidationKind::Layout {
            RuntimeDamage::Layout
        } else {
            RuntimeDamage::Paint
        }
    } else if metadata_changed {
        RuntimeDamage::Metadata
    } else {
        RuntimeDamage::None
    };

    let view = if actual_damage == RuntimeDamage::None {
        session.last_view.script_report = session.report.clone();
        None
    } else {
        session.last_paint_fingerprint = fingerprint;
        session.last_view = candidate.clone();
        Some(candidate)
    };

    RuntimeUpdate {
        view,
        script_report: session.report.clone(),
        damage: actual_damage,
        default_prevented,
    }
}
''')

# Renderer host passes retained update metadata through to the browser process.
replace_once(
    "src/renderer_host.rs",
    "use crate::engine::{DocumentView, Engine};",
    "use crate::engine::{DocumentView, Engine};\nuse crate::script::ScriptReport;",
)
replace_once(
    "src/renderer_host.rs",
    "use crate::renderer_protocol::{DomEventRequest, RenderRequest, RendererCommand, RendererReply};",
    "use crate::renderer_protocol::{\n    DomEventRequest, RenderRequest, RendererCommand, RendererReply, RuntimeDamage,\n};",
)
replace_once(
    "src/renderer_host.rs",
    '''pub struct RuntimeHostUpdate {
    pub view: DocumentView,
    pub mode: RendererMode,
    pub default_prevented: bool,
}''',
    '''pub struct RuntimeHostUpdate {
    pub view: Option<DocumentView>,
    pub script_report: ScriptReport,
    pub damage: RuntimeDamage,
    pub mode: RendererMode,
    pub default_prevented: bool,
}''',
)
replace_once(
    "src/renderer_host.rs",
    '''            RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                mode,
                default_prevented: update.default_prevented,
            }),''',
    '''            RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                script_report: update.script_report,
                damage: update.damage,
                mode,
                default_prevented: update.default_prevented,
            }),''',
)
# Same mapping occurs once more in tick().
replace_once(
    "src/renderer_host.rs",
    '''            RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                mode,
                default_prevented: update.default_prevented,
            }),''',
    '''            RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                script_report: update.script_report,
                damage: update.damage,
                mode,
                default_prevented: update.default_prevented,
            }),''',
)

# Runtime interaction worker stores script state even when the engine correctly
# decides no page repaint is needed.
Path("src/runtime_interaction.rs").write_text(r'''use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
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
''')

# Browser UI applies only actual retained damage. No-paint timer ticks update the
# ScriptReport in place and do not replace the page or request a frame.
replace_once(
    "src/main.rs",
    "use veil_engine::renderer_protocol::DomEventRequest;",
    "use veil_engine::renderer_protocol::{DomEventRequest, RuntimeDamage};",
)
regex_once(
    "src/main.rs",
    r'''            match result\.result \{\n                Ok\(mut view\) => \{.*?\n                \}\n                Err\(error\) => \{\n                    // Interaction failures should not destroy a successfully loaded page\.\n                    self\.tabs\[index\]\.status = format!\("Live interaction unavailable: \{error\}"\);\n                \}\n            \}''',
    r'''            match result.result {
                Ok(update) => {
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
                }
                Err(error) => {
                    // Interaction failures should not destroy a successfully loaded page.
                    self.tabs[index].status = format!("Live interaction unavailable: {error}");
                }
            }''',
)

# Add a tiny status cue for the retained architecture without changing shell UI.
replace_once(
    "src/main.rs",
    '"{} ms · {} blocked · {} CSS · {} scripts · {}",',
    '"{} ms · {} blocked · {} CSS · {} scripts · {} · retained paint",',
)

print("Veil 0.8.7 Gecko-inspired architecture patch applied")
