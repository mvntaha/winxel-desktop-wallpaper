use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_DELETE, VK_END, VK_HOME, VK_LEFT, VK_RIGHT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::image::Image;
use crate::layout::Rect;
use crate::text::{Size, Text};
use crate::ui;

const CLASS: PCWSTR = w!("WinxelPopup");
const FB_W: usize = 300;
const FB_H: usize = 64;
const MAX_CHARS: usize = 200;
const TIMER_CARET: usize = 1;
const CARET_MS: u32 = 530;
const WM_FINISH: u32 = WM_APP + 1;
const CF_UNICODETEXT: u32 = 13;

const BG: u32 = 0xFF14_141E;
const BORDER: u32 = 0xFF5A_5A78;
const FIELD_BG: u32 = 0xFF0A_0A10;
const FIELD_BORDER: u32 = 0xFF3C_3C50;
const DIM: u32 = 0xFFB4_B4C4;
const FIELD: Rect = Rect::new(8, 30, FB_W as i32 - 16, 26);

pub type OnDone = Box<dyn FnOnce(Option<String>)>;

struct State {
    hwnd: HWND,
    title: String,
    buf: Vec<char>,
    caret: usize,
    caret_on: bool,
    scroll_px: i32,
    high_surrogate: Option<u16>,
    fb: Image,
    text: Rc<Text>,
    scale: i32,
    on_done: Option<OnDone>,
}

thread_local! {
    static POPUP: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn register_class(hinst: HINSTANCE) {
    let cursor = unsafe { LoadCursorW(HINSTANCE::default(), IDC_IBEAM).unwrap_or_default() };
    let wc = WNDCLASSW { lpfnWndProc: Some(proc_), hInstance: hinst, lpszClassName: CLASS, hCursor: cursor, hIcon: crate::tray::app_icon(false), ..Default::default() };
    unsafe {
        RegisterClassW(&wc);
    }
}

pub fn open(text: Rc<Text>, title: &str, initial: &str, at: (i32, i32), scale: i32, on_done: OnDone) {
    if let Some(h) = POPUP.with(|p| p.borrow().as_ref().map(|s| s.hwnd)) {
        force_foreground(h);
        return;
    }
    let scale = scale.max(1);
    let (w, h) = (FB_W as i32 * scale, FB_H as i32 * scale);
    unsafe {
        let mut wa = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let x = at.0.clamp(wa.left, (wa.right - w).max(wa.left));
        let y = at.1.clamp(wa.top, (wa.bottom - h).max(wa.top));
        let hinst: HINSTANCE = windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into();
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW, CLASS, w!("Winxel"), WS_POPUP,
            x, y, w, h, HWND::default(), HMENU::default(), hinst, None,
        ) else {
            on_done(None);
            return;
        };
        let buf: Vec<char> = initial.chars().take(MAX_CHARS).collect();
        let caret = buf.len();
        POPUP.with(|p| {
            *p.borrow_mut() = Some(State {
                hwnd,
                title: title.to_string(),
                buf,
                caret,
                caret_on: true,
                scroll_px: 0,
                high_surrogate: None,
                fb: Image { w: FB_W, h: FB_H, px: vec![0; FB_W * FB_H] },
                text,
                scale,
                on_done: Some(on_done),
            })
        });
        render();
        let _ = ShowWindow(hwnd, SW_SHOW);
        force_foreground(hwnd);
        SetTimer(hwnd, TIMER_CARET, CARET_MS, None);
    }
}

fn force_foreground(hwnd: HWND) {
    unsafe {
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            let _ = SetFocus(hwnd);
            return;
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
        if GetForegroundWindow() != hwnd {
            crate::log::line("popup: Windows refused focus (foreground lock); click the popup to type");
        }
    }
}

fn finish(result: Option<String>) {
    let Some(mut st) = POPUP.with(|p| p.borrow_mut().take()) else { return };
    unsafe {
        let _ = KillTimer(st.hwnd, TIMER_CARET);
        let _ = DestroyWindow(st.hwnd);
    }
    if let Some(cb) = st.on_done.take() {
        cb(result);
    }
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    POPUP.with(|p| p.borrow_mut().as_mut().map(f))
}

fn paint(fb: &mut Image, text: &Text, title: &str, buf: &[char], caret: usize, caret_on: bool, scroll_px: i32) -> i32 {
    let all = Rect::new(0, 0, FB_W as i32, FB_H as i32);
    ui::fill(fb, all, BG, all);
    ui::outline(fb, all, BORDER, all);
    text.draw(fb, Size::Body, 10, 6, title, DIM, all);
    ui::fill(fb, FIELD, FIELD_BG, all);
    ui::outline(fb, FIELD, FIELD_BORDER, all);

    let inner = Rect::new(FIELD.x + 4, FIELD.y + 2, FIELD.w - 8, FIELD.h - 4);
    let before: String = buf[..caret].iter().collect();
    let caret_x = text.measure(Size::Body, &before);
    let mut scroll = scroll_px;
    if caret_x - scroll > inner.w - 2 {
        scroll = caret_x - (inner.w - 2);
    }
    if caret_x - scroll < 0 {
        scroll = caret_x;
    }
    let line_h = text.line_height(Size::Body);
    let ty = FIELD.y + (FIELD.h - line_h) / 2;
    let all_text: String = buf.iter().collect();
    text.draw(fb, Size::Body, inner.x - scroll, ty, &all_text, ui::WHITE, inner);
    if caret_on {
        ui::fill(fb, Rect::new(inner.x + caret_x - scroll, ty + 1, 1, line_h - 2), ui::WHITE, inner);
    }
    scroll
}

pub fn preview(text: &Text, title: &str, content: &str) -> Image {
    let mut fb = Image { w: FB_W, h: FB_H, px: vec![0; FB_W * FB_H] };
    let buf: Vec<char> = content.chars().take(MAX_CHARS).collect();
    paint(&mut fb, text, title, &buf, buf.len(), true, 0);
    fb
}

fn render() {
    with_state(|st| {
        st.scroll_px = paint(&mut st.fb, &st.text, &st.title, &st.buf, st.caret, st.caret_on, st.scroll_px);
    });
    if let Some(h) = with_state(|s| s.hwnd) {
        unsafe {
            let _ = InvalidateRect(h, None, false);
        }
    }
}

fn edit(f: impl FnOnce(&mut State)) {
    with_state(|st| {
        f(st);
        st.caret_on = true;
        unsafe {
            SetTimer(st.hwnd, TIMER_CARET, CARET_MS, None);
        }
    });
    render();
}

fn insert(st: &mut State, chars: &[char]) {
    for &c in chars {
        if st.buf.len() >= MAX_CHARS {
            break;
        }
        st.buf.insert(st.caret, c);
        st.caret += 1;
    }
}

fn clipboard_first_line(hwnd: HWND) -> Vec<char> {
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Ok(h) = GetClipboardData(CF_UNICODETEXT) {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 && n < 4096 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                out = s.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).collect();
                let _ = GlobalUnlock(g);
            }
        }
        let _ = CloseClipboard();
        out
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                with_state(|st| {
                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: FB_W as i32,
                            biHeight: -(FB_H as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    SetStretchBltMode(hdc, COLORONCOLOR);
                    StretchDIBits(
                        hdc, 0, 0, FB_W as i32 * st.scale, FB_H as i32 * st.scale, 0, 0, FB_W as i32, FB_H as i32,
                        Some(st.fb.px.as_ptr().cast()), &bmi, DIB_RGB_COLORS, SRCCOPY,
                    );
                });
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER if wp.0 == TIMER_CARET => {
                with_state(|st| st.caret_on = !st.caret_on);
                render();
                LRESULT(0)
            }
            WM_ACTIVATE => {
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    let _ = PostMessageW(hwnd, WM_FINISH, WPARAM(0), LPARAM(0));
                }
                LRESULT(0)
            }
            WM_FINISH => {
                finish(None);
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let vk = wp.0 as u16;
                match vk {
                    v if v == VK_LEFT.0 => edit(|st| st.caret = st.caret.saturating_sub(1)),
                    v if v == VK_RIGHT.0 => edit(|st| st.caret = (st.caret + 1).min(st.buf.len())),
                    v if v == VK_HOME.0 => edit(|st| st.caret = 0),
                    v if v == VK_END.0 => edit(|st| st.caret = st.buf.len()),
                    v if v == VK_DELETE.0 => edit(|st| {
                        if st.caret < st.buf.len() {
                            st.buf.remove(st.caret);
                        }
                    }),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CHAR => {
                let u = wp.0 as u16;
                match u {
                    0x0D => {
                        let text: String = with_state(|st| st.buf.iter().collect()).unwrap_or_default();
                        finish(Some(text));
                    }
                    0x1B => finish(None),
                    0x08 => edit(|st| {
                        if st.caret > 0 {
                            st.caret -= 1;
                            st.buf.remove(st.caret);
                        }
                    }),
                    0x16 => {
                        let chars = clipboard_first_line(hwnd);
                        edit(|st| insert(st, &chars));
                    }
                    0xD800..=0xDBFF => {
                        with_state(|st| st.high_surrogate = Some(u));
                    }
                    0xDC00..=0xDFFF => {
                        let hi = with_state(|st| st.high_surrogate.take()).flatten();
                        if let Some(c) = hi.and_then(|h| char::decode_utf16([h, u]).next()?.ok()) {
                            edit(|st| insert(st, &[c]));
                        }
                    }
                    u if u >= 0x20 && u != 0x7F => {
                        if let Some(c) = char::from_u32(u as u32) {
                            edit(|st| insert(st, &[c]));
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_ACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
