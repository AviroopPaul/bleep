//! The mic: opens the chosen input (or the system default), mixes to mono and
//! resamples to 16 kHz for the model. Chunks go out on a channel.
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::sync::mpsc::{Receiver, Sender};

const CHUNK: usize = 1024;

/// Input device names, for the picker. Opens nothing, so asks for no permission.
pub fn input_devices() -> Vec<String> {
    let mut names: Vec<String> = cpal::default_host().input_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
    // macOS makes these behind the scenes; they are not something to pick.
    names.retain(|n| !n.starts_with("CADefaultDeviceAggregate"));
    names.dedup();
    names
}

pub fn default_input() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| d.name().ok())
}

/// The device to open: the chosen one if it is plugged in, else the default.
pub fn pick(chosen: &str, available: &[String], default: Option<&str>) -> Option<String> {
    if !chosen.is_empty() && available.iter().any(|n| n == chosen) {
        return Some(chosen.to_string());
    }
    default.map(str::to_string)
}

/// Streams 16 kHz mono to the channel while alive.
pub struct Mic {
    _stream: cpal::Stream,
    pub name: String,
}

/// Mono at the device rate in, 16 kHz out, with state kept across calls.
pub struct To16k {
    rs: Option<FftFixedIn<f32>>,
    buf: Vec<f32>,
}

impl To16k {
    pub fn new(rate: u32) -> Result<Self> {
        let rs = if rate == 16_000 { None } else { Some(FftFixedIn::<f32>::new(rate as usize, 16_000, CHUNK, 2, 1).context("resampler")?) };
        Ok(Self { rs, buf: vec![] })
    }

    pub fn push(&mut self, mono: &[f32]) -> Vec<f32> {
        let Some(rs) = self.rs.as_mut() else { return mono.to_vec() };
        self.buf.extend_from_slice(mono);
        let mut out = vec![];
        let mut at = 0;
        while self.buf.len() - at >= CHUNK {
            if let Ok(r) = rs.process(&[&self.buf[at..at + CHUNK]], None) {
                out.extend_from_slice(&r[0]);
            }
            at += CHUNK;
        }
        self.buf.drain(..at);
        out
    }
}

impl Mic {
    pub fn open(name: &str, tx: Sender<Vec<f32>>) -> Result<Mic> {
        let host = cpal::default_host();
        let device = host
            .input_devices()?
            .find(|d| d.name().is_ok_and(|n| n == name))
            .or_else(|| host.default_input_device())
            .context("no microphone found")?;
        let name = device.name().unwrap_or_default();
        let config = device.default_input_config().context("mic format")?;
        let channels = config.channels() as usize;
        let mut to16k = To16k::new(config.sample_rate().0)?;
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &_| {
                    let mono: Vec<f32> = data.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect();
                    let out = to16k.push(&mono);
                    if !out.is_empty() {
                        let _ = tx.send(out);
                    }
                },
                |e| eprintln!("bleep: mic stream: {e}"),
                None,
            )
            .context("open mic stream")?;
        stream.play().context("start mic")?;
        Ok(Mic { _stream: stream, name })
    }
}

/// A channel pair for one mic session.
pub fn channel() -> (Sender<Vec<f32>>, Receiver<Vec<f32>>) {
    std::sync::mpsc::channel()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_falls_back_to_the_default() {
        let have = vec!["MacBook Air Microphone".to_string(), "AirPods".to_string()];
        assert_eq!(pick("AirPods", &have, Some("MacBook Air Microphone")).as_deref(), Some("AirPods"));
        assert_eq!(pick("", &have, Some("MacBook Air Microphone")).as_deref(), Some("MacBook Air Microphone"));
        assert_eq!(pick("Yeti", &have, Some("MacBook Air Microphone")).as_deref(), Some("MacBook Air Microphone"), "unplugged mic uses the default");
        assert_eq!(pick("Yeti", &have, None), None);
    }

    #[test]
    fn resampler_keeps_the_length_ratio() {
        let mut r = To16k::new(48_000).unwrap();
        let mut n = 0;
        for _ in 0..100 {
            n += r.push(&vec![0.1; 480]).len();
        }
        // 48 000 in gives about 16 000 out, less what is still buffered.
        assert!((15_000..=16_000).contains(&n), "{n}");
        let mut same = To16k::new(16_000).unwrap();
        assert_eq!(same.push(&[0.5; 100]).len(), 100);
    }
}
