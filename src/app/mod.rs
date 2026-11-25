//! The three window processes: the quick input (`--popup`), the finished timers (`--alerts`) and Settings
//! (`--settings`). They share the theme, the language and the screenshot driver defined here.

pub mod alerts;
pub mod popup;
pub mod settings;
pub mod widgets;

use crate::config::{self, Config};
use crate::i18n::{self, Lang};
use crate::theme::{self, Pal, ThemeMode};
use eframe::egui::{self, Color32, Rect, Stroke};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The app accent: lemon. Light theme uses the darker shade so text in the accent keeps its contrast.
pub const ACCENT: theme::Accent = theme::Accent::new(0xf5cf3f, 0x916400, 0x141415, 0xffffff);

/// Sets up the accent, the theme and the language for a window. Test instances may force them with
/// `REMINDO_LANG` / `REMINDO_THEME`, without touching the saved settings.
pub fn setup(cc: &eframe::CreationContext<'_>, cfg: &mut Config) -> Lang {
    if config::is_test_instance() {
        if let Ok(l) = std::env::var("REMINDO_LANG") {
            cfg.language = Some(l);
        }
        if let Some(m) = std::env::var("REMINDO_THEME").ok().and_then(|t| match t.as_str() {
            "dark" => Some(ThemeMode::Dark),
            "light" => Some(ThemeMode::Light),
            _ => None,
        }) {
            cfg.theme = m;
        }
    }
    theme::set_accent(ACCENT);
    theme::install(&cc.egui_ctx, cfg.theme);
    cc.egui_ctx.all_styles_mut(|s| s.spacing.interact_size.y = widgets::ROW_BTN_H);
    i18n::resolve(cfg.language.as_deref())
}

/// The window icon, from the same artwork that is embedded in the exe.
pub fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(crate::ICON_PNG).unwrap_or_default()
}

/// Is this run allowed to use the demo data and the screenshot driver? Only with a private data folder.
pub fn demo_allowed() -> bool {
    config::is_test_instance() && (std::env::args().any(|a| a == "--demo") || std::env::var("REMINDO_DEMO").is_ok_and(|v| v == "1"))
}

/// Paints the rounded surface of a floating window (the quick input, a finished timer) and returns its rectangle.
pub fn float_surface(ui: &egui::Ui, p: &Pal) -> Rect {
    let rect = ui.max_rect().shrink(1.0);
    ui.painter().rect(rect, theme::R_WINDOW as f32, p.card, Stroke::new(1.0, p.line_hi), egui::StrokeKind::Inside);
    rect
}

pub fn transparent() -> [f32; 4] {
    Color32::TRANSPARENT.to_normalized_gamma_f32()
}

/// Saves a PNG of the window after a few frames and closes it. Driven by `REMINDO_SHOT=<path.png>` in a demo run;
/// the window renders itself, so nothing is captured from the screen and no input is sent.
pub struct Shot {
    path: PathBuf,
    frames: u32,
    asked: bool,
    done: Arc<AtomicBool>,
}

impl Shot {
    pub fn from_env() -> Option<Shot> {
        if !demo_allowed() {
            return None;
        }
        let path = std::env::var_os("REMINDO_SHOT")?;
        Some(Shot { path: PathBuf::from(path), frames: 0, asked: false, done: Arc::new(AtomicBool::new(false)) })
    }

    pub fn drive(&mut self, ctx: &egui::Context) {
        ctx.request_repaint();
        if self.done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.frames += 1;
        if self.frames > 30 && !self.asked {
            self.asked = true;
            let path = self.path.clone();
            let done = self.done.clone();
            ctx.request_screenshot(move |img| {
                let mut raw = Vec::with_capacity(img.pixels.len() * 4);
                for px in &img.pixels {
                    raw.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
                }
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = image::save_buffer(path, &raw, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8);
                done.store(true, Ordering::Relaxed);
            });
        }
    }
}

/// Asks the daemon something from a window process, on the calling thread.
pub fn ask(msg: &str) -> Option<String> {
    crate::ipc::request("daemon", msg)
}

/// Sends a request on a background thread and ignores the answer (a button click that must not block the frame).
pub fn tell(msg: String) {
    std::thread::spawn(move || {
        let _ = crate::ipc::request("daemon", &msg);
    });
}

/// Where on the monitor a floating window goes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Centred, a little above the middle (the quick input).
    Center,
    /// Centred horizontally, in the upper part (the finished timers).
    Upper,
}

/// A first guess at the position of a `size` window on the monitor with the cursor, in egui points. On Windows the
/// exact spot is set right after creation, because only then is the monitor's scale known for sure.
pub fn initial_position(size: (f32, f32), anchor: Anchor) -> Option<egui::Pos2> {
    let a = crate::platform::cursor_area()?;
    let s = a.scale.max(0.5);
    let (aw, ah) = (a.w as f32 / s, a.h as f32 / s);
    let y = match anchor {
        Anchor::Center => (ah - size.1) * 0.38,
        Anchor::Upper => ah * 0.16,
    };
    Some(egui::pos2(a.x as f32 / s + (aw - size.0) / 2.0, a.y as f32 / s + y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_keeps_its_contrast_on_every_surface() {
        // Computed here, without the global accent (other tests set their own).
        for (name, pal, accent, on_accent) in [("dark", Pal::dark(), ACCENT.dark, ACCENT.on_dark), ("light", Pal::light(), ACCENT.light, ACCENT.on_light)] {
            for (surface, bg) in [("sidebar", pal.sidebar), ("bg", pal.bg), ("card", pal.card), ("raised", pal.raised), ("input", pal.input)] {
                let c = theme::contrast(accent, bg);
                assert!(c >= 4.5, "{name}: accent text on {surface} is {c:.2}");
            }
            let c = theme::contrast(on_accent, accent);
            assert!(c >= 4.5, "{name}: text on the filled accent is {c:.2}");
        }
    }
}
