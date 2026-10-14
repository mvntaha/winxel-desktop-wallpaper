use crate::daycycle::{DEFAULT_KEYFRAMES, Keyframe, PLATE_NAMES, Plate};
use crate::storage;

const DEFAULT_TOML: &str = r#"# Winxel settings. Delete this file to get the defaults back.

clock_24h = false
# Start with Windows (HKCU Run key). Also toggled from the tray menu.
autostart = false
# Animation tick rate.
animation_fps = 10

[pomodoro]
focus_min = 25
short_break_min = 5
long_break_min = 15
# A long break comes after this many focus rounds.
rounds_before_long_break = 4

[hotkeys]
# Modifiers: Ctrl, Alt, Shift, Win. Keys: A-Z, 0-9, F1-F12, Left, Right, Up, Down.
add_todo = "Ctrl+Alt+T"
scrub_forward = "Ctrl+Alt+Right"
scrub_back = "Ctrl+Alt+Left"

# Day/night cycle: plates cross-fade linearly between neighbouring keyframes (sorted by
# time; before the first / after the last it blends across midnight). tint is the RGB
# multiply applied to the sprites at that keyframe.
[[keyframes]]
time = "05:00"
plate = "night"
tint = [0.52, 0.54, 0.85]

[[keyframes]]
time = "06:30"
plate = "dawn"
tint = [1.00, 0.97, 1.00]

[[keyframes]]
time = "09:30"
plate = "day"
tint = [1.00, 1.00, 1.00]

[[keyframes]]
time = "16:30"
plate = "day"
tint = [1.00, 1.00, 1.00]

[[keyframes]]
time = "18:00"
plate = "dusk"
tint = [0.99, 0.72, 0.76]

[[keyframes]]
time = "19:30"
plate = "night"
tint = [0.52, 0.54, 0.85]
"#;

#[derive(Clone, Debug, PartialEq)]
pub struct Pomodoro {
    pub focus_min: u32,
    pub short_break_min: u32,
    pub long_break_min: u32,
    pub rounds_before_long_break: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: u32,
    pub vk: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hotkeys {
    pub add_todo: Hotkey,
    pub scrub_forward: Hotkey,
    pub scrub_back: Hotkey,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub clock_24h: bool,
    pub autostart: bool,
    pub animation_fps: u32,
    pub pomodoro: Pomodoro,
    pub hotkeys: Hotkeys,
    pub keyframes: Vec<Keyframe>,
}

impl Default for Config {
    fn default() -> Config {
        parse(DEFAULT_TOML).0
    }
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    let path = storage::path("config.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_TOML.to_string());
    let new = set_key_line(&text, "autostart", if on { "true" } else { "false" });
    storage::write_atomic(&path, new.as_bytes()).map_err(|e| format!("config.toml: {e}"))
}

fn set_key_line(text: &str, key: &str, value: &str) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let top_end = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let is_key = |l: &str| {
        let t = l.trim_start();
        t.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    match lines[..top_end].iter().position(|l| is_key(l)) {
        Some(i) => lines[i] = format!("{key} = {value}"),
        None => lines.insert(top_end, format!("{key} = {value}")),
    }
    let mut out = lines.join(newline);
    out.push_str(newline);
    out
}

pub fn load() -> (Config, Vec<String>) {
    let path = storage::path("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut problems = Vec::new();
            if let Err(e) = storage::write_atomic(&path, DEFAULT_TOML.as_bytes()) {
                problems.push(format!("config.toml: could not create ({e})"));
            }
            (parse(DEFAULT_TOML).0, problems)
        }
        Err(e) => (Config::default(), vec![format!("config.toml: {e}; using defaults")]),
    }
}

pub fn parse(text: &str) -> (Config, Vec<String>) {
    let mut problems = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!("config.toml: {e}; using defaults"));
            return (parse(DEFAULT_TOML).0, problems);
        }
    };
    let defaults: toml::Table = DEFAULT_TOML.parse().expect("default config parses");
    let mut p = |m: String| problems.push(format!("config.toml: {m}"));

    let get = |t: &toml::Table, d: &toml::Table, k: &str| t.get(k).cloned().or_else(|| d.get(k).cloned());
    let boolean = |k: &str, p: &mut dyn FnMut(String)| match get(&table, &defaults, k) {
        Some(toml::Value::Boolean(b)) => b,
        _ => {
            p(format!("{k} must be true/false"));
            defaults[k].as_bool().unwrap_or(false)
        }
    };
    let clock_24h = boolean("clock_24h", &mut p);
    let autostart = boolean("autostart", &mut p);
    let animation_fps = match get(&table, &defaults, "animation_fps").and_then(|v| v.as_integer()) {
        Some(n) if (1..=30).contains(&n) => n as u32,
        _ => {
            p("animation_fps must be 1..30".into());
            10
        }
    };

    let empty = toml::Table::new();
    let sub = |name: &str| table.get(name).and_then(|v| v.as_table()).unwrap_or(&empty).clone();
    let dsub = |name: &str| defaults[name].as_table().cloned().unwrap_or_default();
    let (pt, pd) = (sub("pomodoro"), dsub("pomodoro"));
    let minutes = |k: &str, p: &mut dyn FnMut(String)| match get(&pt, &pd, k).and_then(|v| v.as_integer()) {
        Some(n) if (1..=240).contains(&n) => n as u32,
        _ => {
            p(format!("pomodoro.{k} must be 1..240"));
            pd[k].as_integer().unwrap_or(1) as u32
        }
    };
    let pomodoro = Pomodoro {
        focus_min: minutes("focus_min", &mut p),
        short_break_min: minutes("short_break_min", &mut p),
        long_break_min: minutes("long_break_min", &mut p),
        rounds_before_long_break: minutes("rounds_before_long_break", &mut p),
    };

    let (ht, hd) = (sub("hotkeys"), dsub("hotkeys"));
    let hotkey = |k: &str, p: &mut dyn FnMut(String)| {
        let s = get(&ht, &hd, k).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        parse_hotkey(&s).unwrap_or_else(|| {
            p(format!("hotkeys.{k} = \"{s}\" is not a valid hotkey"));
            parse_hotkey(hd[k].as_str().unwrap_or("")).expect("default hotkey")
        })
    };
    let hotkeys = Hotkeys {
        add_todo: hotkey("add_todo", &mut p),
        scrub_forward: hotkey("scrub_forward", &mut p),
        scrub_back: hotkey("scrub_back", &mut p),
    };

    let keyframes = match table.get("keyframes") {
        None => DEFAULT_KEYFRAMES.to_vec(),
        Some(v) => parse_keyframes(v).unwrap_or_else(|e| {
            p(format!("keyframes: {e}; using defaults"));
            DEFAULT_KEYFRAMES.to_vec()
        }),
    };
    (Config { clock_24h, autostart, animation_fps, pomodoro, hotkeys, keyframes }, problems)
}

fn parse_keyframes(v: &toml::Value) -> Result<Vec<Keyframe>, String> {
    let arr = v.as_array().ok_or("must be [[keyframes]] tables")?;
    let mut out = Vec::new();
    for (i, k) in arr.iter().enumerate() {
        let t = k.as_table().ok_or(format!("#{} is not a table", i + 1))?;
        let time = t.get("time").and_then(|v| v.as_str()).ok_or(format!("#{}: time = \"HH:MM\"", i + 1))?;
        let (h, m) = time.split_once(':').ok_or(format!("#{}: bad time {time}", i + 1))?;
        let (h, m): (u32, u32) = (h.trim().parse().map_err(|_| format!("bad time {time}"))?, m.trim().parse().map_err(|_| format!("bad time {time}"))?);
        if h > 23 || m > 59 {
            return Err(format!("time {time} out of range"));
        }
        let name = t.get("plate").and_then(|v| v.as_str()).ok_or(format!("#{}: plate", i + 1))?;
        let plate = match PLATE_NAMES.iter().position(|p| *p == name) {
            Some(0) => Plate::Night,
            Some(1) => Plate::Dawn,
            Some(2) => Plate::Day,
            Some(3) => Plate::Dusk,
            _ => return Err(format!("unknown plate {name} (night/dawn/day/dusk)")),
        };
        let tint = match t.get("tint").and_then(|v| v.as_array()) {
            None => [1.0; 3],
            Some(a) => {
                let f: Vec<f32> = a.iter().filter_map(|x| x.as_float().or(x.as_integer().map(|i| i as f64))).map(|x| x as f32).collect();
                match f[..] {
                    [r, g, b] if [r, g, b].iter().all(|c| (0.0..=2.0).contains(c)) => [r, g, b],
                    _ => return Err(format!("#{}: tint = [r, g, b], each 0..2", i + 1)),
                }
            }
        };
        out.push(Keyframe { minute: h * 60 + m, plate, tint });
    }
    if out.is_empty() {
        return Err("need at least one keyframe".into());
    }
    out.sort_by_key(|k| k.minute);
    Ok(out)
}

pub fn parse_hotkey(s: &str) -> Option<Hotkey> {
    let (mut mods, mut vk) = (0u32, None);
    for part in s.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= 0x2,
            "alt" => mods |= 0x1,
            "shift" => mods |= 0x4,
            "win" => mods |= 0x8,
            "left" => vk = Some(0x25),
            "up" => vk = Some(0x26),
            "right" => vk = Some(0x27),
            "down" => vk = Some(0x28),
            k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => vk = Some(k.to_ascii_uppercase().as_bytes()[0] as u32),
            k if k.starts_with('f') && k[1..].parse::<u32>().is_ok_and(|n| (1..=12).contains(&n)) => {
                vk = Some(0x6F + k[1..].parse::<u32>().ok()?)
            }
            _ => return None,
        }
    }
    (mods != 0).then_some(Hotkey { mods, vk: vk? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse_cleanly() {
        let (c, problems) = parse(DEFAULT_TOML);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(!c.clock_24h);
        assert_eq!(c.pomodoro, Pomodoro { focus_min: 25, short_break_min: 5, long_break_min: 15, rounds_before_long_break: 4 });
        assert_eq!(c.keyframes.len(), 6);
        assert_eq!(c.hotkeys.add_todo, Hotkey { mods: 0x3, vk: b'T' as u32 });
    }

    #[test]
    fn bad_values_fall_back_and_are_reported() {
        let (c, problems) = parse("clock_24h = true\nanimation_fps = 500\n[pomodoro]\nfocus_min = 50\nshort_break_min = -3\n[hotkeys]\nadd_todo = \"T\"\n");
        assert!(c.clock_24h);
        assert_eq!(c.animation_fps, 10);
        assert_eq!(c.pomodoro.focus_min, 50);
        assert_eq!(c.pomodoro.short_break_min, 5);
        assert_eq!(c.hotkeys.add_todo, Hotkey { mods: 0x3, vk: b'T' as u32 });
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert_eq!(c.keyframes.len(), 6);
    }

    #[test]
    fn autostart_line_is_rewritten_in_place() {
        let t = "# c\nclock_24h = false\n# Start\nautostart = false   \n\n[pomodoro]\nautostart = 7\n";
        let n = set_key_line(t, "autostart", "true");
        assert_eq!(n, "# c\nclock_24h = false\n# Start\nautostart = true\n\n[pomodoro]\nautostart = 7\n");
        assert!(parse(&n).0.autostart);
        let n = set_key_line("a = 1\r\n[x]\r\n", "autostart", "true");
        assert_eq!(n, "a = 1\r\nautostart = true\r\n[x]\r\n");
    }

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("Ctrl+Alt+Right"), Some(Hotkey { mods: 0x3, vk: 0x27 }));
        assert_eq!(parse_hotkey("shift+win+f5"), Some(Hotkey { mods: 0xC, vk: 0x74 }));
        assert_eq!(parse_hotkey("Q"), None);
        assert_eq!(parse_hotkey("Ctrl+Nope"), None);
    }
}
