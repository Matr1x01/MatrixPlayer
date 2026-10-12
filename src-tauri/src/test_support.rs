//! Fixtures for tests that need real audio files.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Taken by the tests that use the real sound server, so they run one at a
/// time: all PipeWire elements in a process share one connection, and one
/// test's stuck stream would stall the others' (and, through the session
/// manager, every other program's sound).
pub fn system_output() -> MutexGuard<'static, ()> {
    static SYSTEM_OUTPUT: Mutex<()> = Mutex::new(());
    SYSTEM_OUTPUT.lock().unwrap_or_else(|e| e.into_inner())
}

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

/// Encodes `seconds` of white noise as FLAC with `bits` (16 or 24) per
/// sample: every sample differs, so a changed one cannot go unnoticed.
pub fn write_flac_noise(path: &Path, seconds: f64, rate: u32, channels: u32, bits: u32) {
    let buffers = (seconds * f64::from(rate) / 1024.0).ceil() as u32;
    let format = if bits == 16 { "S16LE" } else { "S24_32LE" };
    let pipeline = format!(
        "audiotestsrc num-buffers={buffers} samplesperbuffer=1024 wave=white-noise volume=0.8 \
         ! audio/x-raw,format=S32LE,rate={rate},channels={channels} \
         ! audioconvert dithering=none ! audio/x-raw,format={format} ! flacenc ! filesink name=out"
    );
    encode(&pipeline, path, None);
}

/// Every sample of a FLAC file, decoded to 32-bit integers (a 16-bit
/// sample `s` reads as `s << 16`, a 24-bit one as `s << 8`), interleaved.
pub fn decode_s32(path: &Path) -> Vec<i32> {
    gst::init().unwrap();
    let pipeline = gst::parse::launch(
        "filesrc name=in ! flacparse ! flacdec ! audioconvert dithering=none \
         ! audio/x-raw,format=S32LE ! fakesink name=out sync=false signal-handoffs=true",
    )
    .unwrap()
    .downcast::<gst::Pipeline>()
    .unwrap();
    pipeline.by_name("in").unwrap().set_property("location", path.to_str().unwrap());
    let samples = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = samples.clone();
    pipeline.by_name("out").unwrap().connect("handoff", false, move |args| {
        let buffer = args[1].get::<gst::Buffer>().ok()?;
        let map = buffer.map_readable().ok()?;
        let mut samples = sink.lock().unwrap();
        samples.extend(map.as_slice().chunks_exact(4).map(|b| i32::from_le_bytes(b.try_into().unwrap())));
        None
    });
    pipeline.set_state(gst::State::Playing).unwrap();
    let bus = pipeline.bus().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match bus.timed_pop(gst::ClockTime::from_mseconds(100)).as_ref().map(|m| m.view()) {
            Some(gst::MessageView::Eos(_)) => break,
            Some(gst::MessageView::Error(e)) => panic!("could not decode fixture: {}", e.error()),
            _ if Instant::now() > deadline => panic!("timed out decoding fixture"),
            _ => {}
        }
    }
    pipeline.set_state(gst::State::Null).unwrap();
    std::mem::take(&mut *samples.lock().unwrap())
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

/// A request an [`HttpMock`] received.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub at: Instant,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub user_agent: Option<String>,
}

impl HttpRequest {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// An HTTP answer: status, content type and body.
pub type HttpAnswer = (u16, &'static str, String, Vec<(&'static str, String)>);

/// A local HTTP server for tests of online lookups, answering each request
/// with `answer` and keeping every request it got.
pub struct HttpMock {
    pub base: String,
    pub requests: std::sync::Arc<Mutex<Vec<HttpRequest>>>,
}

impl HttpMock {
    pub async fn start(answer: impl Fn(&HttpRequest) -> HttpAnswer + Send + Sync + 'static) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = std::sync::Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let answer = std::sync::Arc::new(answer);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let (log, answer) = (log.clone(), answer.clone());
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && socket.read(&mut byte).await.is_ok_and(|n| n == 1) {
                        head.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&head);
                    let target = head.split_whitespace().nth(1).unwrap_or("/");
                    let url = url::Url::parse(&format!("http://mock{target}")).unwrap();
                    let user_agent = head.lines().find_map(|l| {
                        let (name, value) = l.split_once(':')?;
                        name.eq_ignore_ascii_case("user-agent").then(|| value.trim().to_owned())
                    });
                    let request = HttpRequest {
                        at: Instant::now(),
                        path: url.path().to_owned(),
                        query: url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect(),
                        user_agent,
                    };
                    let (status, content_type, body, headers) = answer(&request);
                    log.lock().unwrap().push(request);
                    let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self { base, requests }
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}
