use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use url::Url;

use crate::net::PrivacyNetwork;
use crate::privacy::SitePrivacy;
use crate::storage::SharedBrowserStorage;

pub struct MediaLoadRequest {
    pub key: String,
    pub top_level: Url,
    pub url: Url,
    pub privacy: SitePrivacy,
    pub custom_filters: String,
    pub storage: SharedBrowserStorage,
}

#[derive(Debug, Clone)]
pub struct MediaProbe {
    pub final_url: String,
    pub content_type: String,
    pub format: String,
    pub byte_len: usize,
    pub duration_seconds: Option<f64>,
}

pub struct MediaLoadResult {
    pub key: String,
    pub result: Result<MediaProbe, String>,
    pub blocked_count: usize,
    pub blocked_events: Vec<String>,
}

pub struct MediaLoader {
    sender: Sender<MediaLoadResult>,
    receiver: Receiver<MediaLoadResult>,
}

impl MediaLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    pub fn start(&self, request: MediaLoadRequest) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut network = PrivacyNetwork::new_with_storage(request.storage.clone());
            if !request.custom_filters.trim().is_empty() {
                network
                    .blocker_mut()
                    .replace_custom_filters(request.custom_filters.clone());
            }
            let result = network
                .get_media(&request.top_level, &request.url, request.privacy)
                .map(|response| {
                    let (format, duration_seconds) =
                        probe_format(&response.bytes, &response.content_type);
                    MediaProbe {
                        final_url: response.final_url.to_string(),
                        content_type: response.content_type,
                        format,
                        byte_len: response.bytes.len(),
                        duration_seconds,
                    }
                });
            let blocked_count = network.blocked_count();
            let blocked_events = network.take_blocked_events();
            let _ = sender.send(MediaLoadResult {
                key: request.key,
                result,
                blocked_count,
                blocked_events,
            });
        });
    }

    pub fn try_recv(&self) -> Option<MediaLoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn probe_format(bytes: &[u8], content_type: &str) -> (String, Option<f64>) {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return ("WAV/PCM".into(), wav_duration(bytes));
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return ("ISO-BMFF / MP4".into(), None);
    }
    if bytes.starts_with(b"OggS") {
        return ("Ogg".into(), None);
    }
    if bytes.starts_with(b"ID3") || bytes.first().copied() == Some(0xff) {
        return ("MP3/AAC-family".into(), None);
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return ("WebM/Matroska".into(), None);
    }
    if bytes.starts_with(b"fLaC") {
        return ("FLAC".into(), None);
    }
    if !content_type.is_empty() {
        return (content_type.to_owned(), None);
    }
    ("unknown media".into(), None)
}

fn wav_duration(bytes: &[u8]) -> Option<f64> {
    let mut pos = 12usize;
    let mut byte_rate = None;
    let mut data_size = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let start = pos + 8;
        if start + size > bytes.len() {
            break;
        }
        if id == b"fmt " && size >= 12 {
            byte_rate =
                Some(u32::from_le_bytes(bytes[start + 8..start + 12].try_into().ok()?) as f64);
        } else if id == b"data" {
            data_size = Some(size as f64);
        }
        pos = start + size + (size & 1);
    }
    match (byte_rate, data_size) {
        (Some(rate), Some(data)) if rate > 0.0 => Some(data / rate),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_mp4_header() {
        let bytes = b"\0\0\0\x18ftypisom0000";
        assert_eq!(probe_format(bytes, "video/mp4").0, "ISO-BMFF / MP4");
    }
}
