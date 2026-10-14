use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use windows::Win32::System::SystemInformation::GetLocalTime;

const MAX_BYTES: u64 = 512 * 1024;

pub fn data_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("Winxel");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub fn path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("winxel.log"))
}

pub fn init() {
    if let Some(p) = path() {
        if fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
            let _ = fs::remove_file(&p);
        }
    }
    line("---- winxel started");
}

pub fn line(msg: &str) {
    let Some(p) = path() else { return };
    let t = unsafe { GetLocalTime() };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {msg}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
        );
    }
}
