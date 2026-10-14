#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    Todo = 1,
    Pomodoro = 2,
    Music = 3,
    Icons = 4,
}

impl Widget {
    pub fn from_index(i: usize) -> Option<Widget> {
        [Widget::Todo, Widget::Pomodoro, Widget::Music, Widget::Icons].get(i.wrapping_sub(1)).copied()
    }

    pub fn debug_color(self) -> u32 {
        match self {
            Widget::Todo => 0xFF40_FF40,
            Widget::Pomodoro => 0xFFFF_D040,
            Widget::Music => 0xFF40_E0FF,
            Widget::Icons => 0xFFFF_60FF,
        }
    }
}

const MARGIN: i32 = 16;

pub const ICON_SIZE: i32 = 24;
pub const ICON_STOPWATCH: Rect = Rect::new(960 - MARGIN - ICON_SIZE, MARGIN, ICON_SIZE, ICON_SIZE);
pub const ICON_CHECKLIST: Rect = Rect::new(ICON_STOPWATCH.x - 6 - ICON_SIZE, MARGIN, ICON_SIZE, ICON_SIZE);
pub const ICON_ROW: Rect = Rect::new(ICON_CHECKLIST.x, MARGIN, ICON_STOPWATCH.x + ICON_SIZE - ICON_CHECKLIST.x, ICON_SIZE);

pub const MUSIC_BAR: Rect = Rect::new(240, 540 - MARGIN - 48, 480, 48);

pub fn widgets(show_todo: bool, show_pomodoro: bool) -> Vec<(Widget, Rect)> {
    let mut v = vec![(Widget::Icons, ICON_ROW), (Widget::Music, MUSIC_BAR)];
    if show_todo {
        v.push((Widget::Todo, crate::todo::PANEL));
    }
    if show_pomodoro {
        v.push((Widget::Pomodoro, crate::pomodoro::PANEL));
    }
    v
}
