use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use serde_json::Value;
use url::Url;

use crate::engine::{DocumentView, Engine, WebFontResource};
use crate::net::{MultipartPart, PrivacyNetwork};
use crate::privacy::SitePrivacy;
use crate::renderer_host::{RendererHost, RendererMode};
use crate::renderer_protocol::RenderRequest;
use crate::storage::SharedBrowserStorage;

const MAX_STYLESHEETS_PER_PAGE: usize = 24;
const MAX_SCRIPTS_PER_PAGE: usize = 12;
const MAX_WEB_FONTS_PER_PAGE: usize = 8;
const MAX_TOTAL_STYLESHEET_BYTES: usize = 6 * 1024 * 1024;
const MAX_TOTAL_SCRIPT_BYTES: usize = 8 * 1024 * 1024;
const HEAVY_DOCUMENT_BYTES: usize = 1024 * 1024;
const GUARDED_RENDER_HTML_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum NavigationMethod {
    Get,
    PostForm(String),
    PostMultipart(Vec<MultipartPart>),
}

pub struct LoadRequest {
    pub tab_id: u64,
    pub generation: u64,
    pub url: String,
    pub privacy: SitePrivacy,
    pub custom_filters: String,
    pub storage: SharedBrowserStorage,
    pub method: NavigationMethod,
}

pub struct LoadResult {
    pub tab_id: u64,
    pub generation: u64,
    pub elapsed_ms: u128,
    pub blocked_count: usize,
    pub blocked_events: Vec<String>,
    pub renderer_mode: RendererMode,
    pub result: Result<DocumentView, String>,
}

pub struct PageLoader {
    sender: Sender<LoadResult>,
    receiver: Receiver<LoadResult>,
    latest_generation: Arc<Mutex<HashMap<u64, u64>>>,
}

impl PageLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            latest_generation: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&self, request: LoadRequest) {
        if let Ok(mut latest) = self.latest_generation.lock() {
            latest.insert(request.tab_id, request.generation);
        }
        let sender = self.sender.clone();
        let latest_generation = self.latest_generation.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let mut network = PrivacyNetwork::new_with_storage(request.storage.clone());
            if !request.custom_filters.trim().is_empty() {
                network
                    .blocker_mut()
                    .replace_custom_filters(request.custom_filters.clone());
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_document(&mut network, &request, &latest_generation)
            }))
            .unwrap_or_else(|_| Err("Veil recovered from an internal page-load panic.".into()));
            let blocked_count = network.blocked_count();
            let blocked_events = network.take_blocked_events();
            let (renderer_mode, result) = match result {
                Ok((view, mode)) => (mode, Ok(view)),
                Err(error) => (RendererMode::InProcessFallback, Err(error)),
            };

            let _ = sender.send(LoadResult {
                tab_id: request.tab_id,
                generation: request.generation,
                elapsed_ms: started.elapsed().as_millis(),
                blocked_count,
                blocked_events,
                renderer_mode,
                result,
            });
        });
    }

    pub fn try_recv(&self) -> Option<LoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn load_document(
    network: &mut PrivacyNetwork,
    request: &LoadRequest,
    latest_generation: &Arc<Mutex<HashMap<u64, u64>>>,
) -> Result<(DocumentView, RendererMode), String> {
    let url = Url::parse(&request.url).map_err(|e| format!("Invalid URL: {e}"))?;
    let response = match &request.method {
        NavigationMethod::Get => network.get_document(&url, request.privacy)?,
        NavigationMethod::PostForm(body) => {
            network.post_form_document(&url, request.privacy, body)?
        }
        NavigationMethod::PostMultipart(parts) => {
            network.post_multipart_document(&url, request.privacy, parts.clone())?
        }
    };
    ensure_navigation_current(latest_generation, request)?;
    let final_url = response.final_url.clone();
    let guarded_site = is_guarded_heavy_site(&final_url);
    let mut render_privacy = request.privacy;
    let mut render_html = response.body;
    if guarded_site {
        render_privacy.javascript = false;
        render_html = prepare_guarded_html(&render_html);
    }

    // Resource discovery remains deliberately bounded. Full HTML/CSS/JS parsing
    // and render-tree creation happens inside veil-engine when available.
    let discovery = Engine::default();
    let stylesheet_urls = discovery.discover_stylesheets(&render_html, &final_url);
    let mut external_css = Vec::new();
    let mut external_css_bytes = 0usize;
    let mut web_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();
    let stylesheet_limit = if guarded_site {
        8
    } else {
        MAX_STYLESHEETS_PER_PAGE
    };
    let stylesheet_byte_limit = if guarded_site {
        2 * 1024 * 1024
    } else {
        MAX_TOTAL_STYLESHEET_BYTES
    };
    for stylesheet in stylesheet_urls.into_iter().take(stylesheet_limit) {
        ensure_navigation_current(latest_generation, request)?;
        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, render_privacy) {
            if external_css_bytes.saturating_add(resource.body.len()) > stylesheet_byte_limit {
                break;
            }
            external_css_bytes = external_css_bytes.saturating_add(resource.body.len());
            if !guarded_site && web_fonts.len() < MAX_WEB_FONTS_PER_PAGE {
                for source in discovery.discover_web_fonts(&resource.body, &resource.final_url) {
                    ensure_navigation_current(latest_generation, request)?;
                    if web_fonts.len() >= MAX_WEB_FONTS_PER_PAGE {
                        break;
                    }
                    if !seen_fonts.insert(source.url.to_string()) {
                        continue;
                    }
                    if let Ok(font) = network.get_font(&final_url, &source.url, request.privacy) {
                        web_fonts.push(WebFontResource {
                            family: source.family,
                            url: font.final_url.to_string(),
                            bytes: font.bytes,
                        });
                    }
                }
            }
            external_css.push(resource.body);
        }
    }

    let script_urls = if render_privacy.javascript {
        discovery.discover_external_scripts(&render_html, &final_url)
    } else {
        Vec::new()
    };
    let heavy_document = render_html.len() >= HEAVY_DOCUMENT_BYTES || script_urls.len() > 16;
    let script_limit = if heavy_document {
        4
    } else {
        MAX_SCRIPTS_PER_PAGE
    };
    let script_byte_limit = if heavy_document {
        2 * 1024 * 1024
    } else {
        MAX_TOTAL_SCRIPT_BYTES
    };
    let mut external_scripts = Vec::new();
    let mut external_script_bytes = 0usize;
    if render_privacy.javascript {
        for script in script_urls.into_iter().take(script_limit) {
            ensure_navigation_current(latest_generation, request)?;
            if let Ok(resource) = network.get_script(&final_url, &script, request.privacy) {
                if external_script_bytes.saturating_add(resource.body.len()) > script_byte_limit {
                    break;
                }
                external_script_bytes = external_script_bytes.saturating_add(resource.body.len());
                external_scripts.push(resource.body);
            }
        }
    }
    let external_script_count = external_scripts.len();

    let storage_snapshot = request.storage.script_snapshot(&final_url);
    let render_request = RenderRequest {
        session_id: format!("tab-{}-{}", request.tab_id, request.generation),
        url: final_url.to_string(),
        html: render_html,
        privacy: render_privacy,
        custom_filters: request.custom_filters.clone(),
        external_css,
        external_scripts,
        external_script_count,
        storage: storage_snapshot,
    };

    ensure_navigation_current(latest_generation, request)?;
    let (mut view, mode) = RendererHost::default().render(render_request)?;
    request
        .storage
        .apply_script_snapshot(&final_url, &view.script_report.storage);
    for cookie in &view.script_report.cookie_writes {
        request
            .storage
            .store_set_cookie(&final_url, &final_url, cookie);
    }
    view.web_fonts = web_fonts;
    Ok((view, mode))
}

fn ensure_navigation_current(
    latest_generation: &Arc<Mutex<HashMap<u64, u64>>>,
    request: &LoadRequest,
) -> Result<(), String> {
    let current = latest_generation
        .lock()
        .ok()
        .and_then(|latest| latest.get(&request.tab_id).copied())
        .unwrap_or(request.generation);
    if current == request.generation {
        Ok(())
    } else {
        Err("Navigation superseded by a newer request.".into())
    }
}

fn is_guarded_heavy_site(url: &Url) -> bool {
    url.host_str()
        .map(str::to_ascii_lowercase)
        .map(|host| {
            host == "youtube.com"
                || host.ends_with(".youtube.com")
                || host == "youtu.be"
                || host.ends_with(".youtu.be")
        })
        .unwrap_or(false)
}

fn prepare_guarded_html(html: &str) -> String {
    let gallery = youtube_static_gallery(html);
    let safe = cap_render_html(strip_script_blocks(html));
    if gallery.is_empty() {
        safe
    } else {
        inject_before_body_end(safe, &gallery)
    }
}

#[derive(Debug, Clone)]
struct YoutubeCard {
    video_id: String,
    title: String,
    thumbnail: String,
}

fn youtube_static_gallery(html: &str) -> String {
    let Some(raw_json) = extract_yt_initial_data(html) else {
        return String::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_json) else {
        return String::new();
    };

    let mut cards = Vec::new();
    let mut seen = HashSet::new();
    collect_youtube_cards(&value, &mut cards, &mut seen);
    if cards.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        r#"<section id="veil-youtube-static" style="padding:24px 18px 40px 18px">
<h2 style="font-size:24px;margin:0 0 8px 0">YouTube</h2>
<p style="color:#aaa;margin:0 0 18px 0">Veil safe view · static feed recovered without running YouTube's full JavaScript app.</p>
<div style="display:flex;flex-wrap:wrap;gap:18px">"#,
    );
    for card in cards.into_iter().take(24) {
        let title = escape_html(&card.title);
        let thumb = escape_html_attr(&card.thumbnail);
        let href = format!("https://www.youtube.com/watch?v={}", card.video_id);
        out.push_str(&format!(
            r#"<div style="width:300px;max-width:100%;padding:8px"><img src="{thumb}" alt="{title}" width="300" style="width:300px;max-width:100%;border-radius:12px"/><p style="margin:8px 0 0 0;font-size:15px"><a href="{href}">{title}</a></p></div>"#,
        ));
    }
    out.push_str("</div></section>");
    out
}

fn collect_youtube_cards(value: &Value, cards: &mut Vec<YoutubeCard>, seen: &mut HashSet<String>) {
    if cards.len() >= 24 {
        return;
    }
    match value {
        Value::Object(map) => {
            for key in ["videoRenderer", "gridVideoRenderer", "compactVideoRenderer"] {
                if let Some(renderer) = map.get(key) {
                    if let Some(card) = youtube_card_from_renderer(renderer) {
                        if seen.insert(card.video_id.clone()) {
                            cards.push(card);
                            if cards.len() >= 24 {
                                return;
                            }
                        }
                    }
                }
            }
            if let Some(renderer) = map.get("lockupViewModel") {
                if let Some(card) = youtube_card_from_lockup(renderer) {
                    if seen.insert(card.video_id.clone()) {
                        cards.push(card);
                        if cards.len() >= 24 {
                            return;
                        }
                    }
                }
            }
            if let Some(renderer) = map.get("shortsLockupViewModel") {
                if let Some(card) = youtube_card_from_shorts_lockup(renderer) {
                    if seen.insert(card.video_id.clone()) {
                        cards.push(card);
                        if cards.len() >= 24 {
                            return;
                        }
                    }
                }
            }
            for child in map.values() {
                collect_youtube_cards(child, cards, seen);
                if cards.len() >= 24 {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_youtube_cards(child, cards, seen);
                if cards.len() >= 24 {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn youtube_card_from_renderer(renderer: &Value) -> Option<YoutubeCard> {
    let video_id = renderer.get("videoId")?.as_str()?.trim();
    if video_id.is_empty() {
        return None;
    }
    let title_value = renderer.get("title")?;
    let title = title_value
        .get("runs")
        .and_then(Value::as_array)
        .and_then(|runs| runs.first())
        .and_then(|run| run.get("text"))
        .and_then(Value::as_str)
        .or_else(|| title_value.get("simpleText").and_then(Value::as_str))?
        .trim();
    if title.is_empty() {
        return None;
    }
    let thumbnail = thumbnail_from_array(
        renderer
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("thumbnails")),
    )
    .unwrap_or_else(|| canonical_youtube_thumbnail(video_id));

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail,
    })
}

fn youtube_card_from_lockup(renderer: &Value) -> Option<YoutubeCard> {
    if renderer.get("contentType").and_then(Value::as_str) != Some("LOCKUP_CONTENT_TYPE_VIDEO") {
        return None;
    }
    let video_id = renderer.get("contentId")?.as_str()?.trim();
    if video_id.is_empty() {
        return None;
    }
    let title = renderer
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)?
        .trim();
    if title.is_empty() {
        return None;
    }
    let thumbnail = thumbnail_from_array(
        renderer.pointer("/contentImage/thumbnailViewModel/image/sources"),
    )
    .or_else(|| {
        thumbnail_from_array(renderer.pointer(
            "/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/image/sources",
        ))
    })
    .unwrap_or_else(|| canonical_youtube_thumbnail(video_id));

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail,
    })
}

fn youtube_card_from_shorts_lockup(renderer: &Value) -> Option<YoutubeCard> {
    let endpoint_video_id = renderer
        .pointer("/onTap/innertubeCommand/reelWatchEndpoint/videoId")
        .and_then(Value::as_str);
    let entity_video_id = renderer
        .get("entityId")
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix("shorts-shelf-item-"));
    let video_id = endpoint_video_id.or(entity_video_id)?.trim();
    if video_id.is_empty() {
        return None;
    }
    let title = renderer
        .pointer("/overlayMetadata/primaryText/content")
        .and_then(Value::as_str)
        .or_else(|| renderer.get("accessibilityText").and_then(Value::as_str))?
        .trim();
    if title.is_empty() {
        return None;
    }
    let thumbnail = thumbnail_from_array(renderer.pointer("/thumbnail/sources"))
        .or_else(|| {
            thumbnail_from_array(
                renderer.pointer("/thumbnailViewModel/thumbnailViewModel/image/sources"),
            )
        })
        .unwrap_or_else(|| canonical_youtube_thumbnail(video_id));

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail,
    })
}

fn thumbnail_from_array(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .rev()
                .filter_map(|entry| entry.get("url").and_then(Value::as_str))
                .find(|url| url.starts_with("https://") || url.starts_with("http://"))
        })
        .map(str::to_owned)
}

fn canonical_youtube_thumbnail(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

fn extract_yt_initial_data(html: &str) -> Option<&str> {
    for marker in ["var ytInitialData = ", "ytInitialData = "] {
        let Some(marker_pos) = html.find(marker) else {
            continue;
        };
        let after = marker_pos + marker.len();
        let Some(relative_start) = html[after..].find('{') else {
            continue;
        };
        let start = after + relative_start;
        if let Some(end) = balanced_json_object_end(html, start) {
            return Some(&html[start..end]);
        }
    }
    None
}

fn balanced_json_object_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start).copied()? != b'{' {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn inject_before_body_end(mut html: String, fragment: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(pos) = lower.rfind("</body>") {
        html.insert_str(pos, fragment);
    } else {
        html.push_str(fragment);
    }
    html
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_html_attr(value: &str) -> String {
    escape_html(value)
}

fn strip_script_blocks(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len().min(GUARDED_RENDER_HTML_BYTES));
    let mut cursor = 0usize;

    while cursor < html.len() {
        let Some(relative_start) = lower[cursor..].find("<script") else {
            out.push_str(&html[cursor..]);
            break;
        };
        let start = cursor + relative_start;
        out.push_str(&html[cursor..start]);

        let Some(relative_close) = lower[start..].find("</script") else {
            break;
        };
        let close_start = start + relative_close;
        let Some(relative_end) = lower[close_start..].find('>') else {
            break;
        };
        cursor = close_start + relative_end + 1;
    }

    out
}

fn cap_render_html(html: String) -> String {
    if html.len() <= GUARDED_RENDER_HTML_BYTES {
        return html;
    }

    let mut end = GUARDED_RENDER_HTML_BYTES.min(html.len());
    while end > 0 && !html.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(boundary) = html[..end].rfind('>') {
        end = boundary + 1;
    }

    let mut capped = String::with_capacity(end + 32);
    capped.push_str(&html[..end]);
    capped.push_str(
        "
</body></html>",
    );
    capped
}

#[cfg(test)]
mod guarded_tests {
    use super::*;

    #[test]
    fn youtube_is_guarded() {
        assert!(is_guarded_heavy_site(
            &Url::parse("https://www.youtube.com/").unwrap()
        ));
        assert!(is_guarded_heavy_site(
            &Url::parse("https://music.youtube.com/").unwrap()
        ));
        assert!(!is_guarded_heavy_site(
            &Url::parse("https://example.com/").unwrap()
        ));
    }

    #[test]
    fn guarded_html_drops_scripts() {
        let html = "<html><body><h1>keep</h1><script>throw new Error('boom')</script><p>also keep</p></body></html>";
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("keep"));
        assert!(safe.contains("also keep"));
        assert!(!safe.to_ascii_lowercase().contains("<script"));
        assert!(!safe.contains("boom"));
    }

    #[test]
    fn guarded_youtube_recovers_static_thumbnail_cards() {
        let html = r#"<html><body><script>var ytInitialData = {"contents":[{"videoRenderer":{"videoId":"abc123","title":{"runs":[{"text":"Example video"}]},"thumbnail":{"thumbnails":[{"url":"https://i.ytimg.com/vi/abc123/hqdefault.jpg","width":480,"height":360}]}}}]};</script></body></html>"#;
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("veil-youtube-static"));
        assert!(safe.contains("Example video"));
        assert!(safe.contains("https://i.ytimg.com/vi/abc123/hqdefault.jpg"));
        assert!(safe.contains("https://www.youtube.com/watch?v=abc123"));
        assert!(!safe.to_ascii_lowercase().contains("<script"));
    }

    #[test]
    fn guarded_youtube_recovers_current_lockup_view_model() {
        let html = r#"<html><body><script>var ytInitialData = {"contents":[{"lockupViewModel":{"contentId":"modern123","contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentImage":{"thumbnailViewModel":{"image":{"sources":[{"url":"https://i.ytimg.com/vi/modern123/hqdefault.jpg"}]}}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"Modern video"}}}}}]};</script></body></html>"#;
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("Modern video"));
        assert!(safe.contains("https://i.ytimg.com/vi/modern123/hqdefault.jpg"));
        assert!(safe.contains("watch?v=modern123"));
    }

    #[test]
    fn guarded_youtube_recovers_shorts_lockup_view_model() {
        let html = r#"<html><body><script>var ytInitialData = {"contents":[{"shortsLockupViewModel":{"entityId":"shorts-shelf-item-short123","thumbnail":{"sources":[{"url":"https://i.ytimg.com/vi/short123/oar2.jpg"}]},"overlayMetadata":{"primaryText":{"content":"Example Short"}},"onTap":{"innertubeCommand":{"reelWatchEndpoint":{"videoId":"short123"}}}}}]};</script></body></html>"#;
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("Example Short"));
        assert!(safe.contains("https://i.ytimg.com/vi/short123/oar2.jpg"));
        assert!(safe.contains("watch?v=short123"));
    }

    #[test]
    fn renderer_uses_canonical_thumbnail_when_sources_are_missing() {
        let renderer: Value = serde_json::from_str(
            r#"{"videoId":"fallback123","title":{"simpleText":"Fallback thumbnail"}}"#,
        )
        .unwrap();
        let card = youtube_card_from_renderer(&renderer).unwrap();
        assert_eq!(
            card.thumbnail,
            "https://i.ytimg.com/vi/fallback123/hqdefault.jpg"
        );
    }

    #[test]
    fn balanced_json_handles_braces_inside_strings() {
        let text = r#"prefix {"title":"a } brace","nested":{"ok":true}} suffix"#;
        let start = text.find('{').unwrap();
        let end = balanced_json_object_end(text, start).unwrap();
        assert_eq!(
            &text[start..end],
            r#"{"title":"a } brace","nested":{"ok":true}}"#
        );
    }
}
