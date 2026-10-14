use std::path::PathBuf;

use crate::image::Image;
use crate::json::{self, Value};
use crate::layout::Rect;
use crate::storage;
use crate::text::{Size, Text};
use crate::ui::{self, WHITE};

pub const VISIBLE_ROWS: usize = 8;
pub const MAX_LEN: usize = 200;

pub const PANEL: Rect = Rect::new(16, 16, 280, 234);
const HEADER_H: i32 = 34;
const ROW_H: i32 = 24;
const ROWS_TOP: i32 = PANEL.y + HEADER_H + 4;
const ADD_BTN: Rect = Rect::new(PANEL.x + PANEL.w - 32, PANEL.y + 7, 22, 22);

const DIM: u32 = 0xFFB4_B4C4;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Item {
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Add,
    Toggle(usize),
    Delete(usize),
    Row(usize),
    Panel,
}

#[derive(Default)]
pub struct Todos {
    pub items: Vec<Item>,
    pub scroll: usize,
    path: Option<PathBuf>,
}

impl Todos {
    pub fn load() -> (Todos, Option<String>) {
        let path = storage::path("todos.json");
        let mut t = Todos { items: Vec::new(), scroll: 0, path: Some(path.clone()) };
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (t, None),
            Err(e) => return (t, Some(format!("todos.json: {e}"))),
        };
        match json::parse(&text) {
            Ok(v) => {
                t.items = v
                    .get("items")
                    .map(Value::as_arr)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|i| {
                        let text = i.get("text")?.as_str()?.to_string();
                        Some(Item { text, done: i.get("done") == Some(&Value::Bool(true)) })
                    })
                    .collect();
                (t, None)
            }
            Err(e) => {
                let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                (t, Some(format!("todos.json was unreadable ({e}); kept as todos.json.bad")))
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let items = self
            .items
            .iter()
            .map(|i| Value::Obj(vec![("text".into(), Value::Str(i.text.clone())), ("done".into(), Value::Bool(i.done))]))
            .collect();
        let v = Value::Obj(vec![("version".into(), Value::Num(1.0)), ("items".into(), Value::Arr(items))]);
        storage::write_atomic(path, v.to_json().as_bytes()).map_err(|e| format!("todos.json: {e}"))
    }

    pub fn add(&mut self, text: &str) {
        self.items.push(Item { text: clean(text), done: false });
        self.scroll = self.items.len().saturating_sub(VISIBLE_ROWS);
    }

    pub fn set_text(&mut self, i: usize, text: &str) {
        if let Some(it) = self.items.get_mut(i) {
            it.text = clean(text);
        }
    }

    pub fn toggle(&mut self, i: usize) {
        if let Some(it) = self.items.get_mut(i) {
            it.done = !it.done;
        }
    }

    pub fn delete(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
            self.clamp_scroll();
        }
    }

    pub fn scroll_by(&mut self, rows: i32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll as i32 + rows).max(0) as usize;
        self.clamp_scroll();
        self.scroll != before
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.items.len().saturating_sub(VISIBLE_ROWS));
    }

    pub fn hit(&self, x: i32, y: i32) -> Hit {
        if grow(ADD_BTN, 3).contains(x, y) {
            return Hit::Add;
        }
        for slot in 0..VISIBLE_ROWS {
            let idx = self.scroll + slot;
            if idx >= self.items.len() {
                break;
            }
            let row = row_rect(slot);
            if !row.contains(x, y) {
                continue;
            }
            if grow(checkbox_rect(row), 4).contains(x, y) {
                return Hit::Toggle(idx);
            }
            if grow(minus_rect(row), 4).contains(x, y) {
                return Hit::Delete(idx);
            }
            return Hit::Row(idx);
        }
        Hit::Panel
    }

    pub fn draw(&self, text: &Text, dst: &mut Image, within: Rect) {
        let within = ui::intersect(within, PANEL);
        if within.w == 0 || within.h == 0 {
            return;
        }
        ui::panel(dst, PANEL, within);
        text.draw(dst, Size::Title, PANEL.x + 12, PANEL.y + (HEADER_H - text.line_height(Size::Title)) / 2 + 2, "To-do List", WHITE, within);
        ui::button(dst, ADD_BTN, within);
        ui::plus(dst, ADD_BTN, WHITE, within);
        ui::blend_fill(dst, Rect::new(PANEL.x + 8, PANEL.y + HEADER_H, PANEL.w - 16, 1), WHITE, 50, within);

        if self.items.is_empty() {
            let r = row_rect(0);
            text.draw(dst, Size::Body, r.x + 4, r.y + (ROW_H - text.line_height(Size::Body)) / 2, "Nothing yet. Click + to add.", DIM, within);
        }
        for slot in 0..VISIBLE_ROWS {
            let Some(item) = self.items.get(self.scroll + slot) else { break };
            let row = row_rect(slot);
            let cb = checkbox_rect(row);
            ui::outline(dst, cb, WHITE, within);
            if item.done {
                ui::check(dst, cb, WHITE, within);
            }
            let tx = cb.x + cb.w + 8;
            let max_w = minus_rect(row).x - 6 - tx;
            let shown = text.ellipsize(Size::Body, &item.text, max_w);
            let ty = row.y + (ROW_H - text.line_height(Size::Body)) / 2;
            let color = if item.done { DIM } else { WHITE };
            let end = text.draw(dst, Size::Body, tx, ty, &shown, color, within);
            if item.done {
                let sy = ty + text.ascent(Size::Body) * 6 / 10;
                ui::fill(dst, Rect::new(tx - 1, sy, end - tx + 2, 1), color, within);
            }
            ui::minus(dst, minus_rect(row), DIM, within);
        }
        if self.items.len() > VISIBLE_ROWS {
            let track = Rect::new(PANEL.x + PANEL.w - 6, ROWS_TOP, 2, ROW_H * VISIBLE_ROWS as i32);
            ui::blend_fill(dst, track, WHITE, 40, within);
            let n = self.items.len() as i32;
            let th = (track.h * VISIBLE_ROWS as i32 / n).max(8);
            let ty = track.y + (track.h - th) * self.scroll as i32 / (n - VISIBLE_ROWS as i32).max(1);
            ui::fill(dst, Rect::new(track.x, ty, track.w, th), DIM, within);
        }
    }
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().chars().take(MAX_LEN).collect()
}

fn row_rect(slot: usize) -> Rect {
    Rect::new(PANEL.x + 8, ROWS_TOP + slot as i32 * ROW_H, PANEL.w - 18, ROW_H)
}

fn checkbox_rect(row: Rect) -> Rect {
    Rect::new(row.x + 4, row.y + 5, 14, 14)
}

fn minus_rect(row: Rect) -> Rect {
    Rect::new(row.x + row.w - 18, row.y + 4, 16, 16)
}

fn grow(r: Rect, by: i32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + 2 * by, r.h + 2 * by)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize) -> Todos {
        let mut t = Todos::default();
        for i in 0..n {
            t.add(&format!("task {i}"));
        }
        t
    }

    #[test]
    fn scroll_and_delete_stay_in_range() {
        let mut t = list(12);
        assert_eq!(t.scroll, 4);
        assert!(t.scroll_by(-10));
        assert_eq!(t.scroll, 0);
        assert!(!t.scroll_by(-1));
        t.scroll_by(100);
        assert_eq!(t.scroll, 4);
        for _ in 0..6 {
            t.delete(0);
        }
        assert_eq!(t.scroll, 0);
    }

    #[test]
    fn hits_map_to_items_after_scrolling() {
        let mut t = list(10);
        t.scroll = 2;
        let row = row_rect(1);
        let cb = checkbox_rect(row);
        assert_eq!(t.hit(cb.x + 2, cb.y + 2), Hit::Toggle(3));
        let m = minus_rect(row);
        assert_eq!(t.hit(m.x + 2, m.y + 2), Hit::Delete(3));
        assert_eq!(t.hit(row.x + 100, row.y + 10), Hit::Row(3));
        assert_eq!(t.hit(ADD_BTN.x + 5, ADD_BTN.y + 5), Hit::Add);
    }

    #[test]
    fn text_is_cleaned() {
        let mut t = Todos::default();
        t.add("  buy\tmilk \n");
        assert_eq!(t.items[0].text, "buy milk");
    }
}
