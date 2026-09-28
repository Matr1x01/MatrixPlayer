//! The equalizer as the processor runs it.
//!
//! A curve becomes a fixed row of filter slots: every EQ band (flat and
//! disabled ones included), then bass and treble. A band that changes never
//! disturbs the others' memories, and a flat slot is skipped with a memory of
//! exactly zero, so bringing it back in is seamless.
//!
//! Changes reach the audio in one of two ways:
//! - continuous ones (a band's gain, frequency or Q) glide: every 32 frames
//!   each moving band steps towards its target, and only its coefficients are
//!   recomputed;
//! - discrete ones (another mode, compare slot or filter kind, a band that
//!   shapes the sound switched on or off, bypass, the EQ on or off) crossfade:
//!   the old and new chains run side by side for 20 ms and are blended.
//!
//! Nothing here allocates while processing: chains are built when settings
//! change, and the crossfade's scratch buffer when the format does.

use super::biquad::{Coeffs, State, max_boost_db};
use super::settings::{AbSlot, EqBand, EqMode, FilterKind};

/// Moving bands take a step this often (frames).
const STEP_FRAMES: usize = 32;
/// Time constant of a band's glide towards new settings.
const GLIDE_S: f64 = 0.02;
/// Length of the crossfade between two chains.
const CROSSFADE_S: f64 = 0.02;
/// Crossfades are processed in blocks of this many frames (the scratch size).
const BLOCK_FRAMES: usize = 256;

/// Which curve a chain plays: a change of compare slot or mode is a new
/// curve, crossfaded to rather than glided to. `None` is the EQ off.
pub type CurveId = Option<(AbSlot, EqMode)>;

/// Whether `band` has no effect at all (the filter would pass the signal
/// through exactly).
fn is_flat(band: &EqBand) -> bool {
    matches!(band.kind, FilterKind::Peak | FilterKind::LowShelf | FilterKind::HighShelf) && band.gain_db == 0.0
}

/// Whether `band` changes the sound.
fn is_live(band: &EqBand) -> bool {
    band.enabled && !is_flat(band)
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    /// Where the band is heading.
    target: EqBand,
    /// The band as filtered now.
    now: EqBand,
    coeffs: Coeffs,
    live: bool,
}

/// One row of filter slots and their memories.
struct Chain {
    slots: Vec<Slot>,
    /// `states[slot * channels + channel]`
    states: Vec<State>,
    channels: usize,
    rate: f64,
    /// Share of the remaining distance a moving band covers in one step.
    glide: f64,
    /// Frames until the moving bands take their next step.
    until_step: usize,
    moving: bool,
}

impl Chain {
    fn new(bands: &[EqBand], rate: f64, channels: usize) -> Self {
        let slots = bands
            .iter()
            .map(|b| Slot { target: *b, now: *b, coeffs: Coeffs::for_band(b, rate), live: is_live(b) })
            .collect::<Vec<_>>();
        Self {
            states: vec![State::default(); slots.len() * channels],
            slots,
            channels,
            rate,
            glide: glide_step(rate),
            until_step: 0,
            moving: false,
        }
    }

    fn set_format(&mut self, rate: f64, channels: usize) {
        self.rate = rate;
        self.glide = glide_step(rate);
        for slot in &mut self.slots {
            slot.coeffs = Coeffs::for_band(&slot.now, rate);
        }
        if channels != self.channels {
            self.channels = channels;
            self.states = vec![State::default(); self.slots.len() * channels];
        }
    }

    /// Whether `bands` can be glided to: the same slots, of the same kinds,
    /// and no band that shapes the sound switched on or off.
    fn glides_to(&self, bands: &[EqBand]) -> bool {
        self.slots.len() == bands.len()
            && self.slots.iter().zip(bands).all(|(slot, band)| {
                let old = &slot.target;
                old.kind == band.kind && (old.enabled == band.enabled || (is_flat(old) && is_flat(band)))
            })
    }

    fn retarget(&mut self, bands: &[EqBand]) {
        for (slot, band) in self.slots.iter_mut().zip(bands) {
            slot.target = *band;
            // Switching a flat band on or off changes nothing audible.
            slot.now.enabled = band.enabled;
            slot.live = is_live(&slot.now);
            self.moving |= slot.now != slot.target;
        }
        self.until_step = 0;
    }

    /// Takes over the memories of `other`'s slots that sound exactly like
    /// this chain's, so a crossfade only blends what really changed.
    fn warm_from(&mut self, other: &Chain) {
        if other.channels != self.channels {
            return;
        }
        let ch = self.channels;
        for (i, (slot, theirs)) in self.slots.iter().zip(&other.slots).enumerate() {
            if slot.live && theirs.now == slot.target && theirs.now == theirs.target {
                self.states[i * ch..(i + 1) * ch].copy_from_slice(&other.states[i * ch..(i + 1) * ch]);
            }
        }
    }

    /// Moves every moving band one step towards its target.
    fn step(&mut self) {
        let ch = self.channels;
        let mut moving = false;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.now == slot.target {
                continue;
            }
            let (now, target) = (&mut slot.now, &slot.target);
            now.gain_db = approach(now.gain_db, target.gain_db, self.glide, 1e-3);
            now.q = approach(now.q, target.q, self.glide, 1e-4);
            let log_freq = approach(now.freq.ln(), target.freq.ln(), self.glide, 1e-5);
            now.freq = if log_freq == target.freq.ln() { target.freq } else { log_freq.exp() };
            slot.coeffs = Coeffs::for_band(now, self.rate);
            let was_live = slot.live;
            slot.live = is_live(now);
            if was_live && !slot.live {
                // Landed on flat: its memory is what a flat filter holds.
                self.states[i * ch..(i + 1) * ch].fill(State::default());
            }
            moving |= slot.now != slot.target;
        }
        self.moving = moving;
    }

    /// Filters `samples`, gliding moving bands along the way.
    fn process(&mut self, samples: &mut [f64]) {
        let ch = self.channels;
        let mut rest = samples;
        while self.moving && !rest.is_empty() {
            if self.until_step == 0 {
                self.step();
                self.until_step = STEP_FRAMES;
            }
            let frames = (rest.len() / ch).min(self.until_step);
            let (now, later) = rest.split_at_mut(frames * ch);
            self.run(now);
            self.until_step -= frames;
            rest = later;
        }
        self.run(rest);
    }

    /// Filters `samples` with the coefficients as they are.
    fn run(&mut self, samples: &mut [f64]) {
        let ch = self.channels;
        for (i, slot) in self.slots.iter().enumerate() {
            if !slot.live {
                continue;
            }
            let states = &mut self.states[i * ch..(i + 1) * ch];
            for frame in samples.chunks_exact_mut(ch) {
                for (s, state) in frame.iter_mut().zip(states.iter_mut()) {
                    *s = state.run(&slot.coeffs, *s);
                }
            }
        }
    }

    /// Lands every band on its target, with empty memories (after a flush).
    fn settle(&mut self) {
        for slot in &mut self.slots {
            slot.now = slot.target;
            slot.coeffs = Coeffs::for_band(&slot.now, self.rate);
            slot.live = is_live(&slot.now);
        }
        self.moving = false;
        self.states.fill(State::default());
    }

    fn engaged(&self) -> bool {
        self.slots.iter().any(|s| s.live || is_live(&s.target))
    }

    fn target_coeffs(&self) -> Vec<Coeffs> {
        self.slots.iter().filter(|s| is_live(&s.target)).map(|s| Coeffs::for_band(&s.target, self.rate)).collect()
    }
}

fn glide_step(rate: f64) -> f64 {
    1.0 - (-(STEP_FRAMES as f64) / (GLIDE_S * rate)).exp()
}

/// One glide step from `now` towards `target`, landing on it once closer
/// than `snap`.
fn approach(now: f64, target: f64, k: f64, snap: f64) -> f64 {
    let next = now + (target - now) * k;
    if (target - next).abs() < snap { target } else { next }
}

/// The equalizer: the chain being heard, and while a discrete change
/// crossfades, the chain it replaces.
pub struct Equalizer {
    chain: Chain,
    old: Chain,
    curve: CurveId,
    /// Frames of the crossfade still to go, of `fade_len`.
    fade_left: usize,
    fade_len: usize,
    /// A discrete change that arrived mid-crossfade, applied once it ends.
    pending: Option<(CurveId, Vec<EqBand>)>,
    scratch: Vec<f64>,
    rate: f64,
    channels: usize,
    max_boost_db: f64,
}

impl Equalizer {
    pub fn new(rate: f64, channels: usize) -> Self {
        Self {
            chain: Chain::new(&[], rate, channels),
            old: Chain::new(&[], rate, channels),
            curve: None,
            fade_left: 0,
            fade_len: crossfade_frames(rate),
            pending: None,
            scratch: vec![0.0; BLOCK_FRAMES * channels],
            rate,
            channels,
            max_boost_db: 0.0,
        }
    }

    /// A new stream format. Filters are recomputed for the rate; a crossfade
    /// under way is cut short (the stream changes here anyway).
    pub fn set_format(&mut self, rate: f64, channels: usize) {
        self.rate = rate;
        self.channels = channels;
        self.fade_len = crossfade_frames(rate);
        self.scratch = vec![0.0; BLOCK_FRAMES * channels];
        self.finish_crossfade();
        self.chain.set_format(rate, channels);
        self.max_boost_db = max_boost_db(&self.chain.target_coeffs(), rate);
    }

    /// Moves to `bands` (one per slot) of `curve`: gliding when only band
    /// settings moved, crossfading otherwise.
    pub fn update(&mut self, curve: CurveId, bands: Vec<EqBand>) {
        if self.fade_left > 0 {
            if curve == self.curve && self.chain.glides_to(&bands) {
                self.chain.retarget(&bands);
                self.pending = None;
            } else {
                self.pending = Some((curve, bands));
            }
        } else if curve == self.curve && self.chain.glides_to(&bands) {
            self.chain.retarget(&bands);
        } else {
            let mut next = Chain::new(&bands, self.rate, self.channels);
            next.warm_from(&self.chain);
            self.old = std::mem::replace(&mut self.chain, next);
            self.curve = curve;
            self.fade_left = self.fade_len;
        }
        self.max_boost_db = max_boost_db(&self.chain.target_coeffs(), self.rate);
    }

    /// Filters `samples`. Returns true when the equalizer settled (a
    /// crossfade or a glide ended, or a change that waited for one was
    /// applied): whether it is engaged, and its largest boost, may differ.
    pub fn process(&mut self, samples: &mut [f64]) -> bool {
        let was_moving = self.chain.moving;
        if self.fade_left == 0 {
            self.chain.process(samples);
            return was_moving && !self.chain.moving;
        }
        let ch = self.channels;
        let mut settled = false;
        for block in samples.chunks_mut(BLOCK_FRAMES * ch) {
            if self.fade_left == 0 {
                self.chain.process(block);
                continue;
            }
            let old = &mut self.scratch[..block.len()];
            old.copy_from_slice(block);
            self.old.run(old);
            self.chain.process(block);
            for (new, old) in block.chunks_exact_mut(ch).zip(old.chunks_exact(ch)) {
                if self.fade_left == 0 {
                    break;
                }
                let w = 1.0 - self.fade_left as f64 / self.fade_len as f64;
                for (n, o) in new.iter_mut().zip(old) {
                    *n = o + (*n - o) * w;
                }
                self.fade_left -= 1;
            }
            if self.fade_left == 0 {
                self.old = Chain::new(&[], self.rate, self.channels);
                settled = true;
                if let Some((curve, bands)) = self.pending.take() {
                    self.update(curve, bands);
                }
            }
        }
        settled || (was_moving && !self.chain.moving)
    }

    /// Forgets the signal (a flush or a new stream): memories are cleared,
    /// glides land and a crossfade under way completes at once.
    pub fn clear(&mut self) {
        self.finish_crossfade();
        self.chain.settle();
    }

    fn finish_crossfade(&mut self) {
        self.fade_left = 0;
        self.old = Chain::new(&[], self.rate, self.channels);
        if let Some((curve, bands)) = self.pending.take() {
            self.chain = Chain::new(&bands, self.rate, self.channels);
            self.curve = curve;
            self.max_boost_db = max_boost_db(&self.chain.target_coeffs(), self.rate);
        }
    }

    #[cfg(test)]
    pub fn crossfading(&self) -> bool {
        self.fade_left > 0
    }

    /// Whether any band shapes the sound: now, once its glide lands, or in
    /// the chain being crossfaded away.
    pub fn engaged(&self) -> bool {
        self.chain.engaged() || (self.fade_left > 0 && self.old.engaged())
    }

    /// The largest boost of the curve being moved to, at the stream's rate.
    pub fn max_boost_db(&self) -> f64 {
        self.max_boost_db
    }

    pub fn flush_denormals(&mut self) {
        self.chain.states.iter_mut().chain(self.old.states.iter_mut()).for_each(State::flush_denormals);
    }

    /// Filter memories of the chain being heard, slot by slot.
    #[cfg(test)]
    pub fn states(&self) -> &[State] {
        &self.chain.states
    }
}

fn crossfade_frames(rate: f64) -> usize {
    ((CROSSFADE_S * rate) as usize).max(1)
}
