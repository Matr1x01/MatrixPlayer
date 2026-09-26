//! Fixtures for tests that need real audio files.

use std::path::Path;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Encodes `seconds` of a sine tone as a tagged 16-bit/44.1 kHz stereo FLAC.
pub fn write_flac(path: &Path, seconds: f64, title: &str) {
    gst::init().unwrap();
    let buffers = (seconds * 44_100.0 / 1024.0).ceil() as u32;
    let pipeline = gst::parse::launch(&format!(
        "audiotestsrc num-buffers={buffers} samplesperbuffer=1024 freq=440 volume=0.2 \
         ! audio/x-raw,rate=44100,channels=2,format=S16LE \
         ! taginject name=tags ! flacenc ! filesink name=out"
    ))
    .unwrap()
    .downcast::<gst::Pipeline>()
    .unwrap();
    let tags = format!("title=\"{title}\",artist=\"Test Artist\",album=\"Test Album\"");
    pipeline.by_name("tags").unwrap().set_property("tags", tags);
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
