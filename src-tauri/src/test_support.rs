//! Fixtures for tests that need real audio files.

use std::path::Path;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Encodes `seconds` of a sine tone as a tagged 16-bit/44.1 kHz stereo FLAC.
pub fn write_flac(path: &Path, seconds: f64, title: &str) {
    let tags = format!("title=\"{title}\",artist=\"Test Artist\",album=\"Test Album\"");
    write_flac_tags(path, seconds, &tags);
}

/// Like [`write_flac`], with tags in GStreamer's structure syntax, e.g.
/// `title="A",album-artist="B",track-number=(uint)3`.
pub fn write_flac_tags(path: &Path, seconds: f64, tags: &str) {
    let buffers = (seconds * 44_100.0 / 1024.0).ceil() as u32;
    let pipeline = format!(
        "audiotestsrc num-buffers={buffers} samplesperbuffer=1024 freq=440 volume=0.2 \
         ! audio/x-raw,rate=44100,channels=2,format=S16LE \
         ! taginject name=tags ! flacenc ! filesink name=out"
    );
    encode(&pipeline, path, Some(tags));
}

/// Encodes `seconds` of the same 440 Hz tone (0.2 of full scale) as 16-bit
/// FLAC with another rate or channel count.
pub fn write_flac_format(path: &Path, seconds: f64, rate: u32, channels: u32) {
    let buffers = (seconds * f64::from(rate) / 1024.0).ceil() as u32;
    let pipeline = format!(
        "audiotestsrc num-buffers={buffers} samplesperbuffer=1024 freq=440 volume=0.2 \
         ! audio/x-raw,rate={rate},channels={channels},format=S16LE ! flacenc ! filesink name=out"
    );
    encode(&pipeline, path, None);
}

/// Encodes the tone with `encoder`, a pipeline fragment such as
/// `lamemp3enc` or `opusenc ! oggmux`. Returns false, writing nothing, when
/// one of its elements is not installed.
pub fn write_encoded(path: &Path, seconds: f64, encoder: &str) -> bool {
    gst::init().unwrap();
    let buffers = (seconds * 48_000.0 / 1024.0).ceil() as u32;
    let pipeline = format!(
        "audiotestsrc num-buffers={buffers} samplesperbuffer=1024 freq=440 volume=0.2 \
         ! audio/x-raw,rate=48000,channels=2 ! audioconvert ! {encoder} ! filesink name=out"
    );
    if gst::parse::launch(&pipeline).is_err() {
        return false;
    }
    encode(&pipeline, path, None);
    true
}

/// Runs an encoding pipeline that ends in a `filesink` named `out` (and,
/// with `tags`, has a `taginject` named `tags`) until it finishes.
fn encode(description: &str, path: &Path, tags: Option<&str>) {
    gst::init().unwrap();
    let pipeline = gst::parse::launch(description).unwrap().downcast::<gst::Pipeline>().unwrap();
    if let Some(tags) = tags {
        pipeline.by_name("tags").unwrap().set_property("tags", tags);
    }
    pipeline.by_name("out").unwrap().set_property("location", path.to_str().unwrap());

    pipeline.set_state(gst::State::Playing).unwrap();
    let bus = pipeline.bus().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let msg = bus.timed_pop(gst::ClockTime::from_mseconds(100));
        match msg.as_ref().map(|m| m.view()) {
            Some(gst::MessageView::Eos(_)) => break,
            Some(gst::MessageView::Error(e)) => panic!("could not write fixture: {}", e.error()),
            _ if Instant::now() > deadline => panic!("timed out writing fixture"),
            _ => {}
        }
    }
    pipeline.set_state(gst::State::Null).unwrap();
}
