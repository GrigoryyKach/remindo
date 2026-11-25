//! The global shortcut: its text form (`Ctrl+Alt+Space`), the key tables for Windows and X11, and the recorder
//! that turns an egui key press into a shortcut.

use eframe::egui;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Windows key (Super on Linux).
    pub win: bool,
    /// Canonical key name: `A`..`Z`, `0`..`9`, `F1`..`F24`, `Space`, `Enter`, `Tab`, ...
    pub key: String,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey { ctrl: true, alt: true, shift: false, win: false, key: "R".into() }
    }
}

/// Every named key the recorder accepts, with its Windows virtual-key code and X11 keysym.
const KEYS: &[(&str, u32, u32)] = &[
    ("Space", 0x20, 0x20),
    ("Enter", 0x0D, 0xFF0D),
    ("Tab", 0x09, 0xFF09),
    ("Backspace", 0x08, 0xFF08),
    ("Insert", 0x2D, 0xFF63),
    ("Delete", 0x2E, 0xFFFF),
    ("Home", 0x24, 0xFF50),
    ("End", 0x23, 0xFF57),
    ("PageUp", 0x21, 0xFF55),
    ("PageDown", 0x22, 0xFF56),
    ("Left", 0x25, 0xFF51),
    ("Up", 0x26, 0xFF52),
    ("Right", 0x27, 0xFF53),
    ("Down", 0x28, 0xFF54),
    ("Comma", 0xBC, 0x2C),
    ("Period", 0xBE, 0x2E),
    ("Slash", 0xBF, 0x2F),
    ("Semicolon", 0xBA, 0x3B),
    ("Quote", 0xDE, 0x27),
    ("Backquote", 0xC0, 0x60),
    ("Minus", 0xBD, 0x2D),
    ("Equals", 0xBB, 0x3D),
    ("BracketLeft", 0xDB, 0x5B),
    ("BracketRight", 0xDD, 0x5D),
    ("Backslash", 0xDC, 0x5C),
];

fn table(key: &str) -> Option<(u32, u32)> {
    if let Some(&(_, vk, sym)) = KEYS.iter().find(|k| k.0.eq_ignore_ascii_case(key)) {
        return Some((vk, sym));
    }
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_none() {
        if c.is_ascii_uppercase() {
            return Some((c as u32, c.to_ascii_lowercase() as u32));
        }
        if c.is_ascii_digit() {
            return Some((c as u32, c as u32));
        }
    }
    // F1..F24
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some((0x70 + n - 1, 0xFFBE + n - 1));
        }
    }
    None
}

impl Hotkey {
    /// Parses `Ctrl+Alt+Space` (case-insensitive, any order, `Win`/`Super`/`Meta`/`Cmd` for the Windows key).
    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut h = Hotkey { ctrl: false, alt: false, shift: false, win: false, key: String::new() };
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => h.ctrl = true,
                "alt" | "option" => h.alt = true,
                "shift" => h.shift = true,
                "win" | "super" | "meta" | "cmd" | "windows" => h.win = true,
                _ => {
                    if !h.key.is_empty() {
                        return None;
                    }
                    // Canonical spelling of the key.
                    let canon = KEYS.iter().find(|k| k.0.eq_ignore_ascii_case(part)).map(|k| k.0.to_string()).unwrap_or_else(|| part.to_ascii_uppercase());
                    table(&canon)?;
                    h.key = canon;
                }
            }
        }
        if h.key.is_empty() || !h.is_valid() {
            return None;
        }
        Some(h)
    }

    /// A shortcut needs a modifier, except F-keys (a bare letter would steal typing from every program).
    pub fn is_valid(&self) -> bool {
        let f_key = self.key.starts_with('F') && self.key.len() > 1;
        (self.ctrl || self.alt || self.shift || self.win || f_key) && table(&self.key).is_some()
    }

    pub fn vk(&self) -> Option<u32> {
        table(&self.key).map(|t| t.0)
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn keysym(&self) -> Option<u32> {
        table(&self.key).map(|t| t.1)
    }

    /// `MOD_*` flags of `RegisterHotKey`.
    pub fn win_modifiers(&self) -> u32 {
        (self.alt as u32) | (self.ctrl as u32) << 1 | (self.shift as u32) << 2 | (self.win as u32) << 3
    }

    /// The parts for drawing key caps: `["Ctrl", "Alt", "Space"]`.
    pub fn parts(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.ctrl {
            v.push("Ctrl".to_string());
        }
        if self.alt {
            v.push("Alt".to_string());
        }
        if self.shift {
            v.push("Shift".to_string());
        }
        if self.win {
            v.push(if cfg!(windows) { "Win" } else { "Super" }.to_string());
        }
        v.push(self.key.clone());
        v
    }

    /// Turns a key press and the modifiers held with it into a shortcut.
    pub fn from_input(mods: egui::Modifiers, key: egui::Key) -> Option<Hotkey> {
        let name = key_name(key)?;
        let h = Hotkey { ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift, win: mods.mac_cmd, key: name.to_string() };
        h.is_valid().then_some(h)
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = self.parts();
        // Stored with the neutral spelling, so the file does not change with the platform.
        for p in parts.iter_mut() {
            if p == "Super" {
                *p = "Win".into();
            }
        }
        write!(f, "{}", parts.join("+"))
    }
}

/// The canonical name of an egui key, if the shortcut accepts it.
fn key_name(key: egui::Key) -> Option<&'static str> {
    use egui::Key::*;
    Some(match key {
        Space => "Space",
        Enter => "Enter",
        Tab => "Tab",
        Backspace => "Backspace",
        Insert => "Insert",
        Delete => "Delete",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        ArrowLeft => "Left",
        ArrowUp => "Up",
        ArrowRight => "Right",
        ArrowDown => "Down",
        Comma => "Comma",
        Period => "Period",
        Slash => "Slash",
        Semicolon => "Semicolon",
        Quote => "Quote",
        Backtick => "Backquote",
        Minus => "Minus",
        Equals => "Equals",
        OpenBracket => "BracketLeft",
        CloseBracket => "BracketRight",
        Backslash => "Backslash",
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_ctrl_alt_r() {
        let h = Hotkey::default();
        assert_eq!(h.to_string(), "Ctrl+Alt+R");
        assert_eq!(Hotkey::parse("Ctrl+Alt+R"), Some(h.clone()));
        assert_eq!(h.vk(), Some(0x52));
        assert_eq!(h.keysym(), Some(0x72));
        // RegisterHotKey: MOD_ALT = 1, MOD_CONTROL = 2
        assert_eq!(h.win_modifiers(), 3);
    }

    #[test]
    fn round_trips_and_normalises() {
        for s in ["Ctrl+Alt+Space", "Ctrl+Shift+R", "Alt+F9", "Win+Shift+Z", "Ctrl+Alt+0", "F12", "Ctrl+Comma", "Ctrl+Alt+PageDown", "Shift+F24"] {
            let h = Hotkey::parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(Hotkey::parse(&h.to_string()), Some(h.clone()), "{s}");
        }
        assert_eq!(Hotkey::parse("ctrl + alt + space").unwrap().to_string(), "Ctrl+Alt+Space");
        assert_eq!(Hotkey::parse("alt+ctrl+r").unwrap().to_string(), "Ctrl+Alt+R");
        assert!(Hotkey::parse("super+x").unwrap().win);
        assert_eq!(Hotkey::parse("super+x").unwrap().to_string(), "Win+X");
    }

    #[test]
    fn rejects_nonsense() {
        for s in ["", "Ctrl", "Ctrl+Alt", "Space", "A", "Ctrl+A+B", "Ctrl+Foo", "Ctrl+F25", "Ctrl++"] {
            assert_eq!(Hotkey::parse(s), None, "{s}");
        }
        // an F key may stand alone
        assert!(Hotkey::parse("F9").is_some());
    }

    #[test]
    fn key_tables() {
        assert_eq!(Hotkey::parse("Ctrl+A").unwrap().vk(), Some(0x41));
        assert_eq!(Hotkey::parse("Ctrl+A").unwrap().keysym(), Some(0x61));
        assert_eq!(Hotkey::parse("Ctrl+5").unwrap().vk(), Some(0x35));
        assert_eq!(Hotkey::parse("F1").unwrap().vk(), Some(0x70));
        assert_eq!(Hotkey::parse("F12").unwrap().keysym(), Some(0xFFC9));
        assert_eq!(Hotkey::parse("Ctrl+Shift+Alt+Win+Q").unwrap().win_modifiers(), 15);
    }

    #[test]
    fn recorder_maps_egui_keys() {
        let mods = egui::Modifiers { ctrl: true, alt: true, ..Default::default() };
        assert_eq!(Hotkey::from_input(mods, egui::Key::Space).unwrap().to_string(), "Ctrl+Alt+Space");
        // a bare letter is refused
        assert_eq!(Hotkey::from_input(egui::Modifiers::NONE, egui::Key::A), None);
        // Escape is not a shortcut key (it cancels recording)
        assert_eq!(Hotkey::from_input(mods, egui::Key::Escape), None);
        assert_eq!(Hotkey::from_input(egui::Modifiers::NONE, egui::Key::F9).unwrap().to_string(), "F9");
    }
}
