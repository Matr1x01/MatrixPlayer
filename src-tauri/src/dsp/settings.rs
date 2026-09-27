//! What the processing chain should do, as the user configured it.
//!
//! Everything arriving from the UI is validated here and clamped to ranges
//! the processor handles safely, so the audio thread never sees a NaN gain or
//! a filter above Nyquist.

use serde::{Deserialize, Serialize};

pub const MAX_PARAMETRIC_BANDS: usize = 10;
pub const GRAPHIC_10: [f64; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const GRAPHIC_31: [f64; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0,
    1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];
/// Bandwidths matching one octave and one third of an octave.
pub const GRAPHIC_10_Q: f64 = 1.41;
pub const GRAPHIC_31_Q: f64 = 4.32;
pub const MAX_GAIN_DB: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FilterKind {
    #[default]
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    Notch,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    pub enabled: bool,
    pub kind: FilterKind,
    pub freq: f64,
    pub gain_db: f64,
    pub q: f64,
}

impl EqBand {
    pub fn peak(freq: f64, gain_db: f64, q: f64) -> Self {
        Self { enabled: true, kind: FilterKind::Peak, freq, gain_db, q }
    }

    fn sanitize(&mut self) {
        self.freq = finite_or(self.freq, 1000.0).clamp(10.0, 22_000.0);
        self.gain_db = finite_or(self.gain_db, 0.0).clamp(-24.0, MAX_GAIN_DB);
        self.q = finite_or(self.q, 1.0).clamp(0.1, 20.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EqMode {
    #[default]
    Parametric,
    Graphic10,
    Graphic31,
}

/// One complete EQ setting (the design's A and B slots each hold one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EqCurve {
    pub mode: EqMode,
    pub parametric: Vec<EqBand>,
    pub graphic10: Vec<f64>,
    pub graphic31: Vec<f64>,
    /// Bass (low shelf, 100 Hz) and treble (high shelf, 8 kHz) in dB.
    pub bass_db: f64,
    pub treble_db: f64,
}

impl Default for EqCurve {
    fn default() -> Self {
        Self {
            mode: EqMode::Parametric,
            parametric: default_parametric(),
            graphic10: vec![0.0; 10],
            graphic31: vec![0.0; 31],
            bass_db: 0.0,
            treble_db: 0.0,
        }
    }
}

/// A neutral eight-band starting point spread over the spectrum.
pub fn default_parametric() -> Vec<EqBand> {
    vec![
        EqBand { enabled: true, kind: FilterKind::LowShelf, freq: 60.0, gain_db: 0.0, q: 0.7 },
        EqBand::peak(120.0, 0.0, 1.0),
        EqBand::peak(320.0, 0.0, 1.4),
        EqBand::peak(900.0, 0.0, 1.0),
        EqBand::peak(2400.0, 0.0, 1.2),
        EqBand::peak(4800.0, 0.0, 3.0),
        EqBand::peak(8000.0, 0.0, 1.0),
        EqBand { enabled: true, kind: FilterKind::HighShelf, freq: 12000.0, gain_db: 0.0, q: 0.7 },
    ]
}

impl EqCurve {
    fn sanitize(&mut self) {
        self.parametric.truncate(MAX_PARAMETRIC_BANDS);
        self.parametric.iter_mut().for_each(EqBand::sanitize);
        fix_len(&mut self.graphic10, 10);
        fix_len(&mut self.graphic31, 31);
        self.bass_db = finite_or(self.bass_db, 0.0).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
        self.treble_db = finite_or(self.treble_db, 0.0).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
    }

    /// The filters this curve amounts to, EQ bands then tone controls.
    pub fn filters(&self) -> Vec<EqBand> {
        let mut out: Vec<EqBand> = match self.mode {
            EqMode::Parametric => self.parametric.iter().copied().filter(|b| b.enabled).collect(),
            EqMode::Graphic10 => graphic(&GRAPHIC_10, &self.graphic10, GRAPHIC_10_Q),
            EqMode::Graphic31 => graphic(&GRAPHIC_31, &self.graphic31, GRAPHIC_31_Q),
        };
        if self.bass_db.abs() > 0.05 {
            out.push(EqBand { enabled: true, kind: FilterKind::LowShelf, freq: 100.0, gain_db: self.bass_db, q: 0.7 });
        }
        if self.treble_db.abs() > 0.05 {
            out.push(EqBand {
                enabled: true,
                kind: FilterKind::HighShelf,
                freq: 8000.0,
                gain_db: self.treble_db,
                q: 0.7,
            });
        }
        // Flat bands cost CPU and do nothing (pass filters always count).
        out.retain(|b| {
            b.gain_db.abs() > 0.05 || matches!(b.kind, FilterKind::LowPass | FilterKind::HighPass | FilterKind::Notch)
        });
        out
    }
}

fn graphic(freqs: &[f64], gains: &[f64], q: f64) -> Vec<EqBand> {
    freqs.iter().zip(gains).map(|(&freq, &gain_db)| EqBand::peak(freq, gain_db, q)).collect()
}

fn fix_len(values: &mut Vec<f64>, len: usize) {
    values.resize(len, 0.0);
    for v in values.iter_mut() {
        *v = finite_or(*v, 0.0).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ReplayGainMode {
    Off,
    Track,
    #[default]
    Album,
    /// Album gain in order, track gain when shuffling.
    Smart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AbSlot {
    #[default]
    A,
    B,
}

/// Bumped when a default changes in a way saved settings must follow.
pub const SETTINGS_VERSION: u32 = 2;

fn legacy_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DspSettings {
    /// Format of these settings; missing in the first saved format.
    #[serde(default = "legacy_version")]
    pub version: u32,
    pub eq_enabled: bool,
    /// Compare slot in use; `bypass` hears the unequalized signal.
    pub slot: AbSlot,
    pub bypass: bool,
    pub a: EqCurve,
    pub b: EqCurve,
    /// Name of the preset the active slot was loaded from, if any.
    pub preset: Option<String>,
    pub preamp_db: f64,
    /// Lower the preamp by the EQ's largest boost, so it cannot clip.
    pub auto_preamp: bool,
    /// −1 (left) … 1 (right).
    pub balance: f64,
    /// 0 (mono) … 2 (twice as wide); 1 leaves the image alone.
    pub width: f64,
    pub crossfeed: bool,
    pub replay_gain: ReplayGainMode,
    pub replay_gain_preamp_db: f64,
    /// Gain for files without ReplayGain tags while ReplayGain is on. 0 dB
    /// plays them as mastered, as other players do.
    pub replay_gain_fallback_db: f64,
    pub clipping_prevention: bool,
    pub soft_fades: bool,
    pub dither: bool,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            eq_enabled: false,
            slot: AbSlot::A,
            bypass: false,
            a: EqCurve::default(),
            b: EqCurve::default(),
            preset: Some("Flat".into()),
            preamp_db: 0.0,
            auto_preamp: true,
            balance: 0.0,
            width: 1.0,
            crossfeed: false,
            replay_gain: ReplayGainMode::Album,
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            clipping_prevention: true,
            soft_fades: true,
            dither: false,
        }
    }
}

impl DspSettings {
    /// Brings settings saved by an older version up to date. Only values
    /// still at an old default change; anything the user chose is kept.
    pub fn migrate(mut self) -> Self {
        if self.version < 2 {
            // Version 1 played untagged files 8 dB down, which made them
            // sound quieter than in any other player.
            if self.replay_gain_fallback_db == -8.0 {
                self.replay_gain_fallback_db = 0.0;
            }
        }
        self.version = SETTINGS_VERSION;
        self
    }

    pub fn sanitize(mut self) -> Self {
        self.a.sanitize();
        self.b.sanitize();
        self.preamp_db = finite_or(self.preamp_db, 0.0).clamp(-24.0, MAX_GAIN_DB);
        self.balance = finite_or(self.balance, 0.0).clamp(-1.0, 1.0);
        self.width = finite_or(self.width, 1.0).clamp(0.0, 2.0);
        self.replay_gain_preamp_db = finite_or(self.replay_gain_preamp_db, 0.0).clamp(-15.0, 15.0);
        self.replay_gain_fallback_db = finite_or(self.replay_gain_fallback_db, 0.0).clamp(-24.0, 6.0);
        if let Some(name) = &mut self.preset {
            name.truncate(100);
        }
        self
    }

    pub fn curve(&self) -> &EqCurve {
        match self.slot {
            AbSlot::A => &self.a,
            AbSlot::B => &self.b,
        }
    }

    /// Whether the EQ (and tone controls) currently shape the sound.
    pub fn eq_active(&self) -> bool {
        self.eq_enabled && !self.bypass
    }
}

/// A named EQ curve (built in, or saved by the user).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub name: String,
    pub curve: EqCurve,
    #[serde(default)]
    pub builtin: bool,
}

fn graphic_preset(name: &str, gains: [f64; 10]) -> Preset {
    Preset {
        name: name.into(),
        curve: EqCurve { mode: EqMode::Graphic10, graphic10: gains.to_vec(), ..EqCurve::default() },
        builtin: true,
    }
}

pub fn builtin_presets() -> Vec<Preset> {
    vec![
        Preset { name: "Flat".into(), curve: EqCurve::default(), builtin: true },
        graphic_preset("Loudness", [5.0, 4.0, 2.0, 0.0, -1.0, 0.0, 0.5, 2.0, 3.5, 4.0]),
        graphic_preset("Bass Boost", [6.0, 5.0, 3.5, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        graphic_preset("Treble Cut", [0.0, 0.0, 0.0, 0.0, 0.0, -0.5, -1.5, -3.0, -5.0, -6.0]),
        graphic_preset("Acoustic", [3.0, 2.5, 1.5, 0.5, 1.0, 1.0, 2.0, 2.5, 2.0, 1.0]),
        graphic_preset("Classical", [3.0, 2.0, 1.0, 0.0, 0.0, 0.0, -1.0, -1.5, -2.0, -3.0]),
        graphic_preset("Vocal Presence", [-2.0, -1.5, -0.5, 1.0, 2.5, 3.0, 2.5, 1.0, 0.0, -1.0]),
        graphic_preset("Late Night", [-4.0, -3.0, -1.5, 0.0, 0.5, 1.0, 1.0, 0.5, -0.5, -1.5]),
    ]
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizing_clamps_everything_to_safe_ranges() {
        let mut s = DspSettings { preamp_db: f64::NAN, balance: 7.0, width: -1.0, ..DspSettings::default() };
        s.a.parametric = vec![EqBand::peak(90_000.0, 40.0, 0.0); 14];
        s.a.graphic10 = vec![1.0; 3];
        let s = s.sanitize();
        assert_eq!(s.preamp_db, 0.0);
        assert_eq!(s.balance, 1.0);
        assert_eq!(s.width, 0.0);
        assert_eq!(s.a.parametric.len(), MAX_PARAMETRIC_BANDS);
        assert_eq!(s.a.parametric[0].freq, 22_000.0);
        assert_eq!(s.a.parametric[0].gain_db, MAX_GAIN_DB);
        assert_eq!(s.a.parametric[0].q, 0.1);
        assert_eq!(s.a.graphic10, [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn flat_curves_add_no_filters_and_tone_controls_do() {
        let mut curve = EqCurve::default();
        assert!(curve.filters().is_empty());
        curve.bass_db = 3.0;
        let filters = curve.filters();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].kind, FilterKind::LowShelf);
        curve.mode = EqMode::Graphic31;
        curve.graphic31[10] = -2.0;
        assert_eq!(curve.filters().len(), 2);
    }

    #[test]
    fn migrating_moves_only_old_defaults() {
        let old: DspSettings = serde_json::from_str(r#"{"replayGainFallbackDb":-8}"#).unwrap();
        assert_eq!(old.version, 1);
        let migrated = old.migrate();
        assert_eq!((migrated.version, migrated.replay_gain_fallback_db), (SETTINGS_VERSION, 0.0));

        let chosen: DspSettings = serde_json::from_str(r#"{"replayGainFallbackDb":-6}"#).unwrap();
        assert_eq!(chosen.migrate().replay_gain_fallback_db, -6.0, "a user's own value stays");
        let current: DspSettings = serde_json::from_str(r#"{"version":2,"replayGainFallbackDb":-8}"#).unwrap();
        assert_eq!(current.migrate().replay_gain_fallback_db, -8.0, "chosen after the change: kept");
    }

    #[test]
    fn older_settings_fill_in_new_fields() {
        let s: DspSettings = serde_json::from_str(r#"{"eqEnabled":true,"preampDb":-3}"#).unwrap();
        assert!(s.eq_enabled);
        assert_eq!(s.preamp_db, -3.0);
        assert_eq!(s.a.parametric.len(), 8);
        assert_eq!(s.replay_gain, ReplayGainMode::Album);
    }
}
