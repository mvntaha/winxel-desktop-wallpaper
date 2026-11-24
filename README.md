# winxel desktop wallpaper

> a pixel-art live wallpaper for windows with a real-time day-night cycle, an interactive to-do list, a pomodoro timer, a desk clock, and now-playing music controls, built in rust with a tiny memory footprint.

[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Version](https://img.shields.io/badge/version-1.0.0-blue)]()

## table of contents

* [introduction](#introduction)
* [prerequisites](#prerequisites)
* [installation](#installation)
* [usage](#usage)
* [configuration](#configuration)
* [api documentation](#api-documentation)
* [testing](#testing)
* [contributing](#contributing)
* [license](#license)

## introduction

winxel is a cozy cyberpunk-tokyo study room that lives behind your desktop icons. the scene follows your local time, shifting through night, dawn, day, and dusk, while a girl types at her laptop and a cat sleeps on the desk. it doubles as a small productivity dashboard you can click directly on the wallpaper.

**key features:**

* real-time day-night cycle with smooth cross-fades between four room plates
* interactive to-do list with add, check, edit, delete, scrolling, and saved state
* pomodoro timer (25/5/15) with toast notification and chime at round end
* desk clock in 12-hour or 24-hour format
* now-playing music bar with previous, play/pause, and next controls via windows media sessions
* lively sprite animations: breathing, typing, blinking, hover reactions, and night-time neon flicker
* lightweight by design: cpu-composited 960x540 framebuffer, ~33 mb working set, under 1% cpu
* automatic pause when a fullscreen or maximized app covers the wallpaper, the session is locked, or the display is off

## prerequisites

before running the project, ensure the following are installed:

* windows 11 (single monitor, 1920x1080 tested)
* [rust](https://www.rust-lang.org/tools/install) (stable toolchain)
* [python](https://www.python.org/) (3.9+) with `pillow` and `numpy` for the asset converter
* your own wallpaper art in `assets/raw/` (see installation)
* optional: [pixelorama](https://orama-interactive.itch.io/pixelorama) for editing sprites

## installation

step-by-step instructions to set up the project locally:

```bash
# clone the repository
git clone https://github.com/mvntaha/winxel-desktop-wallpaper.git
cd winxel-desktop-wallpaper

# install converter dependencies
pip install pillow numpy

# add your art to assets/raw/ (not included in the repository)
# room_night / room_dawn / room_day / room_dusk, girl_raw, cat_raw, girl_blink_raw

# convert raw art into 960x540 ready-to-use assets
python tools/convert_assets.py
python tools/make_anim_masks.py

# load the build environment (uses rust's own linker)
. .\tools\env.ps1

# build the release binary (one job at a time keeps memory low)
cargo build --release -j 1
```

## usage

run the wallpaper:

```bash
.\target\release\winxel.exe
```

create a portable build:

```bash
.\tools\package.ps1
.\dist\Winxel\winxel.exe
```

useful flags:

```bash
winxel.exe --time 18:30      # freeze the clock to preview a time of day
winxel.exe --dump out.png    # render one frame to a png and exit
winxel.exe --debug-rects     # outline widget click areas
winxel.exe --no-pause        # keep rendering even when windows cover the wallpaper
```

once running, a tray icon appears. press `Win+D` to see the wallpaper if a maximized window is covering it (winxel pauses while hidden to save resources).

## configuration

all settings live in `%APPDATA%\Winxel\`:

* `config.toml` - settings, created with commented defaults on first run
* `todos.json` - your to-do list
* `state.json` - pomodoro progress and panel visibility
* `winxel.log` - runtime log

### config.toml options

```toml
# clock
clock_24h = false

# startup
autostart = false

# animation
animation_fps = 10

# pomodoro (minutes)
focus_min = 25
```

### day-night keyframes (editable in `config.toml`)

```
05:00 night  ->  06:30 dawn  ->  09:30 day
16:30 day    ->  18:00 dusk  ->  19:30 night
```

## api documentation

winxel has no network api. this section lists its controls instead.

### hotkeys

* `Ctrl+Alt+T` - open the add to-do popup
* `Ctrl+Alt+Right` / `Ctrl+Alt+Left` - scrub time forward/back by 15 minutes (hold to run through a full day)

### widgets

* **to-do list** (top-left) - `+` to add, click checkbox to toggle, `-` to delete, double-click to edit, mouse wheel to scroll
* **pomodoro** (top-right) - start, pause, and resume; manual start for each round, long break after 4 rounds
* **panel toggles** (top-right) - checklist icon and stopwatch icon show or hide each panel
* **music bar** (bottom) - previous, play/pause, and next for the active media session

### tray menu

* `Start with Windows` - toggle autostart (off by default)
* `Open data folder`
* `Open log`
* `Quit`

### hover interactions

* hover the girl - her hood ears twitch
* hover the cat - her tail flicks
* click the cat - a pixel heart floats up

## testing

manual testing procedure:

1. press `Win+D` so the desktop is visible, then run winxel
2. add, check, edit, delete, and scroll to-dos with `+` and `Ctrl+Alt+T`, then restart and confirm they persist
3. set `focus_min = 1` in `config.toml`, start a round, and verify the toast and chime
4. play music in a browser or media app and test the title, previous, play/pause, and next buttons
5. hold `Ctrl+Alt+Right` and confirm every panel stays readable across all four plates
6. open a maximized window, press `F11` in a browser, lock with `Win+L`, and check `winxel.log` for `paused` and `resumed` lines
7. double-click a desktop icon outside the widgets and confirm it opens normally

## contributing

contributions are welcome:

1. fork the repository
2. create a feature branch:

```bash
git checkout -b feature/amazing-feature
```

3. commit your changes:

```bash
git commit -m "add amazing feature"
```

4. push to your branch:

```bash
git push origin feature/amazing-feature
```

5. open a pull request

## license

distributed under the mit license. see `LICENSE` for more information.
created by [mvntaha / mau] with love!♡
