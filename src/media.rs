use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_WX_MEDIA: u32 = WM_APP + 30;

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct NowPlaying {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

impl NowPlaying {
    pub fn line(&self) -> String {
        match (self.title.trim(), self.artist.trim()) {
            ("", "") => "Unknown track".into(),
            (t, "") => t.into(),
            ("", a) => a.into(),
            (t, a) => format!("{t} - {a}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Prev,
    PlayPause,
    Next,
}

enum Msg {
    Refresh,
    SessionChanged,
    Cmd(Command),
}

static CURRENT: Mutex<Option<NowPlaying>> = Mutex::new(None);
static TX: Mutex<Option<Sender<Msg>>> = Mutex::new(None);

pub fn current() -> NowPlaying {
    CURRENT.lock().ok().and_then(|c| c.clone()).unwrap_or_default()
}

pub fn send(cmd: Command) {
    if let Some(tx) = TX.lock().ok().and_then(|t| t.clone()) {
        let _ = tx.send(Msg::Cmd(cmd));
    }
}

pub fn start(control: HWND) {
    let (tx, rx) = channel();
    if let Ok(mut t) = TX.lock() {
        *t = Some(tx.clone());
    }
    let control = control.0 as isize;
    let spawned = std::thread::Builder::new().name("winxel-media".into()).spawn(move || {
        match run(control, tx, rx) {
            Err(e) => crate::log::line(&format!("media: unavailable ({e})")),
            Ok(()) => crate::log::line("media: thread stopped"),
        }
    });
    if let Err(e) = spawned {
        crate::log::line(&format!("media: could not start thread ({e})"));
    }
}

fn run(control: isize, tx: Sender<Msg>, rx: Receiver<Msg>) -> windows::core::Result<()> {
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }
    let manager = Manager::RequestAsync()?.get()?;
    crate::log::line("media: session manager ready");
    let t = tx.clone();
    manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Msg::SessionChanged);
        Ok(())
    }))?;

    let mut session: Option<(Session, EventRegistrationToken, EventRegistrationToken)> = None;
    let mut msg = Msg::SessionChanged;
    loop {
        match msg {
            Msg::SessionChanged => {
                if let Some((s, a, b)) = session.take() {
                    let _ = s.RemoveMediaPropertiesChanged(a);
                    let _ = s.RemovePlaybackInfoChanged(b);
                }
                if let Ok(s) = manager.GetCurrentSession() {
                    let (t1, t2) = (tx.clone(), tx.clone());
                    let a = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                        let _ = t1.send(Msg::Refresh);
                        Ok(())
                    }));
                    let b = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                        let _ = t2.send(Msg::Refresh);
                        Ok(())
                    }));
                    if let (Ok(a), Ok(b)) = (a, b) {
                        session = Some((s, a, b));
                    }
                }
                publish(control, read(session.as_ref().map(|s| &s.0)));
            }
            Msg::Refresh => publish(control, read(session.as_ref().map(|s| &s.0))),
            Msg::Cmd(c) => {
                if let Some((s, _, _)) = &session {
                    let r = match c {
                        Command::Prev => s.TrySkipPreviousAsync(),
                        Command::PlayPause => s.TryTogglePlayPauseAsync(),
                        Command::Next => s.TrySkipNextAsync(),
                    };
                    let _ = r.and_then(|op| op.get());
                }
            }
        }
        match rx.recv() {
            Ok(m) => msg = m,
            Err(_) => return Ok(()),
        }
    }
}

fn read(session: Option<&Session>) -> NowPlaying {
    let Some(s) = session else { return NowPlaying::default() };
    let mut np = NowPlaying { has_session: true, ..Default::default() };
    if let Ok(props) = s.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
        np.title = props.Title().map(|h| h.to_string()).unwrap_or_default();
        np.artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
    }
    np.playing = s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).is_ok_and(|st| st == Status::Playing);
    np
}

fn publish(control: isize, np: NowPlaying) {
    let changed = match CURRENT.lock() {
        Ok(mut c) if c.as_ref() != Some(&np) => {
            *c = Some(np);
            true
        }
        _ => false,
    };
    if changed {
        unsafe {
            let _ = PostMessageW(HWND(control as *mut _), WM_WX_MEDIA, WPARAM(0), LPARAM(0));
        }
    }
}
