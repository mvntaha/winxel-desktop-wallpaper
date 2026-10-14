use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use crate::image::Image;
use crate::layout::Rect;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Size {
    Body = 0,
    Title = 1,
    Big = 2,
}

const PX: [f32; 3] = [16.0, 20.0, 48.0];
const THRESHOLD: u8 = 110;

struct Glyph {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    advance: f32,
    bits: Vec<bool>,
}

struct Face {
    ascent: i32,
    line_h: i32,
    glyphs: RefCell<HashMap<char, Glyph>>,
}

pub struct Text {
    font: Option<fontdue::Font>,
    faces: [Face; 3],
}

impl Text {
    pub fn load(path: &Path) -> Result<Text, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let faces = PX.map(|px| {
            let lm = font.horizontal_line_metrics(px);
            let ascent = lm.map_or(px * 0.8, |l| l.ascent).round() as i32;
            let line_h = lm.map_or(px * 1.2, |l| l.new_line_size).round() as i32;
            Face { ascent, line_h, glyphs: RefCell::new(HashMap::new()) }
        });
        let text = Text { font: Some(font), faces };
        for size in [Size::Body, Size::Title, Size::Big] {
            for c in (' '..='~').chain('\u{A0}'..='\u{FF}') {
                text.ensure(size, c);
            }
        }
        Ok(text)
    }

    pub fn none() -> Text {
        Text { font: None, faces: [0, 1, 2].map(|_| Face { ascent: 12, line_h: 16, glyphs: RefCell::new(HashMap::new()) }) }
    }

    fn ensure(&self, size: Size, c: char) {
        let face = &self.faces[size as usize];
        if face.glyphs.borrow().contains_key(&c) {
            return;
        }
        let Some(font) = &self.font else { return };
        let px = PX[size as usize];
        let g = if c != ' ' && font.lookup_glyph_index(c) == 0 {
            missing_box(face.ascent, px)
        } else {
            let (m, cov) = font.rasterize(c, px);
            Glyph {
                x: m.xmin,
                y: face.ascent - (m.ymin + m.height as i32),
                w: m.width as i32,
                h: m.height as i32,
                advance: m.advance_width,
                bits: cov.iter().map(|&v| v >= THRESHOLD).collect(),
            }
        };
        face.glyphs.borrow_mut().insert(c, g);
    }

    pub fn line_height(&self, size: Size) -> i32 {
        self.faces[size as usize].line_h
    }

    pub fn ascent(&self, size: Size) -> i32 {
        self.faces[size as usize].ascent
    }

    pub fn measure(&self, size: Size, s: &str) -> i32 {
        let mut pen = 0.0f32;
        for c in s.chars() {
            self.ensure(size, c);
            pen += self.faces[size as usize].glyphs.borrow().get(&c).map_or(0.0, |g| g.advance);
        }
        pen.round() as i32
    }

    pub fn draw(&self, dst: &mut Image, size: Size, x: i32, y: i32, s: &str, color: u32, within: Rect) -> i32 {
        let face = &self.faces[size as usize];
        let mut pen = x as f32;
        for c in s.chars() {
            self.ensure(size, c);
            let glyphs = face.glyphs.borrow();
            let Some(g) = glyphs.get(&c) else { continue };
            let gx = pen.round() as i32 + g.x;
            let gy = y + g.y;
            for row in 0..g.h {
                let py = gy + row;
                if py < within.y || py >= within.y + within.h || py < 0 || py >= dst.h as i32 {
                    continue;
                }
                for col in 0..g.w {
                    let px = gx + col;
                    if g.bits[(row * g.w + col) as usize]
                        && px >= within.x
                        && px < within.x + within.w
                        && px >= 0
                        && px < dst.w as i32
                    {
                        dst.px[py as usize * dst.w + px as usize] = color;
                    }
                }
            }
            pen += g.advance;
        }
        pen.round() as i32
    }

    pub fn ellipsize(&self, size: Size, s: &str, max_w: i32) -> String {
        if self.measure(size, s) <= max_w {
            return s.to_string();
        }
        let dots = self.measure(size, "...");
        let mut out = String::new();
        for c in s.chars() {
            out.push(c);
            if self.measure(size, &out) + dots > max_w {
                out.pop();
                break;
            }
        }
        out.trim_end().to_string() + "..."
    }
}

fn missing_box(ascent: i32, px: f32) -> Glyph {
    let w = (px * 0.45).round().max(4.0) as i32;
    let h = (ascent as f32 * 0.8).round().max(5.0) as i32;
    let bits = (0..w * h).map(|i| {
        let (x, y) = (i % w, i / w);
        x == 0 || y == 0 || x == w - 1 || y == h - 1
    });
    Glyph { x: 1, y: ascent - h, w, h, advance: (w + 3) as f32, bits: bits.collect() }
}
