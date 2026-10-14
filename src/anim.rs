use std::path::Path;

use crate::image::Image;
use crate::json::Value;
use crate::layout::Rect;

pub const PAD: i32 = 2;

struct Region {
    w: i32,
    mask: Vec<bool>, // sprite-sized
    bbox: Rect,      // sprite space
}

impl Region {
    fn has(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && (y * self.w + x) < self.mask.len() as i32 && self.mask[(y * self.w + x) as usize]
    }
}

enum Pattern {
    Sine { offset: (i32, i32), period_ms: u64, phase_ms: u64 },
    Steps { offsets: Vec<(i32, i32)>, step_ms: u64 },
    Once { offsets: Vec<(i32, i32)>, step_ms: u64, start: Option<u64> },
}

struct Proc {
    region: usize,
    pattern: Pattern,
}

impl Proc {
    fn offset(&self, t: u64) -> (i32, i32) {
        match &self.pattern {
            Pattern::Sine { offset, period_ms, phase_ms } => {
                let x = ((t + phase_ms) % period_ms) as f32 / *period_ms as f32;
                let v = (1.0 - (x * std::f32::consts::TAU).cos()) / 2.0;
                ((offset.0 as f32 * v).round() as i32, (offset.1 as f32 * v).round() as i32)
            }
            Pattern::Steps { offsets, step_ms } => offsets[((t / step_ms) % offsets.len() as u64) as usize],
            Pattern::Once { offsets, step_ms, start } => match start {
                Some(s) if t >= *s => offsets.get(((t - s) / step_ms) as usize).copied().unwrap_or((0, 0)),
                _ => (0, 0),
            },
        }
    }
}

struct Trigger {
    on: String,
    proc_index: Option<usize>,
    effect: Option<String>,
    cooldown_ms: u64,
    last: Option<u64>,
}

struct FrameSwap {
    img: Image,
    at: (i32, i32),
    duration_ms: u64,
    interval_ms: (u64, u64),
    next_start: u64,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AnimState {
    offsets: Vec<(i32, i32)>,
    frames_on: Vec<bool>,
}

pub struct SpriteAnim {
    regions: Vec<Region>,
    procs: Vec<Proc>,
    frames: Vec<FrameSwap>,
    triggers: Vec<Trigger>,
    rng: u64,
    pub force_frames: bool,
}

impl SpriteAnim {
    pub fn from_json(v: &Value, w: usize, h: usize, ready: &Path, seed: u64) -> Result<SpriteAnim, String> {
        let mut names = Vec::new();
        let mut regions = Vec::new();
        for (name, r) in v.get("regions").map(Value::entries).unwrap_or_default() {
            let mut mask = vec![false; w * h];
            for rect in r.get("rects").map(Value::as_arr).unwrap_or_default() {
                let [x, y, rw, rh] = rect.ints()[..] else { return Err(format!("{name}: rect needs [x,y,w,h]")) };
                for yy in y.max(0)..(y + rh).min(h as i32) {
                    for xx in x.max(0)..(x + rw).min(w as i32) {
                        mask[yy as usize * w + xx as usize] = true;
                    }
                }
            }
            if let Some(file) = r.get("mask").and_then(Value::as_str) {
                let m = Image::load_png(&ready.join(file))?;
                if (m.w, m.h) != (w, h) {
                    return Err(format!("{name}: mask {file} is {}x{}, sprite is {w}x{h}", m.w, m.h));
                }
                for (i, p) in m.px.iter().enumerate() {
                    mask[i] |= p >> 24 >= 128;
                }
            }
            let bbox = mask_bbox(&mask, w).ok_or(format!("{name}: empty region"))?;
            names.push(name.as_str());
            regions.push(Region { w: w as i32, mask, bbox });
        }
        let mut procs = Vec::new();
        for p in v.get("procedural").map(Value::as_arr).unwrap_or_default() {
            let rname = p.get("region").and_then(Value::as_str).ok_or("procedural: missing region")?;
            let region = names.iter().position(|n| *n == rname).ok_or(format!("unknown region {rname}"))?;
            let ms = |k: &str, d: u64| p.get(k).and_then(Value::as_f64).map_or(d, |v| v.max(1.0) as u64);
            let pair = |v: &Value| match v.ints()[..] {
                [x, y] => Ok((x, y)),
                _ => Err(format!("{rname}: offsets are [dx,dy]")),
            };
            let pattern = match p.get("pattern").and_then(Value::as_str) {
                Some("sine") => Pattern::Sine {
                    offset: pair(p.get("offset").ok_or(format!("{rname}: sine needs offset"))?)?,
                    period_ms: ms("period_ms", 4000),
                    phase_ms: p.get("phase_ms").and_then(Value::as_f64).map_or(0, |v| v.max(0.0) as u64),
                },
                Some("steps") => {
                    let offsets = p.get("offsets").map(Value::as_arr).unwrap_or_default().iter().map(pair).collect::<Result<Vec<_>, _>>()?;
                    if offsets.is_empty() {
                        return Err(format!("{rname}: steps needs offsets"));
                    }
                    Pattern::Steps { offsets, step_ms: ms("step_ms", 150) }
                }
                other => return Err(format!("{rname}: unknown pattern {other:?}")),
            };
            procs.push(Proc { region, pattern });
        }
        let mut triggers = Vec::new();
        for tr in v.get("triggers").map(Value::as_arr).unwrap_or_default() {
            let on = tr.get("on").and_then(Value::as_str).ok_or("triggers: missing on")?.to_string();
            let effect = tr.get("effect").and_then(Value::as_str).map(str::to_string);
            let proc_index = match tr.get("region").and_then(Value::as_str) {
                None => None,
                Some(rname) => {
                    let region = names.iter().position(|n| *n == rname).ok_or(format!("unknown region {rname}"))?;
                    let offsets = tr
                        .get("offsets")
                        .map(Value::as_arr)
                        .unwrap_or_default()
                        .iter()
                        .map(|o| match o.ints()[..] {
                            [x, y] => Ok((x, y)),
                            _ => Err(format!("{rname}: offsets are [dx,dy]")),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let step_ms = tr.get("step_ms").and_then(Value::as_f64).map_or(100, |v| v.max(1.0) as u64);
                    procs.push(Proc { region, pattern: Pattern::Once { offsets, step_ms, start: None } });
                    Some(procs.len() - 1)
                }
            };
            let cooldown_ms = tr.get("cooldown_ms").and_then(Value::as_f64).map_or(0, |v| v.max(0.0) as u64);
            triggers.push(Trigger { on, proc_index, effect, cooldown_ms, last: None });
        }
        let mut anim = SpriteAnim { regions, procs, frames: Vec::new(), triggers, rng: seed | 1, force_frames: false };
        for f in v.get("frames").map(Value::as_arr).unwrap_or_default() {
            let file = f.get("image").and_then(Value::as_str).ok_or("frames: missing image")?;
            let [ax, ay] = f.get("at").map(Value::ints).unwrap_or_default()[..] else { return Err(format!("{file}: at needs [x,y]")) };
            let iv = f.get("interval_ms").map(Value::ints).unwrap_or_default();
            let interval_ms = match iv[..] {
                [a, b] if a > 0 && b >= a => (a as u64, b as u64),
                _ => (3000, 7000),
            };
            let img = Image::load_png(&ready.join(file))?;
            let duration_ms = f.get("duration_ms").and_then(Value::as_f64).map_or(150, |v| v.max(1.0) as u64);
            let mut fs = FrameSwap { img, at: (ax, ay), duration_ms, interval_ms, next_start: 0 };
            fs.next_start = anim.random_in(interval_ms);
            anim.frames.push(fs);
        }
        Ok(anim)
    }

    fn random_in(&mut self, (lo, hi): (u64, u64)) -> u64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        lo + self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) % (hi - lo + 1)
    }

    pub fn fire(&mut self, on: &str, t: u64) -> Vec<String> {
        let mut effects = Vec::new();
        for tr in &mut self.triggers {
            if tr.on != on || tr.last.is_some_and(|l| t < l + tr.cooldown_ms) {
                continue;
            }
            tr.last = Some(t);
            if let Some(i) = tr.proc_index {
                if let Pattern::Once { start, .. } = &mut self.procs[i].pattern {
                    *start = Some(t);
                }
            }
            effects.extend(tr.effect.clone());
        }
        effects
    }

    pub fn state(&mut self, t: u64) -> AnimState {
        let offsets = self.procs.iter().map(|p| p.offset(t)).collect();
        let mut frames_on = Vec::with_capacity(self.frames.len());
        for i in 0..self.frames.len() {
            while t >= self.frames[i].next_start + self.frames[i].duration_ms {
                let gap = self.random_in(self.frames[i].interval_ms);
                self.frames[i].next_start = self.frames[i].next_start.max(t.saturating_sub(self.frames[i].duration_ms)) + gap;
            }
            frames_on.push(self.force_frames || t >= self.frames[i].next_start);
        }
        AnimState { offsets, frames_on }
    }

    #[cfg(test)]
    pub fn render(&self, base: &Image, st: &AnimState, out: &mut Image) {
        self.render_rect(base, st, out, Rect::new(-PAD, -PAD, out.w as i32, out.h as i32));
    }

    pub fn render_rect(&self, base: &Image, st: &AnimState, out: &mut Image, rect: Rect) {
        let ow = out.w as i32;
        let (x0, y0) = (rect.x.max(-PAD), rect.y.max(-PAD));
        let (x1, y1) = ((rect.x + rect.w).min(ow - PAD), (rect.y + rect.h).min(out.h as i32 - PAD));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let shown: Vec<&FrameSwap> = self.frames.iter().zip(&st.frames_on).filter(|(_, on)| **on).map(|(f, _)| f).collect();
        let static_at = |x: i32, y: i32| -> u32 {
            for f in shown.iter().rev() {
                let (fx, fy) = (x - f.at.0, y - f.at.1);
                if fx >= 0 && fy >= 0 && fx < f.img.w as i32 && fy < f.img.h as i32 {
                    let p = f.img.px[(fy * f.img.w as i32 + fx) as usize];
                    if p >> 24 >= 128 {
                        return p | 0xFF00_0000;
                    }
                }
            }
            if x >= 0 && y >= 0 && x < base.w as i32 && y < base.h as i32 { base.px[y as usize * base.w + x as usize] } else { 0 }
        };
        for y in y0..y1 {
            for x in x0..x1 {
                out.px[((y + PAD) * ow + x + PAD) as usize] = static_at(x, y);
            }
        }
        for (p, &(dx, dy)) in self.procs.iter().zip(&st.offsets) {
            if (dx, dy) == (0, 0) {
                continue;
            }
            let r = &self.regions[p.region];
            let b = r.bbox;
            for y in (b.y + dy.min(0)).max(y0)..(b.y + b.h + dy.max(0)).min(y1) {
                for x in (b.x + dx.min(0)).max(x0)..(b.x + b.w + dx.max(0)).min(x1) {
                    let inside = r.has(x, y);
                    let (mut sx, mut sy) = (x - dx, y - dy);
                    if !inside && !r.has(sx, sy) {
                        continue;
                    }
                    while !r.has(sx, sy) {
                        sx += dx.signum();
                        sy += dy.signum();
                    }
                    out.px[((y + PAD) * ow + x + PAD) as usize] = static_at(sx, sy);
                }
            }
        }
    }

    pub fn dirty(&self, a: &AnimState, b: &AnimState) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        let max_shift = |s: &AnimState| s.offsets.iter().map(|o| o.0.abs().max(o.1.abs())).max().unwrap_or(0);
        let grow = max_shift(a).max(max_shift(b)) + 1;
        for (i, p) in self.procs.iter().enumerate() {
            if a.offsets.get(i) != b.offsets.get(i) {
                acc = Some(union(acc, expand(self.regions[p.region].bbox, grow)));
            }
        }
        for (i, f) in self.frames.iter().enumerate() {
            if a.frames_on.get(i) != b.frames_on.get(i) {
                let r = Rect::new(f.at.0, f.at.1, f.img.w as i32, f.img.h as i32);
                acc = Some(union(acc, expand(r, grow)));
            }
        }
        acc
    }
}

pub fn render_static(base: &Image, out: &mut Image) {
    out.px.fill(0);
    let ow = out.w;
    for y in 0..base.h {
        let d = (y + PAD as usize) * ow + PAD as usize;
        out.px[d..d + base.w].copy_from_slice(&base.px[y * base.w..(y + 1) * base.w]);
    }
}

pub fn padded(base: &Image) -> Image {
    let mut out = Image { w: base.w + 2 * PAD as usize, h: base.h + 2 * PAD as usize, px: Vec::new() };
    out.px = vec![0; out.w * out.h];
    render_static(base, &mut out);
    out
}


fn mask_bbox(mask: &[bool], w: usize) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (i, _) in mask.iter().enumerate().filter(|(_, m)| **m) {
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (x0 <= x1).then(|| Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

pub fn expand(r: Rect, by: i32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + 2 * by, r.h + 2 * by)
}

pub fn union(a: Option<Rect>, b: Rect) -> Rect {
    match a {
        None => b,
        Some(a) => {
            let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
            let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn sprite(w: usize, h: usize) -> Image {
        let px = (0..w * h).map(|i| 0xFF00_0000 | (i / w) as u32).collect();
        Image { w, h, px }
    }

    fn row_at(out: &Image, x: i32, y: i32) -> u32 {
        out.px[((y + PAD) * out.w as i32 + x + PAD) as usize] & 0xFF
    }

    #[test]
    fn shift_up_moves_region_and_fills_gap_from_adjacent_row() {
        let v = parse(r#"{"regions": {"r": {"rects": [[0, 2, 4, 3]]}},
            "procedural": [{"region": "r", "pattern": "steps", "offsets": [[0, -1]], "step_ms": 100}]}"#).unwrap();
        let base = sprite(4, 6);
        let mut a = SpriteAnim::from_json(&v, 4, 6, Path::new("."), 1).unwrap();
        let st = a.state(0);
        let mut out = padded(&base);
        a.render(&base, &st, &mut out);
        assert_eq!((0..6).map(|y| row_at(&out, 1, y)).collect::<Vec<_>>(), vec![0, 2, 3, 4, 4, 5]);
    }

    #[test]
    fn sine_is_whole_pixels_and_peaks_mid_period() {
        let v = parse(r#"{"regions": {"r": {"rects": [[0, 0, 2, 2]]}},
            "procedural": [{"region": "r", "pattern": "sine", "offset": [0, -1], "period_ms": 4000}]}"#).unwrap();
        let mut a = SpriteAnim::from_json(&v, 2, 2, Path::new("."), 1).unwrap();
        assert_eq!(a.state(0).offsets, vec![(0, 0)]);
        assert_eq!(a.state(2000).offsets, vec![(0, -1)]);
        assert_eq!(a.state(3999).offsets, vec![(0, 0)]);
    }

    #[test]
    fn trigger_plays_once_then_settles_with_cooldown() {
        let v = parse(r#"{"regions": {"ears": {"rects": [[0, 0, 2, 2]]}},
            "triggers": [{"on": "hover", "region": "ears", "offsets": [[0, -1], [0, 0], [0, -1]], "step_ms": 100, "cooldown_ms": 3000},
                         {"on": "click", "effect": "heart"}]}"#).unwrap();
        let mut a = SpriteAnim::from_json(&v, 2, 2, Path::new("."), 1).unwrap();
        assert_eq!(a.state(0).offsets, vec![(0, 0)]);
        assert!(a.fire("hover", 1000).is_empty());
        let seq: Vec<_> = [1000, 1150, 1250, 1300, 2000].iter().map(|&t| a.state(t).offsets[0]).collect();
        assert_eq!(seq, vec![(0, -1), (0, 0), (0, -1), (0, 0), (0, 0)]);
        a.fire("hover", 2500);
        assert_eq!(a.state(2500).offsets[0], (0, 0));
        a.fire("hover", 4000);
        assert_eq!(a.state(4000).offsets[0], (0, -1));
        assert_eq!(a.fire("click", 4100), vec!["heart".to_string()]);
    }

    #[test]
    fn incremental_render_matches_full_render() {
        let v = parse(r#"{"regions": {"a": {"rects": [[1, 1, 4, 3]]}, "b": {"rects": [[2, 5, 3, 2]]}},
            "procedural": [{"region": "a", "pattern": "steps", "offsets": [[0, 0], [0, -1]], "step_ms": 100},
                           {"region": "b", "pattern": "steps", "offsets": [[0, 0], [1, 0], [0, 1]], "step_ms": 70}]}"#).unwrap();
        let base = sprite(7, 8);
        let mut a = SpriteAnim::from_json(&v, 7, 8, Path::new("."), 1).unwrap();
        let mut inc = padded(&base);
        let mut st = a.state(0);
        a.render(&base, &st, &mut inc);
        for t in (10..1000).step_by(10) {
            let next = a.state(t);
            if let Some(r) = a.dirty(&st, &next) {
                a.render_rect(&base, &next, &mut inc, r);
            }
            st = next;
            let mut full = padded(&base);
            a.render(&base, &st, &mut full);
            assert_eq!(inc.px, full.px, "t={t}");
        }
    }

    #[test]
    fn steps_alternate() {
        let v = parse(r#"{"regions": {"r": {"rects": [[0, 0, 1, 1]]}},
            "procedural": [{"region": "r", "pattern": "steps", "offsets": [[0, 0], [0, 1]], "step_ms": 150}]}"#).unwrap();
        let mut a = SpriteAnim::from_json(&v, 1, 1, Path::new("."), 1).unwrap();
        let seq: Vec<_> = [0, 149, 150, 299, 300].iter().map(|&t| a.state(t).offsets[0]).collect();
        assert_eq!(seq, vec![(0, 0), (0, 0), (0, 1), (0, 1), (0, 0)]);
    }
}
