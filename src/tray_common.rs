//! What the tray menu does, and the state the daemon hands to the tray. Shared by the Windows and Linux trays.

use crate::i18n::Lang;

/// Everything the tray needs to draw itself.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayState {
    pub lang: Lang,
    pub tooltip: String,
    /// The shortcut as text, shown next to "New timer".
    pub hotkey: String,
    /// Active timers (the menu says "Active timers (3)").
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Settings,
    Support,
    Exit,
}

pub fn perform(a: Action) {
    match a {
        Action::New => crate::daemon::open_popup(),
        Action::Settings => crate::daemon::open_settings(),
        Action::Support => crate::support::open(),
        Action::Exit => crate::daemon::request_quit(),
    }
}

/// Menu labels for a state.
pub fn labels(s: &TrayState) -> (String, String, &'static str, &'static str, &'static str) {
    let t = crate::strings::TABLE.get(s.lang);
    let new = if s.hotkey.is_empty() { t.tray_new.to_string() } else { format!("{}    {}", t.tray_new, s.hotkey) };
    let active = if s.count == 0 { t.tray_active.to_string() } else { format!("{} ({})", t.tray_active, s.count) };
    (new, active, t.tray_settings, crate::support::tray_label(s.lang), t.tray_exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_show_the_shortcut_and_the_count() {
        let s = TrayState { lang: Lang::En, tooltip: String::new(), hotkey: "Ctrl+Alt+Space".into(), count: 3 };
        let (new, active, settings, support, exit) = labels(&s);
        assert_eq!(new, "New timer    Ctrl+Alt+Space");
        assert_eq!(active, "Active timers (3)");
        assert_eq!((settings, exit), ("Settings", "Exit"));
        assert!(support.contains('♥'));
        let s = TrayState { lang: Lang::Ru, tooltip: String::new(), hotkey: String::new(), count: 0 };
        let (new, active, ..) = labels(&s);
        assert_eq!((new.as_str(), active.as_str()), ("Новый таймер", "Активные таймеры"));
    }
}
