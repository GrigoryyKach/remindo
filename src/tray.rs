//! Windows notification-area icon: New timer, Active timers, Settings, Support the author, Exit.
//!
//! The icon lives on the daemon's main thread (the one with the message loop). Other threads change it through
//! `update`, which stores the new state and posts a message; the loop then calls `apply_pending`.

use crate::tray_common::{labels, perform, Action, TrayState};
use std::cell::RefCell;
use std::sync::Mutex;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

struct Tray {
    icon: TrayIcon,
    new: MenuItem,
    active: MenuItem,
    settings: MenuItem,
    support: MenuItem,
    exit: MenuItem,
    last: Option<TrayState>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

static PENDING: Mutex<Option<TrayState>> = Mutex::new(None);

/// Creates the icon. Call on the message-loop thread.
pub fn create(state: &TrayState, icon_png: &[u8]) -> bool {
    let Ok(data) = eframe::icon_data::from_png_bytes(icon_png) else { return false };
    let Ok(icon_img) = Icon::from_rgba(data.rgba.clone(), data.width, data.height) else { return false };
    let (l_new, l_active, l_settings, l_support, l_exit) = labels(state);
    let new = MenuItem::new(l_new, true, None);
    let active = MenuItem::new(l_active, true, None);
    let settings = MenuItem::new(l_settings, true, None);
    let support_item = MenuItem::new(l_support, true, None);
    let exit = MenuItem::new(l_exit, true, None);
    let menu = Menu::new();
    if menu.append_items(&[&new, &active, &settings, &PredefinedMenuItem::separator(), &support_item, &exit]).is_err() {
        return false;
    }
    let Ok(icon) = TrayIconBuilder::new().with_icon(icon_img).with_tooltip(&state.tooltip).with_menu(Box::new(menu)).with_menu_on_left_click(false).build() else {
        return false;
    };
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
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            perform(Action::New);
        }
    }));
    TRAY.with(|t| {
        *t.borrow_mut() = Some(Tray { icon, new, active, settings, support: support_item, exit, last: Some(state.clone()) });
    });
    true
}

/// Stores a new state and asks the loop to draw it. Safe to call from any thread.
pub fn update(state: TrayState) {
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(state);
    }
    crate::winloop::request_tray_refresh();
}

/// Draws the state stored by `update`. Call on the message-loop thread.
pub fn apply_pending() {
    let Some(state) = PENDING.lock().ok().and_then(|mut p| p.take()) else { return };
    TRAY.with(|t| {
        let mut b = t.borrow_mut();
        let Some(tr) = b.as_mut() else { return };
        if tr.last.as_ref() == Some(&state) {
            return;
        }
        let (l_new, l_active, l_settings, l_support, l_exit) = labels(&state);
        tr.new.set_text(l_new);
        tr.active.set_text(l_active);
        tr.settings.set_text(l_settings);
        tr.support.set_text(l_support);
        tr.exit.set_text(l_exit);
        let _ = tr.icon.set_tooltip(Some(&state.tooltip));
        tr.last = Some(state);
    });
}

pub fn remove() {
    TRAY.with(|t| {
        if let Some(tr) = t.borrow().as_ref() {
            let _ = tr.icon.set_visible(false);
        }
        *t.borrow_mut() = None;
    });
}
