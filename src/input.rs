use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, AtomicIsize, AtomicU32, Ordering::Relaxed};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::layout::{Rect, Widget};

pub const WM_WX_CLICK: u32 = WM_APP + 10;
pub const WM_WX_HOVER: u32 = WM_APP + 11;
pub const WM_WX_SPRITE_CLICK: u32 = WM_APP + 12;
const WM_HOOK_ENABLE: u32 = WM_APP + 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Down = 1,
    Up = 2,
    Wheel = 3,
}

pub struct SpriteMask {
    pub name: &'static str,
    pub rect: Rect,
    pub opaque: Vec<bool>,
}

impl SpriteMask {
    fn hit(&self, x: i32, y: i32) -> bool {
        self.rect.contains(x, y) && self.opaque[((y - self.rect.y) * self.rect.w + (x - self.rect.x)) as usize]
    }
}

struct Shared {
    control: isize,
    sprites: Vec<SpriteMask>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
static WIDGET_RECTS: std::sync::RwLock<Vec<(Widget, Rect)>> = std::sync::RwLock::new(Vec::new());
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);
static SCALE: AtomicI32 = AtomicI32::new(2);
static OFF_X: AtomicI32 = AtomicI32::new(0);
static OFF_Y: AtomicI32 = AtomicI32::new(0);

pub fn sprite_name(index: usize) -> Option<&'static str> {
    SHARED.get()?.sprites.get(index.checked_sub(1)?).map(|s| s.name)
}

pub fn set_widgets(rects: Vec<(Widget, Rect)>) {
    if let Ok(mut w) = WIDGET_RECTS.write() {
        *w = rects;
    }
}

pub fn set_mapping(scale: i32, off_x: i32, off_y: i32) {
    SCALE.store(scale.max(1), Relaxed);
    OFF_X.store(off_x, Relaxed);
    OFF_Y.store(off_y, Relaxed);
}

pub fn start(control: HWND, sprites: Vec<SpriteMask>) {
    let _ = SHARED.set(Shared { control: control.0 as isize, sprites });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("winxel-mouse-hook".into())
        .spawn(move || hook_thread(tx))
        .expect("spawn hook thread");
    let _ = rx.recv();
}

pub fn set_enabled(on: bool) {
    let tid = HOOK_THREAD.load(Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_HOOK_ENABLE, WPARAM(on as usize), LPARAM(0));
        }
    }
}

fn hook_thread(ready: std::sync::mpsc::Sender<()>) {
    unsafe {
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        HOOK_THREAD.store(GetCurrentThreadId(), Relaxed);
        install(true);
        let _ = ready.send(());
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOOK_ENABLE {
                install(msg.wParam.0 != 0);
            }
        }
        install(false);
    }
}

fn install(on: bool) {
    unsafe {
        let cur = HOOK.load(Relaxed);
        if on && cur == 0 {
            if let Ok(h) = SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse), None, 0) {
                HOOK.store(h.0 as isize, Relaxed);
            }
        } else if !on && cur != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(cur as *mut _));
            HOOK.store(0, Relaxed);
            set_hover(0);
        }
    }
}

thread_local! {
    static STATE: std::cell::Cell<HookState> = const { std::cell::Cell::new(HookState::new()) };
}

#[derive(Clone, Copy)]
struct HookState {
    swallowed: [bool; 4],
    hover: usize,
    last_check: (u32, i32, i32, bool),
}

impl HookState {
    const fn new() -> HookState {
        HookState { swallowed: [false; 4], hover: 0, last_check: (0, i32::MIN, i32::MIN, false) }
    }
}

fn button_of(msg: u32) -> Option<(usize, Action)> {
    Some(match msg {
        WM_LBUTTONDOWN => (0, Action::Down),
        WM_LBUTTONUP => (0, Action::Up),
        WM_RBUTTONDOWN => (1, Action::Down),
        WM_RBUTTONUP => (1, Action::Up),
        WM_MBUTTONDOWN => (2, Action::Down),
        WM_MBUTTONUP => (2, Action::Up),
        WM_XBUTTONDOWN => (3, Action::Down),
        WM_XBUTTONUP => (3, Action::Up),
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => (0, Action::Wheel),
        _ => return None,
    })
}

fn to_fb(pt: POINT) -> (i32, i32) {
    let s = SCALE.load(Relaxed);
    ((pt.x - OFF_X.load(Relaxed)).div_euclid(s), (pt.y - OFF_Y.load(Relaxed)).div_euclid(s))
}

fn widget_at(x: i32, y: i32) -> Option<usize> {
    let rects = WIDGET_RECTS.read().ok()?;
    rects.iter().find(|(_, r)| r.contains(x, y)).map(|(w, _)| *w as usize)
}

unsafe extern "system" fn ll_mouse(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let next = || CallNextHookEx(HHOOK(HOOK.load(Relaxed) as *mut _), code, wp, lp);
        if code < 0 {
            return next();
        }
        let Some(shared) = SHARED.get() else { return next() };
        let info = &*(lp.0 as *const MSLLHOOKSTRUCT);
        let msg = wp.0 as u32;
        let (x, y) = to_fb(info.pt);
        let mut st = STATE.get();
        let control = HWND(shared.control as *mut _);
        let mut consume = false;

        if msg == WM_MOUSEMOVE {
            let over = shared.sprites.iter().position(|s| s.hit(x, y)).map(|i| i + 1).unwrap_or(0);
            let hover = if over != 0 && desktop_cached(&mut st, info.time, info.pt) { over } else { 0 };
            if hover != st.hover {
                st.hover = hover;
                let _ = PostMessageW(control, WM_WX_HOVER, WPARAM(hover), LPARAM(0));
            }
        } else if let Some((btn, action)) = button_of(msg) {
            let wheel_delta = (info.mouseData >> 16) as u16 as i16;
            let post = |widget: usize| {
                let wparam = action as usize | widget << 4 | btn << 8 | (wheel_delta as u16 as usize) << 16;
                let _ = PostMessageW(control, WM_WX_CLICK, WPARAM(wparam), LPARAM(((y as u16 as isize) << 16) | x as u16 as isize));
            };
            match action {
                Action::Up if st.swallowed[btn] => {
                    st.swallowed[btn] = false;
                    post(widget_at(x, y).unwrap_or(0));
                    consume = true;
                }
                Action::Up => {}
                Action::Down | Action::Wheel => {
                    if let Some(widget) = widget_at(x, y) {
                        if is_desktop_at(info.pt) {
                            if action == Action::Down {
                                st.swallowed[btn] = true;
                            }
                            post(widget);
                            consume = true;
                        }
                    } else if action == Action::Down && btn == 0 {
                        if let Some(i) = shared.sprites.iter().position(|s| s.hit(x, y)) {
                            if is_desktop_at(info.pt) {
                                let xy = LPARAM(((y as u16 as isize) << 16) | x as u16 as isize);
                                let _ = PostMessageW(control, WM_WX_SPRITE_CLICK, WPARAM(i + 1), xy);
                            }
                        }
                    }
                }
            }
        }
        STATE.set(st);
        if consume { LRESULT(1) } else { next() }
    }
}

fn set_hover(h: usize) {
    let mut st = STATE.get();
    if st.hover != h {
        st.hover = h;
        if let Some(shared) = SHARED.get() {
            unsafe {
                let _ = PostMessageW(HWND(shared.control as *mut _), WM_WX_HOVER, WPARAM(h), LPARAM(0));
            }
        }
    }
    STATE.set(st);
}

static SHELL_PID: AtomicU32 = AtomicU32::new(0);
static SHELL_PID_AT: AtomicU32 = AtomicU32::new(0);

pub fn forget_shell() {
    SHELL_PID.store(0, Relaxed);
}

fn shell_pid() -> u32 {
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
    let pid = SHELL_PID.load(Relaxed);
    if pid != 0 && now.wrapping_sub(SHELL_PID_AT.load(Relaxed)) < 5000 {
        return pid;
    }
    let mut fresh = 0u32;
    unsafe {
        if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
            GetWindowThreadProcessId(progman, Some(&mut fresh));
        }
    }
    SHELL_PID.store(fresh, Relaxed);
    SHELL_PID_AT.store(now, Relaxed);
    fresh
}

fn desktop_cached(st: &mut HookState, time: u32, pt: POINT) -> bool {
    let (t, x, y, r) = st.last_check;
    if time.wrapping_sub(t) < 100 && (pt.x - x).abs() < 16 && (pt.y - y).abs() < 16 {
        return r;
    }
    let r = is_desktop_at(pt);
    st.last_check = (time, pt.x, pt.y, r);
    r
}

pub fn is_desktop_at(pt: POINT) -> bool {
    unsafe {
        let mut h = WindowFromPoint(pt);
        let shell_pid = shell_pid();
        if shell_pid == 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if h.is_invalid() || pid != shell_pid {
            return false;
        }
        let mut buf = [0u16; 32];
        for _ in 0..8 {
            let n = GetClassNameW(h, &mut buf) as usize;
            if ["SHELLDLL_DefView", "Progman", "WorkerW"].iter().any(|c| eq_ascii(&buf[..n], c)) {
                return true;
            }
            match GetParent(h) {
                Ok(p) if !p.is_invalid() => h = p,
                _ => return false,
            }
        }
        false
    }
}

fn eq_ascii(wide: &[u16], s: &str) -> bool {
    wide.len() == s.len() && wide.iter().zip(s.bytes()).all(|(&a, b)| a == b as u16)
}
