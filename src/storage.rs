use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// call once at startup; `None` = %APPDATA%\Winxel.
pub fn init(dir: Option<PathBuf>) {
    let dir = dir.or_else(crate::log::data_dir).unwrap_or_else(|| PathBuf::from("."));
    let _ = fs::create_dir_all(&dir);
    let _ = DIR.set(dir);
}

pub fn path(name: &str) -> PathBuf {
    DIR.get().cloned().unwrap_or_else(|| PathBuf::from(".")).join(name)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces() {
        let dir = std::env::temp_dir().join(format!("winxel-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert!(!dir.join("x.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
