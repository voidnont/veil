use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
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
    request_sender: Sender<ImageLoadRequest>,
    receiver: Receiver<ImageLoadResult>,
}

impl ImageLoader {
    pub fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<ImageLoadRequest>();
        let (result_sender, receiver) = mpsc::channel::<ImageLoadResult>();
        let shared_receiver = Arc::new(Mutex::new(request_receiver));

        // Gecko uses bounded task pools instead of creating a new native thread
        // for every decode. Keep enough workers for parallelism while reserving
        // CPU for the UI/render threads.
        for worker in 0..image_worker_count() {
            let requests = shared_receiver.clone();
            let sender = result_sender.clone();
            let _ = thread::Builder::new()
                .name(format!("veil-image-{worker}"))
                .spawn(move || loop {
                    let request = {
                        let Ok(receiver) = requests.lock() else {
                            break;
                        };
                        match receiver.recv() {
                            Ok(request) => request,
                            Err(_) => break,
                        }
                    };
                    let result = process_image_request(request);
                    if sender.send(result).is_err() {
                        break;
                    }
                });
        }

        Self {
            request_sender,
            receiver,
        }
    }

    pub fn start(&self, request: ImageLoadRequest) {
        let _ = self.request_sender.send(request);
    }

    pub fn try_recv(&self) -> Option<ImageLoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn image_worker_count() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4);
    (cores / 2).clamp(2, 6)
}

fn process_image_request(request: ImageLoadRequest) -> ImageLoadResult {
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
    ImageLoadResult {
        key: request.key,
        result,
        blocked_count,
        blocked_events,
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

    let format =
        image::guess_format(bytes).map_err(|_| unsupported_payload_error(content_type, bytes))?;
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
    let transform =
        resvg::tiny_skia::Transform::from_scale(width as f32 / source_w, height as f32 / source_h);
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
    use std::io::Cursor;

    #[test]
    fn worker_pool_is_bounded() {
        assert!((2..=6).contains(&image_worker_count()));
    }

    #[test]
    fn sniffs_png_even_when_http_mime_is_wrong() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([12, 34, 56, 255]),
        ));
        let mut cursor = Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        let decoded =
            decode_image_payload(&cursor.into_inner(), "application/octet-stream").unwrap();
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
