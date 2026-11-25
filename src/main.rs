#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Remindo: quick timers from a hotkey. Type "in 15 minutes turn off the stove" and a small window pops up when it is time.
//!
//! One executable, several short processes:
//! - the **daemon** (`--background`, started with the system) owns the timers, the hotkey and the tray icon and draws nothing;
//! - the **quick input** (`--popup`) opens on the hotkey, parses what you type and hands the timer to the daemon;
//! - the **finished timers** window (`--alerts`) stacks a card for every timer that fired;
//! - **Settings** (the default when you open Remindo, or `--settings`) lists the active timers, the history and the options.
//!
//! They talk over a tiny named pipe (Windows) or Unix socket (Linux): one line in, one line out.

mod app;
mod bus;
mod config;
mod daemon;
mod fmt;
mod hotkey;
#[allow(dead_code)]
mod i18n;
mod ipc;
#[allow(dead_code)]
mod nimvoro;
mod parse;
mod preview;
#[allow(dead_code)]
mod selfupdate;
mod sound;
mod store;
mod strings;
mod support;
#[allow(dead_code)]
mod theme;
mod tray_common;
#[allow(dead_code)]
mod update_ui;
#[cfg(windows)]
mod platform;
#[cfg(not(windows))]
#[path = "platform_linux.rs"]
mod platform;
#[cfg(windows)]
mod tray;
#[cfg(not(windows))]
#[path = "tray_linux.rs"]
mod tray;
#[cfg(windows)]
mod nativepopup;
#[cfg(windows)]
mod winloop;
#[cfg(unix)]
mod unixloop;
#[cfg(unix)]
mod x11grab;

/// The repository, kept in one place (About page and the update check).
pub const REPO_URL: &str = "https://github.com/GrigoryyKach/remindo";
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() {
    selfupdate::startup("remindo");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = config::data_dir();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    // `--ipc <request>`: asks the running daemon and prints its answer (used by the tests in tools/).
    if let Some(i) = args.iter().position(|a| a == "--ipc") {
        match ipc::request("daemon", &args[i + 1..].join(" ")) {
            Some(r) => println!("{r}"),
            None => println!("not running"),
        }
        return;
    }
    if has("--background") || has("--minimized") {
        daemon::run(dir, !has("--popup-egui"));
        return;
    }
    let result = if has("--new") {
        new_timer()
    } else if has("--popup") {
        app::popup::run(dir)
    } else if has("--alerts") {
        app::alerts::run(dir)
    } else {
        // Opening Remindo itself: make sure the daemon runs, then show Settings.
        if !has("--demo") && !ipc::is_running("daemon") {
            platform::spawn(&["--background"]);
        }
        app::settings::run(dir)
    };
    if let Err(e) = result {
        eprintln!("remindo: {e}");
    }
}

/// `remindo --new`: asks the running daemon to open the quick input (this is what a Wayland shortcut runs).
/// Starts the daemon first when it is not running.
fn new_timer() -> eframe::Result<()> {
    if !ipc::is_running("daemon") {
        platform::spawn(&["--background"]);
        for _ in 0..40 {
            if ipc::is_running("daemon") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    platform::allow_foreground();
    ipc::request("daemon", "new");
    Ok(())
}
