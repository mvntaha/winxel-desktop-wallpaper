use crate::config;
use crate::image::Image;
use crate::json::{self, Value};
use crate::layout::Rect;
use crate::storage;
use crate::text::{Size, Text};
use crate::ui::{self, WHITE};

pub const PANEL: Rect = Rect::new(960 - 16 - 220, 48, 220, 132);
const BUTTON: Rect = Rect::new(PANEL.x + 60, PANEL.y + 94, 100, 28);
const DIM: u32 = 0xFFB4_B4C4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    fn key(self) -> &'static str {
        match self {
            Phase::Focus => "focus",
            Phase::ShortBreak => "short_break",
            Phase::LongBreak => "long_break",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ready,
    Running { end_ms: i64 },
    Paused { remaining_ms: i64 },
}

pub struct Pomodoro {
    pub cfg: config::Pomodoro,
    pub phase: Phase,
    pub status: Status,
    pub completed: u32,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Pomodoro {
    pub fn new(cfg: config::Pomodoro) -> Pomodoro {
        Pomodoro { cfg, phase: Phase::Focus, status: Status::Ready, completed: 0 }
    }

    fn length_ms(&self, phase: Phase) -> i64 {
        let min = match phase {
            Phase::Focus => self.cfg.focus_min,
            Phase::ShortBreak => self.cfg.short_break_min,
            Phase::LongBreak => self.cfg.long_break_min,
        };
        min as i64 * 60_000
    }

    pub fn remaining_ms(&self, now: i64) -> i64 {
        match self.status {
            Status::Ready => self.length_ms(self.phase),
            Status::Running { end_ms } => (end_ms - now).max(0),
            Status::Paused { remaining_ms } => remaining_ms,
        }
    }

    pub fn display(&self, now: i64) -> String {
        let s = (self.remaining_ms(now) + 999) / 1000;
        format!("{:02}:{:02}", s / 60, s % 60)
    }

    pub fn button_label(&self) -> &'static str {
        match self.status {
            Status::Ready => "Start",
            Status::Running { .. } => "Pause",
            Status::Paused { .. } => "Resume",
        }
    }

    pub fn phase_label(&self) -> String {
        let n = self.cfg.rounds_before_long_break.max(1);
        match self.phase {
            Phase::Focus => format!("Focus  {}/{}", self.completed % n + 1, n),
            Phase::ShortBreak => "Short break".into(),
            Phase::LongBreak => "Long break".into(),
        }
    }

    pub fn press(&mut self, now: i64) {
        self.status = match self.status {
            Status::Ready => Status::Running { end_ms: now + self.length_ms(self.phase) },
            Status::Running { end_ms } => Status::Paused { remaining_ms: (end_ms - now).max(0) },
            Status::Paused { remaining_ms } => Status::Running { end_ms: now + remaining_ms },
        };
    }

    pub fn end_ms(&self) -> Option<i64> {
        match self.status {
            Status::Running { end_ms } => Some(end_ms),
            _ => None,
        }
    }

    pub fn finish_if_due(&mut self, now: i64) -> Option<Phase> {
        let Status::Running { end_ms } = self.status else { return None };
        if now < end_ms {
            return None;
        }
        let ended = self.phase;
        self.phase = match ended {
            Phase::Focus => {
                self.completed += 1;
                if self.completed % self.cfg.rounds_before_long_break.max(1) == 0 { Phase::LongBreak } else { Phase::ShortBreak }
            }
            Phase::LongBreak => {
                self.completed = 0;
                Phase::Focus
            }
            Phase::ShortBreak => Phase::Focus,
        };
        self.status = Status::Ready;
        Some(ended)
    }

    pub fn to_json(&self) -> Value {
        let (status, n) = match self.status {
            Status::Ready => ("ready", 0),
            Status::Running { end_ms } => ("running", end_ms),
            Status::Paused { remaining_ms } => ("paused", remaining_ms),
        };
        Value::Obj(vec![
            ("phase".into(), Value::Str(self.phase.key().into())),
            ("completed".into(), Value::Num(self.completed as f64)),
            ("status".into(), Value::Str(status.into())),
            (if status == "running" { "end_unix_ms" } else { "remaining_ms" }.into(), Value::Num(n as f64)),
        ])
    }

    pub fn restore(&mut self, v: &Value) {
        self.phase = match v.get("phase").and_then(Value::as_str) {
            Some("short_break") => Phase::ShortBreak,
            Some("long_break") => Phase::LongBreak,
            _ => Phase::Focus,
        };
        self.completed = v.get("completed").and_then(Value::as_f64).map_or(0, |n| n.max(0.0) as u32);
        let num = |k: &str| v.get(k).and_then(Value::as_f64).map(|n| n as i64);
        self.status = match v.get("status").and_then(Value::as_str) {
            Some("running") => num("end_unix_ms").map_or(Status::Ready, |end_ms| Status::Running { end_ms }),
            Some("paused") => num("remaining_ms").map_or(Status::Ready, |r| Status::Paused { remaining_ms: r.max(0) }),
            _ => Status::Ready,
        };
    }

    pub fn hit_button(&self, x: i32, y: i32) -> bool {
        BUTTON.contains(x, y)
    }

    pub fn draw(&self, text: &Text, dst: &mut Image, within: Rect, now: i64) {
        let within = ui::intersect(within, PANEL);
        if within.w == 0 || within.h == 0 {
            return;
        }
        ui::panel(dst, PANEL, within);
        let label = self.phase_label();
        let lw = text.measure(Size::Body, &label);
        text.draw(dst, Size::Body, PANEL.x + (PANEL.w - lw) / 2, PANEL.y + 8, &label, DIM, within);
        let t = self.display(now);
        let tw = text.measure(Size::Big, &t);
        text.draw(dst, Size::Big, PANEL.x + (PANEL.w - tw) / 2, PANEL.y + 26, &t, WHITE, within);
        ui::panel_button(dst, BUTTON, within);
        let b = self.button_label();
        let bw = text.measure(Size::Title, b);
        text.draw(dst, Size::Title, BUTTON.x + (BUTTON.w - bw) / 2, BUTTON.y + (BUTTON.h - text.line_height(Size::Title)) / 2 + 1, b, WHITE, within);
    }
}

// state.json

pub struct SavedState {
    pub pomodoro: Option<Value>,
    pub show_todo: bool,
    pub show_pomodoro: bool,
}

pub fn load_state() -> SavedState {
    let v = std::fs::read_to_string(storage::path("state.json")).ok().and_then(|t| json::parse(&t).ok());
    let flag = |k: &str| v.as_ref().and_then(|v| v.get("panels")).and_then(|p| p.get(k)).map_or(true, |b| *b != Value::Bool(false));
    SavedState { pomodoro: v.as_ref().and_then(|v| v.get("pomodoro")).cloned(), show_todo: flag("todo"), show_pomodoro: flag("pomodoro") }
}

pub fn save_state(p: &Pomodoro, show_todo: bool, show_pomodoro: bool) -> Result<(), String> {
    let v = Value::Obj(vec![
        ("version".into(), Value::Num(1.0)),
        ("pomodoro".into(), p.to_json()),
        (
            "panels".into(),
            Value::Obj(vec![("todo".into(), Value::Bool(show_todo)), ("pomodoro".into(), Value::Bool(show_pomodoro))]),
        ),
    ]);
    storage::write_atomic(&storage::path("state.json"), v.to_json().as_bytes()).map_err(|e| format!("state.json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Pomodoro {
        Pomodoro::new(config::Pomodoro { focus_min: 25, short_break_min: 5, long_break_min: 15, rounds_before_long_break: 4 })
    }

    #[test]
    fn start_pause_resume_and_display() {
        let mut t = p();
        assert_eq!((t.display(0), t.button_label()), ("25:00".into(), "Start"));
        t.press(1_000);
        assert_eq!(t.button_label(), "Pause");
        assert_eq!(t.display(1_000 + 61_500), "23:59");
        t.press(1_000 + 60_000);
        assert_eq!((t.display(999_999), t.button_label()), ("24:00".into(), "Resume"));
        t.press(500_000);
        assert_eq!(t.end_ms(), Some(500_000 + 24 * 60_000));
    }

    #[test]
    fn long_break_after_four_focus_rounds() {
        let mut t = p();
        let mut now = 0;
        let mut seen = Vec::new();
        for _ in 0..9 {
            t.press(now);
            now = t.end_ms().unwrap();
            assert_eq!(t.finish_if_due(now - 1), None);
            seen.push(t.finish_if_due(now).unwrap());
        }
        use Phase::*;
        assert_eq!(seen, vec![Focus, ShortBreak, Focus, ShortBreak, Focus, ShortBreak, Focus, LongBreak, Focus]);
        assert_eq!(t.phase, ShortBreak);
        assert_eq!(t.status, Status::Ready);
    }

    #[test]
    fn state_round_trips() {
        let mut t = p();
        t.press(10);
        t.completed = 2;
        let mut u = p();
        u.restore(&json::parse(&t.to_json().to_json()).unwrap());
        assert_eq!((u.phase, u.status, u.completed), (t.phase, t.status, t.completed));
    }
}
