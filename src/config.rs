//! Settings (`config.json`) and the data folder. The daemon and the window processes all read this file.

use crate::hotkey::Hotkey;
use crate::theme::ThemeMode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `%APPDATA%\Remindo` (`~/.config/Remindo` on Linux), or the folder from `REMINDO_DATA_DIR` (tests and screenshots).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("REMINDO_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Remindo")
}

/// Held by every test that sets, removes or depends on `REMINDO_DATA_DIR`, because the environment is shared by all test threads.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Is a data-dir override active? Test instances use their own names for the single-instance locks, never register
/// the real hotkey, never touch autostart and make no sound.
pub fn is_test_instance() -> bool {
    std::env::var_os("REMINDO_DATA_DIR").is_some_and(|d| !d.is_empty())
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub w: f32,
    pub h: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Interface language code (`en`, `ru`, `uk`); `None` until first resolved.
    pub language: Option<String>,
    pub theme: ThemeMode,
    /// The global shortcut, e.g. `Ctrl+Alt+R`.
    pub hotkey: String,
    /// Play the chime when a timer fires.
    pub sound: bool,
    /// Wanted state of "start with the system"; the real state is read from the system.
    pub autostart: bool,
    pub check_updates: bool,
    pub window: Option<Window>,
    /// Set when the file was missing, unreadable or broken (not saved).
    #[serde(skip)]
    pub notice: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { language: None, theme: ThemeMode::System, hotkey: Hotkey::default().to_string(), sound: true, autostart: false, check_updates: true, window: None, notice: None }
    }
}

/// Writes a file in one step: a temporary file next to it, then a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Moves a broken file aside (`name.bad-<unix time>`) so the user can look at it, and returns the new path.
pub fn keep_aside(path: &Path) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let name = path.file_name()?.to_string_lossy().to_string();
    let to = path.with_file_name(format!("{name}.bad-{stamp}"));
    std::fs::rename(path, &to).ok().map(|_| to)
}

impl Config {
    pub fn load_from(dir: &Path) -> Self {
        let path = dir.join("config.json");
        let Ok(text) = std::fs::read_to_string(&path) else { return Self::default() };
        match serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')) {
            Ok(mut c) => {
                if Hotkey::parse(&c.hotkey).is_none() {
                    c.hotkey = Hotkey::default().to_string();
                }
                c
            }
            Err(_) => {
                let kept = keep_aside(&path);
                let mut c = Self::default();
                c.notice = Some(match kept {
                    Some(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    None => String::new(),
                });
                c
            }
        }
    }

    /// Writes the settings. `false` if they could not be written.
    pub fn save_to(&self, dir: &Path) -> bool {
        match serde_json::to_vec_pretty(self) {
            Ok(json) => write_atomic(&dir.join("config.json"), &json).is_ok(),
            Err(_) => false,
        }
    }

    pub fn hotkey(&self) -> Hotkey {
        Hotkey::parse(&self.hotkey).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("remindo-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn missing_config_has_defaults() {
        let dir = tmp("fresh");
        let c = Config::load_from(&dir);
        assert_eq!(c.theme, ThemeMode::System);
        assert_eq!(c.hotkey, "Ctrl+Alt+R");
        assert!(c.sound && !c.autostart && c.notice.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tmp("rt");
        let mut c = Config::default();
        c.language = Some("uk".into());
        c.theme = ThemeMode::Dark;
        c.hotkey = "Ctrl+Shift+F9".into();
        c.sound = false;
        assert!(c.save_to(&dir));
        let l = Config::load_from(&dir);
        assert_eq!(l.language.as_deref(), Some("uk"));
        assert_eq!(l.theme, ThemeMode::Dark);
        assert_eq!(l.hotkey, "Ctrl+Shift+F9");
        assert!(!l.sound);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bom_unknown_fields_and_bad_hotkey_are_tolerated() {
        let dir = tmp("bom");
        std::fs::write(dir.join("config.json"), "\u{feff}{\"theme\":\"light\",\"future_key\":1,\"hotkey\":\"nonsense\"}").unwrap();
        let c = Config::load_from(&dir);
        assert_eq!(c.theme, ThemeMode::Light);
        assert_eq!(c.hotkey, "Ctrl+Alt+R");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_file_is_kept_aside_with_a_notice() {
        let dir = tmp("broken");
        std::fs::write(dir.join("config.json"), "{ this is not json").unwrap();
        let c = Config::load_from(&dir);
        assert!(c.notice.as_deref().is_some_and(|n| n.starts_with("config.json.bad-")));
        assert!(!dir.join("config.json").exists());
        assert!(std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("config.json.bad-")));
        assert_eq!(c.theme, ThemeMode::System);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn language_order_config_then_marker_then_english() {
        assert_eq!(i18n::resolve(Some("ru")), Lang::Ru);
        assert_eq!(i18n::resolve(Some("uk")), Lang::Uk);
        if i18n::marker_language().is_none() {
            assert_eq!(i18n::resolve(None), Lang::En);
        }
    }
}
