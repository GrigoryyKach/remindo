//! The quick input: a small borderless window opened by the hotkey. One line to type in, a live preview of what it
//! understood, Enter to start. Escape or a click outside closes it.

use super::{ask, float_surface, transparent, widgets, Shot};
use crate::bus::{self, Cmd};
use crate::config::{self, Config};
use crate::i18n::Lang;
use crate::strings::{self, S};
use crate::theme::{self, Btn, Pal};
use crate::{fmt, ipc, parse};
#[cfg(windows)]
use crate::platform;
use eframe::egui::{self, Align2, Key, Margin, Modifiers, Rect, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};
use std::path::PathBuf;

pub const SIZE: (f32, f32) = (520.0, 126.0);

pub struct Popup {
    lang: Lang,
    dir: PathBuf,
    text: String,
    last: Option<String>,
    /// The daemon refused the text (or could not be reached): a hint code or message.
    error: Option<String>,
    frames: u32,
    had_focus: bool,
    demo: bool,
    shot: Option<Shot>,
    placed_at: Option<(i32, i32, i32, i32)>,
}

fn last_path(dir: &std::path::Path) -> PathBuf {
    dir.join("last_input.txt")
}

impl Popup {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config, dir: PathBuf) -> Self {
        let lang = super::setup(cc, &mut cfg);
        bus::init(cc.egui_ctx.clone());
        let demo = super::demo_allowed();
        let last = std::fs::read_to_string(last_path(&dir)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let mut text = String::new();
        if demo {
            text = std::env::var("REMINDO_SHOT_TEXT").unwrap_or_else(|_| lang.pick("in 15 minutes turn off the stove", "через 15 минут выключить плиту", "через 15 хвилин вимкнути плиту").to_string());
        }
        Popup { lang, dir, text, last, error: None, frames: 0, had_focus: false, demo, shot: Shot::from_env(), placed_at: None }
    }

    fn s(&self) -> &'static S {
        strings::TABLE.get(self.lang)
    }

    /// Sends the text to the daemon. Returns whether the popup should close.
    fn submit(&mut self) -> bool {
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return false;
        }
        let t = self.s();
        if self.demo {
            return true;
        }
        let Some(reply) = ask(&format!("text {text}")) else {
            self.error = Some(t.daemon_down.to_string());
            return false;
        };
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
        if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
            let _ = config::write_atomic(&last_path(&self.dir), text.as_bytes());
            return true;
        }
        self.error = Some(
            match v.get("err").and_then(|e| e.as_str()).unwrap_or("") {
                "no_time" | "empty" => t.hint_no_time,
                "no_unit" => t.hint_no_unit,
                "zero" => t.hint_zero,
                "bad_clock" => t.hint_bad_clock,
                "too_many" => t.too_many,
                _ => t.add_failed,
            }
            .to_string(),
        );
        false
    }

    /// The preview line: what the text means, or why it does not parse yet. `(text, is_warning)`.
    fn preview(&self) -> (String, bool, bool) {
        let t = self.s();
        if let Some(e) = &self.error {
            return (e.clone(), true, false);
        }
        let now = chrono::Local::now().naive_local();
        match parse::parse(&self.text, now, self.lang) {
            Ok(p) => {
                let when = fmt::when_naive(p.fire_at, now, p.lang);
                let label = p.label_or_default();
                let len = fmt::dur(p.secs, self.lang);
                let mut line = if p.absolute { format!("{when}  ·  «{label}»  ·  {}", t.in_time.replace("{d}", &len)) } else { format!("{len}  ·  «{label}»  ·  {when}") };
                let mut warn = false;
                for n in &p.notes {
                    match n {
                        parse::Note::ClampedMax => {
                            line.push_str(&format!("  ·  {}", t.hint_clamped));
                            warn = true;
                        }
                        parse::Note::IgnoredSecond(s) => {
                            line.push_str(&format!("  ·  {}", t.hint_ignored.replace("{t}", s)));
                            warn = true;
                        }
                        parse::Note::Tomorrow => {}
                    }
                }
                (line, warn, true)
            }
            Err(parse::Error::Empty) | Err(parse::Error::NoTime) => (t.hint_no_time.to_string(), false, false),
            Err(parse::Error::NoUnit) => (t.hint_no_unit.to_string(), true, false),
            Err(parse::Error::Zero) => (t.hint_zero.to_string(), true, false),
            Err(parse::Error::BadClock) => (t.hint_bad_clock.to_string(), true, false),
        }
    }
}

impl eframe::App for Popup {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        transparent()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        for cmd in bus::drain() {
            match cmd {
                Cmd::Focus => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    #[cfg(windows)]
                    if let Some(h) = platform::own_window() {
                        platform::force_foreground(h);
                    }
                }
                Cmd::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        // Put the window on the monitor with the cursor and take the keyboard, in the first frames.
        #[cfg(windows)]
        if self.frames <= 40 {
            if let Some(h) = platform::own_window() {
                if self.placed_at.is_none() {
                    platform::place(h, SIZE, super::Anchor::Center, true);
                    self.placed_at = platform::window_rect(h);
                }
                if !self.had_focus {
                    platform::force_foreground(h);
                }
            }
        }
        let focused = ctx.input(|i| i.viewport().focused);
        if focused == Some(true) {
            self.had_focus = true;
        }
        // A click outside (focus lost after it had focus) closes the input without a timer.
        if self.shot.is_none() && !self.demo && self.had_focus && focused == Some(false) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(s) = &mut self.shot {
            s.drive(ctx);
        }
        if self.frames < 12 {
            ctx.request_repaint();
        }
        // Keep the preview's clock fresh ("tomorrow at ...") while the window sits there.
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = Pal::of(ui.visuals().dark_mode);
        let t = self.s();
        let outer = float_surface(ui, &p);

        // Keys first, so the text field does not eat them.
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            if let Some(l) = &self.last {
                self.text = l.clone();
                self.error = None;
            }
        }
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));

        let inner = outer.shrink(13.0);
        // The input field.
        let field = Rect::from_min_size(inner.min, vec2(inner.width(), 40.0));
        ui.painter().rect(field, 10.0, p.input, Stroke::new(1.0, p.line_hi), StrokeKind::Inside);
        widgets::icon(ui.painter(), widgets::Icon::Clock, pos2(field.left() + 20.0, field.center().y), 18.0, p.accent);
        let edit_rect = Rect::from_min_max(pos2(field.left() + 38.0, field.top() + 2.0), pos2(field.right() - 10.0, field.bottom() - 2.0));
        let before = self.text.clone();
        let mut edit_ui = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let resp = edit_ui.add(
            TextEdit::singleline(&mut self.text)
                .id_salt("popup-input")
                .hint_text(egui::RichText::new(t.placeholder).color(p.muted))
                .font(theme::reg(18.0))
                .text_color(p.text)
                .desired_width(edit_rect.width())
                .frame(egui::Frame::NONE)
                .margin(Margin::symmetric(0, 8)),
        );
        if self.frames < 30 || !resp.has_focus() && ctx.input(|i| i.focused) {
            resp.request_focus();
        }
        if self.text != before {
            self.error = None;
        }

        // The preview line.
        let (line, warn, ok) = self.preview();
        let prev = Rect::from_min_size(pos2(inner.left() + 4.0, field.bottom() + 8.0), vec2(inner.width() - 8.0, 18.0));
        let col = if warn { p.warning } else if ok { p.text2 } else { p.muted };
        let font = theme::reg(13.5);
        let shown = widgets::fit_text(ui.painter(), &line, &font, prev.width());
        ui.painter().text(prev.left_center(), Align2::LEFT_CENTER, shown, font, col);

        // The bottom row: key hints on the left, the Start button on the right.
        let row = Rect::from_min_size(pos2(inner.left(), inner.bottom() - 28.0), vec2(inner.width(), 28.0));
        let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row_ui.spacing_mut().item_spacing.x = 6.0;
        theme::keycap(&mut row_ui, "Enter", &p);
        theme::hint(&mut row_ui, t.key_start, &p);
        row_ui.add_space(8.0);
        theme::keycap(&mut row_ui, "Esc", &p);
        theme::hint(&mut row_ui, t.key_close, &p);
        if self.last.is_some() {
            row_ui.add_space(8.0);
            theme::keycap(&mut row_ui, "↑", &p);
            theme::hint(&mut row_ui, t.key_recall, &p);
        }
        let mut start_ui = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let start = widgets::btn(&mut start_ui, t.start, Btn::Primary, &p).clicked();

        if enter || start {
            if self.submit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

/// Starts the quick input window. Another one already open is told to come forward instead.
pub fn run(dir: PathBuf) -> eframe::Result<()> {
    let demo = super::demo_allowed();
    if !demo {
        let served = ipc::serve("popup", |m| {
            match m {
                "focus" => bus::push(Cmd::Focus),
                "close" => bus::push(Cmd::Close),
                _ => {}
            }
            "ok".into()
        });
        if !served {
            ipc::request("popup", "focus");
            return Ok(());
        }
    }
    let cfg = Config::load_from(&dir);
    let pos = super::initial_position(SIZE, super::Anchor::Center);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Remindo")
        .with_app_id("local.remindo.popup")
        .with_inner_size([SIZE.0, SIZE.1])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_icon(std::sync::Arc::new(super::app_icon()));
    if let Some(p) = pos {
        viewport = viewport.with_position(p);
    }
    let options = eframe::NativeOptions { viewport, centered: pos.is_none(), ..Default::default() };
    eframe::run_native("Remindo", options, Box::new(move |cc| Ok(Box::new(Popup::new(cc, cfg, dir)))))
}
