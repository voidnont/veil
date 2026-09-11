use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::{Client, Response};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, CONTENT_TYPE, COOKIE, DNT, LOCATION, SET_COOKIE, USER_AGENT,
};
use reqwest::StatusCode;
use url::Url;

use crate::blocker::{BlockContext, Blocker, ResourceType};
use crate::privacy::{is_third_party, SitePrivacy};
use crate::storage::SharedBrowserStorage;

const MAX_REDIRECTS: usize = 8;
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_STYLESHEET_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCRIPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 64 * 1024 * 1024;
const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

pub struct PageResponse {
    pub final_url: Url,
    pub body: String,
}

pub struct ImageResponse {
    pub final_url: Url,
    pub bytes: Vec<u8>,
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
    pub fn new() -> Self { Self::new_with_storage(SharedBrowserStorage::new()) }

    pub fn new_with_storage(storage: SharedBrowserStorage) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.0 VeilEngine/0.8.0"),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.7"));
        headers.insert(DNT, HeaderValue::from_static("1"));
        headers.insert("sec-gpc", HeaderValue::from_static("1"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to construct HTTPS client");

        Self {
            client,
            blocker: Blocker::default(),
            blocked_count: 0,
            blocked_events: Arc::new(Mutex::new(Vec::new())),
            storage,
        }
    }

    pub fn blocker(&self) -> &Blocker { &self.blocker }
    pub fn blocker_mut(&mut self) -> &mut Blocker { &mut self.blocker }
    pub fn blocked_count(&self) -> usize { self.blocked_count }

    pub fn get_document(&mut self, url: &Url, privacy: SitePrivacy) -> Result<PageResponse, String> {
        self.navigate_document(url, privacy, DocumentMethod::Get)
    }

    pub fn post_form_document(&mut self, url: &Url, privacy: SitePrivacy, body: &str) -> Result<PageResponse, String> {
        self.navigate_document(url, privacy, DocumentMethod::PostForm(body.to_owned()))
    }

    pub fn post_multipart_document(
        &mut self,
        url: &Url,
        privacy: SitePrivacy,
        parts: Vec<MultipartPart>,
    ) -> Result<PageResponse, String> {
        let total: usize = parts.iter().map(|part| part.data.len()).sum();
        if total > MAX_UPLOAD_BYTES { return Err("Multipart upload exceeds Veil Browser's 32 MiB safety limit.".into()); }
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
            self.enforce(&current, &initial_top_level, ResourceType::Document, hop_privacy)?;

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
                        .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                        .body(body)
                }
            }
            .header(ACCEPT, "text/html,application/xhtml+xml;q=0.9,text/plain;q=0.8,*/*;q=0.5");

            if let Some(cookie) = self.storage.cookie_header_for(&initial_top_level, &current, true, safe_method) {
                request = request.header(COOKIE, cookie);
            }
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
            if !status.is_success() { return Err(format!("Server returned HTTP {status}")); }

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

    pub fn get_stylesheet(&mut self, top_level: &Url, url: &Url, privacy: SitePrivacy) -> Result<TextResponse, String> {
        self.get_text_subresource(top_level, url, privacy, ResourceType::Stylesheet, "text/css,*/*;q=0.1", MAX_STYLESHEET_BYTES)
    }

    pub fn get_script(&mut self, top_level: &Url, url: &Url, privacy: SitePrivacy) -> Result<TextResponse, String> {
        self.get_text_subresource(top_level, url, privacy, ResourceType::Script, "text/javascript,application/javascript,*/*;q=0.1", MAX_SCRIPT_BYTES)
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
        let response = self.get_binary_subresource(top_level, url, privacy, resource_type, accept, limit)?;
        Ok(TextResponse { final_url: response.final_url, body: String::from_utf8_lossy(&response.bytes).into_owned() })
    }

    pub fn get_font(&mut self, top_level: &Url, url: &Url, privacy: SitePrivacy) -> Result<BinaryResponse, String> {
        self.get_binary_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Font,
            "font/woff2,font/woff,application/font-woff,application/octet-stream;q=0.5,*/*;q=0.1",
            MAX_FONT_BYTES,
        )
    }

    pub fn get_media(&mut self, top_level: &Url, url: &Url, privacy: SitePrivacy) -> Result<BinaryResponse, String> {
        self.get_binary_subresource(top_level, url, privacy, ResourceType::Media, "video/*,audio/*,*/*;q=0.2", MAX_MEDIA_BYTES)
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
        let mut current = url.clone();
        for _ in 0..=MAX_REDIRECTS {
            self.enforce(&current, top_level, resource_type, privacy)?;
            let mut request = self.client.get(current.clone()).header(ACCEPT, accept);
            if let Some(cookie) = self.storage.cookie_header_for(top_level, &current, false, true) {
                request = request.header(COOKIE, cookie);
            }
            let response = request.send().map_err(|e| format!("Subresource request failed: {e}"))?;
            self.store_response_cookies(top_level, &current, &response);
            if is_redirect(response.status()) {
                current = resolve_redirect(&current, &response)?;
                continue;
            }
            let final_url = response.url().clone();
            let status = response.status();
            if !status.is_success() { return Err(format!("Subresource returned HTTP {status}")); }
            let response_type = content_type(&response);
            let bytes = read_limited(response, limit).map_err(|e| format!("Failed to read subresource: {e}"))?;
            return Ok(BinaryResponse { final_url, bytes, content_type: response_type });
        }
        Err("Too many subresource redirects.".into())
    }

    pub fn get_image(&mut self, top_level: &Url, url: &Url, privacy: SitePrivacy) -> Result<ImageResponse, String> {
        if !privacy.load_images { return Err("Images are disabled for this site.".into()); }
        let response = self.get_binary_subresource(
            top_level,
            url,
            privacy,
            ResourceType::Image,
            "image/avif,image/webp,image/png,image/jpeg,image/gif,image/x-icon,*/*;q=0.2",
            MAX_IMAGE_BYTES,
        )?;
        if !response.content_type.is_empty() && !response.content_type.starts_with("image/") {
            return Err(format!("Blocked non-image response: {}", response.content_type));
        }
        Ok(ImageResponse { final_url: response.final_url, bytes: response.bytes })
    }

    fn store_response_cookies(&self, top_level: &Url, request: &Url, response: &Response) {
        for value in response.headers().get_all(SET_COOKIE).iter() {
            if let Ok(header) = value.to_str() { self.storage.store_set_cookie(top_level, request, header); }
        }
    }

    fn enforce(&mut self, request: &Url, top_level: &Url, resource_type: ResourceType, privacy: SitePrivacy) -> Result<(), String> {
        let third_party = resource_type != ResourceType::Document && is_third_party(top_level, request);
        if privacy.block_third_party && third_party {
            let reason = "third-party subresource blocked";
            self.record_block(request, reason, resource_type);
            return Err(reason.into());
        }
        if privacy.shields {
            let decision = self.blocker.check(&BlockContext { url: request, top_level, resource_type, third_party });
            if decision.blocked {
                self.record_block(request, &decision.reason, resource_type);
                return Err(format!("Privacy Shield blocked request ({})", decision.reason));
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
        if let Ok(mut events) = self.blocked_events.lock() { return std::mem::take(&mut *events); }
        Vec::new()
    }
}

fn encode_multipart(parts: &[MultipartPart]) -> Result<(String, Vec<u8>), String> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let boundary = format!("----VeilBrowserBoundary{nonce:x}");
    let mut body = Vec::new();
    for part in parts {
        if part.data.len() > MAX_UPLOAD_BYTES || body.len().saturating_add(part.data.len()) > MAX_UPLOAD_BYTES {
            return Err("Multipart upload exceeds Veil Browser's 32 MiB safety limit.".into());
        }
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let name = escape_disposition(&part.name);
        match &part.filename {
            Some(filename) => {
                let filename = escape_disposition(filename);
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n").as_bytes());
                let content_type = part.content_type.as_deref().unwrap_or("application/octet-stream");
                body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
            }
            None => {
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
            }
        }
        body.extend_from_slice(&part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((boundary, body))
}

fn escape_disposition(value: &str) -> String {
    value.chars().filter(|ch| !matches!(ch, '\r' | '\n' | '\0')).collect::<String>().replace('"', "'")
}

fn validate_http_url(url: &Url) -> Result<(), String> {
    match url.scheme() { "http" | "https" => Ok(()), _ => Err("Only HTTP and HTTPS are supported.".into()) }
}

fn content_type(response: &Response) -> String {
    response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or("").to_ascii_lowercase()
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT)
}

fn resolve_redirect(base: &Url, response: &Response) -> Result<Url, String> {
    let location = response.headers().get(LOCATION).and_then(|value| value.to_str().ok()).ok_or_else(|| "Redirect response had no valid Location header.".to_owned())?;
    base.join(location).map_err(|e| format!("Invalid redirect target: {e}"))
}

fn read_limited(response: Response, limit: usize) -> Result<Vec<u8>, String> {
    let mut reader = response.take((limit + 1) as u64);
    let mut bytes = Vec::with_capacity(limit.min(256 * 1024));
    reader.read_to_end(&mut bytes).map_err(|e| format!("response read failed: {e}"))?;
    if bytes.len() > limit { return Err(format!("response exceeds {} MiB safety limit", limit / (1024 * 1024))); }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_encoder_emits_file_and_text_parts() {
        let parts = vec![
            MultipartPart { name: "q".into(), filename: None, content_type: None, data: b"hello".to_vec() },
            MultipartPart { name: "upload".into(), filename: Some("a.txt".into()), content_type: Some("text/plain".into()), data: b"abc".to_vec() },
        ];
        let (boundary, body) = encode_multipart(&parts).unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains(&boundary));
        assert!(text.contains("filename=\"a.txt\""));
        assert!(text.contains("hello"));
    }
}
