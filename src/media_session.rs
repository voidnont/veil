use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Loading,
    Playing,
    Paused,
    Ended,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSession {
    pub key: String,
    pub state: PlaybackState,
    pub duration: Option<f64>,
    pub current_time: f64,
    pub volume: f32,
    pub muted: bool,
    pub playback_rate: f32,
    pub buffered_until: f64,
    pub error: Option<String>,
}

impl MediaSession {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            state: PlaybackState::Idle,
            duration: None,
            current_time: 0.0,
            volume: 1.0,
            muted: false,
            playback_rate: 1.0,
            buffered_until: 0.0,
            error: None,
        }
    }
    pub fn play(&mut self) {
        if self.state != PlaybackState::Failed {
            self.state = PlaybackState::Playing;
        }
    }
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
    }
    pub fn seek(&mut self, seconds: f64) {
        let max = self.duration.unwrap_or(f64::MAX);
        self.current_time = seconds.max(0.0).min(max);
    }
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }
    pub fn set_rate(&mut self, rate: f32) {
        self.playback_rate = rate.clamp(0.25, 4.0);
    }
}

#[derive(Default)]
pub struct MediaSessionManager {
    sessions: HashMap<String, MediaSession>,
}
impl MediaSessionManager {
    pub fn get_or_create(&mut self, key: &str) -> &mut MediaSession {
        self.sessions
            .entry(key.to_owned())
            .or_insert_with(|| MediaSession::new(key))
    }
    pub fn remove(&mut self, key: &str) {
        self.sessions.remove(key);
    }
    pub fn clear(&mut self) {
        self.sessions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clamps_seek_and_volume() {
        let mut s = MediaSession::new("x");
        s.duration = Some(10.0);
        s.seek(20.0);
        s.set_volume(2.0);
        assert_eq!(s.current_time, 10.0);
        assert_eq!(s.volume, 1.0);
    }
}
