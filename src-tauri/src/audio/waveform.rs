//! Waveform overviews for the seek bar.
//!
//! A track is decoded once, as fast as the CPU allows and without touching
//! the audio output, into RMS levels over short blocks. RMS rather than peak:
//! loud masters touch full scale every second or so, so peaks draw a flat
//! bar, while RMS follows how loud each passage sounds. The result is cached
//! on disk keyed by the file's path, size and modification time.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_audio as gst_audio;

use crate::error::{AppError, AppResult};

/// Blocks per second kept before resampling to the requested width.
const BLOCKS_PER_SECOND: u32 = 50;
const MAX_POINTS: usize = 4000;
/// The level byte 0; 255 is 0 dBFS, linear in dB between.
const FLOOR_DB: f32 = -60.0;
/// Gives up on files that decode absurdly slowly (a network mount, say).
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct WaveformCache {
    dir: PathBuf,
}

impl WaveformCache {
    pub fn new(dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// RMS levels (0–255 for −60–0 dBFS) of `path` in `points` columns.
    /// Blocking: may decode the whole file.
    pub fn get(&self, path: &Path, points: usize) -> AppResult<Vec<u8>> {
        let points = points.clamp(1, MAX_POINTS);
        let cached = self.dir.join(format!("{:016x}.rms", key(path)?));
        let blocks = match std::fs::read(&cached) {
            Ok(blocks) if !blocks.is_empty() => blocks,
            _ => {
                let blocks = decode_blocks(path)?;
                if let Err(e) = std::fs::write(&cached, &blocks) {
                    log::warn!("could not cache the waveform of {}: {e}", path.display());
                }
                blocks
            }
        };
        Ok(resample(&blocks, points))
    }
}

fn key(path: &Path) -> AppResult<u64> {
    let meta = std::fs::metadata(path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    Ok(hasher.finish())
}

/// Decodes `path` into one level byte per 1/50 s.
fn decode_blocks(path: &Path) -> AppResult<Vec<u8>> {
    let uri = crate::filesystem::path_to_uri(path)?;
    let pipeline = gst::Pipeline::builder().name("matrixplayer-waveform").build();
    let decode = gst::ElementFactory::make("uridecodebin").property("uri", &uri).build()?;
    let convert = gst::ElementFactory::make("audioconvert").build()?;
    let caps = gst::ElementFactory::make("capsfilter")
        .property(
            "caps",
            gst_audio::AudioCapsBuilder::new_interleaved().format(gst_audio::AudioFormat::F32le).channels(1).build(),
        )
        .build()?;
    let sink =
        gst::ElementFactory::make("fakesink").property("sync", false).property("signal-handoffs", true).build()?;
    pipeline.add_many([&decode, &convert, &caps, &sink])?;
    gst::Element::link_many([&convert, &caps, &sink])?;

    let convert_weak = convert.downgrade();
    decode.connect_pad_added(move |_, pad| {
        let is_audio = pad
            .current_caps()
            .or_else(|| Some(pad.query_caps(None)))
            .and_then(|c| c.structure(0).map(|s| s.name().starts_with("audio/")))
            .unwrap_or(false);
        if let Some(convert) = convert_weak.upgrade()
            && is_audio
            && let Some(sink) = convert.static_pad("sink")
            && !sink.is_linked()
        {
            let _ = pad.link(&sink);
        }
    });

    let levels = Arc::new(Mutex::new(Levels::default()));
    {
        let levels = levels.clone();
        sink.connect("handoff", false, move |args| {
            let buffer = args[1].get::<gst::Buffer>().ok()?;
            let pad = args[2].get::<gst::Pad>().ok()?;
            let rate = pad.current_caps().and_then(|c| gst_audio::AudioInfo::from_caps(&c).ok()).map(|i| i.rate())?;
            let map = buffer.map_readable().ok()?;
            let samples = map.as_slice().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            levels.lock().unwrap().push(samples, rate);
            None
        });
    }

    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline.bus().expect("pipelines have a bus");
    let started = Instant::now();
    let result = loop {
        let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(250)) else {
            if started.elapsed() > TIMEOUT {
                break Err(AppError::Audio("Reading the waveform took too long".into()));
            }
            continue;
        };
        match msg.view() {
            gst::MessageView::Eos(_) => break Ok(()),
            gst::MessageView::Error(e) => break Err(AppError::Audio(format!("Could not decode: {}", e.error()))),
            _ => {}
        }
    };
    let _ = pipeline.set_state(gst::State::Null);
    result?;
    let blocks = {
        let mut levels = levels.lock().unwrap();
        levels.finish();
        std::mem::take(&mut levels.blocks)
    };
    if blocks.is_empty() {
        return Err(AppError::Audio("The file contains no audio".into()));
    }
    Ok(blocks)
}

#[derive(Default)]
struct Levels {
    blocks: Vec<u8>,
    squares: f64,
    filled: u32,
}

impl Levels {
    fn push(&mut self, samples: impl Iterator<Item = f32>, rate: u32) {
        let per_block = (rate / BLOCKS_PER_SECOND).max(1);
        for s in samples {
            self.squares += f64::from(s * s);
            self.filled += 1;
            if self.filled == per_block {
                self.finish();
            }
        }
    }

    fn finish(&mut self) {
        if self.filled > 0 {
            self.blocks.push(level(self.squares / f64::from(self.filled)));
            self.squares = 0.0;
            self.filled = 0;
        }
    }
}

/// The level byte of a mean square.
fn level(mean_square: f64) -> u8 {
    let db = 10.0 * mean_square.log10() as f32;
    (((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The mean square of a level byte.
fn mean_square(level: u8) -> f64 {
    10f64.powf(f64::from(f32::from(level) / 255.0 * -FLOOR_DB + FLOOR_DB) / 10.0)
}

/// The RMS over each of `points` equal spans of `blocks`.
fn resample(blocks: &[u8], points: usize) -> Vec<u8> {
    (0..points)
        .map(|i| {
            let start = i * blocks.len() / points;
            let end = ((i + 1) * blocks.len() / points).max(start + 1).min(blocks.len());
            blocks
                .get(start..end)
                .map(|s| level(s.iter().map(|&b| mean_square(b)).sum::<f64>() / s.len() as f64))
                .unwrap_or(0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::write_flac;

    #[test]
    fn decodes_peaks_and_caches_them() {
        let dir = tempfile::tempdir().unwrap();
        let track = dir.path().join("a.flac");
        write_flac(&track, 2.0, "A");
        let cache = WaveformCache::new(dir.path().join("waves")).unwrap();

        let first = cache.get(&track, 100).unwrap();
        assert_eq!(first.len(), 100);
        // The fixture is a steady sine at 0.2 of full scale: RMS −17 dBFS.
        // Its last block is under one cycle of the sine, so it reads apart.
        assert!(first[..99].iter().all(|p| (181..=185).contains(p)), "{first:?}");
        assert_eq!(std::fs::read_dir(dir.path().join("waves")).unwrap().count(), 1);
        assert_eq!(cache.get(&track, 100).unwrap(), first, "the cached copy matches");
    }

    #[test]
    fn resampling_takes_the_rms_of_each_span() {
        assert_eq!(resample(&[200, 200, 90, 90], 2), [200, 90]);
        // Full scale and silence average to half the energy: −3 dB.
        assert_eq!(resample(&[255, 0], 1), [242]);
        assert_eq!(resample(&[5, 7], 4), [5, 5, 7, 7]);
    }

    #[test]
    fn files_that_are_not_audio_fail_cleanly() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.flac");
        std::fs::write(&bad, b"nope").unwrap();
        let cache = WaveformCache::new(dir.path().join("waves")).unwrap();
        assert!(cache.get(&bad, 10).is_err());
    }
}
