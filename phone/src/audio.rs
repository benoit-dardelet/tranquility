//! Phone audio: tone generators (ringtone, ringback, busy, DTMF key tones,
//! message chime, alarm) mixed with the call downlink from the baseband.
//! Output is 48 kHz signed 16-bit mono; the platform driver pulls samples
//! with [`crate::ui::Phone::audio`].
use crate::prelude::*;
use crate::services::{CallState, Core};

pub const RATE: u32 = 48_000;
const TABLE: usize = 1024;

/// Sine table built with Bhaskara I's approximation (no libm in no_std).
pub fn sine_table() -> Vec<i16> {
    (0..TABLE)
        .map(|i| {
            let x = (i % (TABLE / 2)) as f32 / (TABLE / 2) as f32 * core::f32::consts::PI;
            let p = x * (core::f32::consts::PI - x);
            let v = 16.0 * p / (5.0 * core::f32::consts::PI * core::f32::consts::PI - 4.0 * p);
            let v = if i < TABLE / 2 { v } else { -v };
            (v * 32767.0) as i16
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Key(char),
    Message,
    CallEnd,
    Busy,
}

fn dtmf(c: char) -> (f32, f32) {
    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0];
    let (r, cl) = match c {
        '1' => (0, 0),
        '2' => (0, 1),
        '3' => (0, 2),
        '4' => (1, 0),
        '5' => (1, 1),
        '6' => (1, 2),
        '7' => (2, 0),
        '8' => (2, 1),
        '9' => (2, 2),
        '*' => (3, 0),
        '0' => (3, 1),
        _ => (3, 2),
    };
    (rows[r], cols[cl])
}

/// One scheduled tone segment: frequencies, start/end in samples.
#[derive(Clone, Copy, Debug)]
struct Seg {
    f: [f32; 2],
    start: u64,
    end: u64,
    gain: f32,
}

pub struct Audio {
    sin: Vec<i16>,
    t: u64,
    segs: Vec<Seg>,
    voice: Vec<i16>,
    voice_pos: usize,
    pub enabled: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

const fn ms(n: u64) -> u64 {
    n * RATE as u64 / 1000
}

impl Audio {
    pub fn new() -> Audio {
        Audio { sin: sine_table(), t: 0, segs: Vec::new(), voice: Vec::new(), voice_pos: 0, enabled: true }
    }

    fn tone(&self, f: f32, t: u64) -> f32 {
        // index = f * t * TABLE / RATE (mod TABLE), in integers
        let idx = ((f * 64.0) as u64).wrapping_mul(t) * (TABLE as u64 / 64) / RATE as u64;
        self.sin[idx as usize % TABLE] as f32 / 32767.0
    }

    fn add(&mut self, f: [f32; 2], start_ms: u64, len_ms: u64, gain: f32) {
        let start = self.t + ms(start_ms);
        self.segs.push(Seg { f, start, end: start + ms(len_ms), gain });
    }

    pub fn cue(&mut self, c: Cue) {
        match c {
            Cue::Key(d) => {
                let (a, b) = dtmf(d);
                self.add([a, b], 0, 120, 0.25);
            }
            Cue::Message => {
                self.add([1047.0, 0.0], 0, 110, 0.35);
                self.add([1319.0, 0.0], 130, 160, 0.35);
            }
            Cue::CallEnd => {
                for i in 0..3 {
                    self.add([425.0, 0.0], i * 250, 150, 0.3);
                }
            }
            Cue::Busy => {
                for i in 0..4 {
                    self.add([480.0, 620.0], i * 1000, 500, 0.25);
                }
            }
        }
    }

    /// Continuous sounds that depend on phone state (ms since boot `now`).
    fn state_sample(&self, c: &Core, now_ms: u64, t: u64) -> f32 {
        if c.alarm_ringing {
            let p = now_ms % 1300;
            return if p < 800 && (p / 100) % 2 == 0 { 0.4 * self.tone(880.0, t) } else { 0.0 };
        }
        match &c.call {
            Some(call) if call.state == CallState::Incoming && c.settings.ringer && !call.silenced => {
                // little arpeggio: E5 G5 C6 G5, twice, then rest
                const NOTES: [f32; 4] = [659.3, 784.0, 1046.5, 784.0];
                let p = now_ms % 2600;
                if p >= 1600 {
                    return 0.0;
                }
                let n = NOTES[((p / 200) % 4) as usize];
                let env = 1.0 - (p % 200) as f32 / 260.0;
                0.35 * env * (self.tone(n, t) + 0.3 * self.tone(3.0 * n, t))
            }
            Some(call) if call.state == CallState::Alerting && !call.incoming => {
                // North American ringback: 440+480 Hz, 2 s on, 4 s off
                let p = now_ms.saturating_sub(call.started) % 6000;
                if p < 2000 {
                    0.12 * (self.tone(440.0, t) + self.tone(480.0, t))
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// Fill `out` with mono 48 kHz samples.
    pub fn fill(&mut self, c: &mut Core, out: &mut [i16]) {
        let active = c.call.as_ref().is_some_and(|x| x.state == CallState::Active);
        // pull 8 kHz call downlink from the baseband and upsample 6x
        if active {
            let need = out.len().div_ceil(6) + 1;
            if self.voice.len() - self.voice_pos < need {
                self.voice.drain(..self.voice_pos);
                self.voice_pos = 0;
                let mut v = vec![0i16; need.max(160)];
                if c.modem.voice(&mut v) {
                    self.voice.extend(v);
                }
            }
        } else {
            self.voice.clear();
            self.voice_pos = 0;
        }
        let now = c.now;
        self.segs.retain(|s| s.end > self.t);
        let muted = !self.enabled;
        for (i, o) in out.iter_mut().enumerate() {
            let t = self.t + i as u64;
            let mut v = self.state_sample(c, now + i as u64 * 1000 / RATE as u64, t);
            for s in &self.segs {
                if t >= s.start && t < s.end {
                    let mut x = self.tone(s.f[0], t);
                    if s.f[1] > 0.0 {
                        x = 0.5 * (x + self.tone(s.f[1], t));
                    }
                    v += s.gain * x;
                }
            }
            if active {
                let idx = self.voice_pos + i / 6;
                let a = *self.voice.get(idx).unwrap_or(&0) as f32;
                let b = *self.voice.get(idx + 1).unwrap_or(&0) as f32;
                let frac = (i % 6) as f32 / 6.0;
                v += (a + (b - a) * frac) / 32768.0;
            }
            *o = if muted { 0 } else { (v.clamp(-1.0, 1.0) * 32000.0) as i16 };
        }
        if active {
            self.voice_pos = (self.voice_pos + out.len() / 6).min(self.voice.len());
        }
        self.t += out.len() as u64;
    }
}

/// Simulated far-end talker: formant-filtered harmonic "babble" at 8 kHz
/// (narrowband voice), so an active call sounds like someone speaking.
pub struct Talker {
    sin: Vec<i16>,
    n: u64,
    f0: f32,
    base_f0: f32,
    gains: Vec<f32>,
    phase: u32,
    syl_len: u64,
    syl_start: u64,
    silent: bool,
    seed: u32,
}

const VOWELS: [(f32, f32); 5] = [(730.0, 1090.0), (530.0, 1840.0), (270.0, 2290.0), (570.0, 840.0), (300.0, 870.0)];

impl Talker {
    pub fn new(base_f0: f32, seed: u32) -> Talker {
        let mut t = Talker { sin: sine_table(), n: 0, f0: base_f0, base_f0, gains: Vec::new(), phase: 0, syl_len: 1, syl_start: 0, silent: true, seed: seed | 1 };
        t.next_syllable();
        t
    }

    fn rand(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }

    fn next_syllable(&mut self) {
        self.syl_start = self.n;
        let r = self.rand();
        self.silent = r % 6 == 0;
        self.syl_len = 8000 * (120 + (r >> 8) as u64 % 160) / 1000;
        let (f1, f2) = VOWELS[(r >> 16) as usize % VOWELS.len()];
        self.f0 = self.base_f0 * (0.85 + ((r >> 4) % 30) as f32 / 100.0);
        self.gains = (1..=20)
            .map(|k| {
                let f = k as f32 * self.f0;
                if f > 3400.0 {
                    return 0.0;
                }
                let a = 1.0 / (1.0 + ((f - f1) / 90.0) * ((f - f1) / 90.0));
                let b = 0.6 / (1.0 + ((f - f2) / 120.0) * ((f - f2) / 120.0));
                (a + b) / k as f32
            })
            .collect();
    }

    pub fn fill(&mut self, out: &mut [i16]) {
        for o in out.iter_mut() {
            if self.n - self.syl_start >= self.syl_len {
                self.next_syllable();
            }
            let pos = (self.n - self.syl_start) as f32 / self.syl_len as f32;
            let env = if self.silent { 0.0 } else { 4.0 * pos * (1.0 - pos) };
            // slight falling intonation within the syllable
            let f0 = self.f0 * (1.05 - 0.1 * pos);
            self.phase = self.phase.wrapping_add((f0 * (4_294_967_296.0 / 8000.0)) as u32);
            let mut v = 0.0f32;
            for (k, g) in self.gains.iter().enumerate() {
                if *g == 0.0 {
                    break;
                }
                let ph = self.phase.wrapping_mul(k as u32 + 1) >> 22;
                v += g * self.sin[ph as usize] as f32;
            }
            *o = (v * env * 0.5).clamp(-32000.0, 32000.0) as i16;
            self.n += 1;
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn sine_accuracy() {
        let s = sine_table();
        for (i, v) in s.iter().enumerate() {
            let exact = ((i as f64) / TABLE as f64 * 2.0 * std::f64::consts::PI).sin() * 32767.0;
            assert!((*v as f64 - exact).abs() < 70.0, "{} {} {}", i, v, exact);
        }
    }
    #[test]
    fn talker_is_audible() {
        let mut t = Talker::new(200.0, 7);
        let mut b = vec![0i16; 8000];
        t.fill(&mut b);
        let rms = (b.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / b.len() as f64).sqrt();
        assert!(rms > 1000.0, "rms {}", rms);
    }
}
