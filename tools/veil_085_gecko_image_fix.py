from pathlib import Path


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# Cargo: add the formats/pipeline pieces Veil now actually supports.
path = Path("Cargo.toml")
text = path.read_text()
text = replace_once(text, 'version = "0.8.4"', 'version = "0.8.5"', "package version")
text = replace_once(
    text,
    'image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp", "ico"] }',
    'image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp", "ico", "bmp"] }\nresvg = { version = "0.48", default-features = false, features = ["text", "raster-images"] }\nbase64 = "0.22"\npercent-encoding = "2.3"',
    "image dependencies",
)
path.write_text(text)


# Network: Gecko sniffs the bytes and only falls back to the response MIME type.
# Veil used to reject a payload before the decoder ever saw it.
path = Path("src/net.rs")
text = path.read_text()
text = replace_once(
    text,
    'const IMAGE_ACCEPT: &str = "image/webp,image/png,image/jpeg,image/gif,image/x-icon,*/*;q=0.1";',
    'const IMAGE_ACCEPT: &str = "image/webp,image/png,image/jpeg,image/gif,image/svg+xml,image/bmp,image/x-icon,*/*;q=0.1";',
    "image Accept header",
)
text = replace_once(
    text,
    'pub struct ImageResponse {\n    pub final_url: Url,\n    pub bytes: Vec<u8>,\n}',
    'pub struct ImageResponse {\n    pub final_url: Url,\n    pub bytes: Vec<u8>,\n    pub content_type: String,\n}',
    "ImageResponse content type",
)
text = replace_once(
    text,
    '"Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.4 VeilEngine/0.8.4",',
    '"Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.5 VeilEngine/0.8.5",',
    "network version",
)
old_get_image = '''        if !response.content_type.is_empty() && !response.content_type.starts_with("image/") {
            return Err(format!(
                "Blocked non-image response: {}",
                response.content_type
            ));
        }
        Ok(ImageResponse {
            final_url: response.final_url,
            bytes: response.bytes,
        })'''
new_get_image = '''        // Match Gecko's image loader behavior: do not reject solely from the HTTP
        // Content-Type. The image worker sniffs the bytes first because real CDNs
        // sometimes serve valid image bytes as application/octet-stream or with a
        // stale/wrong MIME type.
        Ok(ImageResponse {
            final_url: response.final_url,
            bytes: response.bytes,
            content_type: response.content_type,
        })'''
text = replace_once(text, old_get_image, new_get_image, "Gecko-style MIME handling")
path.write_text(text)


# Image worker: sniff bytes, decode SVG separately, and support data:image URLs.
Path("src/image_loader.rs").write_text(r'''use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use percent_encoding::percent_decode_str;
use url::Url;

use crate::net::PrivacyNetwork;
use crate::privacy::SitePrivacy;
use crate::storage::SharedBrowserStorage;

const MAX_DECODED_PIXELS: usize = 16_000_000;
const MAX_DATA_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_SVG_DIMENSION: u32 = 4096;

pub struct ImageLoadRequest {
    pub key: String,
    pub top_level: Url,
    pub url: Url,
    pub privacy: SitePrivacy,
    pub custom_filters: String,
    pub storage: SharedBrowserStorage,
}

pub struct DecodedImage {
    pub final_url: String,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

pub struct ImageLoadResult {
    pub key: String,
    pub result: Result<DecodedImage, String>,
    pub blocked_count: usize,
    pub blocked_events: Vec<String>,
}

pub struct ImageLoader {
    sender: Sender<ImageLoadResult>,
    receiver: Receiver<ImageLoadResult>,
}

impl ImageLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    pub fn start(&self, request: ImageLoadRequest) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut network = PrivacyNetwork::new_with_storage(request.storage.clone());
            if !request.custom_filters.trim().is_empty() {
                network
                    .blocker_mut()
                    .replace_custom_filters(request.custom_filters.clone());
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if request.url.scheme() == "data" {
                    let (content_type, bytes) = decode_data_image(request.url.as_str())?;
                    let decoded = decode_image_payload(&bytes, &content_type)?;
                    return Ok(DecodedImage {
                        final_url: request.url.to_string(),
                        size: decoded.size,
                        rgba: decoded.rgba,
                    });
                }

                let response = network.get_image(&request.top_level, &request.url, request.privacy)?;
                let decoded = decode_image_payload(&response.bytes, &response.content_type)?;
                Ok(DecodedImage {
                    final_url: response.final_url.to_string(),
                    size: decoded.size,
                    rgba: decoded.rgba,
                })
            }))
            .unwrap_or_else(|_| Err("Veil recovered from an image worker panic.".into()));

            let blocked_count = network.blocked_count();
            let blocked_events = network.take_blocked_events();
            let _ = sender.send(ImageLoadResult {
                key: request.key,
                result,
                blocked_count,
                blocked_events,
            });
        });
    }

    pub fn try_recv(&self) -> Option<ImageLoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

struct DecodedPixels {
    size: [usize; 2],
    rgba: Vec<u8>,
}

fn decode_image_payload(bytes: &[u8], content_type: &str) -> Result<DecodedPixels, String> {
    if bytes.is_empty() {
        return Err("Image response was empty.".into());
    }

    // Gecko's image request path sniffs content before trusting the channel MIME.
    // Do the same here, with SVG kept on a vector-specific path.
    if looks_like_svg(bytes, content_type) {
        return decode_svg(bytes);
    }

    let format = image::guess_format(bytes)
        .map_err(|_| unsupported_payload_error(content_type, bytes))?;
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|error| format!("Image decode failed ({format:?}): {error}"))?;
    finish_raster(decoded)
}

fn finish_raster(decoded: image::DynamicImage) -> Result<DecodedPixels, String> {
    let width = decoded.width() as usize;
    let height = decoded.height() as usize;
    validate_dimensions(width, height)?;
    let rgba = decoded.to_rgba8();
    Ok(DecodedPixels {
        size: [rgba.width() as usize, rgba.height() as usize],
        rgba: rgba.into_raw(),
    })
}

fn validate_dimensions(width: usize, height: usize) -> Result<(), String> {
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| "Image dimensions overflowed Veil's safety limit.".to_owned())?;
    if width == 0 || height == 0 {
        return Err("Image has zero width or height.".into());
    }
    if pixels > MAX_DECODED_PIXELS {
        return Err("Image exceeds Veil's 16 megapixel safety limit.".into());
    }
    Ok(())
}

fn looks_like_svg(bytes: &[u8], content_type: &str) -> bool {
    if content_type
        .split(';')
        .next()
        .map(str::trim)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("image/svg+xml"))
    {
        return true;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    let trimmed = head.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    trimmed.starts_with("<svg")
        || (trimmed.starts_with("<?xml") && trimmed.contains("<svg"))
        || (trimmed.starts_with("<!--") && trimmed.contains("<svg"))
}

fn decode_svg(bytes: &[u8]) -> Result<DecodedPixels, String> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &options)
        .map_err(|error| format!("SVG parse failed: {error}"))?;
    let source = tree.size();
    let source_w = source.width().max(1.0);
    let source_h = source.height().max(1.0);

    let mut width = source_w.ceil().clamp(1.0, MAX_SVG_DIMENSION as f32) as u32;
    let mut height = source_h.ceil().clamp(1.0, MAX_SVG_DIMENSION as f32) as u32;
    let pixels = (width as u64).saturating_mul(height as u64);
    if pixels > MAX_DECODED_PIXELS as u64 {
        let scale = ((MAX_DECODED_PIXELS as f64) / pixels as f64).sqrt() as f32;
        width = ((width as f32 * scale).floor() as u32).max(1);
        height = ((height as f32 * scale).floor() as u32).max(1);
    }
    validate_dimensions(width as usize, height as usize)?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "Could not allocate SVG render surface.".to_owned())?;
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / source_w,
        height as f32 / source_h,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    unpremultiply_rgba(&mut rgba);
    Ok(DecodedPixels {
        size: [width as usize, height as usize],
        rgba,
    })
}

fn unpremultiply_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = (((*channel as u16) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

fn unsupported_payload_error(content_type: &str, bytes: &[u8]) -> String {
    let kind = content_type.trim();
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(80)])
        .replace('\n', " ")
        .replace('\r', " ");
    if kind.is_empty() {
        format!("Unsupported image payload; first bytes: {head:?}")
    } else {
        format!("Unsupported image payload ({kind}); first bytes: {head:?}")
    }
}

fn decode_data_image(url: &str) -> Result<(String, Vec<u8>), String> {
    let payload = url
        .strip_prefix("data:")
        .ok_or_else(|| "Invalid data image URL.".to_owned())?;
    let (meta, encoded) = payload
        .split_once(',')
        .ok_or_else(|| "Data image URL has no payload.".to_owned())?;
    if encoded.len() > MAX_DATA_IMAGE_BYTES.saturating_mul(2) {
        return Err("Embedded image exceeds Veil's safety limit.".into());
    }

    let mut parts = meta.split(';');
    let mut content_type = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    if content_type.is_empty() {
        content_type = "text/plain".into();
    }
    if !content_type.starts_with("image/") {
        return Err(format!("Blocked non-image data URL: {content_type}"));
    }
    let base64_encoded = parts.any(|part| part.trim().eq_ignore_ascii_case("base64"));
    let decoded = percent_decode_str(encoded).collect::<Vec<u8>>();
    let bytes = if base64_encoded {
        BASE64_STANDARD
            .decode(decoded)
            .map_err(|error| format!("Invalid base64 image data: {error}"))?
    } else {
        decoded
    };
    if bytes.len() > MAX_DATA_IMAGE_BYTES {
        return Err("Embedded image exceeds Veil's 12 MiB safety limit.".into());
    }
    Ok((content_type, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_png_even_when_http_mime_is_wrong() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([12, 34, 56, 255]),
        ));
        let mut cursor = Cursor::new(Vec::new());
        image.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        let decoded = decode_image_payload(&cursor.into_inner(), "application/octet-stream").unwrap();
        assert_eq!(decoded.size, [2, 2]);
    }

    #[test]
    fn decodes_svg_on_vector_path() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg' width='24' height='12'><rect width='24' height='12' fill='#ff0000'/></svg>"#;
        let decoded = decode_image_payload(svg, "image/svg+xml").unwrap();
        assert_eq!(decoded.size, [24, 12]);
        assert_eq!(decoded.rgba.len(), 24 * 12 * 4);
    }

    #[test]
    fn decodes_percent_encoded_data_svg() {
        let url = "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='8'%20height='6'%3E%3Crect%20width='8'%20height='6'%20fill='red'/%3E%3C/svg%3E";
        let (kind, bytes) = decode_data_image(url).unwrap();
        let decoded = decode_image_payload(&bytes, &kind).unwrap();
        assert_eq!(decoded.size, [8, 6]);
    }
}
''')


# Let the UI queue data:image URLs instead of rejecting them before the worker.
path = Path("src/main.rs")
text = path.read_text()
text = replace_once(
    text,
    'if !matches!(image_url.scheme(), "http" | "https") {',
    'if !matches!(image_url.scheme(), "http" | "https" | "data") {',
    "image URL schemes",
)
path.write_text(text)


# Responsive selection: Gecko chooses the lowest candidate at/above the target
# density instead of always taking the largest source. Veil's old largest-only
# behavior frequently selected unnecessarily huge assets and then hit safety caps.
path = Path("src/engine.rs")
text = path.read_text()
text = replace_once(
    text,
    '            | "image/x-icon"\n            | "image/vnd.microsoft.icon"',
    '            | "image/x-icon"\n            | "image/vnd.microsoft.icon"\n            | "image/bmp"\n            | "image/svg+xml"',
    "picture supported formats",
)
old_srcset = r'''fn best_srcset_candidate(set: &str) -> Option<String> {
    let mut best: Option<(f32, String)> = None;
    for raw in set.split(',') {
        let mut parts = raw.split_whitespace();
        let Some(url) = parts.next().map(str::trim).filter(|url| !url.is_empty()) else {
            continue;
        };
        let descriptor = parts.next().unwrap_or_default();
        let score = if let Some(width) = descriptor.strip_suffix('w') {
            width.parse::<f32>().unwrap_or(1.0)
        } else if let Some(scale) = descriptor.strip_suffix('x') {
            scale.parse::<f32>().unwrap_or(1.0) * 10_000.0
        } else {
            1.0
        };
        if best
            .as_ref()
            .map(|(best_score, _)| score >= *best_score)
            .unwrap_or(true)
        {
            best = Some((score, url.to_owned()));
        }
    }
    best.map(|(_, url)| url)
}'''
new_srcset = r'''fn best_srcset_candidate(set: &str) -> Option<String> {
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
            (raw_width.parse::<f32>().ok().filter(|value| *value > 0.0), None)
        } else if let Some(raw_density) = descriptor.strip_suffix('x') {
            (None, raw_density.parse::<f32>().ok().filter(|value| *value > 0.0))
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
        for candidate in candidates.iter().filter(|candidate| candidate.width.is_some()) {
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
}'''
text = replace_once(text, old_srcset, new_srcset, "responsive srcset selection")
old_test_anchor = '''    #[test]\n    fn parses_simple_get_form() {'''
new_tests = '''    #[test]\n    fn srcset_prefers_one_x_instead_of_largest_density() {\n        assert_eq!(\n            best_srcset_candidate("small.jpg 1x, medium.jpg 2x, huge.jpg 3x").as_deref(),\n            Some("small.jpg")\n        );\n    }\n\n    #[test]\n    fn srcset_prefers_reasonable_width_instead_of_largest_asset() {\n        assert_eq!(\n            best_srcset_candidate("a.jpg 320w, b.jpg 640w, c.jpg 1280w, d.jpg 4096w").as_deref(),\n            Some("c.jpg")\n        );\n    }\n\n    #[test]\n    fn parses_simple_get_form() {'''
text = replace_once(text, old_test_anchor, new_tests, "srcset regression tests")
path.write_text(text)


# Normal installer should package the tested version after this workflow commits.
path = Path(".github/workflows/veil.yml")
text = path.read_text()
text = replace_once(text, "AppVersion=0.8.4", "AppVersion=0.8.5", "installer version")
path.write_text(text)
