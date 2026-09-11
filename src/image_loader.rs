use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use url::Url;

use crate::net::PrivacyNetwork;
use crate::privacy::SitePrivacy;
use crate::storage::SharedBrowserStorage;

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
                network.blocker_mut().replace_custom_filters(request.custom_filters.clone());
            }
            let result = network
                .get_image(&request.top_level, &request.url, request.privacy)
                .and_then(|response| {
                    let decoded = image::load_from_memory(&response.bytes)
                        .map_err(|e| format!("Image decode failed: {e}"))?;
                    let rgba = decoded.to_rgba8();
                    Ok(DecodedImage {
                        final_url: response.final_url.to_string(),
                        size: [rgba.width() as usize, rgba.height() as usize],
                        rgba: rgba.into_raw(),
                    })
                });
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
