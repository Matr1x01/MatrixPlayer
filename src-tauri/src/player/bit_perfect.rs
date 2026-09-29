//! Whether playback is bit-perfect right now: the decoded samples reaching
//! the device unchanged, at their own rate and resolution.
//!
//! In bit-perfect playback nothing in MatrixPlayer changes a sample (the
//! processing is bypassed, the volume fixed at unity, and integers survive
//! the float stage exactly), so what is left to judge is the path around
//! it: what the decoder produced, what the output took, and — through the
//! sound server — what PipeWire does after that.

use serde::Serialize;

use crate::audio::StreamFormat;
use crate::audio::output::PipeWireClock;

/// How playback reaches the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Processing as configured.
    Normal,
    /// Nothing processed, through the sound server.
    Shared,
    /// Nothing processed, straight to the hardware.
    Exclusive,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum BitPerfect {
    /// Bit-perfect playback is off.
    Off,
    /// Nothing has played in this mode yet.
    Pending,
    /// The samples reach the device unchanged. Through the sound server
    /// (`shared`) that holds only while no other program plays and its own
    /// volume is at 100%.
    Yes { shared: bool },
    /// Resampled, from the track's rate to the device's.
    Resampled { from: u32, to: u32 },
    /// The device takes fewer bits than the track has.
    Truncated { from: u32, to: u32 },
    /// Channels mixed to fit the device.
    Remixed { from: u32, to: u32 },
    /// A float stream (from a lossy decoder) rounded to integers.
    FromFloat,
}

/// Bits of an integer sample a format carries exactly: a float keeps as many
/// as its mantissa (24 for 32-bit float, 53 for 64-bit).
fn exact_bits(format: &StreamFormat) -> u32 {
    match (format.float, format.bits) {
        (false, bits) => bits,
        (true, bits) if bits >= 64 => 53,
        (true, _) => 24,
    }
}

/// PipeWire mixes in 32-bit float.
const PIPEWIRE_MIX: StreamFormat =
    StreamFormat { sample_format: String::new(), bits: 32, float: true, sample_rate: 0, channels: 0 };

pub fn verdict(
    mode: Mode,
    decoded: Option<&StreamFormat>,
    output: Option<&StreamFormat>,
    pipewire: Option<&PipeWireClock>,
) -> BitPerfect {
    if mode == Mode::Normal {
        return BitPerfect::Off;
    }
    let (Some(decoded), Some(output)) = (decoded, output) else {
        return BitPerfect::Pending;
    };
    let shared = mode == Mode::Shared;
    if output.sample_rate != decoded.sample_rate {
        return BitPerfect::Resampled { from: decoded.sample_rate, to: output.sample_rate };
    }
    if shared && let Some(clock) = pipewire {
        let device = clock.rate_for(decoded.sample_rate);
        if device != decoded.sample_rate {
            return BitPerfect::Resampled { from: decoded.sample_rate, to: device };
        }
    }
    if output.channels != decoded.channels {
        return BitPerfect::Remixed { from: decoded.channels, to: output.channels };
    }
    if decoded.float {
        if !output.float {
            return BitPerfect::FromFloat;
        }
        if output.bits < decoded.bits {
            return BitPerfect::Truncated { from: decoded.bits, to: output.bits };
        }
    } else {
        let carried = if shared { exact_bits(output).min(exact_bits(&PIPEWIRE_MIX)) } else { exact_bits(output) };
        if carried < decoded.bits {
            return BitPerfect::Truncated { from: decoded.bits, to: carried };
        }
    }
    BitPerfect::Yes { shared }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(sample_format: &str, bits: u32, float: bool, rate: u32, channels: u32) -> StreamFormat {
        StreamFormat { sample_format: sample_format.into(), bits, float, sample_rate: rate, channels }
    }

    fn clock(allowed: &[u32]) -> PipeWireClock {
        PipeWireClock { rate: 48_000, allowed_rates: allowed.to_vec(), force_rate: 0 }
    }

    #[test]
    fn exclusive_integer_streams_padded_or_not_are_bit_perfect() {
        let s16 = format("S16LE", 16, false, 44_100, 2);
        let s24 = format("S24LE", 24, false, 96_000, 2);
        for (decoded, output) in [
            (&s16, format("S16LE", 16, false, 44_100, 2)),
            (&s16, format("S32LE", 32, false, 44_100, 2)),
            (&s24, format("S32LE", 32, false, 96_000, 2)),
        ] {
            assert_eq!(verdict(Mode::Exclusive, Some(decoded), Some(&output), None), BitPerfect::Yes { shared: false });
        }
    }

    #[test]
    fn says_what_breaks_it() {
        let s24 = format("S24LE", 24, false, 44_100, 2);
        let v = |output: StreamFormat| verdict(Mode::Exclusive, Some(&s24), Some(&output), None);
        assert_eq!(v(format("S32LE", 32, false, 48_000, 2)), BitPerfect::Resampled { from: 44_100, to: 48_000 });
        assert_eq!(v(format("S16LE", 16, false, 44_100, 2)), BitPerfect::Truncated { from: 24, to: 16 });
        assert_eq!(v(format("S32LE", 32, false, 44_100, 6)), BitPerfect::Remixed { from: 2, to: 6 });
        let mp3 = format("F32LE", 32, true, 44_100, 2);
        assert_eq!(
            verdict(Mode::Exclusive, Some(&mp3), Some(&format("S32LE", 32, false, 44_100, 2)), None),
            BitPerfect::FromFloat
        );
        assert_eq!(verdict(Mode::Normal, Some(&s24), Some(&s24), None), BitPerfect::Off);
        assert_eq!(verdict(Mode::Exclusive, Some(&s24), None, None), BitPerfect::Pending);
    }

    #[test]
    fn through_pipewire_the_device_rate_and_its_float_mix_count() {
        let s16 = format("S16LE", 16, false, 44_100, 2);
        let s32 = format("S32LE", 32, false, 44_100, 2);
        let stream = format("F64LE", 64, true, 44_100, 2);
        let shared = |decoded: &StreamFormat, allowed: &[u32]| {
            verdict(Mode::Shared, Some(decoded), Some(&stream), Some(&clock(allowed)))
        };
        assert_eq!(shared(&s16, &[48_000]), BitPerfect::Resampled { from: 44_100, to: 48_000 });
        assert_eq!(shared(&s16, &[44_100, 48_000]), BitPerfect::Yes { shared: true });
        assert_eq!(shared(&s32, &[44_100, 48_000]), BitPerfect::Truncated { from: 32, to: 24 });
        assert_eq!(
            verdict(Mode::Shared, Some(&s16), Some(&stream), None),
            BitPerfect::Yes { shared: true },
            "clock unknown"
        );
    }
}
