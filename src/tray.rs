use windows::Win32::Foundation::{HINSTANCE, HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

pub const WM_TRAY: u32 = WM_APP + 1;
pub const ID_QUIT: usize = 1;
pub const ID_AUTOSTART: usize = 2;
pub const ID_OPEN_DATA: usize = 3;
pub const ID_OPEN_LOG: usize = 4;

pub fn open_path(path: &std::path::Path, is_file: bool) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    let p = windows::core::HSTRING::from(path.as_os_str());
    unsafe {
        let r = ShellExecuteW(HWND::default(), w!("open"), &p, None, None, SW_SHOWNORMAL);
        if is_file && (r.0 as isize) <= 32 {
            let _ = ShellExecuteW(HWND::default(), w!("open"), w!("notepad.exe"), &p, None, SW_SHOWNORMAL);
        }
    }
}

pub fn app_icon(small: bool) -> HICON {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into();
        let (cx, cy) = if small { (SM_CXSMICON, SM_CYSMICON) } else { (SM_CXICON, SM_CYICON) };
        LoadImageW(hinst, PCWSTR(1 as _), IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_SHARED)
            .map(|h| HICON(h.0))
            .or_else(|_| LoadIconW(HINSTANCE::default(), IDI_APPLICATION))
            .unwrap_or_default()
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = app_icon(true);
    let tip: Vec<u16> = "Winxel".encode_utf16().collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    nid
}

pub fn add(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data(hwnd));
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd));
    }
}

pub fn notify(hwnd: HWND, title: &str, body: &str) {
    let mut nid = data(hwnd);
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND | NIIF_RESPECT_QUIET_TIME;
    let t: Vec<u16> = title.encode_utf16().take(nid.szInfoTitle.len() - 1).collect();
    nid.szInfoTitle[..t.len()].copy_from_slice(&t);
    let b: Vec<u16> = body.encode_utf16().take(nid.szInfo.len() - 1).collect();
    nid.szInfo[..b.len()].copy_from_slice(&b);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn show_menu(hwnd: HWND, autostart_on: bool) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = if autostart_on { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | check, ID_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN_DATA, w!("Open data folder"));
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN_LOG, w!("Open log"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
    }
}
