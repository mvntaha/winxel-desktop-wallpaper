#![windows_subsystem = "windows"]

mod ambient;
mod anim;
mod autostart;
mod chime;
mod compositor;
mod config;
mod daycycle;
mod desktop;
mod image;
mod input;
mod json;
mod layout;
mod log;
mod media;
mod music;
mod pause;
mod pomodoro;
mod popup;
mod storage;
mod text;
mod todo;
mod tray;
mod ui;

use std::cell::RefCell;

use compositor::Compositor;
use image::Image;
use pause::PauseState;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const NO_HWND: HWND = HWND(std::ptr::null_mut());

const CONTROL_CLASS: PCWSTR = w!("WinxelControl");
const WALLPAPER_CLASS: PCWSTR = w!("WinxelWallpaper");
const TIMER_ATTACH: usize = 1;
const TIMER_BLEND: usize = 2;
const TIMER_ANIM: usize = 4;
const TIMER_POMODORO_END: usize = 5;
const BLEND_INTERVAL_MS: u32 = 30_000;
const HOTKEY_SCRUB_FWD: i32 = 1;
const HOTKEY_SCRUB_BACK: i32 = 2;
const HOTKEY_ADD_TODO: i32 = 3;
const SCRUB_STEP_MIN: f32 = 15.0;
const TIMER_PAUSE_POLL: usize = 3;
const PAUSE_POLL_MS: u32 = 1_000;
const ATTACH_RETRY_MS: u32 = 500;
const ATTACH_MAX_TRIES: u32 = 40;

struct Clock {
    frozen: Option<f32>,
    offset_min: f32,
}

impl Clock {
    fn minutes(&self) -> f32 {
        let base = self.frozen.unwrap_or_else(|| unsafe {
            let t = GetLocalTime();
            t.wHour as f32 * 60.0 + t.wMinute as f32 + t.wSecond as f32 / 60.0
        });
        (base + self.offset_min).rem_euclid(daycycle::DAY_MINUTES)
    }
}

struct Args {
    time: Option<f32>,
    dump: Option<std::path::PathBuf>,
    debug_rects: bool,
    anim_ms: u64,
    blink: bool,
    data_dir: Option<std::path::PathBuf>,
    popup: Option<String>,
    music: Option<String>,
    fire: Vec<String>,
    no_pause: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { time: None, dump: None, debug_rects: false, anim_ms: 0, blink: false, data_dir: None, popup: None, music: None, fire: Vec::new(), no_pause: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--time" => {
                let v = it.next().ok_or("--time needs HH:MM")?;
                let (h, m) = v.split_once(':').ok_or("--time needs HH:MM")?;
                let (h, m): (u32, u32) = (h.parse().map_err(|_| "bad hour")?, m.parse().map_err(|_| "bad minute")?);
                if h > 23 || m > 59 {
                    return Err(format!("--time {v} out of range"));
                }
                args.time = Some((h * 60 + m) as f32);
            }
            "--dump" => args.dump = Some(it.next().ok_or("--dump needs a path")?.into()),
            "--debug-rects" => args.debug_rects = true,
            "--anim-ms" => args.anim_ms = it.next().and_then(|v| v.parse().ok()).ok_or("--anim-ms needs milliseconds")?,
            "--blink" => args.blink = true,
            "--data-dir" => args.data_dir = Some(it.next().ok_or("--data-dir needs a folder")?.into()),
            "--popup" => args.popup = Some(it.next().ok_or("--popup needs text")?),
            "--music" => args.music = Some(it.next().ok_or("--music needs \"Title|Artist\"")?),
            "--fire" => args.fire.push(it.next().ok_or("--fire needs NAME:EVENT[:X,Y]")?),
            "--no-pause" => args.no_pause = true,
            _ => {
                return Err(format!(
                    "unknown argument {a}\n\nwinxel [--time HH:MM] [--debug-rects] [--data-dir DIR] [--dump out.png [--anim-ms N] [--blink]]"
                ))
            }
        }
    }
    Ok(args)
}

#[allow(deprecated)]
fn log_manifest_check() {
    use windows::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOW};
    let mut v = OSVERSIONINFOW { dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    let ok = unsafe { GetVersionExW(&mut v).is_ok() };
    let active = ok && v.dwMajorVersion >= 10;
    log::line(&format!(
        "manifest: Windows reports {}.{} build {} to this process ({})",
        v.dwMajorVersion,
        v.dwMinorVersion,
        v.dwBuildNumber,
        if active { "compatibility manifest active" } else { "NO compatibility manifest: the wallpaper window will be invisible" }
    ));
}

fn fatal(text: &str) {
    log::line(&format!("fatal: {}", text.replace('\n', " ")));
    message(&format!("Winxel can't start.\n\n{text}"), MB_ICONERROR);
}

fn message(text: &str, icon: MESSAGEBOX_STYLE) {
    let w: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(NO_HWND, PCWSTR(w.as_ptr()), w!("Winxel"), icon);
    }
}

struct App {
    control: HWND,
    wallpaper: HWND,
    comp: Compositor,
    clock: Clock,
    anim_start: std::time::Instant,
    msg_taskbar_created: u32,
    attach_tries: u32,
    pause: PauseState,
    last_row_click: Option<(usize, std::time::Instant)>,
    last_pomodoro_face: String,
    cfg: config::Config,
    no_pause: bool,
    cpu_window: Option<(std::time::Instant, u64)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.borrow_mut().as_mut().map(f))
}

fn screen_rect() -> RECT {
    unsafe { RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) } }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("Winxel crashed: {info}");
        log::line(&msg);
        message(&format!("{msg}\n\nDetails are in %APPDATA%\\Winxel\\winxel.log."), MB_ICONERROR);
    }));
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => return message(&e, MB_ICONERROR),
    };
    let Some(assets) = image::assets_dir() else {
        return fatal("The \"assets\" folder was not found next to winxel.exe.\n\nKeep winxel.exe together with its assets folder (copy the whole Winxel folder).");
    };
    let (mut comp, mut asset_problems) = Compositor::load(Some(&assets.join("ready")), Some(&assets.join("anim.json")));
    comp.debug_rects = args.debug_rects;
    match text::Text::load(&assets.join("fonts").join("Pixelta.ttf")) {
        Ok(t) => comp.ui.text = std::rc::Rc::new(t),
        Err(e) => asset_problems.push(e),
    }
    if !asset_problems.is_empty() {
        return fatal(&format!(
            "Some files Winxel needs are missing or unreadable in\n{}\n\n- {}\n\nRe-copy the whole Winxel folder (winxel.exe with its assets folder).",
            assets.display(),
            asset_problems.join("\n- ")
        ));
    }
    storage::init(args.data_dir.clone());
    let mut problems = Vec::new();
    let (cfg, cfg_problems) = config::load();
    problems.extend(cfg_problems);
    let (todos, todo_err) = todo::Todos::load();
    comp.ui.todo = todos;
    problems.extend(todo_err);
    let saved = pomodoro::load_state();
    comp.ui.pomodoro = pomodoro::Pomodoro::new(cfg.pomodoro.clone());
    if let Some(p) = &saved.pomodoro {
        comp.ui.pomodoro.restore(p);
    }
    if let Some(ended) = comp.ui.pomodoro.finish_if_due(pomodoro::now_ms()) {
        log::line(&format!("pomodoro: {ended:?} ended while Winxel was closed"));
    }
    comp.ui.show_todo = saved.show_todo;
    comp.ui.show_pomodoro = saved.show_pomodoro;
    let clock = Clock { frozen: args.time, offset_min: 0.0 };

    if let Some(path) = args.dump {
        comp.ui.clock = ui::ClockFace::at(clock.minutes(), cfg.clock_24h);
        comp.force_frames(args.blink);
        for f in &args.fire {
            let mut parts = f.splitn(3, ':');
            let (name, on) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let at = parts
                .next()
                .and_then(|xy| xy.split_once(','))
                .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                .unwrap_or((0, 0));
            comp.fire(name, on, 0, at);
        }
        if let Some(m) = &args.music {
            let (title, artist) = m.split_once('|').unwrap_or((m.as_str(), ""));
            let now = media::NowPlaying { has_session: true, title: title.into(), artist: artist.into(), playing: true };
            comp.ui.music.set(now);
            let text = comp.ui.text.clone();
            comp.ui.music.advance(&text, (args.anim_ms / 100) as u32);
        }
        comp.update(&daycycle::mix(&cfg.keyframes, clock.minutes()));
        comp.tick(args.anim_ms);
        if let Err(e) = comp.frame.save_png(&path) {
            message(&e, MB_ICONERROR);
        }
        if let Some(content) = &args.popup {
            let img = popup::preview(&comp.ui.text, "New task - Enter adds, Esc cancels", content);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("dump");
            if let Err(e) = img.save_png(&path.with_file_name(format!("{stem}_popup.png"))) {
                message(&e, MB_ICONERROR);
            }
        }
        return;
    }

    unsafe {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        log::line(&format!("launch: {exe}"));
        let Ok(mutex) = CreateMutexW(None, true, w!("Local\\Winxel.SingleInstance")) else {
            return fatal("Could not create the single-instance lock.");
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            log::line("another Winxel is running: asking it to quit so this one takes over");
            if let Ok(h) = FindWindowW(CONTROL_CLASS, w!("Winxel")) {
                let _ = PostMessageW(h, WM_COMMAND, WPARAM(tray::ID_QUIT), LPARAM(0));
            }
            let r = WaitForSingleObject(mutex, 5000);
            if r != WAIT_OBJECT_0 && r != WAIT_ABANDONED {
                return fatal("Winxel is already running and didn't close when asked.\n\nQuit it from its tray icon, then start Winxel again.");
            }
            log::line("previous Winxel closed: taking over");
        }
        log::init();
        log_manifest_check();
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into();
        let cursor = LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default();
        for (class, proc_) in [
            (CONTROL_CLASS, Some(control_proc as unsafe extern "system" fn(_, _, _, _) -> _)),
            (WALLPAPER_CLASS, Some(wallpaper_proc as unsafe extern "system" fn(_, _, _, _) -> _)),
        ] {
            let wc = WNDCLASSW { lpfnWndProc: proc_, hInstance: hinst, lpszClassName: class, hCursor: cursor, hIcon: tray::app_icon(false), ..Default::default() };
            RegisterClassW(&wc);
        }
        popup::register_class(hinst);

        for p in &problems {
            log::line(&format!("data: {p}"));
        }

        let Ok(control) = CreateWindowExW(
            WS_EX_TOOLWINDOW, CONTROL_CLASS, w!("Winxel"), WS_POPUP,
            0, 0, 0, 0, NO_HWND, HMENU::default(), hinst, None,
        ) else {
            return;
        };

        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                control,
                wallpaper: HWND::default(),
                comp,
                clock,
                anim_start: std::time::Instant::now(),
                msg_taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
                attach_tries: 0,
                pause: PauseState::default(),
                last_row_click: None,
                last_pomodoro_face: String::new(),
                cfg,
                no_pause: args.no_pause,
                cpu_window: None,
            })
        });

        tray::add(control);
        if let Some(masks) = with_app(|a| a.comp.sprite_masks()) {
            input::start(control, masks);
        }
        update_hit_rects();
        refresh_scene();
        start_attach();
        arm_pomodoro_alarm();
        media::start(control);
        if let Some(on) = with_app(|a| a.cfg.autostart) {
            apply_autostart(on);
        }
        SetTimer(control, TIMER_BLEND, BLEND_INTERVAL_MS, None);
        SetTimer(control, TIMER_ANIM, anim_interval_ms(), None);
        SetTimer(control, TIMER_PAUSE_POLL, PAUSE_POLL_MS, None);
        if let Err(e) = pause::register(control) {
            log::line(&format!("lock/display notifications unavailable: {e}"));
        }
        let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);
        poll_pause();
        if let Some(hk) = with_app(|a| a.cfg.hotkeys.clone()) {
            for (id, key, repeat, name) in [
                (HOTKEY_SCRUB_FWD, hk.scrub_forward, true, "scrub_forward"),
                (HOTKEY_SCRUB_BACK, hk.scrub_back, true, "scrub_back"),
                (HOTKEY_ADD_TODO, hk.add_todo, false, "add_todo"),
            ] {
                let mods = HOT_KEY_MODIFIERS(key.mods) | if repeat { HOT_KEY_MODIFIERS(0) } else { MOD_NOREPEAT };
                if RegisterHotKey(control, id, mods, key.vk).is_err() {
                    log::line(&format!("hotkey {name} is taken by another app"));
                }
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn start_attach() {
    with_app(|a| a.attach_tries = 0);
    try_attach();
}

fn try_attach() {
    let Some((control, old)) = with_app(|a| (a.control, a.wallpaper)) else { return };
    unsafe {
        if !old.is_invalid() && IsWindow(old).as_bool() {
            let _ = DestroyWindow(old);
        }
        let ok = desktop::locate().and_then(|d| {
            let screen = screen_rect();
            let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).ok()?.into();
            let hwnd = match desktop::create(WALLPAPER_CLASS, hinst, &d, screen) {
                Ok(h) => h,
                Err(e) => {
                    log::line(&format!("attach failed ({:?} layout): {e}", d.layout));
                    return None;
                }
            };
            let (mut key, mut alpha, mut flags) = (COLORREF(0), 0u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
            let layered = GetLayeredWindowAttributes(hwnd, Some(&mut key), Some(&mut alpha), Some(&mut flags));
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            log::line(&format!(
                "attached: {:?} layout, parent {:?}, rect ({},{})-({},{}), visible {}, layered attrs {}",
                d.layout,
                GetParent(hwnd).unwrap_or_default().0,
                r.left, r.top, r.right, r.bottom,
                IsWindowVisible(hwnd).as_bool(),
                match layered { Ok(()) => format!("ok (alpha {alpha})"), Err(e) => format!("FAILED: {e}") }
            ));
            Some(hwnd)
        });
        match ok {
            Some(hwnd) => {
                with_app(|a| a.wallpaper = hwnd);
                let screen = screen_rect();
                let (scale, dx, dy) = fb_mapping(screen.right - screen.left, screen.bottom - screen.top);
                input::set_mapping(scale, screen.left + dx, screen.top + dy);
                let _ = KillTimer(control, TIMER_ATTACH);
            }
            None => {
                with_app(|a| a.wallpaper = HWND::default());
                let tries = with_app(|a| {
                    a.attach_tries += 1;
                    a.attach_tries
                })
                .unwrap_or(ATTACH_MAX_TRIES);
                if tries < ATTACH_MAX_TRIES {
                    SetTimer(control, TIMER_ATTACH, ATTACH_RETRY_MS, None);
                } else {
                    let _ = KillTimer(control, TIMER_ATTACH);
                    MessageBoxW(NO_HWND, w!("Could not find the desktop window to attach to."), w!("Winxel"), MB_ICONERROR);
                }
            }
        }
    }
}

fn refresh_scene() {
    let Some(dirty) = with_app(|a| {
        if a.pause.paused() {
            return Vec::new();
        }
        let minutes = a.clock.minutes();
        let clock = sync_clock(a, minutes);
        if a.comp.update(&daycycle::mix(&a.cfg.keyframes, minutes)) {
            vec![layout::Rect::new(0, 0, image::FB_W as i32, image::FB_H as i32)]
        } else {
            clock.into_iter().collect()
        }
    }) else {
        return;
    };
    invalidate_fb(&dirty);
}

fn sync_clock(a: &mut App, minutes: f32) -> Option<layout::Rect> {
    let face = ui::ClockFace::at(minutes, a.cfg.clock_24h);
    if face == a.comp.ui.clock {
        return None;
    }
    a.comp.ui.clock = face;
    a.comp.redraw(ui::CLOCK_RECT).then_some(ui::CLOCK_RECT)
}

fn invalidate_fb(rects: &[layout::Rect]) {
    let Some(wallpaper) = with_app(|a| a.wallpaper) else { return };
    if wallpaper.is_invalid() || rects.is_empty() {
        return;
    }
    let screen = screen_rect();
    let (scale, dx, dy) = fb_mapping(screen.right - screen.left, screen.bottom - screen.top);
    for r in rects {
        let rc = RECT { left: dx + r.x * scale, top: dy + r.y * scale, right: dx + (r.x + r.w) * scale, bottom: dy + (r.y + r.h) * scale };
        unsafe {
            let _ = InvalidateRect(wallpaper, Some(&rc), false);
        }
    }
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    timed(PROF_POLL, poll_pause);
}

const PROF_TICK: usize = 0;
const PROF_POLL: usize = 1;
const PROF_PAINT: usize = 2;
thread_local! {
    static PROF: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) };
    static PROF_CALLS: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) };
}

fn timed<R>(slot: usize, f: impl FnOnce() -> R) -> R {
    let t = std::time::Instant::now();
    let r = f();
    PROF.with(|p| {
        let mut v = p.get();
        v[slot] += t.elapsed().as_micros() as u64;
        p.set(v);
    });
    PROF_CALLS.with(|c| {
        let mut v = c.get();
        v[slot] += 1;
        c.set(v);
    });
    r
}

fn poll_pause() {
    let Some((mut st, own, no_pause)) = with_app(|a| (a.pause, [a.control, a.wallpaper], a.no_pause)) else { return };
    if !st.needs_poll() {
        return;
    }
    let old = st;
    let fullscreen = pause::fullscreen_foreground(&own);
    let maximized = pause::covering_maximized(&own);
    st.fullscreen = fullscreen.is_some();
    st.maximized = maximized.is_some();
    st.screensaver = pause::screensaver_running();
    if st == old || no_pause {
        return;
    }
    let cause = fullscreen.or(maximized).map(pause::Cause::describe).unwrap_or_default();
    set_pause(st, &cause);
}

fn set_pause(new: PauseState, cause: &str) {
    let new = if with_app(|a| a.no_pause) == Some(true) { PauseState::default() } else { new };
    let Some((old, control)) = with_app(|a| (std::mem::replace(&mut a.pause, new), a.control)) else { return };
    if old == new {
        return;
    }
    let cause = if cause.is_empty() { String::new() } else { format!(" [{cause}]") };
    unsafe {
        if new.needs_poll() != old.needs_poll() {
            if new.needs_poll() {
                SetTimer(control, TIMER_PAUSE_POLL, PAUSE_POLL_MS, None);
            } else {
                let _ = KillTimer(control, TIMER_PAUSE_POLL);
            }
        }
        match (old.paused(), new.paused()) {
            (false, true) => {
                let _ = KillTimer(control, TIMER_BLEND);
                let _ = KillTimer(control, TIMER_ANIM);
                input::set_enabled(false);
                with_app(|a| a.cpu_window = None);
                log::line(&format!("paused: {}{cause}", new.describe()));
            }
            (true, false) => {
                log::line("resumed");
                input::set_enabled(true);
                with_app(|a| a.comp.invalidate());
                refresh_scene();
                SetTimer(control, TIMER_BLEND, BLEND_INTERVAL_MS, None);
                SetTimer(control, TIMER_ANIM, anim_interval_ms(), None);
            }
            _ => log::line(&format!("pause reasons: {}{cause}", new.describe())),
        }
    }
}

fn anim_tick() {
    let Some(rects) = with_app(|a| {
        if a.pause.paused() {
            return Vec::new();
        }
        let mut rects = a.comp.tick(a.anim_start.elapsed().as_millis() as u64);
        rects.extend(sync_clock(a, a.clock.minutes()));
        rects.extend(sync_pomodoro(a));
        let text = a.comp.ui.text.clone();
        if a.comp.ui.music.tick(&text) && a.comp.redraw(music::TEXT_AREA) {
            rects.push(music::TEXT_AREA);
        }
        cpu_report(a);
        rects
    }) else {
        return;
    };
    invalidate_fb(&rects);
}

fn process_cpu() -> u64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u).is_err() {
            return 0;
        }
    }
    let ft = |f: FILETIME| (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64;
    ft(k) + ft(u)
}

fn cpu_report(a: &mut App) {
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
    let now = std::time::Instant::now();
    match a.cpu_window {
        None => {
            PROF.with(|p| p.set([0; 3]));
            PROF_CALLS.with(|p| p.set([0; 3]));
            a.cpu_window = Some((now, process_cpu()));
        }
        Some((start, cpu0)) if now - start >= WINDOW => {
            let cpu = process_cpu();
            let pct = (cpu - cpu0) as f64 / 1e7 / (now - start).as_secs_f64() * 100.0;
            let [tick, poll, paint] = PROF.with(|p| p.replace([0; 3]));
            let [n_tick, n_poll, n_paint] = PROF_CALLS.with(|p| p.replace([0; 3]));
            log::line(&format!(
                "cpu: {pct:.2}% of one core, average over {:.0} s unpaused (animation on); main thread: tick {} ms ({n_tick}x), pause poll {} ms ({n_poll}x), paint {} ms ({n_paint}x)",
                (now - start).as_secs_f64(),
                tick / 1000,
                poll / 1000,
                paint / 1000
            ));
            a.cpu_window = Some((now, cpu));
        }
        Some(_) => {}
    }
}

fn widget_input(widget: layout::Widget, action: usize, button: usize, wheel: i16, x: i32, y: i32) {
    match widget {
        layout::Widget::Todo => todo_input(action, button, wheel, x, y),
        layout::Widget::Pomodoro if action == 1 && button == 0 => {
            if with_app(|a| a.comp.ui.pomodoro.hit_button(x, y)) == Some(true) {
                with_app(|a| a.comp.ui.pomodoro.press(pomodoro::now_ms()));
                pomodoro_changed();
            }
        }
        layout::Widget::Music if action == 1 && button == 0 => {
            if let Some(cmd) = with_app(|a| a.comp.ui.music.hit(x, y)).flatten() {
                media::send(cmd);
            }
        }
        layout::Widget::Icons if action == 1 && button == 0 => {
            if layout::ICON_CHECKLIST.contains(x, y) {
                toggle_panel(true);
            } else if layout::ICON_STOPWATCH.contains(x, y) {
                toggle_panel(false);
            }
        }
        _ => {}
    }
}

fn apply_autostart(on: bool) {
    match autostart::apply(on) {
        Ok(autostart::Change::None) => {}
        Ok(autostart::Change::Set(cmd)) => log::line(&format!("autostart: on ({cmd})")),
        Ok(autostart::Change::Removed) => log::line("autostart: off"),
        Err(e) => log::line(&format!("autostart: {e}")),
    }
}

fn anim_interval_ms() -> u32 {
    with_app(|a| 1000 / a.cfg.animation_fps.max(1)).unwrap_or(100)
}

fn update_hit_rects() {
    if let Some((t, p)) = with_app(|a| (a.comp.ui.show_todo, a.comp.ui.show_pomodoro)) {
        input::set_widgets(layout::widgets(t, p));
    }
}

fn toggle_panel(todo: bool) {
    let rect = if todo { todo::PANEL } else { pomodoro::PANEL };
    with_app(|a| {
        let ui = &mut a.comp.ui;
        if todo {
            ui.show_todo = !ui.show_todo;
        } else {
            ui.show_pomodoro = !ui.show_pomodoro;
        }
        save_state(a);
        a.comp.redraw(rect);
        a.comp.redraw(layout::ICON_ROW);
    });
    update_hit_rects();
    invalidate_fb(&[rect, layout::ICON_ROW]);
}

fn save_state(a: &App) {
    let ui = &a.comp.ui;
    if let Err(e) = pomodoro::save_state(&ui.pomodoro, ui.show_todo, ui.show_pomodoro) {
        log::line(&format!("save failed: {e}"));
    }
}

fn pomodoro_changed() {
    let shown = with_app(|a| {
        save_state(a);
        a.last_pomodoro_face.clear();
        a.comp.ui.show_pomodoro && a.comp.redraw(pomodoro::PANEL)
    });
    arm_pomodoro_alarm();
    if shown == Some(true) {
        invalidate_fb(&[pomodoro::PANEL]);
    }
}

fn arm_pomodoro_alarm() {
    let Some((control, end)) = with_app(|a| (a.control, a.comp.ui.pomodoro.end_ms())) else { return };
    unsafe {
        match end {
            Some(end) => {
                let ms = (end - pomodoro::now_ms()).clamp(1, u32::MAX as i64 / 2) as u32;
                SetTimer(control, TIMER_POMODORO_END, ms, None);
            }
            None => {
                let _ = KillTimer(control, TIMER_POMODORO_END);
            }
        }
    }
}

fn pomodoro_alarm() {
    let Some((control, ended, next)) = with_app(|a| {
        let p = &mut a.comp.ui.pomodoro;
        let ended = p.finish_if_due(pomodoro::now_ms());
        (a.control, ended, p.phase_label())
    }) else {
        return;
    };
    let Some(ended) = ended else {
        arm_pomodoro_alarm();
        return;
    };
    let (title, body) = match ended {
        pomodoro::Phase::Focus => ("Focus round done", format!("Nice work. Next: {next}. Press Start when ready.")),
        _ => ("Break's over", format!("Next: {next}. Press Start when ready.")),
    };
    tray::notify(control, title, &body);
    chime::play();
    log::line(&format!("pomodoro: {ended:?} ended"));
    unsafe {
        let _ = KillTimer(control, TIMER_POMODORO_END);
    }
    pomodoro_changed();
}

fn sync_pomodoro(a: &mut App) -> Option<layout::Rect> {
    if !a.comp.ui.show_pomodoro {
        return None;
    }
    let p = &a.comp.ui.pomodoro;
    let face = format!("{} {} {}", p.display(pomodoro::now_ms()), p.button_label(), p.phase_label());
    if face == a.last_pomodoro_face {
        return None;
    }
    a.last_pomodoro_face = face;
    a.comp.redraw(pomodoro::PANEL).then_some(pomodoro::PANEL)
}

fn todo_input(action: usize, button: usize, wheel: i16, x: i32, y: i32) {
    if action == 3 {
        let notches = (wheel as i32 / 120).abs().max(1);
        let rows = if wheel > 0 { -notches } else { notches };
        if with_app(|a| a.comp.ui.todo.scroll_by(rows)) == Some(true) {
            todo_changed(false, |_| {});
        }
        return;
    }
    if action != 1 || button != 0 {
        return;
    }
    let Some(hit) = with_app(|a| a.comp.ui.todo.hit(x, y)) else { return };
    match hit {
        todo::Hit::Add => open_todo_popup(None),
        todo::Hit::Toggle(i) => todo_changed(true, |t| t.toggle(i)),
        todo::Hit::Delete(i) => todo_changed(true, |t| t.delete(i)),
        todo::Hit::Row(i) => {
            let double = with_app(|a| {
                let now = std::time::Instant::now();
                let limit = std::time::Duration::from_millis(unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() } as u64);
                let double = matches!(a.last_row_click, Some((r, t)) if r == i && now - t <= limit);
                a.last_row_click = if double { None } else { Some((i, now)) };
                double
            });
            if double == Some(true) {
                open_todo_popup(Some(i));
            }
        }
        todo::Hit::Panel => {}
    }
}

fn todo_changed(persist: bool, f: impl FnOnce(&mut todo::Todos)) {
    with_app(|a| {
        f(&mut a.comp.ui.todo);
        if persist {
            if let Err(e) = a.comp.ui.todo.save() {
                log::line(&format!("save failed: {e}"));
            }
        }
        a.comp.redraw(todo::PANEL);
    });
    invalidate_fb(&[todo::PANEL]);
}

fn open_todo_popup(edit: Option<usize>) {
    let Some((text, initial)) = with_app(|a| {
        let initial = edit.and_then(|i| a.comp.ui.todo.items.get(i)).map(|it| it.text.clone()).unwrap_or_default();
        (a.comp.ui.text.clone(), initial)
    }) else {
        return;
    };
    let screen = screen_rect();
    let (scale, dx, dy) = fb_mapping(screen.right - screen.left, screen.bottom - screen.top);
    let at = (dx + todo::PANEL.x * scale, dy + (todo::PANEL.y + todo::PANEL.h + 6) * scale);
    let title = if edit.is_some() { "Edit task - Enter saves, Esc cancels" } else { "New task - Enter adds, Esc cancels" };
    popup::open(
        text,
        title,
        &initial,
        at,
        scale,
        Box::new(move |result| {
            let Some(s) = result.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else { return };
            todo_changed(true, |t| match edit {
                Some(i) => t.set_text(i, &s),
                None => t.add(&s),
            });
        }),
    );
}

fn scrub(minutes: f32) {
    with_app(|a| a.clock.offset_min = (a.clock.offset_min + minutes).rem_euclid(daycycle::DAY_MINUTES));
    refresh_scene();
}

fn quit() {
    log::line("quit");
    let Some((control, wallpaper)) = with_app(|a| (a.control, a.wallpaper)) else { return };
    unsafe {
        tray::remove(control);
        if !wallpaper.is_invalid() {
            let _ = DestroyWindow(wallpaper);
        }
        desktop::restore_wallpaper();
        PostQuitMessage(0);
    }
}

unsafe extern "system" fn control_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            tray::WM_TRAY => {
                let ev = (lp.0 & 0xFFFF) as u32;
                if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                    let on = with_app(|a| a.cfg.autostart).unwrap_or(false);
                    tray::show_menu(hwnd, on);
                }
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == tray::ID_QUIT => {
                quit();
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == tray::ID_AUTOSTART => {
                let on = !with_app(|a| a.cfg.autostart).unwrap_or(false);
                match config::set_autostart(on) {
                    Ok(()) => {
                        with_app(|a| a.cfg.autostart = on);
                        apply_autostart(on);
                    }
                    Err(e) => log::line(&format!("autostart: {e}")),
                }
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == tray::ID_OPEN_DATA => {
                tray::open_path(&storage::path(""), false);
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == tray::ID_OPEN_LOG => {
                if let Some(p) = log::path() {
                    tray::open_path(&p, true);
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_ATTACH => {
                try_attach();
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_ANIM => {
                timed(PROF_TICK, anim_tick);
                LRESULT(0)
            }
            media::WM_WX_MEDIA => {
                let now = media::current();
                log::line(&format!(
                    "media: {}",
                    if now.has_session { format!("session ({})", if now.playing { "playing" } else { "not playing" }) } else { "no session".into() }
                ));
                let dirty = with_app(|a| {
                    a.comp.ui.music.set(now);
                    !a.pause.paused() && a.comp.redraw(layout::MUSIC_BAR)
                });
                if dirty == Some(true) {
                    invalidate_fb(&[layout::MUSIC_BAR]);
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_POMODORO_END => {
                pomodoro_alarm();
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_BLEND => {
                refresh_scene();
                LRESULT(0)
            }
            input::WM_WX_CLICK => {
                let (action, widget, button) = (wp.0 & 0xF, wp.0 >> 4 & 0xF, wp.0 >> 8 & 0xF);
                let wheel = (wp.0 >> 16) as u16 as i16;
                let (x, y) = (lp.0 as u16 as i16 as i32, (lp.0 >> 16) as u16 as i16 as i32);
                if let Some(w) = layout::Widget::from_index(widget) {
                    widget_input(w, action, button, wheel, x, y);
                }
                LRESULT(0)
            }
            input::WM_WX_HOVER => {
                if let Some(name) = input::sprite_name(wp.0) {
                    with_app(|a| {
                        let t = a.anim_start.elapsed().as_millis() as u64;
                        a.comp.fire(name, "hover", t, (0, 0));
                    });
                }
                LRESULT(0)
            }
            input::WM_WX_SPRITE_CLICK => {
                if let Some(name) = input::sprite_name(wp.0) {
                    let at = (lp.0 as u16 as i16 as i32, (lp.0 >> 16) as u16 as i16 as i32);
                    with_app(|a| {
                        let t = a.anim_start.elapsed().as_millis() as u64;
                        a.comp.fire(name, "click", t, at);
                    });
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_PAUSE_POLL => {
                timed(PROF_POLL, poll_pause);
                LRESULT(0)
            }
            WM_WTSSESSION_CHANGE => {
                if let Some(mut st) = with_app(|a| a.pause) {
                    match wp.0 {
                        pause::WTS_SESSION_LOCK => st.locked = true,
                        pause::WTS_SESSION_UNLOCK => st.locked = false,
                        _ => {}
                    }
                    set_pause(st, "");
                    poll_pause();
                }
                LRESULT(0)
            }
            WM_POWERBROADCAST if wp.0 == pause::PBT_POWERSETTINGCHANGE => {
                if let (Some(off), Some(mut st)) = (pause::display_off_from(lp.0), with_app(|a| a.pause)) {
                    st.display_off = off;
                    set_pause(st, "");
                    poll_pause();
                }
                LRESULT(1)
            }
            WM_HOTKEY => {
                match wp.0 as i32 {
                    HOTKEY_SCRUB_FWD => scrub(SCRUB_STEP_MIN),
                    HOTKEY_SCRUB_BACK => scrub(-SCRUB_STEP_MIN),
                    HOTKEY_ADD_TODO => open_todo_popup(None),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                start_attach();
                LRESULT(0)
            }
            _ if msg != 0 && Some(msg) == with_app(|a| a.msg_taskbar_created) => {
                log::line("explorer restarted: re-attaching");
                input::forget_shell();
                tray::add(hwnd);
                start_attach();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn wallpaper_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                timed(PROF_PAINT, || {
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    with_app(|a| paint(hwnd, hdc, &a.comp.frame, ps.rcPaint));
                    let _ = EndPaint(hwnd, &ps);
                });
                thread_local! { static PAINTED: std::cell::Cell<isize> = const { std::cell::Cell::new(0) }; }
                if PAINTED.with(|p| p.replace(hwnd.0 as isize)) != hwnd.0 as isize {
                    let r = ps_rect_log(hwnd);
                    log::line(&format!("first paint of wallpaper window {:?}: client {r}", hwnd.0));
                }
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn ps_rect_log(hwnd: HWND) -> String {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    format!("{}x{}", rc.right - rc.left, rc.bottom - rc.top)
}

fn fb_mapping(cw: i32, ch: i32) -> (i32, i32, i32) {
    let scale = (cw / image::FB_W as i32).min(ch / image::FB_H as i32).max(1);
    (scale, (cw - image::FB_W as i32 * scale) / 2, (ch - image::FB_H as i32 * scale) / 2)
}

fn paint(hwnd: HWND, hdc: HDC, frame: &Image, dirty: RECT) {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (cw, ch) = (rc.right, rc.bottom);
        let (scale, dx, dy) = fb_mapping(cw, ch);
        let (dw, dh) = (frame.w as i32 * scale, frame.h as i32 * scale);

        if dw < cw || dh < ch {
            let black = HBRUSH(GetStockObject(BLACK_BRUSH).0);
            for bar in [
                RECT { left: 0, top: 0, right: cw, bottom: dy },
                RECT { left: 0, top: dy + dh, right: cw, bottom: ch },
                RECT { left: 0, top: dy, right: dx, bottom: dy + dh },
                RECT { left: dx + dw, top: dy, right: cw, bottom: dy + dh },
            ] {
                FillRect(hdc, &bar, black);
            }
        }

        let fx0 = ((dirty.left - dx).div_euclid(scale)).clamp(0, frame.w as i32);
        let fy0 = ((dirty.top - dy).div_euclid(scale)).clamp(0, frame.h as i32);
        let fx1 = ((dirty.right - dx + scale - 1).div_euclid(scale)).clamp(0, frame.w as i32);
        let fy1 = ((dirty.bottom - dy + scale - 1).div_euclid(scale)).clamp(0, frame.h as i32);
        if fx1 <= fx0 || fy1 <= fy0 {
            return;
        }
        let (fw, fh) = (fx1 - fx0, fy1 - fy0);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: frame.w as i32,
                biHeight: -fh,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let rows = &frame.px[fy0 as usize * frame.w..fy1 as usize * frame.w];
        SetStretchBltMode(hdc, COLORONCOLOR);
        StretchDIBits(
            hdc, dx + fx0 * scale, dy + fy0 * scale, fw * scale, fh * scale, fx0, 0, fw, fh,
            Some(rows.as_ptr().cast()), &bmi, DIB_RGB_COLORS, SRCCOPY,
        );
    }
}
