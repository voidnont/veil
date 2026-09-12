use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::blocking::{Client, Response};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, CONTENT_TYPE, COOKIE, DNT,
    EXPIRES, LOCATION, SET_COOKIE, USER_AGENT, VARY,
};
use reqwest::StatusCode;
use url::Url;

use crate::blocker::{BlockContext, Blocker, ResourceType};
use crate::privacy::{is_third_party, site_key_for_url, SitePrivacy};
use crate::storage::SharedBrowserStorage;

const MAX_REDIRECTS: usize = 8;
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_STYLESHEET_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCRIPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 64 * 1024 * 1024;
const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;
const IMAGE_ACCEPT: &str =
    "image/webp,image/png,image/jpeg,image/gif,image/svg+xml,image/bmp,image/x-icon,*/*;q=0.1";

static SHARED_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
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
        while self.bytes > MEMORY_CACHE_LIMIT_BYTES || self.entries.len() > MEMORY_CACHE_MAX_ENTRIES
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
    if vary
        .split(',')
        .map(str::trim)
        .any(|value| value == "cookie" || value == "*")
    {
        return None;
    }
    cache_ttl(headers)
}

fn build_shared_http_client() -> Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.8 VeilEngine/0.8.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.7"));
    headers.insert(DNT, HeaderValue::from_static("1"));
    headers.insert("sec-gpc", HeaderValue::from_static("1"));

    // Like Gecko's Necko layer, all browser resource brokers share the same
    // underlying HTTP client so TLS sessions and keep-alive connections can be
    // reused across documents, images, stylesheets, fonts and scripts.
    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .tcp_nodelay(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to construct shared HTTPS client")
}

fn shared_http_client() -> Client {
    SHARED_HTTP_CLIENT
        .get_or_init(build_shared_http_client)
        .clone()
}

pub struct PageResponse {
    pub final_url: Url,
    pub body: String,
}

pub struct ImageResponse {
    pub final_url: Url,
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub struct BinaryResponse {
    pub final_url: Url,
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub struct TextResponse {
    pub final_url: Url,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
enum DocumentMethod {
    Get,
    PostForm(String),
    PostMultipart(Vec<MultipartPart>),
}

pub struct PrivacyNetwork {
    client: Client,
    blocker: Blocker,
    blocked_count: usize,
    blocked_events: Arc<Mutex<Vec<String>>>,
    storage: SharedBrowserStorage,
}

impl PrivacyNetwork {
    pub fn new() -> Self {
        Self::new_with_storage(SharedBrowserStorage::new())
    }

    pub fn new_with_storage(storage: SharedBrowserStorage) -> Self {
        let client = shared_http_client();

        Self {
            client,
            blocker: Blocker::default(),
            blocked_count: 0,
            blocked_events: Arc::new(Mutex::new(Vec::new())),
            storage,
        }
    }

    pub fn blocker(&self) -> &Blocker {
        &self.blocker
    }
    pub fn blocker_mut(&mut self) -> &mut Blocker {
        &mut self.blocker
    }
    pub fn blocked_count(&self) -> usize {
        self.blocked_count
    }

    pub fn get_document(
        &mut self,
        url: &Url,
        privacy: SitePrivacy,
    ) -> Result<PageResponse, String> {
        self.navigate_document(url, privacy, DocumentMethod::Get)
    }

    pub fn post_form_document(
        &mut self,
        url: &Url,
        privacy: SitePrivacy,
        body: &str,
    ) -> Result<PageResponse, String> {
        self.navigate_document(url, privacy, DocumentMethod::PostForm(body.to_owned()))
    }

    pub fn post_multipart_document(
        &mut self,
        url: &Url,
        privacy: SitePrivacy,
        parts: Vec<MultipartPart>,
    ) -> Result<PageResponse, String> {
        let total: usize = parts.iter().map(|part| part.data.len()).sum();
        if total > MAX_UPLOAD_BYTES {
            return Err("Multipart upload exceeds Veil Browser's 32 MiB safety limit.".into());
        }
        self.navigate_document(url, privacy, DocumentMethod::PostMultipart(parts))
    }

    fn navigate_document(
        &mut self,
        url: &Url,
        privacy: SitePrivacy,
        method: DocumentMethod,
    ) -> Result<PageResponse, String> {
        validate_http_url(url)?;
        let initial_top_level = url.clone();
        let mut current = url.clone();
        let mut method = method;

        for _ in 0..=MAX_REDIRECTS {
            let mut hop_privacy = privacy;
            if is_third_party(&initial_top_level, &current) {
                hop_privacy.shields = true;
            }
            self.enforce(
                &current,
                &initial_top_level,
                ResourceType::Document,
                hop_privacy,
            )?;

            let safe_method = matches!(&method, DocumentMethod::Get);
            let mut request = match &method {
                DocumentMethod::Get => self.client.get(current.clone()),
                DocumentMethod::PostForm(body) => self
                    .client
                    .post(current.clone())
                    .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(body.clone()),
                DocumentMethod::PostMultipart(parts) => {
                    let (boundary, body) = encode_multipart(parts)?;
                    self.client
                        .post(current.clone())
                        .header(
                            CONTENT_TYPE,
                            format!("multipart/form-data; boundary={boundary}"),
                        )
                        .body(body)
                }
            }
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml;q=0.9,text/plain;q=0.8,*/*;q=0.5",
            );

            if let Some(cookie) =
                self.storage
                    .cookie_header_for(&initial_top_level, &current, true, safe_method)
            {
                request = request.header(COOKIE, cookie);
            }
            request = request.header("priority", RequestPriority::High.header_value());
            let _permit = network_scheduler().acquire(RequestPriority::High);
            let response = request.send().map_err(|e| format!("Network error: {e}"))?;
            self.store_response_cookies(&initial_top_level, &current, &response);

            if is_redirect(response.status()) {
                let status = response.status();
                current = resolve_redirect(&current, &response)?;
                // 303 always becomes GET. 301/302 convert POST to GET. 307/308 preserve.
                if matches!(status, StatusCode::SEE_OTHER)
                    || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND)
                        && !matches!(&method, DocumentMethod::Get))
                {
                    method = DocumentMethod::Get;
                }
                continue;
            }

            let final_url = response.url().clone();
            let status = response.status();
            if !status.is_success() {
                return Err(format!("Server returned HTTP {status}"));
            }

            let content_type = content_type(&response);
            if !content_type.is_empty()
                && !content_type.contains("text/html")
                && !content_type.contains("application/xhtml")
                && !content_type.contains("text/plain")
            {
                return Err(format!("Unsupported content type: {content_type}"));
            }

            let bytes = read_limited(response, MAX_DOCUMENT_BYTES)
                .map_err(|e| format!("Failed to read response: {e}"))?;
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Ok(PageResponse { final_url, body });
        }

        Err("Too many redirects.".into())
    }

    pub fn get_stylesheet(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
    ) -> Result<TextResponse, String> {
        self.get_text_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Stylesheet,
            "text/css,*/*;q=0.1",
            MAX_STYLESHEET_BYTES,
        )
    }

    pub fn get_script(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
    ) -> Result<TextResponse, String> {
        self.get_text_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Script,
            "text/javascript,application/javascript,*/*;q=0.1",
            MAX_SCRIPT_BYTES,
        )
    }

    fn get_text_subresource(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
        resource_type: ResourceType,
        accept: &str,
        limit: usize,
    ) -> Result<TextResponse, String> {
        let response =
            self.get_binary_subresource(top_level, url, privacy, resource_type, accept, limit)?;
        Ok(TextResponse {
            final_url: response.final_url,
            body: String::from_utf8_lossy(&response.bytes).into_owned(),
        })
    }

    pub fn get_font(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
    ) -> Result<BinaryResponse, String> {
        self.get_binary_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Font,
            "font/woff2,font/woff,application/font-woff,application/octet-stream;q=0.5,*/*;q=0.1",
            MAX_FONT_BYTES,
        )
    }

    pub fn get_media(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
    ) -> Result<BinaryResponse, String> {
        self.get_binary_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Media,
            "video/*,audio/*,*/*;q=0.2",
            MAX_MEDIA_BYTES,
        )
    }

    fn get_binary_subresource(
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
        let initial_cookie = self.storage.cookie_header_for(top_level, url, false, true);
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

            if cacheable_resource(resource_type) && bytes.len() <= MEMORY_CACHE_MAX_ENTRY_BYTES {
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

    pub fn get_image(
        &mut self,
        top_level: &Url,
        url: &Url,
        privacy: SitePrivacy,
    ) -> Result<ImageResponse, String> {
        if !privacy.load_images {
            return Err("Images are disabled for this site.".into());
        }
        let response = self.get_binary_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Image,
            IMAGE_ACCEPT,
            MAX_IMAGE_BYTES,
        )?;
        // Match Gecko's image loader behavior: do not reject solely from the HTTP
        // Content-Type. The image worker sniffs the bytes first because real CDNs
        // sometimes serve valid image bytes as application/octet-stream or with a
        // stale/wrong MIME type.
        Ok(ImageResponse {
            final_url: response.final_url,
            bytes: response.bytes,
            content_type: response.content_type,
        })
    }

    fn store_response_cookies(&self, top_level: &Url, request: &Url, response: &Response) {
        for value in response.headers().get_all(SET_COOKIE).iter() {
            if let Ok(header) = value.to_str() {
                self.storage.store_set_cookie(top_level, request, header);
            }
        }
    }

    fn enforce(
        &mut self,
        request: &Url,
        top_level: &Url,
        resource_type: ResourceType,
        privacy: SitePrivacy,
    ) -> Result<(), String> {
        let third_party =
            resource_type != ResourceType::Document && is_third_party(top_level, request);
        if privacy.block_third_party && third_party {
            let reason = "third-party subresource blocked";
            self.record_block(request, reason, resource_type);
            return Err(reason.into());
        }
        if privacy.shields {
            let decision = self.blocker.check(&BlockContext {
                url: request,
                top_level,
                resource_type,
                third_party,
            });
            if decision.blocked {
                self.record_block(request, &decision.reason, resource_type);
                return Err(format!(
                    "Privacy Shield blocked request ({})",
                    decision.reason
                ));
            }
        }
        Ok(())
    }

    fn record_block(&mut self, url: &Url, reason: &str, resource_type: ResourceType) {
        self.blocked_count += 1;
        if let Ok(mut events) = self.blocked_events.lock() {
            events.push(format!("{:?}: {} — {}", resource_type, url, reason));
        }
    }

    pub fn take_blocked_events(&mut self) -> Vec<String> {
        if let Ok(mut events) = self.blocked_events.lock() {
            return std::mem::take(&mut *events);
        }
        Vec::new()
    }
}

fn encode_multipart(parts: &[MultipartPart]) -> Result<(String, Vec<u8>), String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let boundary = format!("----VeilBrowserBoundary{nonce:x}");
    let mut body = Vec::new();
    for part in parts {
        if part.data.len() > MAX_UPLOAD_BYTES
            || body.len().saturating_add(part.data.len()) > MAX_UPLOAD_BYTES
        {
            return Err("Multipart upload exceeds Veil Browser's 32 MiB safety limit.".into());
        }
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let name = escape_disposition(&part.name);
        match &part.filename {
            Some(filename) => {
                let filename = escape_disposition(filename);
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n").as_bytes());
                let content_type = part
                    .content_type
                    .as_deref()
                    .unwrap_or("application/octet-stream");
                body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
            }
            None => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
            }
        }
        body.extend_from_slice(&part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((boundary, body))
}

fn escape_disposition(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '\r' | '\n' | '\0'))
        .collect::<String>()
        .replace('"', "'")
}

fn validate_http_url(url: &Url) -> Result<(), String> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err("Only HTTP and HTTPS are supported.".into()),
    }
}

fn content_type(response: &Response) -> String {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

fn resolve_redirect(base: &Url, response: &Response) -> Result<Url, String> {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "Redirect response had no valid Location header.".to_owned())?;
    base.join(location)
        .map_err(|e| format!("Invalid redirect target: {e}"))
}

fn read_limited(response: Response, limit: usize) -> Result<Vec<u8>, String> {
    let mut reader = response.take((limit + 1) as u64);
    let mut bytes = Vec::with_capacity(limit.min(256 * 1024));
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("response read failed: {e}"))?;
    if bytes.len() > limit {
        return Err(format!(
            "response exceeds {} MiB safety limit",
            limit / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_accept_only_advertises_compiled_decoders() {
        assert!(!IMAGE_ACCEPT.contains("avif"));
        assert!(IMAGE_ACCEPT.contains("image/webp"));
        assert!(IMAGE_ACCEPT.contains("image/png"));
        assert!(IMAGE_ACCEPT.contains("image/jpeg"));
    }

    #[test]
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
        assert_eq!(
            RequestPriority::for_resource(ResourceType::Document),
            RequestPriority::High
        );
        assert_eq!(
            RequestPriority::for_resource(ResourceType::Stylesheet),
            RequestPriority::High
        );
        assert_eq!(
            RequestPriority::for_resource(ResourceType::Image),
            RequestPriority::Low
        );
        assert!(MAX_LOW_REQUESTS < MAX_NETWORK_REQUESTS);
    }

    #[test]
    fn multipart_encoder_emits_file_and_text_parts() {
        let parts = vec![
            MultipartPart {
                name: "q".into(),
                filename: None,
                content_type: None,
                data: b"hello".to_vec(),
            },
            MultipartPart {
                name: "upload".into(),
                filename: Some("a.txt".into()),
                content_type: Some("text/plain".into()),
                data: b"abc".to_vec(),
            },
        ];
        let (boundary, body) = encode_multipart(&parts).unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains(&boundary));
        assert!(text.contains("filename=\"a.txt\""));
        assert!(text.contains("hello"));
    }
}
