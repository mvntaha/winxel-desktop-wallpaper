use std::rc::Rc;

use crate::image::Image;
use crate::layout::{ICON_CHECKLIST, ICON_ROW, ICON_STOPWATCH, MUSIC_BAR, Rect};
use crate::music::Music;
use crate::pomodoro::{self, Pomodoro};
use crate::text::Text;
use crate::todo::{self, Todos};

pub const CLOCK_RECT: Rect = Rect::new(187, 401, 90, 32);
const CLOCK_RGB: u32 = 0xFF6E_F0FF;
const CLOCK_SCALE: i32 = 3;

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct ClockFace {
    pub digits: String,
    pub marker: Option<&'static str>,
}

impl ClockFace {
    pub fn at(minutes: f32, h24: bool) -> ClockFace {
        let m = minutes.max(0.0) as u32 % (24 * 60);
        let (h, mm) = (m / 60, m % 60);
        if h24 {
            ClockFace { digits: format!("{h:02}:{mm:02}"), marker: None }
        } else {
            let h12 = if h % 12 == 0 { 12 } else { h % 12 };
            ClockFace { digits: format!("{h12}:{mm:02}"), marker: Some(if h < 12 { "AM" } else { "PM" }) }
        }
    }
}

pub const WHITE: u32 = 0xFFFF_FFFF;

const DIM_ICON: u32 = 0xFF8C_8CA0;

pub struct Ui {
    pub text: Rc<Text>,
    pub clock: ClockFace,
    pub todo: Todos,
    pub show_todo: bool,
    pub pomodoro: Pomodoro,
    pub show_pomodoro: bool,
    pub music: Music,
}

impl Default for Ui {
    fn default() -> Ui {
        Ui {
            text: Rc::new(Text::none()),
            clock: ClockFace::default(),
            todo: Todos::default(),
            show_todo: true,
            pomodoro: Pomodoro::new(crate::config::Config::default().pomodoro),
            show_pomodoro: true,
            music: Music::default(),
        }
    }
}

impl Ui {
    pub fn draw(&self, dst: &mut Image, clip: Rect) {
        if intersects(clip, CLOCK_RECT) {
            draw_clock(dst, &self.clock, clip);
        }
        if self.show_todo && intersects(clip, todo::PANEL) {
            self.todo.draw(&self.text, dst, clip);
        }
        if self.show_pomodoro && intersects(clip, pomodoro::PANEL) {
            self.pomodoro.draw(&self.text, dst, clip, pomodoro::now_ms());
        }
        if intersects(clip, ICON_ROW) {
            draw_icons(dst, self.show_todo, self.show_pomodoro, clip);
        }
        if intersects(clip, MUSIC_BAR) {
            self.music.draw(&self.text, dst, clip);
        }
    }
}

fn draw_icons(dst: &mut Image, todo_on: bool, pomo_on: bool, clip: Rect) {
    let within = intersect(clip, ICON_ROW);
    for (r, on) in [(ICON_CHECKLIST, todo_on), (ICON_STOPWATCH, pomo_on)] {
        panel(dst, r, within);
        if on {
            button(dst, r, within);
        }
    }
    let c = if todo_on { WHITE } else { DIM_ICON };
    let (x0, y0) = (ICON_CHECKLIST.x + 5, ICON_CHECKLIST.y + 5);
    for row in 0..3 {
        let y = y0 + row * 5;
        outline(dst, Rect::new(x0, y, 4, 4), c, within);
        fill(dst, Rect::new(x0 + 6, y + 1, 8, 2), c, within);
    }
    fill(dst, Rect::new(x0 + 1, y0 + 1, 2, 2), c, within);
    let c = if pomo_on { WHITE } else { DIM_ICON };
    let (cx, cy, rad) = (ICON_STOPWATCH.x + 12, ICON_STOPWATCH.y + 13, 7.0f32);
    for y in -8..=8 {
        for x in -8..=8 {
            let d = ((x * x + y * y) as f32).sqrt();
            if d >= rad - 1.4 && d <= rad + 0.4 {
                fill(dst, Rect::new(cx + x, cy + y, 1, 1), c, within);
            }
        }
    }
    fill(dst, Rect::new(cx - 2, ICON_STOPWATCH.y + 3, 4, 2), c, within);
    fill(dst, Rect::new(cx - 1, cy - 4, 2, 5), c, within);
    fill(dst, Rect::new(cx + 1, cy - 1, 2, 2), c, within);
}

// drawing primitives

pub fn panel(dst: &mut Image, r: Rect, within: Rect) {
    const ALPHA: u32 = 150;
    for y in r.y..r.y + r.h {
        let inset = match (y - r.y).min(r.y + r.h - 1 - y) {
            0 => 2,
            1 => 1,
            _ => 0,
        };
        blend_fill(dst, Rect::new(r.x + inset, y, r.w - 2 * inset, 1), 0xFF0A_0A12, ALPHA, within);
    }
}

pub fn button(dst: &mut Image, r: Rect, within: Rect) {
    blend_fill(dst, Rect::new(r.x + 1, r.y, r.w - 2, r.h), WHITE, 28, within);
    blend_fill(dst, Rect::new(r.x, r.y + 1, 1, r.h - 2), WHITE, 28, within);
    blend_fill(dst, Rect::new(r.x + r.w - 1, r.y + 1, 1, r.h - 2), WHITE, 28, within);
}

pub fn panel_button(dst: &mut Image, r: Rect, within: Rect) {
    blend_fill(dst, Rect::new(r.x + 1, r.y + 1, r.w - 2, r.h - 2), WHITE, 40, within);
    for edge in [
        Rect::new(r.x + 1, r.y, r.w - 2, 1),
        Rect::new(r.x + 1, r.y + r.h - 1, r.w - 2, 1),
        Rect::new(r.x, r.y + 1, 1, r.h - 2),
        Rect::new(r.x + r.w - 1, r.y + 1, 1, r.h - 2),
    ] {
        blend_fill(dst, edge, WHITE, 90, within);
    }
}

pub fn blend_fill(dst: &mut Image, r: Rect, color: u32, alpha: u32, within: Rect) {
    let c = intersect(intersect(r, within), Rect::new(0, 0, dst.w as i32, dst.h as i32));
    let (a, ia) = (alpha.min(255), 255 - alpha.min(255));
    let (cr, cg, cb) = (color >> 16 & 0xFF, color >> 8 & 0xFF, color & 0xFF);
    for y in c.y..c.y + c.h {
        let row = y as usize * dst.w;
        for p in &mut dst.px[row + c.x as usize..row + (c.x + c.w) as usize] {
            let (r0, g0, b0) = (*p >> 16 & 0xFF, *p >> 8 & 0xFF, *p & 0xFF);
            let r1 = (r0 * ia + cr * a) / 255;
            let g1 = (g0 * ia + cg * a) / 255;
            let b1 = (b0 * ia + cb * a) / 255;
            *p = 0xFF00_0000 | r1 << 16 | g1 << 8 | b1;
        }
    }
}

pub fn outline(dst: &mut Image, r: Rect, color: u32, within: Rect) {
    fill(dst, Rect::new(r.x, r.y, r.w, 1), color, within);
    fill(dst, Rect::new(r.x, r.y + r.h - 1, r.w, 1), color, within);
    fill(dst, Rect::new(r.x, r.y, 1, r.h), color, within);
    fill(dst, Rect::new(r.x + r.w - 1, r.y, 1, r.h), color, within);
}

pub fn plus(dst: &mut Image, r: Rect, color: u32, within: Rect) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    fill(dst, Rect::new(cx - 5, cy - 1, 10, 2), color, within);
    fill(dst, Rect::new(cx - 1, cy - 5, 2, 10), color, within);
}

pub fn minus(dst: &mut Image, r: Rect, color: u32, within: Rect) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    fill(dst, Rect::new(cx - 4, cy - 1, 8, 2), color, within);
}

pub fn check(dst: &mut Image, r: Rect, color: u32, within: Rect) {
    let (x0, y0) = (r.x + 3, r.y + r.h / 2);
    for i in 0..3 {
        fill(dst, Rect::new(x0 + i, y0 + i, 2, 2), color, within);
    }
    for i in 0..6 {
        fill(dst, Rect::new(x0 + 3 + i, y0 + 2 - i, 2, 2), color, within);
    }
}

pub fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

pub fn intersect(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
}

fn draw_clock(dst: &mut Image, face: &ClockFace, clip: Rect) {
    let s = CLOCK_SCALE;
    let gap = 2;
    let glyph_w = |c: char| if c == ':' { s } else { 5 * s };
    let digits_w: i32 = face.digits.chars().map(glyph_w).sum::<i32>() + gap * (face.digits.chars().count() as i32 - 1);
    let marker_w = face.marker.map_or(0, |m| m.len() as i32 * 6 - 1 + 3);
    let marker = face.marker.filter(|_| digits_w + marker_w <= CLOCK_RECT.w);
    let total = digits_w + if marker.is_some() { marker_w } else { 0 };
    let mut x = CLOCK_RECT.x + (CLOCK_RECT.w - total) / 2;
    let y = CLOCK_RECT.y + (CLOCK_RECT.h - 7 * s) / 2;
    let within = intersect(clip, CLOCK_RECT);
    for c in face.digits.chars() {
        if c == ':' {
            for dy in [2 * s, 4 * s] {
                fill(dst, Rect::new(x, y + dy, s, s), CLOCK_RGB, within);
            }
        } else {
            blit_glyph(dst, c, x, y, s, CLOCK_RGB, within);
        }
        x += glyph_w(c) + gap;
    }
    if let Some(m) = marker {
        let mut mx = x - gap + 3;
        let my = y + 7 * s - 7;
        for c in m.chars() {
            blit_glyph(dst, c, mx, my, 1, CLOCK_RGB, within);
            mx += 6;
        }
    }
}

fn glyph5x7(c: char) -> [u8; 7] {
    match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        _ => [0; 7],
    }
}

fn blit_glyph(dst: &mut Image, c: char, x: i32, y: i32, s: i32, color: u32, within: Rect) {
    for (row, bits) in glyph5x7(c).iter().enumerate() {
        for col in 0..5 {
            if bits & (0x10 >> col) != 0 {
                fill(dst, Rect::new(x + col * s, y + row as i32 * s, s, s), color, within);
            }
        }
    }
}

pub fn fill(dst: &mut Image, r: Rect, color: u32, within: Rect) {
    let x0 = r.x.max(within.x).max(0);
    let y0 = r.y.max(within.y).max(0);
    let x1 = (r.x + r.w).min(within.x + within.w).min(dst.w as i32);
    let y1 = (r.y + r.h).min(within.y + within.h).min(dst.h as i32);
    for y in y0..y1 {
        let row = y as usize * dst.w;
        dst.px[row + x0 as usize..row + x1.max(x0) as usize].fill(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_faces() {
        assert_eq!(ClockFace::at(0.0, false), ClockFace { digits: "12:00".into(), marker: Some("AM") });
        assert_eq!(ClockFace::at(13.0 * 60.0 + 5.9, false), ClockFace { digits: "1:05".into(), marker: Some("PM") });
        assert_eq!(ClockFace::at(12.0 * 60.0, false).marker, Some("PM"));
        assert_eq!(ClockFace::at(23.0 * 60.0 + 59.0, true), ClockFace { digits: "23:59".into(), marker: None });
    }
}
