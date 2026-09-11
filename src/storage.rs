use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::privacy::{site_key_for_url};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptStorageSnapshot {
    pub local: HashMap<String, String>,
    pub session: HashMap<String, String>,
    pub cookie: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SameSite {
    Strict,
    Lax,
    None,
}

#[derive(Debug, Clone)]
struct Cookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    host_only: bool,
    same_site: SameSite,
    #[allow(dead_code)]
    partitioned: bool,
    expires: Option<SystemTime>,
    created: u64,
}

#[derive(Default)]
struct BrowserStorageState {
    cookies: HashMap<String, Vec<Cookie>>,
    local: HashMap<String, HashMap<String, String>>,
    session: HashMap<String, HashMap<String, String>>,
    cookie_sequence: u64,
}

#[derive(Clone, Default)]
pub struct SharedBrowserStorage {
    inner: Arc<Mutex<BrowserStorageState>>,
}

impl SharedBrowserStorage {
    pub fn new() -> Self { Self::default() }

    pub fn script_snapshot(&self, top_level: &Url) -> ScriptStorageSnapshot {
        let partition = site_key_for_url(top_level);
        let (local, session) = if let Ok(state) = self.inner.lock() {
            (
                state.local.get(&partition).cloned().unwrap_or_default(),
                state.session.get(&partition).cloned().unwrap_or_default(),
            )
        } else {
            (HashMap::new(), HashMap::new())
        };
        ScriptStorageSnapshot {
            local,
            session,
            cookie: self.script_cookie_string(top_level, top_level),
        }
    }

    pub fn apply_script_snapshot(&self, top_level: &Url, snapshot: &ScriptStorageSnapshot) {
        let partition = site_key_for_url(top_level);
        if partition.is_empty() { return; }
        let Ok(mut state) = self.inner.lock() else { return; };
        state.local.insert(partition.clone(), snapshot.local.clone());
        state.session.insert(partition, snapshot.session.clone());
    }

    pub fn clear_site(&self, top_level: &Url) {
        let partition = site_key_for_url(top_level);
        let Ok(mut state) = self.inner.lock() else { return; };
        state.cookies.remove(&partition);
        state.local.remove(&partition);
        state.session.remove(&partition);
    }

    pub fn clear_all(&self) {
        if let Ok(mut state) = self.inner.lock() { *state = BrowserStorageState::default(); }
    }

    /// Compatibility wrapper for subresource-style requests.
    pub fn cookie_header(&self, top_level: &Url, request: &Url) -> Option<String> {
        self.cookie_header_for(top_level, request, false, true)
    }

    /// Builds a partitioned Cookie header with Secure, Domain, Path, expiry and
    /// SameSite enforcement. `safe_method` should be true for GET/HEAD-style
    /// navigations and false for POST-style navigations.
    pub fn cookie_header_for(
        &self,
        top_level: &Url,
        request: &Url,
        is_top_level_navigation: bool,
        safe_method: bool,
    ) -> Option<String> {
        let partition = site_key_for_url(top_level);
        let host = request.host_str()?.to_ascii_lowercase();
        let request_path = if request.path().is_empty() { "/" } else { request.path() };
        let secure_request = request.scheme() == "https";
        let same_site = same_site(top_level, request);
        let now = SystemTime::now();

        let Ok(mut state) = self.inner.lock() else { return None; };
        let jar = state.cookies.get_mut(&partition)?;
        jar.retain(|cookie| cookie.expires.map(|deadline| deadline > now).unwrap_or(true));

        let mut eligible: Vec<&Cookie> = jar.iter().filter(|cookie| {
            if cookie.secure && !secure_request { return false; }
            let domain_matches = if cookie.host_only {
                host == cookie.domain
            } else {
                host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain))
            };
            if !domain_matches || !path_matches(request_path, &cookie.path) { return false; }

            match cookie.same_site {
                SameSite::Strict => same_site,
                SameSite::Lax => same_site || (is_top_level_navigation && safe_method),
                SameSite::None => true,
            }
        }).collect();

        // RFC-style preference: longest path first, then oldest creation time.
        eligible.sort_by(|a, b| b.path.len().cmp(&a.path.len()).then_with(|| a.created.cmp(&b.created)));
        if eligible.is_empty() { return None; }
        Some(eligible.into_iter().map(|cookie| format!("{}={}", cookie.name, cookie.value)).collect::<Vec<_>>().join("; "))
    }

    pub fn store_set_cookie(&self, top_level: &Url, request: &Url, header: &str) {
        let partition = site_key_for_url(top_level);
        let Some(request_host) = request.host_str().map(|host| host.to_ascii_lowercase()) else { return; };
        if partition.is_empty() { return; }

        let mut parts = header.split(';');
        let Some(first) = parts.next() else { return; };
        let Some((name, value)) = first.split_once('=') else { return; };
        let name = name.trim();
        if name.is_empty() || contains_cookie_ctl(name) { return; }

        let mut domain = request_host.clone();
        let mut host_only = true;
        let mut path = default_cookie_path(request.path());
        let mut secure = false;
        let mut http_only = false;
        let mut same_site = SameSite::Lax;
        let mut partitioned = false;
        let mut expires = None;
        let mut delete = false;

        for raw in parts {
            let raw = raw.trim();
            let (key, value) = raw
                .split_once('=')
                .map(|(key, value)| (key.trim().to_ascii_lowercase(), Some(value.trim())))
                .unwrap_or((raw.to_ascii_lowercase(), None));
            match key.as_str() {
                "domain" => {
                    if let Some(value) = value {
                        let candidate = value.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();
                        if candidate.is_empty() { return; }
                        if request_host == candidate || request_host.ends_with(&format!(".{}", candidate)) {
                            domain = candidate;
                            host_only = false;
                        } else {
                            return;
                        }
                    }
                }
                "path" => if let Some(value) = value { if value.starts_with('/') { path = value.to_owned(); } },
                "secure" => secure = true,
                "httponly" => http_only = true,
                "partitioned" => partitioned = true,
                "samesite" => {
                    same_site = match value.unwrap_or("").to_ascii_lowercase().as_str() {
                        "strict" => SameSite::Strict,
                        "none" => SameSite::None,
                        _ => SameSite::Lax,
                    };
                }
                "max-age" => {
                    if let Some(seconds) = value.and_then(|value| value.parse::<i64>().ok()) {
                        if seconds <= 0 {
                            delete = true;
                        } else {
                            expires = SystemTime::now().checked_add(std::time::Duration::from_secs(seconds as u64));
                        }
                    }
                }
                "expires" if expires.is_none() => {
                    if let Some(value) = value {
                        if let Ok(parsed) = httpdate::parse_http_date(value) {
                            if parsed <= SystemTime::now() { delete = true; }
                            expires = Some(parsed);
                        }
                    }
                }
                _ => {}
            }
        }

        // Modern browser constraints.
        if secure && request.scheme() != "https" { return; }
        if same_site == SameSite::None && !secure { return; }
        if partitioned && !secure { return; }
        if name.starts_with("__Secure-") && !secure { return; }
        if name.starts_with("__Host-") && (!secure || !host_only || path != "/") { return; }

        let Ok(mut state) = self.inner.lock() else { return; };
        let sequence = state.cookie_sequence;
        state.cookie_sequence = state.cookie_sequence.saturating_add(1);
        let jar = state.cookies.entry(partition).or_default();
        jar.retain(|cookie| !(cookie.name == name && cookie.domain == domain && cookie.path == path));
        if !delete {
            jar.push(Cookie {
                name: name.to_owned(),
                value: value.trim().to_owned(),
                domain,
                path,
                secure,
                http_only,
                host_only,
                same_site,
                partitioned,
                expires,
                created: sequence,
            });
        }
    }

    /// Returns a document.cookie-style string containing only non-HttpOnly
    /// cookies visible to the current document partition.
    pub fn script_cookie_string(&self, top_level: &Url, document: &Url) -> String {
        let partition = site_key_for_url(top_level);
        let Some(host) = document.host_str().map(|host| host.to_ascii_lowercase()) else { return String::new(); };
        let path = if document.path().is_empty() { "/" } else { document.path() };
        let secure = document.scheme() == "https";
        let now = SystemTime::now();
        let Ok(mut state) = self.inner.lock() else { return String::new(); };
        let Some(jar) = state.cookies.get_mut(&partition) else { return String::new(); };
        jar.retain(|cookie| cookie.expires.map(|deadline| deadline > now).unwrap_or(true));
        jar.iter()
            .filter(|cookie| {
                !cookie.http_only
                    && (!cookie.secure || secure)
                    && (if cookie.host_only { host == cookie.domain } else { host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain)) })
                    && path_matches(path, &cookie.path)
            })
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn cookie_count_for_site(&self, top_level: &Url) -> usize {
        let partition = site_key_for_url(top_level);
        let Ok(state) = self.inner.lock() else { return 0; };
        state.cookies.get(&partition).map(Vec::len).unwrap_or(0)
    }
}

fn same_site(top_level: &Url, request: &Url) -> bool {
    top_level.scheme() == request.scheme() && site_key_for_url(top_level) == site_key_for_url(request)
}

fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    if request_path == cookie_path { return true; }
    if !request_path.starts_with(cookie_path) { return false; }
    cookie_path.ends_with('/') || request_path.as_bytes().get(cookie_path.len()) == Some(&b'/')
}

fn default_cookie_path(path: &str) -> String {
    if !path.starts_with('/') || path == "/" { return "/".into(); }
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(index) => path[..index].to_owned(),
    }
}

fn contains_cookie_ctl(value: &str) -> bool {
    value.bytes().any(|byte| byte <= 0x20 || byte == 0x7f || matches!(byte, b'(' | b')' | b'<' | b'>' | b'@' | b',' | b';' | b':' | b'\\' | b'"' | b'/' | b'[' | b']' | b'?' | b'=' | b'{' | b'}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_partitioned_by_top_level_site() {
        let storage = SharedBrowserStorage::new();
        let a = Url::parse("https://a.example/").unwrap();
        let b = Url::parse("https://other.example.net/").unwrap();
        let cdn = Url::parse("https://cdn.example/assets").unwrap();
        storage.store_set_cookie(&a, &cdn, "sid=one; Domain=example; Path=/; Secure; SameSite=None");
        assert_eq!(storage.cookie_header_for(&a, &cdn, false, true).as_deref(), Some("sid=one"));
        assert!(storage.cookie_header_for(&b, &cdn, false, true).is_none());
    }

    #[test]
    fn samesite_strict_blocks_cross_site_context() {
        let storage = SharedBrowserStorage::new();
        let top = Url::parse("https://site.test/").unwrap();
        let cross = Url::parse("https://idp.example/login").unwrap();
        storage.store_set_cookie(&top, &cross, "sid=x; Path=/; Secure; SameSite=Strict");
        assert!(storage.cookie_header_for(&top, &cross, true, true).is_none());
    }

    #[test]
    fn host_prefix_requires_secure_host_only_root_cookie() {
        let storage = SharedBrowserStorage::new();
        let url = Url::parse("https://example.com/").unwrap();
        storage.store_set_cookie(&url, &url, "__Host-id=ok; Secure; Path=/");
        assert_eq!(storage.cookie_header_for(&url, &url, true, true).as_deref(), Some("__Host-id=ok"));
    }

    #[test]
    fn httponly_cookie_is_not_exposed_to_script() {
        let storage = SharedBrowserStorage::new();
        let url = Url::parse("https://example.com/").unwrap();
        storage.store_set_cookie(&url, &url, "secret=x; Secure; HttpOnly; Path=/");
        assert!(storage.script_cookie_string(&url, &url).is_empty());
    }
}
