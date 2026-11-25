//! Commands sent to a window from other threads (a second hotkey press asking it to come forward).

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Bring the window forward and focus it.
    Focus,
    /// Close the window.
    Close,
}

static QUEUE: Mutex<Vec<Cmd>> = Mutex::new(Vec::new());
static CTX: OnceLock<eframe::egui::Context> = OnceLock::new();

/// Gives the bus the context so a pushed command wakes the window. Commands pushed earlier are kept.
pub fn init(ctx: eframe::egui::Context) {
    let _ = CTX.set(ctx);
}

/// Queues a command and wakes the window if it exists.
pub fn push(cmd: Cmd) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(cmd);
    }
    if let Some(c) = CTX.get() {
        c.request_repaint();
    }
}

pub fn drain() -> Vec<Cmd> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}
