//! The resident part of Remindo: owns the timers, the hotkey and the tray, and answers the window processes over IPC.
//!
//! It draws nothing. The quick input, the finished-timer window and Settings are separate short-lived processes
//! (`--popup`, `--alerts`, `--settings`) that the daemon starts and that talk to it with one-line requests.

use crate::config::{self, Config};
use crate::hotkey::Hotkey;
use crate::i18n::{self, Lang};
use crate::store::{Alert, LoadNote, Store};
use crate::tray_common::TrayState;
use crate::{fmt, ipc, parse, platform, sound, strings, tray};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

pub struct Daemon {
    pub dir: PathBuf,
    pub cfg: Config,
    pub store: Store,
    pub lang: Lang,
    /// Messages for the Settings window (a broken file was set aside, an old one upgraded).
    pub notices: Vec<String>,
    /// Why the shortcut could not be registered (`None` when it works).
    pub hotkey_error: Option<String>,
}

static STATE: OnceLock<Mutex<Daemon>> = OnceLock::new();
static QUIT: AtomicBool = AtomicBool::new(false);
/// The popup is the native Win32 window in this process (Windows, unless `--popup-egui`).
static NATIVE: AtomicBool = AtomicBool::new(false);
static FIRST_TICK: AtomicBool = AtomicBool::new(true);
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
/// Registering the real shortcut is skipped in a test instance, unless `REMINDO_TEST_HOTKEY=1`.
static HOTKEY_LIVE: AtomicBool = AtomicBool::new(true);

#[cfg_attr(not(windows), allow(dead_code))]
pub fn native_popup() -> bool {
    NATIVE.load(Ordering::Relaxed)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn with<R>(f: impl FnOnce(&mut Daemon) -> R) -> Option<R> {
    let m = STATE.get()?;
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    Some(f(&mut g))
}

/// Wakes the scheduler thread (a timer was added, the clock may have jumped).
pub fn wake() {
    let (lock, cv) = &WAKE;
    if let Ok(mut flag) = lock.lock() {
        *flag = true;
        cv.notify_all();
    }
}

pub fn request_quit() {
    QUIT.store(true, Ordering::Relaxed);
    wake();
    #[cfg(windows)]
    crate::winloop::post_quit();
    #[cfg(unix)]
    crate::unixloop::post_quit();
}

// ───────────────────────── opening the windows ─────────────────────────

#[cfg(not(test))]
fn spawn(args: &[&str]) {
    platform::spawn(args);
}

/// Tests record what would have been started instead of starting it.
#[cfg(test)]
pub static SPAWNED: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(test)]
fn spawn(args: &[&str]) {
    SPAWNED.lock().unwrap_or_else(|e| e.into_inner()).push(args.join(" "));
}

/// Opens the quick input, or brings the open one forward. Never opens a second one.
pub fn open_popup() {
    #[cfg(windows)]
    if native_popup() {
        crate::winloop::post_show_popup();
        return;
    }
    platform::allow_foreground();
    if ipc::request("popup", "focus").is_some() {
        return;
    }
    spawn(&["--popup"]);
}

/// The same, off the caller's thread (the message loop must not wait for a pipe).
pub fn open_popup_async() {
    let _ = std::thread::Builder::new().name("remindo-open-popup".into()).spawn(open_popup);
}

pub fn open_settings() {
    platform::allow_foreground();
    if ipc::request("settings", "focus").is_some() {
        return;
    }
    spawn(&["--settings"]);
}

/// Makes sure the finished-timer window process is running.
fn ensure_alerts() {
    if ipc::request("alerts", "ping").is_none() {
        spawn(&["--alerts"]);
    }
}

// ───────────────────────── state ─────────────────────────

impl Daemon {
    fn save(&self) {
        let _ = self.store.save(&self.dir);
    }

    fn tray_state(&self) -> TrayState {
        let t = strings::TABLE.get(self.lang);
        let now = now_ms();
        let tip = match self.store.next_due() {
            Some(due) => t.tray_tip_next.replace("{d}", &fmt::dur(((due - now) / 1000).max(1), self.lang)),
            None => t.tray_tip_none.to_string(),
        };
        TrayState { lang: self.lang, tooltip: format!("Remindo · {tip}"), hotkey: self.cfg.hotkey.clone(), count: self.store.timers.len() }
    }

    /// Adds a timer from what the user typed. `Err` is a short code the window turns into a hint.
    pub fn add_text(&mut self, text: &str) -> Result<u64, &'static str> {
        let now = now_ms();
        let local_now = fmt::local(now).naive_local();
        let p = parse::parse(text, local_now, self.lang).map_err(error_code)?;
        let label = p.label_or_default();
        let (fire_at, clock) = if p.absolute {
            (local_to_ms(p.fire_at).unwrap_or(now + p.secs * 1000), Some((chrono::Timelike::hour(&p.fire_at), chrono::Timelike::minute(&p.fire_at))))
        } else {
            (now + p.secs * 1000, None)
        };
        let id = self.store.add(now, &label, fire_at, p.secs, clock).ok_or("too_many")?;
        self.save();
        wake();
        Ok(id)
    }

    /// Edits an active timer from text: a new time and/or note. Text without a time changes only the note.
    fn edit_text(&mut self, id: u64, text: &str) -> Result<(), &'static str> {
        let now = now_ms();
        let local_now = fmt::local(now).naive_local();
        let old_label = self.store.timers.iter().find(|t| t.id == id).map(|t| t.label.clone()).ok_or("gone")?;
        match parse::parse(text, local_now, self.lang) {
            Ok(p) => {
                let label = if p.label.is_empty() { old_label } else { p.label.clone() };
                let (fire_at, clock) = if p.absolute {
                    (local_to_ms(p.fire_at).unwrap_or(now + p.secs * 1000), Some((chrono::Timelike::hour(&p.fire_at), chrono::Timelike::minute(&p.fire_at))))
                } else {
                    (now + p.secs * 1000, None)
                };
                self.store.edit(now, id, Some(&label), Some(fire_at), Some(clock));
            }
            Err(parse::Error::NoTime) => {
                let label = text.trim();
                if label.is_empty() {
                    return Err("empty");
                }
                self.store.edit(now, id, Some(label), None, None);
            }
            Err(e) => return Err(error_code(e)),
        }
        self.save();
        wake();
        Ok(())
    }

    fn start_again(&mut self, index: usize) -> bool {
        let now = now_ms();
        let Some(h) = self.store.history.get(index).cloned() else { return false };
        let fire_at = match h.clock {
            Some((hh, mm)) => next_clock_ms(now, hh, mm),
            None => now + h.secs.max(1) * 1000,
        };
        let ok = self.store.start_again(now, index, fire_at).is_some();
        if ok {
            self.save();
            wake();
        }
        ok
    }

    fn state_json(&self) -> String {
        json!({
            "now": now_ms(),
            "timers": self.store.timers,
            "history": self.store.history,
            "alerts": self.store.alerts,
            "hotkey": self.cfg.hotkey,
            "hotkey_error": self.hotkey_error,
            "notices": self.notices,
            "sound": self.cfg.sound,
            "lang": self.lang.code(),
        })
        .to_string()
    }
}

fn error_code(e: parse::Error) -> &'static str {
    match e {
        parse::Error::Empty => "empty",
        parse::Error::NoTime => "no_time",
        parse::Error::NoUnit => "no_unit",
        parse::Error::Zero => "zero",
        parse::Error::BadClock => "bad_clock",
    }
}

fn local_to_ms(dt: chrono::NaiveDateTime) -> Option<i64> {
    use chrono::TimeZone;
    chrono::Local.from_local_datetime(&dt).earliest().map(|d| d.timestamp_millis())
}

/// The next time the clock shows `h:m` after `now_ms`.
pub fn next_clock_ms(now_ms: i64, h: u32, m: u32) -> i64 {
    let local_now = fmt::local(now_ms).naive_local();
    let mut day = local_now.date();
    for _ in 0..3 {
        if let Some(dt) = day.and_hms_opt(h.min(23), m.min(59), 0) {
            if dt > local_now {
                if let Some(ms) = local_to_ms(dt) {
                    return ms;
                }
            }
        }
        day += chrono::Duration::days(1);
    }
    now_ms + 60_000
}

pub fn update_tray() {
    if let Some(s) = with(|d| d.tray_state()) {
        tray::update(s);
    }
}

// ───────────────────────── scheduler ─────────────────────────

/// What happens when timers fire: the chime and the finished-timer window.
fn on_fired(fired: &[Alert], sound_on: bool) {
    if fired.is_empty() {
        return;
    }
    if sound_on {
        sound::play();
    }
    ensure_alerts();
}

/// One pass of the scheduler: moves due timers to the alerts. Returns how long to wait.
fn tick() -> Duration {
    let now = now_ms();
    // The first pass after the daemon started: anything that was due before is a timer that came due while closed.
    let limit = if FIRST_TICK.swap(false, Ordering::Relaxed) { 1_000 } else { crate::store::MISSED_AFTER_MS };
    let outcome = with(|d| {
        let fired = d.store.due_with(now, limit);
        if !fired.is_empty() {
            d.save();
        }
        (fired, d.store.next_due(), d.cfg.sound)
    });
    let Some((fired, next, sound_on)) = outcome else { return Duration::from_secs(1) };
    if !fired.is_empty() {
        on_fired(&fired, sound_on);
        update_tray();
    }
    // Wake at the due time; never sleep longer than a few seconds, so a clock jump or a resume is noticed.
    let cap = if next.is_some() { 3_000 } else { 30_000 };
    let wait = next.map(|t| (t - now_ms()).clamp(0, cap)).unwrap_or(cap);
    Duration::from_millis(wait as u64)
}

fn scheduler_loop() {
    while !QUIT.load(Ordering::Relaxed) {
        let wait = tick();
        let (lock, cv) = &WAKE;
        let mut flag = lock.lock().unwrap_or_else(|e| e.into_inner());
        if !*flag {
            flag = cv.wait_timeout(flag, wait).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
        }
        *flag = false;
    }
}

// ───────────────────────── IPC ─────────────────────────

fn ok_json() -> String {
    json!({"ok": true}).to_string()
}

fn err_json(code: &str) -> String {
    json!({"ok": false, "err": code}).to_string()
}

/// One request line in, one reply line out.
pub fn handle(msg: &str) -> String {
    let (cmd, rest) = msg.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((msg, ""));
    let id = || rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok());
    match cmd {
        "ping" => "pong".into(),
        "new" => {
            open_popup_async();
            "ok".into()
        }
        "settings" => {
            std::thread::spawn(open_settings);
            "ok".into()
        }
        "state" => with(|d| d.state_json()).unwrap_or_else(|| "{}".into()),
        "alerts" => with(|d| json!(d.store.alerts).to_string()).unwrap_or_else(|| "[]".into()),
        "text" => {
            let r = with(|d| d.add_text(rest)).unwrap_or(Err("down"));
            update_tray();
            match r {
                Ok(id) => json!({"ok": true, "id": id}).to_string(),
                Err(code) => err_json(code),
            }
        }
        "cancel" | "plus" | "edit" | "dismiss" | "repeat" | "snooze" | "again" => {
            let reply = with(|d| mutate(d, cmd, rest, id())).unwrap_or_else(|| err_json("down"));
            update_tray();
            reply
        }
        "dismiss_all" => {
            with(|d| {
                d.store.dismiss_all();
                d.save();
            });
            ok_json()
        }
        "clear_history" => {
            with(|d| {
                d.store.clear_history();
                d.save();
            });
            ok_json()
        }
        "hotkey" => set_hotkey(rest),
        "reload" => {
            with(|d| {
                d.cfg = Config::load_from(&d.dir);
                d.lang = i18n::resolve(d.cfg.language.as_deref());
            });
            update_tray();
            ok_json()
        }
        "quit" => {
            request_quit();
            "ok".into()
        }
        _ => err_json("unknown"),
    }
}

fn mutate(d: &mut Daemon, cmd: &str, rest: &str, id: Option<u64>) -> String {
    let now = now_ms();
    let ok = match (cmd, id) {
        ("cancel", Some(id)) => d.store.cancel(id),
        ("plus", Some(id)) => {
            let secs = rest.split_whitespace().nth(1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(300);
            d.store.plus(id, secs)
        }
        ("edit", Some(id)) => {
            let text = rest.split_once(' ').map(|x| x.1).unwrap_or("");
            return match d.edit_text(id, text) {
                Ok(()) => ok_json(),
                Err(c) => err_json(c),
            };
        }
        ("dismiss", Some(id)) => d.store.dismiss(id),
        ("repeat", Some(id)) => d.store.repeat(now, id).is_some(),
        ("snooze", Some(id)) => {
            let secs = rest.split_whitespace().nth(1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(300);
            d.store.snooze(now, id, secs).is_some()
        }
        ("again", Some(i)) => return if d.start_again(i as usize) { ok_json() } else { err_json("gone") },
        _ => false,
    };
    if ok {
        d.save();
        wake();
        ok_json()
    } else {
        err_json("gone")
    }
}

fn set_hotkey(spec: &str) -> String {
    let Some(h) = Hotkey::parse(spec) else { return err_json("invalid") };
    let live = HOTKEY_LIVE.load(Ordering::Relaxed);
    // The daemon lock is not held while the message loop thread registers the key.
    let result = if live { rebind(&h) } else { Ok(()) };
    match result {
        Ok(()) => {
            with(|d| {
                d.cfg.hotkey = h.to_string();
                d.hotkey_error = None;
                d.cfg.save_to(&d.dir);
            });
            update_tray();
            ok_json()
        }
        Err(e) => err_json(if e == "taken" { "taken" } else { "failed" }),
    }
}

fn rebind(h: &Hotkey) -> Result<(), String> {
    #[cfg(windows)]
    return crate::winloop::rebind(h);
    #[cfg(unix)]
    return crate::unixloop::rebind(h);
}

// ───────────────────────── process entry ─────────────────────────

/// Runs the daemon until Exit. Returns at once when another daemon already runs for this data folder.
pub fn run(dir: PathBuf, native: bool) {
    NATIVE.store(native && cfg!(windows), Ordering::Relaxed);
    let mut cfg = Config::load_from(&dir);
    if config::is_test_instance() {
        if let Ok(l) = std::env::var("REMINDO_LANG") {
            cfg.language = Some(l);
        }
        match std::env::var("REMINDO_THEME").as_deref() {
            Ok("dark") => cfg.theme = crate::theme::ThemeMode::Dark,
            Ok("light") => cfg.theme = crate::theme::ThemeMode::Light,
            _ => {}
        }
    }
    let (store, note) = Store::load(&dir);
    if !ipc::serve("daemon", handle) {
        return;
    }
    let lang = i18n::resolve(cfg.language.as_deref());
    if cfg.language.is_none() {
        cfg.language = Some(lang.code().to_string());
        cfg.save_to(&dir);
    }
    let t = strings::TABLE.get(lang);
    let mut notices = Vec::new();
    match &note {
        Some(LoadNote::Corrupt(n)) => notices.push(t.notice_timers_corrupt.replace("{name}", n)),
        Some(LoadNote::Newer(n)) => notices.push(t.notice_timers_newer.replace("{name}", n)),
        Some(LoadNote::Migrated(n)) => notices.push(t.notice_timers_migrated.replace("{name}", n)),
        None => {}
    }
    if let Some(n) = &cfg.notice {
        notices.push(t.notice_config_corrupt.replace("{name}", n));
    }
    let hotkey = cfg.hotkey();
    let has_pending = !store.alerts.is_empty();
    let _ = STATE.set(Mutex::new(Daemon { dir, cfg, store, lang, notices, hotkey_error: None }));
    HOTKEY_LIVE.store(!config::is_test_instance() || std::env::var("REMINDO_TEST_HOTKEY").is_ok_and(|v| v == "1"), Ordering::Relaxed);

    std::thread::Builder::new().name("remindo-scheduler".into()).spawn(scheduler_loop).ok();
    if has_pending {
        ensure_alerts();
    }
    let live = HOTKEY_LIVE.load(Ordering::Relaxed);
    #[cfg(windows)]
    if config::is_test_instance() {
        if let Ok(path) = std::env::var("REMINDO_NATIVE_SHOT") {
            let text = std::env::var("REMINDO_SHOT_TEXT").unwrap_or_default();
            crate::nativepopup::request_shot(path, text);
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(800));
                crate::winloop::post_show_popup();
            });
        }
    }
    let on_ready = move |hotkey_result: Result<(), String>| {
        with(|d| d.hotkey_error = hotkey_result.err());
        if let Some(s) = with(|d| d.tray_state()) {
            tray::create(&s, crate::ICON_PNG);
        }
    };
    #[cfg(windows)]
    {
        crate::winloop::run(live.then_some(hotkey), on_ready);
        tray::remove();
    }
    #[cfg(unix)]
    {
        crate::unixloop::run(live.then_some(hotkey), on_ready);
        tray::remove();
    }
    QUIT.store(true, Ordering::Relaxed);
    wake();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daemon state is one global, so one test walks through everything the window processes can ask.
    #[test]
    fn requests_end_to_end() {
        let _env = config::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("remindo-daemon-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("REMINDO_DATA_DIR", &dir) };
        HOTKEY_LIVE.store(false, Ordering::Relaxed);
        let cfg = Config::load_from(&dir);
        let _ = STATE.set(Mutex::new(Daemon { dir: dir.clone(), cfg, store: Store::default(), lang: Lang::En, notices: Vec::new(), hotkey_error: None }));
        let v = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap();

        assert_eq!(handle("ping"), "pong");
        // adding from text, the way the quick input does
        let a = v(handle("text in 2 seconds tea"));
        assert_eq!(a["ok"], true);
        let b = v(handle("text через 1 час позвонить маме"));
        let c = v(handle("text at 3 call"));
        assert_eq!((b["ok"].as_bool(), c["ok"].as_bool()), (Some(true), Some(true)));
        assert_eq!(v(handle("text plita"))["err"], "no_time");
        assert_eq!(v(handle("text in 5"))["err"], "no_unit");
        assert_eq!(v(handle("text in 0 min"))["err"], "zero");
        assert_eq!(v(handle("text "))["err"], "empty");
        let state = v(handle("state"));
        assert_eq!(state["timers"].as_array().unwrap().len(), 3);
        assert_eq!(state["timers"][1]["label"], "позвонить маме");
        assert_eq!(state["history"].as_array().unwrap().len(), 3);
        // the same timers are on disk
        assert_eq!(Store::load(&dir).0.timers.len(), 3);

        // +5 min on an active timer, edit, cancel
        let id_b = b["id"].as_u64().unwrap();
        assert_eq!(v(handle(&format!("plus {id_b} 300")))["ok"], true);
        assert_eq!(v(handle(&format!("edit {id_b} позвонить папе")))["ok"], true);
        assert_eq!(v(handle(&format!("edit {id_b} in 10 min wash up")))["ok"], true);
        let state = v(handle("state"));
        let t = state["timers"].as_array().unwrap().iter().find(|t| t["id"] == id_b).unwrap().clone();
        assert_eq!(t["label"], "wash up");
        assert!(t["secs"].as_i64().unwrap() >= 599 && t["secs"].as_i64().unwrap() <= 600);
        assert_eq!(v(handle(&format!("cancel {}", c["id"])))["ok"], true);
        assert_eq!(v(handle(&format!("cancel {}", c["id"])))["err"], "gone");

        // the first timer fires after two seconds: it becomes an alert and a window is requested
        std::thread::sleep(Duration::from_millis(2100));
        let wait = tick();
        assert!(wait <= Duration::from_secs(30));
        let alerts = v(handle("alerts"));
        assert_eq!(alerts.as_array().unwrap().len(), 1);
        assert_eq!(alerts[0]["label"], "tea");
        assert!(SPAWNED.lock().unwrap().iter().any(|s| s == "--alerts"));
        // snooze five minutes, then it is an active timer again and the alert is gone
        let aid = alerts[0]["id"].as_u64().unwrap();
        assert_eq!(v(handle(&format!("snooze {aid} 300")))["ok"], true);
        assert!(v(handle("alerts")).as_array().unwrap().is_empty());
        let state = v(handle("state"));
        assert!(state["timers"].as_array().unwrap().iter().any(|t| t["label"] == "tea" && t["secs"] == 300));

        // history: start again, clear
        assert_eq!(v(handle("again 0"))["ok"], true);
        assert_eq!(v(handle("again 99"))["err"], "gone");
        assert_eq!(v(handle("clear_history"))["ok"], true);
        assert!(v(handle("state"))["history"].as_array().unwrap().is_empty());

        // the shortcut: a bad one is refused, a good one is saved
        assert_eq!(v(handle("hotkey Ctrl+Nonsense"))["err"], "invalid");
        assert_eq!(v(handle("hotkey Ctrl+Shift+F9"))["ok"], true);
        assert_eq!(Config::load_from(&dir).hotkey, "Ctrl+Shift+F9");
        assert_eq!(v(handle("state"))["hotkey"], "Ctrl+Shift+F9");

        // "new" and "settings" ask for a window, and never more than one popup is requested while one is open
        SPAWNED.lock().unwrap().clear();
        assert_eq!(handle("new"), "ok");
        assert_eq!(handle("settings"), "ok");
        std::thread::sleep(Duration::from_millis(300));
        let spawned = SPAWNED.lock().unwrap().clone();
        assert!(spawned.contains(&"--popup".to_string()) && spawned.contains(&"--settings".to_string()), "{spawned:?}");
        // a running popup answers "focus": no second one is started
        let role_up = ipc::serve("popup", |_| "ok".into());
        assert!(role_up);
        SPAWNED.lock().unwrap().clear();
        assert_eq!(handle("new"), "ok");
        std::thread::sleep(Duration::from_millis(300));
        assert!(SPAWNED.lock().unwrap().iter().all(|s| s != "--popup"));

        assert_eq!(v(handle("bogus"))["err"], "unknown");
        unsafe { std::env::remove_var("REMINDO_DATA_DIR") };
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_single_instance_lock_and_the_next_clock_time() {
        let _env = config::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let role = format!("daemon-test-{}", std::process::id());
        assert!(ipc::serve(&role, |m| format!("got {m}")));
        // a second daemon for the same folder cannot take the endpoint
        assert!(!ipc::serve(&role, |_| String::new()));
        assert_eq!(ipc::request(&role, "new").as_deref(), Some("got new"));
        // the next 18:30 is today when it is before 18:30 and tomorrow after it
        use chrono::TimeZone;
        let at = |h, m| chrono::Local.with_ymd_and_hms(2026, 11, 25, h, m, 0).unwrap().timestamp_millis();
        assert_eq!(next_clock_ms(at(14, 0), 18, 30), at(18, 30));
        assert_eq!(next_clock_ms(at(19, 0), 18, 30), at(18, 30) + 86_400_000);
        assert_eq!(next_clock_ms(at(18, 30), 18, 30), at(18, 30) + 86_400_000);
    }
}
