use crate::image::Image;
use crate::json::Value;
use crate::layout::Rect;

pub struct Ambient {
    threshold: f32,
    neon: Vec<Rect>,
    neon_dim: u32,
    neon_interval: (u64, u64),
    neon_ticks: (u64, u64),
    shimmer: Vec<Rect>,
    shimmer_every: u32,
    shimmer_amount: f32,
    shimmer_period: u64,
    shimmer_step: u64,
    active: bool,
    dim_until: u64,
    next_flicker: u64,
    dimmed: bool,
    last_step: Option<u64>,
    rng: u64,
    picks: Vec<(usize, u32)>,
    pub force_dim: bool,
}

const TICK_MS: u64 = 100;
const LIT_LUMA: u32 = 120;

impl Ambient {
    pub fn from_json(v: &Value, seed: u64) -> Ambient {
        let rects = |v: Option<&Value>| -> Vec<Rect> {
            v.map(Value::as_arr)
                .unwrap_or_default()
                .iter()
                .filter_map(|r| match r.ints()[..] {
                    [x, y, w, h] => Some(Rect::new(x, y, w, h)),
                    _ => None,
                })
                .collect()
        };
        let pair = |v: Option<&Value>, d: (u64, u64)| match v.map(Value::ints).unwrap_or_default()[..] {
            [a, b] if a >= 0 && b >= a => (a as u64, b as u64),
            _ => d,
        };
        let num = |v: Option<&Value>, d: f64| v.and_then(Value::as_f64).unwrap_or(d);
        let neon = v.get("neon");
        let sh = v.get("shimmer");
        let mut a = Ambient {
            threshold: num(v.get("night_threshold"), 0.6) as f32,
            neon: rects(neon.and_then(|n| n.get("rects"))),
            neon_dim: (num(neon.and_then(|n| n.get("dim")), 0.45).clamp(0.0, 1.0) * 256.0) as u32,
            neon_interval: pair(neon.and_then(|n| n.get("interval_ms")), (4000, 12000)),
            neon_ticks: pair(neon.and_then(|n| n.get("ticks")), (1, 2)),
            shimmer: rects(sh.and_then(|s| s.get("rects"))),
            shimmer_every: num(sh.and_then(|s| s.get("every")), 3.0).max(1.0) as u32,
            shimmer_amount: num(sh.and_then(|s| s.get("amount")), 0.18) as f32,
            shimmer_period: num(sh.and_then(|s| s.get("period_ms")), 7000.0).max(100.0) as u64,
            shimmer_step: num(sh.and_then(|s| s.get("step_ms")), 500.0).max(TICK_MS as f64) as u64,
            active: false,
            dim_until: 0,
            next_flicker: 0,
            dimmed: false,
            last_step: None,
            rng: seed | 1,
            picks: Vec::new(),
            force_dim: false,
        };
        a.next_flicker = a.random_in(a.neon_interval);
        a
    }

    fn random_in(&mut self, (lo, hi): (u64, u64)) -> u64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        lo + self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) % (hi - lo + 1)
    }

    pub fn tick(&mut self, t: u64, night: f32) -> Vec<Rect> {
        let active = night >= self.threshold;
        let mut dirty = Vec::new();
        if !active {
            if self.active {
                self.active = false;
                self.dimmed = false;
                self.last_step = None;
                dirty.extend(&self.neon);
                dirty.extend(&self.shimmer);
            }
            return dirty;
        }
        self.active = true;
        if t >= self.next_flicker {
            let ticks = self.random_in(self.neon_ticks);
            self.dim_until = t + ticks * TICK_MS;
            let gap = self.random_in(self.neon_interval);
            self.next_flicker = t + gap;
        }
        let dimmed = self.force_dim || t < self.dim_until;
        if dimmed != self.dimmed {
            self.dimmed = dimmed;
            dirty.extend(&self.neon);
        }
        let step = t / self.shimmer_step;
        if self.last_step != Some(step) {
            self.last_step = Some(step);
            dirty.extend(&self.shimmer);
        }
        dirty
    }

    pub fn apply(&self, frame: &mut Image, within: Rect) {
        if !self.active {
            return;
        }
        if self.dimmed {
            for r in &self.neon {
                for_lit(frame, *r, within, |p, _, _| scale(p, self.neon_dim));
            }
        }
        let step_t = self.last_step.unwrap_or(0) * self.shimmer_step;
        let fw = frame.w;
        for &(i, h) in &self.picks {
            let (x, y) = ((i % fw) as i32, (i / fw) as i32);
            if !within.contains(x, y) {
                continue;
            }
            let phase = (h >> 8) as f32 / (1 << 24) as f32;
            let s = ((step_t % self.shimmer_period) as f32 / self.shimmer_period as f32 + phase) * std::f32::consts::TAU;
            frame.px[i] = scale(frame.px[i], ((1.0 + self.shimmer_amount * s.sin()) * 256.0) as u32);
        }
    }

    pub fn prepare(&mut self, bg: &[u32], w: usize, h: usize) {
        self.picks = self.shimmer.iter().flat_map(|r| windows_in(bg, w, h, *r, self.shimmer_every)).collect();
    }
}

fn for_lit(frame: &mut Image, r: Rect, within: Rect, f: impl Fn(u32, i32, i32) -> u32) {
    let x0 = r.x.max(within.x).max(0);
    let y0 = r.y.max(within.y).max(0);
    let x1 = (r.x + r.w).min(within.x + within.w).min(frame.w as i32);
    let y1 = (r.y + r.h).min(within.y + within.h).min(frame.h as i32);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y as usize * frame.w + x as usize;
            let p = frame.px[i];
            let luma = ((p >> 16 & 0xFF) * 77 + (p >> 8 & 0xFF) * 150 + (p & 0xFF) * 29) >> 8;
            if luma >= LIT_LUMA {
                frame.px[i] = f(p, x, y);
            }
        }
    }
}

fn luma(p: u32) -> u32 {
    ((p >> 16 & 0xFF) * 77 + (p >> 8 & 0xFF) * 150 + (p & 0xFF) * 29) >> 8
}

fn windows_in(px: &[u32], w: usize, h: usize, r: Rect, every: u32) -> Vec<(usize, u32)> {
    const WINDOW_LUMA: u32 = 150;
    const EDGE: u32 = 45;
    let (w, h) = (w as i32, h as i32);
    let x0 = r.x.max(1);
    let y0 = r.y.max(1);
    let x1 = (r.x + r.w).min(w - 1);
    let y1 = (r.y + r.h).min(h - 1);
    let at = |x: i32, y: i32| luma(px[(y * w + x) as usize]);
    let mut out = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let l = at(x, y);
            if l < WINDOW_LUMA {
                continue;
            }
            let darkest = at(x - 1, y).min(at(x + 1, y)).min(at(x, y - 1)).min(at(x, y + 1));
            let hsh = hash(x, y);
            if l >= darkest + EDGE && hsh % every == 0 {
                out.push(((y * w + x) as usize, hsh));
            }
        }
    }
    out
}

fn scale(p: u32, k: u32) -> u32 {
    let c = |s: u32| ((p >> s & 0xFF) * k >> 8).min(255) << s;
    0xFF00_0000 | c(16) | c(8) | c(0)
}

fn hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0xC2B2_AE3D);
    h ^ (h >> 13)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn amb() -> Ambient {
        let v = parse(r#"{"night_threshold": 0.6, "neon": {"rects": [[0,0,2,2]], "interval_ms": [1000, 1000], "ticks": [2, 2]},
                          "shimmer": {"rects": [[4,4,2,2]], "step_ms": 500}}"#).unwrap();
        Ambient::from_json(&v, 7)
    }

    #[test]
    fn idle_by_day() {
        let mut a = amb();
        for t in (0..20_000).step_by(100) {
            assert!(a.tick(t, 0.2).is_empty());
        }
    }

    #[test]
    fn neon_dips_for_its_ticks_at_night() {
        let mut a = amb();
        a.tick(0, 1.0);
        let dips: Vec<u64> = (100..=3000).step_by(100).filter(|&t| !a.tick(t, 1.0).iter().all(|r| r.x == 4)).collect();
        assert_eq!(dips, vec![1000, 1200, 2000, 2200, 3000]);
    }

    #[test]
    fn restores_once_when_day_comes() {
        let mut a = amb();
        a.tick(0, 1.0);
        assert!(!a.tick(100, 0.0).is_empty());
        assert!(a.tick(200, 0.0).is_empty());
    }
}
