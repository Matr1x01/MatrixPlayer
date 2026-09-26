//! The native processing chain between the decoder and the audio output.
//!
//! The chain is installed as playbin's `audio-filter`, so it lives inside the
//! playback pipeline and survives gapless track changes. Every stage works on
//! 64-bit float samples:
//!
//! ```text
//! decoded PCM ─ audioconvert ─ F64 ─ [stages…] ─ volume ─ audioconvert ─ output
//! ```
//!
//! Stages are created once and adjusted in place; enabling or disabling
//! processing never rebuilds the pipeline mid-track. Future stages
//! (ReplayGain, preamp, graphic/parametric EQ, crossfeed, limiter) are
//! inserted in [`DspChain::build`] before the volume stage, in the order the
//! Audio / DSP screen shows them.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_audio as gst_audio;

use crate::error::AppResult;

pub struct DspChain {
    bin: gst::Bin,
    volume: gst::Element,
}

impl DspChain {
    pub fn build() -> AppResult<Self> {
        let bin = gst::Bin::builder().name("fermata-dsp").build();

        let to_float = gst::ElementFactory::make("audioconvert").name("dsp-in").build()?;
        let float_caps = gst::ElementFactory::make("capsfilter")
            .name("dsp-f64")
            .property(
                "caps",
                gst_audio::AudioCapsBuilder::new_interleaved().format(gst_audio::AudioFormat::F64le).build(),
            )
            .build()?;
        let volume = gst::ElementFactory::make("volume").name("dsp-volume").build()?;
        // Converts back to whatever the output accepts. Dither is applied only
        // if the device needs fewer bits than the float processing produced.
        let to_output = gst::ElementFactory::make("audioconvert").name("dsp-out").build()?;

        let chain = [&to_float, &float_caps, &volume, &to_output];
        bin.add_many(chain)?;
        gst::Element::link_many(chain)?;

        let sink_pad = to_float.static_pad("sink").expect("audioconvert has a sink pad");
        let src_pad = to_output.static_pad("src").expect("audioconvert has a src pad");
        bin.add_pad(&gst::GhostPad::builder_with_target(&sink_pad)?.name("sink").build())?;
        bin.add_pad(&gst::GhostPad::builder_with_target(&src_pad)?.name("src").build())?;

        Ok(Self { bin, volume })
    }

    pub fn element(&self) -> &gst::Element {
        self.bin.upcast_ref()
    }

    /// The chain's input pad, where decoded stream caps arrive.
    pub fn input_pad(&self) -> gst::Pad {
        self.bin.static_pad("sink").expect("dsp bin has a sink ghost pad")
    }

    /// Sets the playback gain as a linear amplitude factor (1.0 = unity).
    pub fn set_volume(&self, linear: f64) {
        self.volume.set_property("volume", linear.clamp(0.0, 1.0));
    }

    pub fn set_muted(&self, muted: bool) {
        self.volume.set_property("mute", muted);
    }
}
