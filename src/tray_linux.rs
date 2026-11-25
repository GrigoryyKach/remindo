//! Notification-area icon (StatusNotifier / AppIndicator): New timer, Active timers, Settings, Support the author, Exit.
//!
//! `tray-icon` needs a GTK main loop, so the icon lives on its own GTK thread and the daemon talks to it through
//! a channel. The AppIndicator protocol has no click events: the icon only opens its menu. GNOME needs the
//! "AppIndicator and KStatusNotifierItem Support" extension; without a tray host the daemon still runs, only
//! without an icon (the shortcut and `remindo --new` keep working).

use crate::tray_common::{labels, perform, Action, TrayState};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

enum Msg {
    State(TrayState),
    Remove,
}

static TX: Mutex<Option<Sender<Msg>>> = Mutex::new(None);

/// Is a StatusNotifier host running? Without one the icon would be invisible.
fn tray_host_present() -> bool {
    let out = Command::new("gdbus")
        .args(["call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.NameHasOwner", "org.kde.StatusNotifierWatcher"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).contains("true"),
        // gdbus is missing or the bus is unreachable: assume the best.
        _ => true,
    }
}

/// Creates the icon on its own GTK thread. `false` if there is no tray to show it in.
pub fn create(state: &TrayState, icon_png: &[u8]) -> bool {
    let (tx, rx) = channel::<Msg>();
    let (ready_tx, ready_rx) = channel::<bool>();
    let png = icon_png.to_vec();
    let state = state.clone();
    if std::thread::Builder::new().name("remindo-tray".into()).spawn(move || run(state, png, rx, ready_tx)).is_err() {
        return false;
    }
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(true) => {
            if let Ok(mut g) = TX.lock() {
                *g = Some(tx);
            }
            true
        }
        _ => false,
    }
}

/// Hands a new state to the tray (any thread).
pub fn update(state: TrayState) {
    if let Ok(g) = TX.lock() {
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(Msg::State(state));
        }
    }
}

pub fn remove() {
    if let Ok(g) = TX.lock() {
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(Msg::Remove);
        }
    }
}

fn run(state: TrayState, icon_png: Vec<u8>, rx: std::sync::mpsc::Receiver<Msg>, ready: Sender<bool>) {
    let built = (|| {
        gtk::init().ok()?;
        let data = eframe::icon_data::from_png_bytes(&icon_png).ok()?;
        let icon = Icon::from_rgba(data.rgba.clone(), data.width, data.height).ok()?;
        let (l_new, l_active, l_settings, l_support, l_exit) = labels(&state);
        let new = MenuItem::new(l_new, true, None);
        let active = MenuItem::new(l_active, true, None);
        let settings = MenuItem::new(l_settings, true, None);
        let support_item = MenuItem::new(l_support, true, None);
        let exit = MenuItem::new(l_exit, true, None);
        let menu = Menu::new();
        menu.append_items(&[&new, &active, &settings, &PredefinedMenuItem::separator(), &support_item, &exit]).ok()?;
        let tray = TrayIconBuilder::new().with_icon(icon).with_tooltip(&state.tooltip).with_menu(Box::new(menu)).build().ok()?;
        Some((tray, new, active, settings, support_item, exit))
    })();
    let Some((tray, new, active, settings, support_item, exit)) = built else {
        let _ = ready.send(false);
        return;
    };
    if !tray_host_present() {
        let _ = ready.send(false);
        return;
    }

    let ids = (new.id().clone(), active.id().clone(), settings.id().clone(), support_item.id().clone(), exit.id().clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let a = if e.id == ids.0 {
            Action::New
        } else if e.id == ids.1 || e.id == ids.2 {
            Action::Settings
        } else if e.id == ids.3 {
            Action::Support
        } else if e.id == ids.4 {
            Action::Exit
        } else {
            return;
        };
        perform(a);
    }));
    let _ = ready.send(true);

    let mut last = Some(state);
    gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::State(s) => {
                    if last.as_ref() == Some(&s) {
                        continue;
                    }
                    let (l_new, l_active, l_settings, l_support, l_exit) = labels(&s);
                    new.set_text(l_new);
                    active.set_text(l_active);
                    settings.set_text(l_settings);
                    support_item.set_text(l_support);
                    exit.set_text(l_exit);
                    let _ = tray.set_tooltip(Some(&s.tooltip));
                    last = Some(s);
                }
                Msg::Remove => {
                    let _ = tray.set_visible(false);
                    gtk::main_quit();
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        gtk::glib::ControlFlow::Continue
    });
    gtk::main();
}
