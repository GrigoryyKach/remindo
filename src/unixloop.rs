//! The daemon's main loop on Linux: the X11 shortcut (when there is one), the tray, and waiting for Exit.
//!
//! On Wayland a program cannot grab keys. The XDG GlobalShortcuts portal could, but it is not wired in yet:
//! Settings tells the user to bind a system shortcut to `remindo --new`, which opens the quick input over IPC.

use crate::hotkey::Hotkey;
use std::sync::{Condvar, Mutex};

static QUIT: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland") || (std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none())
}

/// Runs until Exit. `hotkey` is grabbed first (`None`: not at all, in a test run); `setup` creates the tray and
/// learns why the shortcut could not be taken, if so.
pub fn run(hotkey: Option<Hotkey>, setup: impl FnOnce(Result<(), String>)) {
    let result = match hotkey {
        // The shortcut cannot be grabbed on Wayland; XWayland would only see keys sent to X11 windows.
        Some(_) if is_wayland() => Err("wayland".to_string()),
        Some(h) => crate::x11grab::start(Some(h), crate::daemon::open_popup_async),
        None => Ok(()),
    };
    setup(result);
    let (lock, cv) = &QUIT;
    let mut quit = lock.lock().unwrap_or_else(|e| e.into_inner());
    while !*quit {
        quit = cv.wait(quit).unwrap_or_else(|e| e.into_inner());
    }
    crate::x11grab::stop();
}

pub fn post_quit() {
    let (lock, cv) = &QUIT;
    if let Ok(mut q) = lock.lock() {
        *q = true;
        cv.notify_all();
    }
}

pub fn rebind(h: &Hotkey) -> Result<(), String> {
    if is_wayland() {
        return Err("wayland".into());
    }
    crate::x11grab::set(h)
}
