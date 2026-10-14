use std::path::Path;

use crate::ambient::Ambient;
use crate::anim::{self, AnimState, PAD, SpriteAnim};
use crate::daycycle::{Mix, PLATE_NAMES};
use crate::image::{FB_H, FB_W, Image};
use crate::input::SpriteMask;
use crate::json::{self, Value};
use crate::layout::Rect;
use crate::ui::Ui;

pub struct Sprite {
    pub name: &'static str,
    pub img: Image,
    pub x: i32,
    pub y: i32,
    anim: Option<SpriteAnim>,
    work: Image,
    state: AnimState,
}

pub struct Compositor {
    plates: Vec<Image>,
    sprites: Vec<Sprite>,
    bg: Vec<u32>,
    pub frame: Image,
    pub debug_rects: bool,
    pub ui: Ui,
    built: Option<(usize, usize, u32, [u32; 3])>,
    hearts: Vec<Heart>,
    anim_t: u64,
    ambient: Option<Ambient>,
    night_w: f32,
}

#[derive(Clone, Copy)]
struct Heart {
    x: i32,
    y: i32,
    start: u64,
}

const HEART_MS: u64 = 800;
const HEART_RISE: i32 = 12;
const MAX_HEARTS: usize = 6;
const HEART: [&str; 8] = [".##...##.", "#o##.####", "#########", "#########", ".#######.", "..#####..", "...###...", "....#...."];
const HEART_RGB: u32 = 0xFFFF_5C8A;
const HEART_HI: u32 = 0xFFFF_D6E2;

impl Heart {
    fn progress(&self, t: u64) -> f32 {
        (t.saturating_sub(self.start) as f32 / HEART_MS as f32).min(1.0)
    }

    fn pos(&self, t: u64) -> (i32, i32) {
        let p = self.progress(t);
        let rise = (HEART_RISE as f32 * (1.0 - (1.0 - p) * (1.0 - p))).round() as i32;
        (self.x - 4, self.y - 8 - rise)
    }

    fn path_rect(&self) -> Rect {
        Rect::new(self.x - 4, self.y - 8 - HEART_RISE, 9, 8 + HEART_RISE)
    }
}

impl Compositor {
    pub fn load(ready: Option<&Path>, anim_json: Option<&Path>) -> (Compositor, Vec<String>) {
        let mut problems = Vec::new();
        let plates = PLATE_NAMES
            .iter()
            .map(|name| {
                let loaded = ready
                    .ok_or_else(|| "assets/ready not found".to_string())
                    .and_then(|d| Image::load_png(&d.join(format!("room_{name}.png"))))
                    .and_then(|img| match (img.w, img.h) {
                        (FB_W, FB_H) => Ok(img),
                        (w, h) => Err(format!("room_{name}.png is {w}x{h}, expected {FB_W}x{FB_H}")),
                    });
                loaded.unwrap_or_else(|e| {
                    problems.push(e);
                    Image::placeholder()
                })
            })
            .collect();
        let anims = anim_json.map(|p| {
            std::fs::read_to_string(p)
                .map_err(|e| format!("{}: {e}", p.display()))
                .and_then(|t| json::parse(&t).map_err(|e| format!("anim.json: {e}")))
        });
        let anims = match anims {
            Some(Ok(v)) => v,
            Some(Err(e)) => {
                problems.push(e);
                Value::Null
            }
            None => Value::Null,
        };
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        let sprites = ["girl", "cat"]
            .iter()
            .enumerate()
            .filter_map(|(i, &name)| {
                let d = ready?;
                let res = Image::load_png(&d.join(format!("{name}.png"))).and_then(|img| {
                    let meta = std::fs::read_to_string(d.join(format!("{name}.json")))
                        .map_err(|e| format!("{name}.json: {e}"))
                        .and_then(|t| json::parse(&t).map_err(|e| format!("{name}.json: {e}")))?;
                    let x = meta.get("x").and_then(Value::as_i32).ok_or(format!("{name}.json: no x"))?;
                    let y = meta.get("y").and_then(Value::as_i32).ok_or(format!("{name}.json: no y"))?;
                    let anim = match anims.get(name) {
                        Some(v) => match SpriteAnim::from_json(v, img.w, img.h, d, seed.wrapping_add(i as u64 * 0x9E37)) {
                            Ok(a) => Some(a),
                            Err(e) => {
                                problems.push(format!("anim.json {name}: {e}"));
                                None
                            }
                        },
                        None => None,
                    };
                    let work = anim::padded(&img);
                    Ok(Sprite { name, img, x, y, anim, work, state: AnimState::default() })
                });
                res.map_err(|e| problems.push(e)).ok()
            })
            .collect();
        let comp = Compositor {
            plates,
            sprites,
            bg: vec![0; FB_W * FB_H],
            frame: Image { w: FB_W, h: FB_H, px: vec![0; FB_W * FB_H] },
            debug_rects: false,
            ui: Ui::default(),
            built: None,
            hearts: Vec::new(),
            anim_t: 0,
            ambient: anims.get("ambient").map(|v| Ambient::from_json(v, seed ^ 0xA5A5)),
            night_w: 0.0,
        };
        (comp, problems)
    }

    pub fn invalidate(&mut self) {
        self.built = None;
    }

    pub fn fire(&mut self, name: &str, on: &str, t: u64, at: (i32, i32)) {
        let Some(s) = self.sprites.iter_mut().find(|s| s.name == name) else { return };
        let Some(a) = &mut s.anim else { return };
        for effect in a.fire(on, t) {
            if effect == "heart" && self.hearts.len() < MAX_HEARTS {
                self.hearts.push(Heart { x: at.0, y: at.1, start: t });
            }
        }
    }

    pub fn force_frames(&mut self, on: bool) {
        if let Some(amb) = &mut self.ambient {
            amb.force_dim = on;
        }
        for s in &mut self.sprites {
            if let Some(a) = &mut s.anim {
                a.force_frames = on;
            }
        }
    }

    pub fn update(&mut self, mix: &Mix) -> bool {
        let t = (mix.t * 256.0).round() as u32;
        let tint = mix.tint.map(|c| (c * 256.0).round().max(0.0) as u32);
        let key = (mix.a as usize, mix.b as usize, t, tint);
        if self.built == Some(key) {
            return false;
        }
        if self.built.map(|k| (k.0, k.1, k.2)) != Some((key.0, key.1, key.2)) {
            blend(&mut self.bg, &self.plates[key.0].px, &self.plates[key.1].px, t);
            if let Some(amb) = &mut self.ambient {
                amb.prepare(&self.bg, FB_W, FB_H);
            }
        }
        self.built = Some(key);
        let nightish = |p: usize| p == crate::daycycle::Plate::Night as usize || p == crate::daycycle::Plate::Dusk as usize;
        let tf = t as f32 / 256.0;
        self.night_w = if nightish(key.0) { 1.0 - tf } else { 0.0 } + if nightish(key.1) { tf } else { 0.0 };
        self.redraw(Rect::new(0, 0, FB_W as i32, FB_H as i32));
        true
    }

    pub fn tick(&mut self, t_ms: u64) -> Vec<Rect> {
        let mut dirty = Vec::new();
        for s in &mut self.sprites {
            let Some(a) = &mut s.anim else { continue };
            let st = a.state(t_ms);
            if st == s.state {
                continue;
            }
            let changed = if s.state == AnimState::default() {
                Some(Rect::new(-PAD, -PAD, s.work.w as i32, s.work.h as i32))
            } else {
                a.dirty(&s.state, &st)
            };
            if let Some(r) = changed {
                a.render_rect(&s.img, &st, &mut s.work, r);
            }
            s.state = st;
            if let Some(r) = changed.and_then(|r| clip(Rect::new(r.x + s.x, r.y + s.y, r.w, r.h))) {
                dirty.push(r);
            }
        }
        self.anim_t = t_ms;
        for h in &self.hearts {
            dirty.extend(clip(h.path_rect()));
        }
        self.hearts.retain(|h| t_ms < h.start + HEART_MS);
        if let Some(amb) = &mut self.ambient {
            dirty.extend(amb.tick(t_ms, self.night_w).into_iter().filter_map(clip));
        }
        if self.built.is_some() {
            for r in &dirty {
                self.redraw(*r);
            }
        }
        dirty
    }

    pub fn redraw(&mut self, r: Rect) -> bool {
        let Some((_, _, _, tint)) = self.built else { return false };
        let Some(r) = clip(r) else { return false };
        let w = FB_W;
        for y in r.y..r.y + r.h {
            let row = y as usize * w;
            let (a, b) = (row + r.x as usize, row + (r.x + r.w) as usize);
            self.frame.px[a..b].copy_from_slice(&self.bg[a..b]);
        }
        if let Some(amb) = &self.ambient {
            amb.apply(&mut self.frame, r);
        }
        for s in &self.sprites {
            draw_tinted(&mut self.frame, &s.work, s.x - PAD, s.y - PAD, tint, r);
        }
        for h in &self.hearts {
            draw_heart(&mut self.frame, h, self.anim_t, r);
        }
        self.ui.draw(&mut self.frame, r);
        if self.debug_rects {
            for (w, wr) in crate::layout::widgets(self.ui.show_todo, self.ui.show_pomodoro) {
                outline(&mut self.frame, wr, w.debug_color(), r);
            }
        }
        true
    }
}

fn draw_heart(dst: &mut Image, h: &Heart, t: u64, within: Rect) {
    let p = h.progress(t);
    if p >= 1.0 {
        return;
    }
    let alpha = ((1.0 - ((p - 0.33) / 0.67).max(0.0)) * 255.0) as u32;
    let (x0, y0) = h.pos(t);
    for (row, line) in HEART.iter().enumerate() {
        for (col, c) in line.bytes().enumerate() {
            let color = match c {
                b'#' => HEART_RGB,
                b'o' => HEART_HI,
                _ => continue,
            };
            crate::ui::blend_fill(dst, Rect::new(x0 + col as i32, y0 + row as i32, 1, 1), color, alpha, within);
        }
    }
}

fn clip(r: Rect) -> Option<Rect> {
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    let (x1, y1) = ((r.x + r.w).min(FB_W as i32), (r.y + r.h).min(FB_H as i32));
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

impl Compositor {
    pub fn sprite_masks(&self) -> Vec<SpriteMask> {
        self.sprites
            .iter()
            .map(|s| SpriteMask {
                name: s.name,
                rect: Rect::new(s.x, s.y, s.img.w as i32, s.img.h as i32),
                opaque: s.img.px.iter().map(|p| p >> 24 >= 128).collect(),
            })
            .collect()
    }
}

fn outline(dst: &mut Image, r: Rect, color: u32, within: Rect) {
    let w = dst.w as i32;
    let mut put = |x: i32, y: i32| {
        if within.contains(x, y) {
            dst.px[(y * w + x) as usize] = color;
        }
    };
    for x in r.x..r.x + r.w {
        put(x, r.y);
        put(x, r.y + r.h - 1);
    }
    for y in r.y..r.y + r.h {
        put(r.x, y);
        put(r.x + r.w - 1, y);
    }
}

fn blend(out: &mut [u32], a: &[u32], b: &[u32], t: u32) {
    match t {
        0 => out.copy_from_slice(a),
        256.. => out.copy_from_slice(b),
        _ => {
            let s = 256 - t;
            for ((o, &pa), &pb) in out.iter_mut().zip(a).zip(b) {
                let rb = (((pa & 0x00FF_00FF) * s + (pb & 0x00FF_00FF) * t) >> 8) & 0x00FF_00FF;
                let ag = (((pa >> 8) & 0x00FF_00FF) * s + ((pb >> 8) & 0x00FF_00FF) * t) & 0xFF00_FF00;
                *o = rb | ag;
            }
        }
    }
}

fn draw_tinted(dst: &mut Image, img: &Image, x: i32, y: i32, tint: [u32; 3], within: Rect) {
    let [tr, tg, tb] = tint;
    let x0 = x.max(within.x);
    let y0 = y.max(within.y);
    let x1 = (x + img.w as i32).min(within.x + within.w);
    let y1 = (y + img.h as i32).min(within.y + within.h);
    for yy in y0..y1 {
        let srow = (yy - y) as usize * img.w;
        let drow = yy as usize * dst.w;
        for xx in x0..x1 {
            let p = img.px[srow + (xx - x) as usize];
            if p >> 24 < 128 {
                continue;
            }
            let b = ((p & 0xFF) * tb >> 8).min(255);
            let g = ((p >> 8 & 0xFF) * tg >> 8).min(255);
            let r = ((p >> 16 & 0xFF) * tr >> 8).min(255);
            dst.px[drow + xx as usize] = 0xFF00_0000 | r << 16 | g << 8 | b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_endpoints_and_mid() {
        let (a, b) = ([0xFF10_2030u32], [0xFF90_A0B0u32]);
        let mut o = [0u32];
        blend(&mut o, &a, &b, 0);
        assert_eq!(o[0], a[0]);
        blend(&mut o, &a, &b, 256);
        assert_eq!(o[0], b[0]);
        blend(&mut o, &a, &b, 128);
        assert_eq!(o[0], 0xFF50_6070);
    }

    #[test]
    #[ignore]
    fn tick_cost() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let (mut c, problems) = Compositor::load(Some(&root.join("ready")), Some(&root.join("anim.json")));
        assert!(problems.is_empty(), "{problems:?}");
        c.update(&crate::daycycle::mix(&crate::daycycle::DEFAULT_KEYFRAMES, 1320.0));
        let start = std::time::Instant::now();
        let (mut ticks_with_work, mut px) = (0, 0i64);
        for i in 0..600u64 {
            let rects = c.tick(i * 100);
            if !rects.is_empty() {
                ticks_with_work += 1;
            }
            px += rects.iter().map(|r| (r.w * r.h) as i64).sum::<i64>();
        }
        let el = start.elapsed();
        println!(
            "600 ticks: {:.2} ms total, {:.1} us/tick avg; {ticks_with_work} ticks redrew, {} fb px/s repainted on average",
            el.as_secs_f64() * 1e3,
            el.as_secs_f64() * 1e6 / 600.0,
            px / 60
        );
    }
}
