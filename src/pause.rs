use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::Power::RegisterPowerSettingNotification;
use windows::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::UI::Shell::{QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

pub const WTS_SESSION_LOCK: usize = 0x7;
pub const WTS_SESSION_UNLOCK: usize = 0x8;
pub const PBT_POWERSETTINGCHANGE: usize = 0x8013;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PauseState {
    pub locked: bool,
    pub display_off: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub screensaver: bool,
}

impl PauseState {
    pub fn paused(&self) -> bool {
        self.locked || self.display_off || self.fullscreen || self.maximized || self.screensaver
    }

    pub fn needs_poll(&self) -> bool {
        !self.locked && !self.display_off
    }

    pub fn describe(&self) -> String {
        let mut v = Vec::new();
        for (on, name) in [
            (self.locked, "locked"),
            (self.display_off, "display off"),
            (self.fullscreen, "fullscreen app"),
            (self.maximized, "maximized window"),
            (self.screensaver, "screensaver"),
        ] {
            if on {
                v.push(name);
            }
        }
        if v.is_empty() { "none".into() } else { v.join(", ") }
    }
}

pub fn register(hwnd: HWND) -> windows::core::Result<()> {
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        RegisterPowerSettingNotification(
            windows::Win32::Foundation::HANDLE(hwnd.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            REGISTER_NOTIFICATION_FLAGS(0),
        )?;
    }
    Ok(())
}

pub fn display_off_from(lparam: isize) -> Option<bool> {
    #[repr(C)]
    struct PowerBroadcastSetting {
        guid: windows::core::GUID,
        len: u32,
        data: u32,
    }
    let s = unsafe { &*(lparam as *const PowerBroadcastSetting) };
    (s.guid == GUID_CONSOLE_DISPLAY_STATE && s.len >= 1).then_some(s.data & 0xFF == 0)
}

pub fn screensaver_running() -> bool {
    let mut running = BOOL(0);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETSCREENSAVERRUNNING,
            0,
            Some(&mut running as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    running.as_bool()
}

fn is_app_window(h: HWND, own: &[HWND], shell_pid: u32) -> bool {
    unsafe {
        if h.is_invalid() || own.contains(&h) || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == shell_pid && class_name(h) != "CabinetWClass" {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        cloaked == 0
    }
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

#[derive(Clone, Copy)]
pub enum Cause {
    Window(HWND),
    Quns(i32),
}

impl Cause {
    pub fn describe(self) -> String {
        match self {
            Cause::Quns(s) => format!("QUNS state {s}"),
            Cause::Window(h) => {
                let mut buf = [0u16; 80];
                let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
                format!("{} \"{}\"", class_name(h), String::from_utf16_lossy(&buf[..n]))
            }
        }
    }
}

fn shell() -> Option<(HWND, u32)> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut pid = 0u32;
        GetWindowThreadProcessId(progman, Some(&mut pid));
        Some((progman, pid))
    }
}

pub fn fullscreen_foreground(own: &[HWND]) -> Option<Cause> {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                return Some(Cause::Quns(state.0));
            }
        }
        let (_, shell_pid) = shell()?;
        let fg = GetForegroundWindow();
        if !is_app_window(fg, own, shell_pid) || IsZoomed(fg).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(fg, &mut r).ok()?;
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            return None;
        }
        let m = mi.rcMonitor;
        let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        covers.then_some(Cause::Window(fg))
    }
}

pub fn covering_maximized(own: &[HWND]) -> Option<Cause> {
    struct Ctx<'a> {
        own: &'a [HWND],
        shell_pid: u32,
        monitor: HMONITOR,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lp.0 as *mut Ctx);
            if IsZoomed(h).as_bool()
                && is_app_window(h, ctx.own, ctx.shell_pid)
                && MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) == ctx.monitor
            {
                ctx.found = Some(h);
                return false.into();
            }
            true.into()
        }
    }
    unsafe {
        let (progman, shell_pid) = shell()?;
        let monitor = MonitorFromWindow(progman, MONITOR_DEFAULTTOPRIMARY);
        let mut ctx = Ctx { own, shell_pid, monitor, found: None };
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut _ as isize));
        ctx.found.map(Cause::Window)
    }
}
