//! The global shortcut on X11: `XGrabKey` on the root window, on its own thread.
//!
//! The thread waits for key events; a change of shortcut is handed over through a channel and the thread is woken
//! by a client message sent to its helper window from a second connection. Wayland has no such grab (see
//! `unixloop::run`): there the user binds a system shortcut to `remindo --new`.

use crate::hotkey::Hotkey;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, GrabMode, ModMask, WindowClass};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

enum Msg {
    Set(Hotkey, Sender<Result<(), String>>),
    Stop,
}

static TX: Mutex<Option<Sender<Msg>>> = Mutex::new(None);
/// The helper window the thread listens on (0 until it exists).
static WAKE_WINDOW: AtomicU32 = AtomicU32::new(0);

/// The keycode that produces `keysym`.
fn keycode_for(conn: &RustConnection, keysym: u32) -> Option<u8> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
    let per = map.keysyms_per_keycode as usize;
    if per == 0 {
        return None;
    }
    map.keysyms.chunks(per).position(|c| c.contains(&keysym)).map(|i| min + i as u8)
}

fn modifiers(h: &Hotkey) -> ModMask {
    let mut m = ModMask::from(0u16);
    if h.ctrl {
        m = m | ModMask::CONTROL;
    }
    if h.alt {
        m = m | ModMask::M1;
    }
    if h.shift {
        m = m | ModMask::SHIFT;
    }
    if h.win {
        m = m | ModMask::M4;
    }
    m
}

/// Caps Lock and Num Lock change the modifier state of a key press: grab every combination.
fn lock_variants() -> [ModMask; 4] {
    [ModMask::from(0u16), ModMask::LOCK, ModMask::M2, ModMask::LOCK | ModMask::M2]
}

fn grab(conn: &RustConnection, root: u32, h: &Hotkey) -> Result<u8, String> {
    let keysym = h.keysym().ok_or("unknown key")?;
    let code = keycode_for(conn, keysym).ok_or("key not on this keyboard")?;
    let base = modifiers(h);
    for lock in lock_variants() {
        // `check` turns the asynchronous BadAccess ("another client has this grab") into an error right here.
        conn.grab_key(true, root, base | lock, code, GrabMode::ASYNC, GrabMode::ASYNC).map_err(|e| e.to_string())?.check().map_err(|_| "taken".to_string())?;
    }
    conn.flush().map_err(|e| e.to_string())?;
    Ok(code)
}

fn ungrab(conn: &RustConnection, root: u32, h: &Hotkey, code: u8) {
    let base = modifiers(h);
    for lock in lock_variants() {
        let _ = conn.ungrab_key(code, root, base | lock);
    }
    let _ = conn.flush();
}

fn run(initial: Option<Hotkey>, on_press: fn(), rx: Receiver<Msg>, first: Sender<Result<(), String>>) {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        let _ = first.send(Err("no X11 display".into()));
        return;
    };
    let root = conn.setup().roots[screen].root;
    let Ok(win) = conn.generate_id() else {
        let _ = first.send(Err("no window id".into()));
        return;
    };
    let created = conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map(|c| c.check());
    if created.is_err() {
        let _ = first.send(Err("cannot create the helper window".into()));
        return;
    }
    WAKE_WINDOW.store(win, Ordering::Relaxed);

    let mut current: Option<(Hotkey, u8)> = None;
    let initial_result = match initial {
        Some(h) => match grab(&conn, root, &h) {
            Ok(code) => {
                current = Some((h, code));
                Ok(())
            }
            Err(e) => Err(e),
        },
        None => Ok(()),
    };
    let _ = first.send(initial_result);

    loop {
        match conn.wait_for_event() {
            Ok(Event::KeyPress(k)) => {
                if current.as_ref().is_some_and(|c| c.1 == k.detail) {
                    on_press();
                }
            }
            Ok(Event::ClientMessage(_)) => {
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        Msg::Set(h, reply) => {
                            // Grab the new one first: the old one is let go only when that worked.
                            let r = match grab(&conn, root, &h) {
                                Ok(code) => {
                                    if let Some((old, old_code)) = current.take() {
                                        ungrab(&conn, root, &old, old_code);
                                    }
                                    current = Some((h, code));
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            };
                            let _ = reply.send(r);
                        }
                        Msg::Stop => {
                            if let Some((h, code)) = current.take() {
                                ungrab(&conn, root, &h, code);
                            }
                            return;
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Starts the grab thread and returns whether the shortcut could be taken.
pub fn start(h: Option<Hotkey>, on_press: fn()) -> Result<(), String> {
    let (tx, rx) = channel();
    let (first_tx, first_rx) = channel();
    std::thread::Builder::new().name("remindo-x11-hotkey".into()).spawn(move || run(h, on_press, rx, first_tx)).map_err(|e| e.to_string())?;
    let r = first_rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| Err("timeout".into()));
    if let Ok(mut g) = TX.lock() {
        *g = Some(tx);
    }
    r
}

fn wake() -> Result<(), String> {
    let win = WAKE_WINDOW.load(Ordering::Relaxed);
    if win == 0 {
        return Err("not running".into());
    }
    let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let ev = ClientMessageEvent::new(32, win, AtomEnum::STRING, [0u32; 5]);
    conn.send_event(false, win, EventMask::NO_EVENT, ev).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())
}

/// Moves the shortcut. The old one stays when the new one is taken.
pub fn set(h: &Hotkey) -> Result<(), String> {
    let (reply_tx, reply_rx) = channel();
    {
        let g = TX.lock().map_err(|_| "lock".to_string())?;
        g.as_ref().ok_or("not running")?.send(Msg::Set(h.clone(), reply_tx)).map_err(|_| "gone".to_string())?;
    }
    wake()?;
    reply_rx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| Err("timeout".into()))
}

pub fn stop() {
    if let Ok(g) = TX.lock() {
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(Msg::Stop);
        }
    }
    let _ = wake();
}
