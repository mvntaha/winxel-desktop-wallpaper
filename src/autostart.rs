use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, w};

pub const VALUE: &str = "Winxel";

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    None,
    Set(String),
    Removed,
}

pub fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("exe path: {e}"))?;
    Ok(format!("\"{}\"", exe.display()))
}

pub fn apply(enabled: bool) -> Result<Change, String> {
    apply_value(VALUE, enabled, &command()?)
}

pub fn apply_value(name: &str, enabled: bool, cmd: &str) -> Result<Change, String> {
    unsafe {
        let mut key = HKEY::default();
        let r = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        );
        if r != ERROR_SUCCESS {
            return Err(format!("open Run key: {r:?}"));
        }
        let result = (|| {
            let current = read(key, name);
            let hname = HSTRING::from(name);
            match (enabled, current) {
                (true, Some(c)) if c == cmd => Ok(Change::None),
                (true, _) => {
                    let wide: Vec<u16> = cmd.encode_utf16().chain(Some(0)).collect();
                    let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                    let r = RegSetValueExW(key, &hname, 0, REG_SZ, Some(bytes));
                    if r == ERROR_SUCCESS { Ok(Change::Set(cmd.to_string())) } else { Err(format!("set Run value: {r:?}")) }
                }
                (false, None) => Ok(Change::None),
                (false, Some(_)) => {
                    let r = RegDeleteValueW(key, &hname);
                    if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
                        Ok(Change::Removed)
                    } else {
                        Err(format!("delete Run value: {r:?}"))
                    }
                }
            }
        })();
        let _ = RegCloseKey(key);
        result
    }
}

fn read(key: HKEY, name: &str) -> Option<String> {
    unsafe {
        let hname = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut len = 0u32;
        if RegQueryValueExW(key, &hname, None, Some(&mut kind), None, Some(&mut len)) != ERROR_SUCCESS || len == 0 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        if RegQueryValueExW(key, &hname, None, Some(&mut kind), Some(buf.as_mut_ptr() as *mut u8), Some(&mut len)) != ERROR_SUCCESS {
            return None;
        }
        let s = String::from_utf16_lossy(&buf);
        Some(s.trim_end_matches('\0').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_is_idempotent_and_removal_cleans_up() {
        let name = "WinxelTest";
        let cmd = "\"C:\\Some Folder\\winxel.exe\"";
        let _ = apply_value(name, false, cmd);
        assert_eq!(apply_value(name, true, cmd).unwrap(), Change::Set(cmd.into()));
        assert_eq!(apply_value(name, true, cmd).unwrap(), Change::None);
        assert_eq!(apply_value(name, true, "\"D:\\x.exe\"").unwrap(), Change::Set("\"D:\\x.exe\"".into()));
        assert_eq!(apply_value(name, false, cmd).unwrap(), Change::Removed);
        assert_eq!(apply_value(name, false, cmd).unwrap(), Change::None);
    }
}
