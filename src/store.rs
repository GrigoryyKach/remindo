//! The timers: active ones, finished ones waiting to be closed, and the history. Pure data and rules; the daemon
//! owns one `Store`, wakes at `next_due` and writes it to `timers.json` after every change.
//!
//! All times are Unix milliseconds in UTC (absolute, so a timer survives a restart, a reboot and a time zone change).

use crate::config::{keep_aside, write_atomic};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Current file format. Older files are migrated (after a backup); newer ones are left alone.
pub const SCHEMA: u32 = 1;
/// A timer that fires this much later than due counts as missed ("missed at 14:05").
pub const MISSED_AFTER_MS: i64 = 5_000;
pub const HISTORY_MAX: usize = 50;
pub const TIMERS_MAX: usize = 200;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Timer {
    pub id: u64,
    pub label: String,
    pub set_at: i64,
    pub fire_at: i64,
    /// The length it was created with, in seconds (for "Repeat" and the "set for 15 min" line).
    pub secs: i64,
    /// Set when the text named a clock time ("at 18:30"); "Start again" then picks the next 18:30.
    pub clock: Option<(u32, u32)>,
}

/// A timer that has fired and waits for Close, Repeat or +5 min.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Alert {
    pub id: u64,
    pub label: String,
    pub set_at: i64,
    pub fire_at: i64,
    pub fired_at: i64,
    pub secs: i64,
    pub missed: bool,
    pub clock: Option<(u32, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HistoryItem {
    pub label: String,
    pub secs: i64,
    pub clock: Option<(u32, u32)>,
    /// When it was last started.
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub next_id: u64,
    pub timers: Vec<Timer>,
    pub alerts: Vec<Alert>,
    pub history: Vec<HistoryItem>,
}

impl Default for Store {
    fn default() -> Self {
        Store { version: SCHEMA, next_id: 1, timers: Vec::new(), alerts: Vec::new(), history: Vec::new() }
    }
}

/// What loading found, for the notice in the settings window.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadNote {
    /// The file was broken; it was moved to this name and a clean store started.
    Corrupt(String),
    /// The file was written by a newer version; it was moved aside untouched.
    Newer(String),
    /// An older file was upgraded; the old one is kept as this backup.
    Migrated(String),
}

impl Store {
    pub fn path(dir: &Path) -> std::path::PathBuf {
        dir.join("timers.json")
    }

    /// Reads `timers.json`: migrates old formats (with a backup first), keeps a broken file aside and starts clean.
    pub fn load(dir: &Path) -> (Store, Option<LoadNote>) {
        let path = Self::path(dir);
        let Ok(text) = std::fs::read_to_string(&path) else { return (Store::default(), None) };
        let value: Result<serde_json::Value, _> = serde_json::from_str(text.trim_start_matches('\u{feff}'));
        let Ok(mut value) = value else { return Self::broken(&path) };
        if !value.is_object() {
            return Self::broken(&path);
        }
        let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if version > SCHEMA {
            let name = keep_aside(&path).and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
            return (Store::default(), Some(LoadNote::Newer(name)));
        }
        let mut note = None;
        if version < SCHEMA {
            // Back up before touching anything.
            let backup = dir.join(format!("timers.json.bak-v{version}"));
            let _ = std::fs::copy(&path, &backup);
            migrate(&mut value, version);
            note = Some(LoadNote::Migrated(backup.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
        }
        match serde_json::from_value::<Store>(value) {
            Ok(mut s) => {
                s.version = SCHEMA;
                s.repair();
                if note.is_some() {
                    let _ = s.save(dir);
                }
                (s, note)
            }
            Err(_) => Self::broken(&path),
        }
    }

    fn broken(path: &Path) -> (Store, Option<LoadNote>) {
        let name = keep_aside(path).and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
        (Store::default(), Some(LoadNote::Corrupt(name)))
    }

    /// Makes the ids consistent after loading (a hand-edited file, or a migration).
    fn repair(&mut self) {
        let max = self.timers.iter().map(|t| t.id).chain(self.alerts.iter().map(|a| a.id)).max().unwrap_or(0);
        if self.next_id <= max {
            self.next_id = max + 1;
        }
        self.history.truncate(HISTORY_MAX);
        self.timers.truncate(TIMERS_MAX);
    }

    pub fn save(&self, dir: &Path) -> bool {
        match serde_json::to_vec_pretty(self) {
            Ok(json) => write_atomic(&Self::path(dir), &json).is_ok(),
            Err(_) => false,
        }
    }

    // ───────────────────────── changes ─────────────────────────

    /// Adds a timer that fires at `fire_at`. Returns its id, or `None` when there are too many.
    pub fn add(&mut self, now: i64, label: &str, fire_at: i64, secs: i64, clock: Option<(u32, u32)>) -> Option<u64> {
        if self.timers.len() >= TIMERS_MAX {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.timers.push(Timer { id, label: label.to_string(), set_at: now, fire_at, secs, clock });
        self.remember(now, label, secs, clock);
        Some(id)
    }

    /// Puts an entry on top of the history (an identical older one moves up instead of repeating).
    fn remember(&mut self, now: i64, label: &str, secs: i64, clock: Option<(u32, u32)>) {
        self.history.retain(|h| !(h.label == label && h.secs == secs && h.clock == clock));
        self.history.insert(0, HistoryItem { label: label.to_string(), secs, clock, at: now });
        self.history.truncate(HISTORY_MAX);
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let n = self.timers.len();
        self.timers.retain(|t| t.id != id);
        self.timers.len() != n
    }

    /// Moves an active timer later (or earlier, for a negative amount) by `secs`.
    pub fn plus(&mut self, id: u64, secs: i64) -> bool {
        match self.timers.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                t.fire_at += secs * 1000;
                t.secs = (t.secs + secs).max(1);
                true
            }
            None => false,
        }
    }

    /// Changes the label and/or the fire time of an active timer.
    pub fn edit(&mut self, now: i64, id: u64, label: Option<&str>, fire_at: Option<i64>, clock: Option<Option<(u32, u32)>>) -> bool {
        match self.timers.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                if let Some(l) = label {
                    t.label = l.to_string();
                }
                if let Some(f) = fire_at {
                    t.fire_at = f;
                    t.secs = ((f - now) / 1000).max(1);
                }
                if let Some(c) = clock {
                    t.clock = c;
                }
                true
            }
            None => false,
        }
    }

    /// Moves every timer that is due into the alerts. Returns the new alerts.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn due(&mut self, now: i64) -> Vec<Alert> {
        self.due_with(now, MISSED_AFTER_MS)
    }

    /// Like [`Store::due`], with its own limit for calling a late timer "missed" (the first pass after the app
    /// started uses a short one: whatever was due before the start was missed).
    pub fn due_with(&mut self, now: i64, missed_after_ms: i64) -> Vec<Alert> {
        let (due, rest): (Vec<Timer>, Vec<Timer>) = std::mem::take(&mut self.timers).into_iter().partition(|t| t.fire_at <= now);
        self.timers = rest;
        let mut fired: Vec<Alert> = due
            .into_iter()
            .map(|t| Alert { id: t.id, label: t.label, set_at: t.set_at, fire_at: t.fire_at, fired_at: now, secs: t.secs, missed: now - t.fire_at > missed_after_ms, clock: t.clock })
            .collect();
        fired.sort_by_key(|a| (a.fire_at, a.id));
        self.alerts.extend(fired.iter().cloned());
        fired
    }

    /// The soonest fire time among the active timers.
    pub fn next_due(&self) -> Option<i64> {
        self.timers.iter().map(|t| t.fire_at).min()
    }

    pub fn dismiss(&mut self, id: u64) -> bool {
        let n = self.alerts.len();
        self.alerts.retain(|a| a.id != id);
        self.alerts.len() != n
    }

    pub fn dismiss_all(&mut self) {
        self.alerts.clear();
    }

    /// Closes a finished timer and starts the same length again with the same label.
    pub fn repeat(&mut self, now: i64, id: u64) -> Option<u64> {
        let a = self.alerts.iter().find(|a| a.id == id)?.clone();
        self.dismiss(id);
        self.add(now, &a.label, now + a.secs.max(1) * 1000, a.secs, a.clock)
    }

    /// Closes a finished timer and starts a new one that fires in `secs` (the "+5 min" snooze).
    pub fn snooze(&mut self, now: i64, id: u64, secs: i64) -> Option<u64> {
        let a = self.alerts.iter().find(|a| a.id == id)?.clone();
        self.dismiss(id);
        let new_id = self.next_id;
        self.next_id += 1;
        if self.timers.len() >= TIMERS_MAX {
            return None;
        }
        // A snooze is not a new history entry: the original is already there.
        self.timers.push(Timer { id: new_id, label: a.label, set_at: now, fire_at: now + secs * 1000, secs, clock: None });
        Some(new_id)
    }

    /// Starts a history entry again. `fire_at` is computed by the caller (a clock entry needs the local time zone).
    pub fn start_again(&mut self, now: i64, index: usize, fire_at: i64) -> Option<u64> {
        let h = self.history.get(index)?.clone();
        self.add(now, &h.label, fire_at, h.secs, h.clock)
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

/// Upgrades a parsed file from `from` to the current schema, in place.
fn migrate(value: &mut serde_json::Value, from: u32) {
    let Some(obj) = value.as_object_mut() else { return };
    if from < 1 {
        // Version 0 had no `version`, `next_id`, `alerts` or `history`, and called the label `text`.
        if let Some(timers) = obj.get_mut("timers").and_then(|t| t.as_array_mut()) {
            for t in timers {
                if let Some(o) = t.as_object_mut() {
                    if !o.contains_key("label") {
                        if let Some(text) = o.remove("text") {
                            o.insert("label".into(), text);
                        }
                    }
                    for key in ["set_at", "secs"] {
                        o.entry(key).or_insert(serde_json::json!(0));
                    }
                }
            }
        }
    }
    obj.insert("version".into(), serde_json::json!(SCHEMA));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("remindo-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    const T0: i64 = 1_800_000_000_000;

    #[test]
    fn timers_fire_in_order_of_their_due_time() {
        let mut s = Store::default();
        let c = s.add(T0, "c", T0 + 30_000, 30, None).unwrap();
        let a = s.add(T0, "a", T0 + 10_000, 10, None).unwrap();
        let b = s.add(T0, "b", T0 + 20_000, 20, None).unwrap();
        assert_eq!(s.next_due(), Some(T0 + 10_000));
        assert!(s.due(T0 + 5_000).is_empty());
        let fired = s.due(T0 + 25_000);
        assert_eq!(fired.iter().map(|x| x.id).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(s.next_due(), Some(T0 + 30_000));
        assert_eq!(s.alerts.len(), 2);
        let fired = s.due(T0 + 31_000);
        assert_eq!(fired[0].id, c);
        assert_eq!(s.next_due(), None);
    }

    #[test]
    fn a_late_timer_is_marked_missed() {
        let mut s = Store::default();
        s.add(T0, "on time", T0 + 1_000, 1, None);
        s.add(T0, "late", T0 + 2_000, 2, None);
        let fired = s.due(T0 + 1_200);
        assert!(!fired[0].missed);
        let fired = s.due(T0 + 60_000);
        assert!(fired[0].missed);
        assert_eq!(fired[0].fire_at, T0 + 2_000);
    }

    #[test]
    fn the_first_pass_after_a_start_uses_a_short_limit() {
        let mut s = Store::default();
        s.add(T0, "closed", T0 + 1_000, 1, None);
        // 2.5 s late is not "missed" in normal running ...
        assert!(!s.clone().due(T0 + 3_500)[0].missed);
        // ... but it is when the app has just started: it was not running when this was due.
        assert!(s.due_with(T0 + 3_500, 1_000)[0].missed);
    }

    #[test]
    fn cancel_plus_and_edit() {
        let mut s = Store::default();
        let id = s.add(T0, "tea", T0 + 60_000, 60, None).unwrap();
        assert!(s.plus(id, 300));
        assert_eq!(s.timers[0].fire_at, T0 + 360_000);
        assert_eq!(s.timers[0].secs, 360);
        assert!(s.edit(T0, id, Some("green tea"), Some(T0 + 120_000), None));
        assert_eq!((s.timers[0].label.as_str(), s.timers[0].fire_at, s.timers[0].secs), ("green tea", T0 + 120_000, 120));
        assert!(s.cancel(id));
        assert!(!s.cancel(id));
        assert!(!s.plus(id, 5));
        assert!(s.timers.is_empty());
    }

    #[test]
    fn repeat_and_snooze() {
        let mut s = Store::default();
        s.add(T0, "stove", T0 + 900_000, 900, None);
        let fired = s.due(T0 + 900_000);
        let id = fired[0].id;
        // Repeat: the same length, the same label, the alert is gone.
        let new = s.repeat(T0 + 901_000, id).unwrap();
        assert!(s.alerts.is_empty());
        let t = s.timers.iter().find(|t| t.id == new).unwrap();
        assert_eq!((t.label.as_str(), t.secs, t.fire_at), ("stove", 900, T0 + 901_000 + 900_000));
        assert!(s.repeat(T0, id).is_none());
        // Snooze five minutes.
        let fired = s.due(T0 + 2_000_000);
        let new = s.snooze(T0 + 2_000_000, fired[0].id, 300).unwrap();
        assert!(s.alerts.is_empty());
        let t = s.timers.iter().find(|t| t.id == new).unwrap();
        assert_eq!((t.label.as_str(), t.fire_at), ("stove", T0 + 2_300_000));
    }

    #[test]
    fn history_keeps_the_last_fifty_without_duplicates() {
        let mut s = Store::default();
        for i in 0..70 {
            s.add(T0 + i, &format!("t{i}"), T0 + 10_000, 10, None);
            s.timers.clear();
        }
        assert_eq!(s.history.len(), HISTORY_MAX);
        assert_eq!(s.history[0].label, "t69");
        // The same entry again moves to the top.
        s.add(T0, "t30", T0 + 1, 10, None);
        assert_eq!(s.history[0].label, "t30");
        assert_eq!(s.history.iter().filter(|h| h.label == "t30").count(), 1);
        assert_eq!(s.history.len(), HISTORY_MAX);
        assert!(s.start_again(T0, 3, T0 + 5_000).is_some());
        s.clear_history();
        assert!(s.history.is_empty());
    }

    #[test]
    fn survives_a_restart_and_fires_what_was_due_while_closed() {
        let dir = tmp("restart");
        let mut s = Store::default();
        s.add(T0, "soon", T0 + 10_000, 10, None);
        s.add(T0, "later", T0 + 3_600_000, 3600, None);
        s.add(T0, "clock", T0 + 20_000, 20, Some((18, 30)));
        s.due(T0 + 15_000);
        assert!(s.save(&dir));
        let (mut back, note) = Store::load(&dir);
        assert!(note.is_none());
        assert_eq!(back, s);
        // The app was closed for two hours: "later" and "clock" are missed.
        let fired = back.due(T0 + 7_200_000);
        assert_eq!(fired.len(), 2);
        assert!(fired.iter().all(|a| a.missed));
        assert!(fired.iter().any(|a| a.clock == Some((18, 30))));
        // The alert that was already waiting is still there too.
        assert_eq!(back.alerts.len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_and_the_store_starts_clean() {
        let dir = tmp("corrupt");
        std::fs::write(Store::path(&dir), "{ not json at all").unwrap();
        let (s, note) = Store::load(&dir);
        assert!(s.timers.is_empty() && s.next_id == 1);
        match note {
            Some(LoadNote::Corrupt(name)) => assert!(name.starts_with("timers.json.bad-")),
            other => panic!("{other:?}"),
        }
        assert!(!Store::path(&dir).exists());
        assert!(std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("timers.json.bad-")));
        // An array instead of an object is broken too.
        std::fs::write(Store::path(&dir), "[1,2,3]").unwrap();
        assert!(matches!(Store::load(&dir).1, Some(LoadNote::Corrupt(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_old_file_is_backed_up_and_migrated() {
        let dir = tmp("migrate");
        let old = r#"{"timers":[{"id":7,"text":"tea","fire_at":1800000060000}]}"#;
        std::fs::write(Store::path(&dir), old).unwrap();
        let (s, note) = Store::load(&dir);
        assert_eq!(note, Some(LoadNote::Migrated("timers.json.bak-v0".into())));
        assert_eq!(s.version, SCHEMA);
        assert_eq!(s.timers.len(), 1);
        assert_eq!(s.timers[0].label, "tea");
        assert_eq!(s.next_id, 8);
        // The backup is the original file, byte for byte.
        assert_eq!(std::fs::read_to_string(dir.join("timers.json.bak-v0")).unwrap(), old);
        // The migrated file loads without a note next time.
        let (again, note) = Store::load(&dir);
        assert!(note.is_none());
        assert_eq!(again.timers.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_newer_file_is_left_alone() {
        let dir = tmp("newer");
        std::fs::write(Store::path(&dir), r#"{"version":99,"timers":[]}"#).unwrap();
        let (s, note) = Store::load(&dir);
        assert!(s.timers.is_empty());
        assert!(matches!(note, Some(LoadNote::Newer(n)) if n.starts_with("timers.json.bad-")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_fields_and_a_bom_are_tolerated() {
        let dir = tmp("bom");
        std::fs::write(Store::path(&dir), "\u{feff}{\"version\":1,\"next_id\":3,\"future\":true,\"timers\":[]}").unwrap();
        let (s, note) = Store::load(&dir);
        assert!(note.is_none());
        assert_eq!(s.next_id, 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn too_many_timers_are_refused() {
        let mut s = Store::default();
        for i in 0..TIMERS_MAX {
            assert!(s.add(T0, &format!("t{i}"), T0 + 1000, 1, None).is_some());
        }
        assert!(s.add(T0, "one more", T0 + 1000, 1, None).is_none());
    }
}
