use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct DecodedWav {
    pub sample_rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
    pub duration_seconds: f64,
}

pub struct AudioPlayer {
    stream: Option<cpal::Stream>,
    position_frames: Option<Arc<AtomicU64>>,
    output_rate: u32,
    total_output_frames: u64,
    current_key: Option<String>,
    paused: bool,
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self {
            stream: None,
            position_frames: None,
            output_rate: 48_000,
            total_output_frames: 0,
            current_key: None,
            paused: false,
        }
    }
}

impl AudioPlayer {
    pub fn play_wav(&mut self, key: String, bytes: &[u8]) -> Result<(), String> {
        let wav = decode_wav_pcm(bytes)?;
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| "No audio output device is available".to_owned())?;
        let ranges: Vec<_> = device.supported_output_configs()
            .map_err(|e| format!("Could not query audio output formats: {e}"))?
            .filter(|range| range.sample_format() == cpal::SampleFormat::F32)
            .collect();
        let supported = ranges.iter().copied().find_map(|range| range.try_with_standard_sample_rate())
            .or_else(|| ranges.first().copied().map(|range| range.with_max_sample_rate()))
            .ok_or_else(|| "The default audio device does not expose an f32 PCM output format".to_owned())?;
        let config = supported.config();
        let output_channels = config.channels as usize;
        let output_rate = config.sample_rate;
        if output_channels == 0 || output_rate == 0 { return Err("Invalid audio output configuration".into()); }

        let source_channels = wav.channels;
        let source_rate = wav.sample_rate;
        let source = Arc::new(wav.samples);
        let source_frames = source.len() / source_channels.max(1);
        let total_output_frames = ((source_frames as u128 * output_rate as u128) / source_rate.max(1) as u128) as u64;
        let position = Arc::new(AtomicU64::new(0));
        let callback_position = position.clone();
        let callback_source = source.clone();

        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(output_channels) {
                    let output_frame = callback_position.fetch_add(1, Ordering::Relaxed);
                    if output_frame >= total_output_frames {
                        frame.fill(0.0);
                        continue;
                    }
                    let source_frame = ((output_frame as u128 * source_rate as u128) / output_rate as u128) as usize;
                    let source_frame = source_frame.min(source_frames.saturating_sub(1));
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        let source_channel = if source_channels == 1 { 0 } else { channel.min(source_channels - 1) };
                        *sample = callback_source[source_frame * source_channels + source_channel];
                    }
                }
            },
            |_error| {},
            None,
        ).map_err(|e| format!("Could not open audio output: {e}"))?;
        stream.play().map_err(|e| format!("Could not start audio playback: {e}"))?;

        self.stream = Some(stream);
        self.position_frames = Some(position);
        self.output_rate = output_rate;
        self.total_output_frames = total_output_frames;
        self.current_key = Some(key);
        self.paused = false;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        if let Some(stream) = &self.stream {
            stream.pause().map_err(|e| format!("Could not pause audio: {e}"))?;
            self.paused = true;
        }
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if let Some(stream) = &self.stream {
            stream.play().map_err(|e| format!("Could not resume audio: {e}"))?;
            self.paused = false;
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
        self.position_frames = None;
        self.total_output_frames = 0;
        self.current_key = None;
        self.paused = false;
    }

    pub fn is_current(&self, key: &str) -> bool {
        self.current_key.as_deref() == Some(key) && self.stream.is_some()
    }

    pub fn is_paused(&self) -> bool { self.paused }

    pub fn progress(&self) -> f32 {
        let Some(position) = &self.position_frames else { return 0.0; };
        if self.total_output_frames == 0 { return 0.0; }
        (position.load(Ordering::Relaxed) as f64 / self.total_output_frames as f64).clamp(0.0, 1.0) as f32
    }

    pub fn position_seconds(&self) -> f64 {
        let Some(position) = &self.position_frames else { return 0.0; };
        position.load(Ordering::Relaxed) as f64 / self.output_rate.max(1) as f64
    }
}

pub fn decode_wav_pcm(bytes: &[u8]) -> Result<DecodedWav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Not a RIFF/WAVE file".into());
    }
    let mut pos = 12usize;
    let mut format = None;
    let mut channels = None;
    let mut sample_rate = None;
    let mut bits = None;
    let mut data = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().map_err(|_| "Invalid WAV chunk")?) as usize;
        let start = pos + 8;
        let end = start.checked_add(size).ok_or_else(|| "WAV chunk overflow".to_owned())?;
        if end > bytes.len() { break; }
        if id == b"fmt " && size >= 16 {
            format = Some(u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap()));
            channels = Some(u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap()) as usize);
            sample_rate = Some(u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap()));
            bits = Some(u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap()));
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
        }
        pos = end + (size & 1);
    }

    let format = format.ok_or_else(|| "WAV has no fmt chunk".to_owned())?;
    let channels = channels.filter(|value| *value > 0 && *value <= 8).ok_or_else(|| "Unsupported WAV channel count".to_owned())?;
    let sample_rate = sample_rate.filter(|value| *value >= 8_000 && *value <= 384_000).ok_or_else(|| "Unsupported WAV sample rate".to_owned())?;
    let bits = bits.ok_or_else(|| "WAV has no bit depth".to_owned())?;
    let data = data.ok_or_else(|| "WAV has no audio data chunk".to_owned())?;
    let mut samples = Vec::new();

    match (format, bits) {
        (1, 8) => samples.extend(data.iter().map(|value| (*value as f32 - 128.0) / 128.0)),
        (1, 16) => {
            for chunk in data.chunks_exact(2) {
                samples.push(i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0);
            }
        }
        (1, 24) => {
            for chunk in data.chunks_exact(3) {
                let raw = (chunk[0] as i32) | ((chunk[1] as i32) << 8) | ((chunk[2] as i32) << 16);
                let signed = if raw & 0x0080_0000 != 0 { raw | !0x00ff_ffff } else { raw };
                samples.push(signed as f32 / 8_388_608.0);
            }
        }
        (1, 32) => {
            for chunk in data.chunks_exact(4) {
                samples.push(i32::from_le_bytes(chunk.try_into().unwrap()) as f32 / 2_147_483_648.0);
            }
        }
        (3, 32) => {
            for chunk in data.chunks_exact(4) {
                samples.push(f32::from_le_bytes(chunk.try_into().unwrap()).clamp(-1.0, 1.0));
            }
        }
        _ => return Err(format!("Unsupported WAV encoding: format {format}, {bits}-bit")),
    }
    if samples.is_empty() { return Err("WAV contains no decodable samples".into()); }
    let duration_seconds = samples.len() as f64 / channels as f64 / sample_rate as f64;
    Ok(DecodedWav { sample_rate, channels, samples, duration_seconds })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_tiny_pcm16_wav() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF"); wav.extend_from_slice(&40u32.to_le_bytes()); wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt "); wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes()); wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes()); wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data"); wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&0i16.to_le_bytes()); wav.extend_from_slice(&16384i16.to_le_bytes());
        let decoded = decode_wav_pcm(&wav).unwrap();
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.sample_rate, 8000);
        assert_eq!(decoded.samples.len(), 2);
        assert!(decoded.samples[1] > 0.49 && decoded.samples[1] < 0.51);
    }
}
