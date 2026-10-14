use crate::image::Image;
use crate::layout::{MUSIC_BAR, Rect};
use crate::media::{Command, NowPlaying};
use crate::text::{Size, Text};
use crate::ui::{self, WHITE};

const BTN: i32 = 24;
const PREV: Rect = Rect::new(MUSIC_BAR.x + 10, MUSIC_BAR.y + 12, BTN, BTN);
const PLAY: Rect = Rect::new(PREV.x + BTN + 6, PREV.y, BTN, BTN);
const NEXT: Rect = Rect::new(PLAY.x + BTN + 6, PREV.y, BTN, BTN);
pub const TEXT_AREA: Rect = Rect::new(NEXT.x + BTN + 14, MUSIC_BAR.y + 4, MUSIC_BAR.x + MUSIC_BAR.w - 12 - (NEXT.x + BTN + 14), MUSIC_BAR.h - 8);
const DIM: u32 = 0xFF8C_8CA0;
const MARQUEE_GAP: i32 = 48;
const MARQUEE_STEP: i32 = 2;
const MARQUEE_HOLD: u32 = 20;

#[derive(Default)]
pub struct Music {
    pub now: NowPlaying,
    offset: i32,
    hold: u32,
}

impl Music {
    pub fn set(&mut self, now: NowPlaying) {
        if now.line() != self.now.line() {
            self.offset = 0;
            self.hold = MARQUEE_HOLD;
        }
        self.now = now;
    }

    fn label(&self) -> String {
        if self.now.has_session { self.now.line() } else { "Nothing playing".into() }
    }

    pub fn tick(&mut self, text: &Text) -> bool {
        let w = text.measure(Size::Body, &self.label());
        if w <= TEXT_AREA.w {
            return false;
        }
        if self.hold > 0 {
            self.hold -= 1;
            return false;
        }
        self.offset += MARQUEE_STEP;
        if self.offset >= w + MARQUEE_GAP {
            self.offset = 0;
            self.hold = MARQUEE_HOLD;
        }
        true
    }

    pub fn advance(&mut self, text: &Text, ticks: u32) {
        self.hold = 0;
        for _ in 0..ticks {
            self.tick(text);
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Command> {
        if !self.now.has_session {
            return None;
        }
        [(PREV, Command::Prev), (PLAY, Command::PlayPause), (NEXT, Command::Next)]
            .into_iter()
            .find(|(r, _)| Rect::new(r.x - 3, r.y - 3, r.w + 6, r.h + 6).contains(x, y))
            .map(|(_, c)| c)
    }

    pub fn draw(&self, text: &Text, dst: &mut Image, within: Rect) {
        let within = ui::intersect(within, MUSIC_BAR);
        if within.w == 0 || within.h == 0 {
            return;
        }
        ui::panel(dst, MUSIC_BAR, within);
        let c = if self.now.has_session { WHITE } else { DIM };
        prev_icon(dst, PREV, c, within);
        if self.now.playing { pause_icon(dst, PLAY, c, within) } else { play_icon(dst, PLAY, c, within) }
        next_icon(dst, NEXT, c, within);

        let label = self.label();
        let area = ui::intersect(within, TEXT_AREA);
        let w = text.measure(Size::Body, &label);
        let y = TEXT_AREA.y + (TEXT_AREA.h - text.line_height(Size::Body)) / 2;
        let color = if self.now.has_session { WHITE } else { DIM };
        text.draw(dst, Size::Body, TEXT_AREA.x - self.offset, y, &label, color, area);
        if w > TEXT_AREA.w {
            text.draw(dst, Size::Body, TEXT_AREA.x - self.offset + w + MARQUEE_GAP, y, &label, color, area);
        }
    }
}

fn triangle_right(dst: &mut Image, x: i32, cy: i32, h: i32, c: u32, within: Rect) {
    for dy in -h / 2..=h / 2 {
        let w = h / 2 - dy.abs() + 1;
        ui::fill(dst, Rect::new(x, cy + dy, w, 1), c, within);
    }
}

fn triangle_left(dst: &mut Image, right: i32, cy: i32, h: i32, c: u32, within: Rect) {
    for dy in -h / 2..=h / 2 {
        let w = h / 2 - dy.abs() + 1;
        ui::fill(dst, Rect::new(right - w, cy + dy, w, 1), c, within);
    }
}

fn play_icon(dst: &mut Image, r: Rect, c: u32, within: Rect) {
    triangle_right(dst, r.x + 8, r.y + 12, 12, c, within);
}

fn pause_icon(dst: &mut Image, r: Rect, c: u32, within: Rect) {
    ui::fill(dst, Rect::new(r.x + 7, r.y + 6, 3, 12), c, within);
    ui::fill(dst, Rect::new(r.x + 14, r.y + 6, 3, 12), c, within);
}

fn prev_icon(dst: &mut Image, r: Rect, c: u32, within: Rect) {
    ui::fill(dst, Rect::new(r.x + 6, r.y + 7, 2, 10), c, within);
    triangle_left(dst, r.x + 17, r.y + 12, 10, c, within);
}

fn next_icon(dst: &mut Image, r: Rect, c: u32, within: Rect) {
    triangle_right(dst, r.x + 7, r.y + 12, 10, c, within);
    ui::fill(dst, Rect::new(r.x + 16, r.y + 7, 2, 10), c, within);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_only_with_a_session() {
        let mut m = Music::default();
        assert!(m.hit(PLAY.x + 5, PLAY.y + 5).is_none());
        m.set(NowPlaying { has_session: true, title: "t".into(), artist: "a".into(), playing: true });
        assert!(matches!(m.hit(PLAY.x + 5, PLAY.y + 5), Some(Command::PlayPause)));
        assert!(matches!(m.hit(NEXT.x + 5, NEXT.y + 5), Some(Command::Next)));
        assert!(matches!(m.hit(PREV.x + 5, PREV.y + 5), Some(Command::Prev)));
        assert!(m.hit(TEXT_AREA.x + 50, TEXT_AREA.y + 5).is_none());
    }

    #[test]
    fn line_format() {
        let np = |t: &str, a: &str| NowPlaying { has_session: true, title: t.into(), artist: a.into(), playing: false };
        assert_eq!(np("Song", "Band").line(), "Song - Band");
        assert_eq!(np("Song", " ").line(), "Song");
        assert_eq!(np("", "").line(), "Unknown track");
    }
}
