use windows::Win32::Foundation::{BOOL, COLORREF, HINSTANCE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::MapWindowPoints;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Classic,
    Raised,
}

pub struct Desktop {
    pub layout: Layout,
    pub progman: HWND,
    pub defview: HWND,
    pub workerw: HWND,
}

pub fn locate() -> Option<Desktop> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, None);

        let ex = GetWindowLongPtrW(progman, GWL_EXSTYLE) as u32;
        if ex & WS_EX_NOREDIRECTIONBITMAP.0 != 0 {
            let defview = FindWindowExW(progman, HWND::default(), w!("SHELLDLL_DefView"), PCWSTR::null()).ok()?;
            let workerw = FindWindowExW(progman, HWND::default(), w!("WorkerW"), PCWSTR::null()).unwrap_or_default();
            return Some(Desktop { layout: Layout::Raised, progman, defview, workerw });
        }

        let mut found = (HWND::default(), HWND::default()); // (defview, workerw)
        let _ = EnumWindows(Some(find_classic), LPARAM(&mut found as *mut _ as isize));
        let (defview, workerw) = found;
        if workerw.is_invalid() {
            return None;
        }
        Some(Desktop { layout: Layout::Classic, progman, defview, workerw })
    }
}

unsafe extern "system" fn find_classic(top: HWND, lp: LPARAM) -> BOOL {
    unsafe {
        if let Ok(defview) = FindWindowExW(top, HWND::default(), w!("SHELLDLL_DefView"), PCWSTR::null()) {
            let out = &mut *(lp.0 as *mut (HWND, HWND));
            out.0 = defview;
            out.1 = FindWindowExW(HWND::default(), top, w!("WorkerW"), PCWSTR::null()).unwrap_or_default();
            return false.into();
        }
        true.into()
    }
}

pub fn create(class: PCWSTR, hinst: HINSTANCE, d: &Desktop, screen: RECT) -> windows::core::Result<HWND> {
    unsafe {
        let parent = match d.layout {
            Layout::Raised => d.progman,
            Layout::Classic => d.workerw,
        };
        let mut pts = [POINT { x: screen.left, y: screen.top }, POINT { x: screen.right, y: screen.bottom }];
        MapWindowPoints(HWND::default(), parent, &mut pts);
        let (x, y) = (pts[0].x, pts[0].y);
        let (cx, cy) = (pts[1].x - x, pts[1].y - y);

        match d.layout {
            Layout::Raised => {
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_NOACTIVATE, class, w!("Winxel"), WS_CHILD | WS_VISIBLE,
                    x, y, cx, cy, d.progman, HMENU::default(), hinst, None,
                )?;
                SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
                SetWindowPos(hwnd, d.defview, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)?;
                ensure_workerw_bottom(d);
                Ok(hwnd)
            }
            Layout::Classic => {
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class, w!("Winxel"), WS_POPUP,
                    screen.left, screen.top, screen.right - screen.left, screen.bottom - screen.top,
                    HWND::default(), HMENU::default(), hinst, None,
                )?;
                let style = (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & !WS_POPUP.0) | WS_CHILD.0;
                SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
                if let Err(e) = SetParent(hwnd, parent) {
                    let _ = DestroyWindow(hwnd);
                    return Err(e);
                }
                SetWindowPos(hwnd, HWND::default(), x, y, cx, cy, SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW)?;
                Ok(hwnd)
            }
        }
    }
}

fn ensure_workerw_bottom(d: &Desktop) {
    unsafe {
        if d.workerw.is_invalid() {
            return;
        }
        let last = GetWindow(d.progman, GW_CHILD).and_then(|first| GetWindow(first, GW_HWNDLAST));
        if last.ok() != Some(d.workerw) {
            let _ = SetWindowPos(d.workerw, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

pub fn restore_wallpaper() {
    unsafe {
        let mut path = [0u16; 260];
        if SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            path.len() as u32,
            Some(path.as_mut_ptr().cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let _ = SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                Some(path.as_mut_ptr().cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
    }
}
