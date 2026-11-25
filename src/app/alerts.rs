//! The finished-timer window: one small topmost window that stacks a card for every timer that has fired.
//! It is shown without taking the focus from the program in front, so a game or a text field keeps its keys.
//! Close, Repeat and +5 min act on one timer; the window ends when the last one is gone.

use super::{ask, float_surface, tell, transparent, widgets, Shot};
use crate::bus::{self, Cmd};
use crate::config::Config;
use crate::i18n::Lang;
use crate::store::Alert;
use crate::strings::{self, S};
use crate::theme::{self, Btn, Pal};
use crate::{fmt, ipc};
#[cfg(windows)]
use crate::platform;
use eframe::egui::{self, Align2, Key, Modifiers, Rect, ScrollArea, Stroke, Ui, pos2, vec2};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const WIDTH: f32 = 440.0;
const CARD_H: f32 = 128.0;
const GAP: f32 = 8.0;
const PAD: f32 = 12.0;
const HEADER_H: f32 = 26.0;
const MAX_VISIBLE: usize = 4;

pub struct Alerts {
    lang: Lang,
    list: Arc<Mutex<Vec<Alert>>>,
    /// Ids closed here that the next poll may still report.
    gone: Vec<u64>,
    seen_any: bool,
    quiet_frames: u32,
    frames: u32,
    placed: bool,
    size: (f32, f32),
    demo: bool,
    shot: Option<Shot>,
}

fn demo_alerts(lang: Lang) -> Vec<Alert> {
    let now = crate::daemon::now_ms();
    let make = |id: u64, label: &str, ago_min: i64, secs: i64, missed: bool| Alert {
        id,
        label: label.to_string(),
        set_at: now - (ago_min + secs / 60) * 60_000,
        fire_at: now - ago_min * 60_000,
        fired_at: now,
        secs,
        missed,
        clock: None,
    };
    vec![
        make(1, lang.pick("Turn off the stove", "Выключить плиту", "Вимкнути плиту"), 0, 900, false),
        make(2, lang.pick("Call the dentist", "Позвонить в стоматологию", "Зателефонувати до стоматології"), 25, 3600, true),
    ]
}

impl Alerts {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config) -> Self {
        let lang = super::setup(cc, &mut cfg);
        bus::init(cc.egui_ctx.clone());
        let demo = super::demo_allowed();
        let list = Arc::new(Mutex::new(if demo { demo_alerts(lang) } else { Vec::new() }));
        if !demo {
            // Poll the daemon for the timers that are waiting; also done once right now.
            let (l, ctx) = (list.clone(), cc.egui_ctx.clone());
            std::thread::Builder::new()
                .name("remindo-alerts-poll".into())
                .spawn(move || loop {
                    if let Some(reply) = ask("alerts") {
                        if let Ok(v) = serde_json::from_str::<Vec<Alert>>(&reply) {
                            if let Ok(mut g) = l.lock() {
                                *g = v;
                            }
                            ctx.request_repaint();
                        }
                    } else {
                        // The daemon is gone: nothing more will fire.
                        if let Ok(mut g) = l.lock() {
                            g.clear();
                        }
                        ctx.request_repaint();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(400));
                })
                .ok();
        }
        Alerts { lang, list, gone: Vec::new(), seen_any: demo, quiet_frames: 0, frames: 0, placed: false, size: (WIDTH, 0.0), demo, shot: Shot::from_env() }
    }

    fn s(&self) -> &'static S {
        strings::TABLE.get(self.lang)
    }

    fn visible(&self) -> Vec<Alert> {
        let g = self.list.lock().map(|g| g.clone()).unwrap_or_default();
        g.into_iter().filter(|a| !self.gone.contains(&a.id)).collect()
    }

    fn act(&mut self, id: u64, cmd: &str) {
        self.gone.push(id);
        if !self.demo {
            tell(format!("{cmd} {id}"));
        }
    }

    /// The window height for `n` cards.
    fn height_for(n: usize) -> f32 {
        let shown = n.min(MAX_VISIBLE).max(1) as f32;
        let header = if n >= 2 { HEADER_H } else { 0.0 };
        PAD * 2.0 + header + shown * CARD_H + (shown - 1.0) * GAP
    }

    fn card(&mut self, ui: &mut Ui, p: &Pal, a: &Alert) {
        let t = self.s();
        let rect = ui.available_rect_before_wrap();
        let rect = Rect::from_min_size(rect.min, vec2(rect.width(), CARD_H));
        ui.painter().rect(rect, theme::R_CARD as f32, p.bg, Stroke::new(1.0, p.line), egui::StrokeKind::Inside);
        let inner = rect.shrink2(vec2(16.0, 12.0));
        // title line
        widgets::icon(ui.painter(), widgets::Icon::Bell, pos2(inner.left() + 8.0, inner.top() + 8.0), 16.0, p.accent);
        ui.painter().text(pos2(inner.left() + 24.0, inner.top() + 8.0), Align2::LEFT_CENTER, t.fire_title, theme::medium(12.5), p.accent);
        // the label, big
        let font = theme::medium(22.0);
        let label = widgets::fit_text(ui.painter(), &a.label, &font, inner.width());
        ui.painter().text(pos2(inner.left(), inner.top() + 36.0), Align2::LEFT_CENTER, label, font, p.text);
        // the line under it
        let set = if a.clock.is_some() {
            t.fire_set_clock.replace("{t}", &fmt::hm(a.set_at)).replace("{c}", &fmt::hm(a.fire_at))
        } else {
            t.fire_set.replace("{t}", &fmt::hm(a.set_at)).replace("{d}", &fmt::dur(a.secs, self.lang))
        };
        let sub = if a.missed { format!("{set}  ·  {}", t.fire_missed.replace("{t}", &fmt::hm(a.fire_at))) } else { set };
        let sub = widgets::fit_text(ui.painter(), &sub, &theme::reg(13.0), inner.width());
        ui.painter().text(pos2(inner.left(), inner.top() + 62.0), Align2::LEFT_CENTER, sub, theme::reg(13.0), if a.missed { p.warning } else { p.text2 });
        // buttons
        let row = Rect::from_min_size(pos2(inner.left(), inner.bottom() - 32.0), vec2(inner.width(), 32.0));
        let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row_ui.spacing_mut().item_spacing.x = 8.0;
        row_ui.push_id(a.id, |ui| {
            if widgets::btn(ui, t.close, Btn::Primary, p).clicked() {
                self.act(a.id, "dismiss");
            }
            if widgets::btn(ui, t.repeat, Btn::Secondary, p).clicked() {
                self.act(a.id, "repeat");
            }
            if widgets::btn(ui, t.snooze5, Btn::Secondary, p).clicked() {
                self.act(a.id, "snooze");
            }
        });
        ui.allocate_rect(rect, egui::Sense::hover());
    }
}

impl eframe::App for Alerts {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        transparent()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        for cmd in bus::drain() {
            if cmd == Cmd::Close {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        let n = self.visible().len();
        if n > 0 {
            self.seen_any = true;
            self.quiet_frames = 0;
        } else if self.shot.is_none() {
            // Nothing left (all closed, or nothing was ever pending): end after a short grace period.
            self.quiet_frames += 1;
            if self.quiet_frames > if self.seen_any { 2 } else { 40 } {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // Size to the stack. The top edge stays, the window grows downwards.
        if n > 0 {
            let want = (WIDTH, Self::height_for(n));
            if self.size != want {
                self.size = want;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(want.0, want.1)));
            }
            #[cfg(windows)]
            if !self.placed || self.frames < 6 {
                if let Some(h) = platform::own_window() {
                    if !self.placed {
                        platform::place(h, (want.0, want.1), super::Anchor::Upper, false);
                        self.placed = true;
                    }
                }
            }
        }
        if let Some(s) = &mut self.shot {
            s.drive(ctx);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = Pal::of(ui.visuals().dark_mode);
        let t = self.s();
        let outer = float_surface(ui, &p);
        let list = self.visible();
        // Escape and Enter close the first card when the window has the keyboard.
        if let Some(first) = list.first() {
            let esc = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
            if esc || enter {
                self.act(first.id, "dismiss");
            }
        }
        let inner = outer.shrink(PAD - 1.0);
        let mut top = inner.top();
        if list.len() >= 2 {
            let r = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), HEADER_H - 4.0));
            let mut hui = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if widgets::btn(&mut hui, t.close_all, Btn::Link, &p).clicked() {
                let ids: Vec<u64> = list.iter().map(|a| a.id).collect();
                self.gone.extend(ids);
                if !self.demo {
                    tell("dismiss_all".into());
                }
            }
            top += HEADER_H;
        }
        let area = Rect::from_min_max(pos2(inner.left(), top), inner.max);
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
        cui.spacing_mut().item_spacing.y = GAP;
        ScrollArea::vertical().auto_shrink([false, false]).show(&mut cui, |ui| {
            ui.spacing_mut().item_spacing.y = GAP;
            for a in &list {
                self.card(ui, &p, a);
            }
        });
    }
}

/// Starts the finished-timer window (one per data folder).
pub fn run(dir: PathBuf) -> eframe::Result<()> {
    let demo = super::demo_allowed();
    if !demo {
        let served = ipc::serve("alerts", |m| {
            if m == "close" {
                bus::push(Cmd::Close);
            }
            "pong".into()
        });
        if !served {
            return Ok(());
        }
    }
    let cfg = Config::load_from(&dir);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Remindo")
        .with_app_id("local.remindo.alerts")
        .with_inner_size([WIDTH, Alerts::height_for(1)])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        // The window must not take the keyboard from the program in front (a game, a text field).
        .with_active(false)
        .with_icon(std::sync::Arc::new(super::app_icon()));
    let pos = super::initial_position((WIDTH, Alerts::height_for(1)), super::Anchor::Upper);
    let viewport = match pos {
        Some(p) => viewport.with_position(p),
        None => viewport,
    };
    let options = eframe::NativeOptions { viewport, centered: pos.is_none(), ..Default::default() };
    eframe::run_native("Remindo", options, Box::new(move |cc| Ok(Box::new(Alerts::new(cc, cfg)))))
}
