//! The Settings window: active timers (live), the history, the shortcut, sound, startup, theme, language, support
//! and About. It is a client of the daemon: it asks for the state every second and sends requests back.

use super::{ask, tell, widgets, Shot};
use crate::bus::{self, Cmd};
use crate::config::{self, Config, Window};
use crate::hotkey::Hotkey;
use crate::i18n::{self, Lang};
use crate::nimvoro::{self, Updater, UpdateState};
use crate::store::{HistoryItem, Timer};
use crate::strings::{self, S};
use crate::theme::{self, Btn, Pal};
use crate::{daemon, fmt, ipc, platform, support, update_ui};
use eframe::egui::{self, Align, Frame, Key, Layout, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Nav {
    Timers,
    History,
    Settings,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct DState {
    timers: Vec<Timer>,
    history: Vec<HistoryItem>,
    hotkey: String,
    hotkey_error: Option<String>,
    notices: Vec<String>,
}

#[derive(Default)]
struct Snap {
    /// The daemon answered the last request.
    reachable: bool,
    state: DState,
}

pub struct Settings {
    cfg: Config,
    lang: Lang,
    dir: PathBuf,
    nav: Nav,
    snap: Arc<Mutex<Snap>>,
    updater: Updater,
    autostart: bool,
    autostart_failed: bool,
    /// The timer being edited and its text.
    editing: Option<(u64, String)>,
    recording: bool,
    /// The result of the last shortcut change: (message, success).
    hk_msg: Option<(String, bool)>,
    hk_result: Arc<Mutex<Option<(String, bool)>>>,
    dismissed: usize,
    demo: bool,
    shot: Option<Shot>,
    frames: u32,
}

fn demo_state(lang: Lang) -> DState {
    let now = daemon::now_ms();
    let timer = |id: u64, label: &str, left_s: i64, secs: i64, clock: Option<(u32, u32)>| Timer { id, label: label.to_string(), set_at: now - (secs - left_s) * 1000, fire_at: now + left_s * 1000, secs, clock };
    let timers = vec![
        timer(1, lang.pick("Turn off the stove", "Выключить плиту", "Вимкнути плиту"), 12 * 60 + 41, 15 * 60, None),
        timer(2, lang.pick("Team call", "Созвон с командой", "Дзвінок із командою"), 3 * 3600 + 20 * 60, 4 * 3600, None),
        timer(3, lang.pick("Tea", "Чай", "Чай"), 2 * 60 + 5, 5 * 60, None),
    ];
    let hist = |label: &str, secs: i64, clock: Option<(u32, u32)>, ago: i64| HistoryItem { label: label.to_string(), secs, clock, at: now - ago * 60_000 };
    let history = vec![
        hist(lang.pick("Turn off the stove", "Выключить плиту", "Вимкнути плиту"), 900, None, 2),
        hist(lang.pick("Tea", "Чай", "Чай"), 300, None, 6),
        hist(lang.pick("Team call", "Созвон с командой", "Дзвінок із командою"), 14_400, Some((18, 30)), 9),
        hist(lang.pick("Stretch", "Размяться", "Розім'ятися"), 2700, None, 70),
        hist(lang.pick("Take the pills", "Выпить таблетки", "Випити таблетки"), 28_800, None, 180),
        hist(lang.pick("Water the plants", "Полить цветы", "Полити квіти"), 3600, None, 1500),
    ];
    DState { timers, history, hotkey: "Ctrl+Alt+R".into(), hotkey_error: None, notices: Vec::new() }
}

impl Settings {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config, dir: PathBuf) -> Self {
        let lang = super::setup(cc, &mut cfg);
        bus::init(cc.egui_ctx.clone());
        let demo = super::demo_allowed();
        let mut cfg_dirty = false;
        if cfg.language.as_deref() != Some(lang.code()) {
            cfg.language = Some(lang.code().to_string());
            cfg_dirty = true;
        }
        let snap = Arc::new(Mutex::new(Snap { reachable: demo, state: if demo { demo_state(lang) } else { DState::default() } }));
        if !demo {
            let (s, ctx) = (snap.clone(), cc.egui_ctx.clone());
            std::thread::Builder::new()
                .name("remindo-settings-poll".into())
                .spawn(move || loop {
                    let reply = ask("state");
                    if let Ok(mut g) = s.lock() {
                        match reply.and_then(|r| serde_json::from_str::<DState>(&r).ok()) {
                            Some(st) => {
                                g.reachable = true;
                                g.state = st;
                            }
                            None => g.reachable = false,
                        }
                    }
                    ctx.request_repaint();
                    std::thread::sleep(Duration::from_millis(1000));
                })
                .ok();
        }
        let updater = Updater::new("remindo", crate::REPO_URL.trim_start_matches("https://github.com/"), env!("CARGO_PKG_VERSION")).relaunch_args(&["--background"]).before_restart(stop_daemon);
        if cfg.check_updates && !config::is_test_instance() {
            let ctx = cc.egui_ctx.clone();
            updater.check(move || ctx.request_repaint());
        }
        let nav = if config::is_test_instance() {
            match std::env::var("REMINDO_NAV").as_deref() {
                Ok("history") => Nav::History,
                Ok("settings") => Nav::Settings,
                _ => Nav::Timers,
            }
        } else {
            Nav::Timers
        };
        let me = Settings {
            cfg,
            lang,
            dir,
            nav,
            snap,
            updater,
            autostart: platform::autostart_enabled(),
            autostart_failed: false,
            editing: None,
            recording: false,
            hk_msg: None,
            hk_result: Arc::new(Mutex::new(None)),
            dismissed: 0,
            demo,
            shot: Shot::from_env(),
            frames: 0,
        };
        if cfg_dirty && !demo {
            me.cfg.save_to(&me.dir);
        }
        me
    }

    fn s(&self) -> &'static S {
        strings::TABLE.get(self.lang)
    }

    fn state(&self) -> (bool, DState) {
        self.snap.lock().map(|g| (g.reachable, g.state.clone())).unwrap_or_default()
    }

    fn save_cfg(&self) {
        if self.demo {
            return;
        }
        self.cfg.save_to(&self.dir);
        tell("reload".into());
    }
}

fn stop_daemon() {
    if ipc::request("daemon", "quit").is_some() {
        for _ in 0..30 {
            if !ipc::is_running("daemon") {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

// ───────────────────────── small layout helpers ─────────────────────────

fn group<R>(ui: &mut Ui, p: &Pal, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(24.0);
    theme::section(ui, title, p);
    ui.add_space(8.0);
    theme::card(p)
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn switch_row(ui: &mut Ui, p: &Pal, title: &str, hint: &str, value: &mut bool) -> bool {
    let mut changed = false;
    let w = ui.available_width();
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(w - 56.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).font(theme::reg(14.0)).color(p.text));
            if !hint.is_empty() {
                theme::hint(ui, hint, p);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed = theme::switch(ui, value, p).changed();
        });
    });
    ui.add_space(8.0);
    changed
}

fn combo_row<T: PartialEq + Copy>(ui: &mut Ui, p: &Pal, title: &str, id: &str, current: T, options: &[(T, String)], width: f32) -> Option<T> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::reg(14.0)).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = options.iter().find(|o| o.0 == current).map(|o| o.1.as_str()).unwrap_or("");
            widgets::combo(id, label, width).show_ui(ui, |ui| {
                for (v, name) in options {
                    if ui.selectable_label(*v == current, RichText::new(name).font(theme::reg(13.5))).clicked() {
                        picked = Some(*v);
                    }
                }
            });
        });
    });
    ui.add_space(8.0);
    picked
}

/// A page: a 24 px title, an optional right-aligned header, then a scrolling body in a column no wider than 820 px.
fn page(ui: &mut Ui, p: &Pal, title: &str, subtitle: Option<&str>, right: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) {
    let avail = ui.available_width();
    let avail_h = ui.available_height();
    let col_w = (avail - 14.0).clamp(300.0, 820.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(((avail - 14.0 - col_w) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(col_w);
            ui.set_height(avail_h);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(title).font(theme::medium(24.0)).color(p.text));
                    if let Some(s) = subtitle {
                        theme::hint(ui, s, p);
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), right);
            });
            ui.add_space(16.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(col_w - 12.0);
                body(ui);
                ui.add_space(24.0);
            });
        });
    });
}

fn nav_row(ui: &mut Ui, p: &Pal, ic: widgets::Icon, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if selected {
        ui.painter().rect(rect, theme::R_CONTROL as f32, widgets::selected_fill(p), Stroke::new(1.0, p.line_hi), StrokeKind::Inside);
        ui.painter().hline(rect.x_range().shrink(8.0), rect.top() + 0.5, Stroke::new(1.0, p.top_light()));
    } else {
        ui.painter().rect_filled(rect, theme::R_CONTROL as f32, widgets::hover_fill(ui, resp.id, resp.hovered(), p));
    }
    let col = if selected { p.text } else { p.text2 };
    widgets::icon(ui.painter(), ic, pos2(rect.left() + 20.0, rect.center().y), 16.0, if selected { p.text } else { p.muted });
    let font = if selected { theme::medium(14.0) } else { theme::reg(14.0) };
    ui.painter().text(pos2(rect.left() + 36.0, rect.center().y), egui::Align2::LEFT_CENTER, label, font, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl Settings {
    fn sidebar(&mut self, ui: &mut Ui, p: &Pal, reachable: bool, count: usize) {
        let t = self.s();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (mark, _) = ui.allocate_exact_size(vec2(28.0, 36.0), Sense::hover());
            widgets::mark_glyph(ui.painter(), mark.center(), p.accent);
            ui.label(RichText::new("Remindo").font(theme::medium(18.0)).color(p.text));
        });
        ui.add_space(16.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let label = if count > 0 { format!("{}  ({count})", t.nav_timers) } else { t.nav_timers.to_string() };
        if nav_row(ui, p, widgets::Icon::Clock, &label, self.nav == Nav::Timers).clicked() {
            self.nav = Nav::Timers;
        }
        if nav_row(ui, p, widgets::Icon::Repeat, t.nav_history, self.nav == Nav::History).clicked() {
            self.nav = Nav::History;
        }
        let h = ui.available_height();
        ui.add_space((h - 36.0 - 40.0).max(8.0));
        let (color, text) = if reachable { (p.positive, "") } else { (p.negative, t.daemon_off) };
        let _ = text;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
        ui.painter().circle_filled(pos2(rect.left() + 20.0, rect.center().y), 4.0, color);
        let status = if reachable { self.lang.pick("Running in the tray", "Работает в трее", "Працює в треї") } else { self.lang.pick("Not running", "Не запущен", "Не запущено") };
        ui.painter().text(pos2(rect.left() + 36.0, rect.center().y), egui::Align2::LEFT_CENTER, status, theme::reg(12.0), p.text2);
        if nav_row(ui, p, widgets::Icon::Gear, t.nav_settings, self.nav == Nav::Settings).clicked() {
            self.nav = Nav::Settings;
        }
    }

    // ───────────────────────── timers ─────────────────────────

    fn timers_view(&mut self, ui: &mut Ui, p: &Pal, st: &DState, reachable: bool) {
        let t = self.s();
        let lang = self.lang;
        let mut open_new = false;
        let hotkey = if st.hotkey.is_empty() { self.cfg.hotkey.clone() } else { st.hotkey.clone() };
        page(
            ui,
            p,
            t.active_timers,
            None,
            |ui| {
                if widgets::btn(ui, t.new_timer, Btn::Primary, p).clicked() {
                    open_new = true;
                }
            },
            |ui| {
                self.banners(ui, p, st, reachable);
                if st.timers.is_empty() {
                    theme::card(p).inner_margin(Margin::symmetric(24, 28)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            widgets::icon(ui.painter(), widgets::Icon::Clock, pos2(ui.max_rect().center().x, ui.cursor().top() + 14.0), 28.0, p.muted);
                            ui.add_space(36.0);
                            ui.label(RichText::new(t.no_timers).font(theme::medium(16.0)).color(p.text));
                            ui.add_space(4.0);
                            theme::hint(ui, t.no_timers_hint, p);
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let parts = Hotkey::parse(&hotkey).map(|h| h.parts()).unwrap_or_default();
                                let w: f32 = parts.iter().map(|s| s.chars().count() as f32 * 8.0 + 30.0).sum();
                                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                                for part in &parts {
                                    theme::keycap(ui, part, p);
                                }
                            });
                        });
                    });
                    return;
                }
                let mut timers = st.timers.clone();
                timers.sort_by_key(|x| (x.fire_at, x.id));
                let now = daemon::now_ms();
                for tm in &timers {
                    ui.add_space(8.0);
                    self.timer_row(ui, p, tm, now, lang);
                }
            },
        );
        if open_new {
            tell("new".into());
        }
    }

    fn timer_row(&mut self, ui: &mut Ui, p: &Pal, tm: &Timer, now: i64, lang: Lang) {
        let t = self.s();
        let editing = self.editing.as_ref().is_some_and(|e| e.0 == tm.id);
        let h = if editing { 84.0 } else { 64.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        ui.painter().rect(rect, theme::R_CARD as f32, p.card, Stroke::new(1.0, p.line), StrokeKind::Inside);
        let inner = rect.shrink2(vec2(16.0, 10.0));
        if editing {
            let mut action = None;
            let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(inner.min, vec2(inner.width(), 32.0))).layout(Layout::left_to_right(Align::Center)));
            row_ui.spacing_mut().item_spacing.x = 8.0;
            let Some((_, text)) = self.editing.as_mut() else { return };
            let resp = widgets::input(&mut row_ui, text, "", inner.width() - 200.0);
            resp.request_focus_if_new(&row_ui);
            let enter = resp.lost_focus() && row_ui.input(|i| i.key_pressed(Key::Enter));
            if widgets::btn(&mut row_ui, t.save, Btn::Primary, p).clicked() || enter {
                action = Some(true);
            }
            if widgets::btn(&mut row_ui, t.cancel, Btn::Secondary, p).clicked() || row_ui.input(|i| i.key_pressed(Key::Escape)) {
                action = Some(false);
            }
            ui.painter().text(pos2(inner.left(), inner.bottom() - 6.0), egui::Align2::LEFT_CENTER, t.edit_hint, theme::reg(12.0), p.muted);
            match action {
                Some(true) => {
                    if let Some((id, text)) = self.editing.take() {
                        if !self.demo {
                            let ctx = ui.ctx().clone();
                            std::thread::spawn(move || {
                                let _ = ask(&format!("edit {id} {}", text.trim()));
                                ctx.request_repaint();
                            });
                        }
                    }
                }
                Some(false) => self.editing = None,
                None => {}
            }
            return;
        }
        let left_s = ((tm.fire_at - now) as f64 / 1000.0).ceil() as i64;
        // label and the line under it
        let label = widgets::fit_text(ui.painter(), &tm.label, &theme::medium(15.0), inner.width() - 330.0);
        ui.painter().text(pos2(inner.left(), inner.top() + 12.0), egui::Align2::LEFT_CENTER, label, theme::medium(15.0), p.text);
        let sub = format!("{}  ·  {}", fmt::when(tm.fire_at, now, lang), lang.pick("set for", "на", "на").to_string() + " " + &fmt::dur(tm.secs, lang));
        ui.painter().text(pos2(inner.left(), inner.top() + 33.0), egui::Align2::LEFT_CENTER, sub, theme::reg(12.5), p.text2);
        // right side: buttons, then the countdown
        let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)));
        right_ui.spacing_mut().item_spacing.x = 6.0;
        if widgets::icon_button(&mut right_ui, widgets::Icon::Trash, 32.0, p).on_hover_text(t.cancel).clicked() {
            self.timer_cmd(format!("cancel {}", tm.id));
        }
        if widgets::icon_button(&mut right_ui, widgets::Icon::Edit, 32.0, p).on_hover_text(t.edit).clicked() {
            self.editing = Some((tm.id, tm.label.clone()));
        }
        if widgets::btn(&mut right_ui, t.plus5, Btn::Secondary, p).clicked() {
            self.timer_cmd(format!("plus {} 300", tm.id));
        }
        right_ui.add_space(8.0);
        right_ui.label(RichText::new(fmt::countdown(left_s, lang)).font(theme::mono(20.0)).color(if left_s <= 60 { p.accent } else { p.text }));
    }

    fn timer_cmd(&self, cmd: String) {
        if !self.demo {
            tell(cmd);
        }
    }

    fn banners(&mut self, ui: &mut Ui, p: &Pal, st: &DState, reachable: bool) {
        let t = self.s();
        if !reachable && !self.demo {
            theme::card(p).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t.daemon_off).font(theme::reg(13.5)).color(p.warning));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::btn(ui, t.daemon_start, Btn::Primary, p).clicked() {
                            platform::spawn(&["--background"]);
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }
        let notices: Vec<&String> = st.notices.iter().skip(self.dismissed).collect();
        for n in notices {
            theme::card(p).inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(n).font(theme::reg(13.0)).color(p.warning)).wrap());
                });
            });
            ui.add_space(8.0);
        }
        if let Some(n) = &self.cfg.notice {
            let msg = t.notice_config_corrupt.replace("{name}", n);
            let mut dismiss = false;
            theme::card(p).inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(msg).font(theme::reg(13.0)).color(p.warning)).wrap());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::btn(ui, t.dismiss, Btn::Link, p).clicked() {
                            dismiss = true;
                        }
                    });
                });
            });
            ui.add_space(8.0);
            if dismiss {
                self.cfg.notice = None;
            }
        }
    }

    // ───────────────────────── history ─────────────────────────

    fn history_view(&mut self, ui: &mut Ui, p: &Pal, st: &DState, reachable: bool) {
        let t = self.s();
        let lang = self.lang;
        let mut clear = false;
        let mut again = None;
        page(
            ui,
            p,
            t.history,
            Some(t.history_hint),
            |ui| {
                if !st.history.is_empty() && widgets::btn(ui, t.clear_history, Btn::Secondary, p).clicked() {
                    clear = true;
                }
            },
            |ui| {
                self.banners(ui, p, st, reachable);
                if st.history.is_empty() {
                    theme::card(p).inner_margin(Margin::symmetric(24, 28)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| ui.label(RichText::new(t.no_history).font(theme::reg(14.0)).color(p.muted)));
                    });
                    return;
                }
                for (i, h) in st.history.iter().enumerate() {
                    ui.add_space(6.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
                    ui.painter().rect(rect, theme::R_CARD as f32, p.card, Stroke::new(1.0, p.line), StrokeKind::Inside);
                    let inner = rect.shrink2(vec2(16.0, 8.0));
                    let label = widgets::fit_text(ui.painter(), &h.label, &theme::medium(14.5), inner.width() - 170.0);
                    ui.painter().text(pos2(inner.left(), inner.top() + 10.0), egui::Align2::LEFT_CENTER, label, theme::medium(14.5), p.text);
                    let spec = match h.clock {
                        Some((hh, mm)) => t.fires_at.replace("{t}", &format!("{hh:02}:{mm:02}")),
                        None => fmt::dur(h.secs, lang),
                    };
                    let ago = fmt::when(h.at, daemon::now_ms(), lang);
                    ui.painter().text(pos2(inner.left(), inner.top() + 29.0), egui::Align2::LEFT_CENTER, format!("{spec}  ·  {ago}"), theme::reg(12.5), p.text2);
                    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)));
                    rui.push_id(i, |ui| {
                        if widgets::btn(ui, t.start_again, Btn::Secondary, p).clicked() {
                            again = Some(i);
                        }
                    });
                }
            },
        );
        if clear {
            self.timer_cmd("clear_history".into());
        }
        if let Some(i) = again {
            self.timer_cmd(format!("again {i}"));
            self.nav = Nav::Timers;
        }
    }

    // ───────────────────────── settings ─────────────────────────

    fn settings_view(&mut self, ui: &mut Ui, p: &Pal, st: &DState, reachable: bool) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let lang = self.lang;
        // The result of a shortcut change that was sent from here.
        if let Some(r) = self.hk_result.lock().ok().and_then(|mut g| g.take()) {
            self.hk_msg = Some(r);
        }
        page(
            ui,
            p,
            t.nav_settings,
            None,
            |_| {},
            |ui| {
                self.banners(ui, p, st, reachable);
                group(ui, p, t.group_general, |ui| {
                    let langs: Vec<(Lang, String)> = Lang::ALL.iter().map(|l| (*l, l.native_name().to_string())).collect();
                    if let Some(l) = combo_row(ui, p, i18n::language_label(lang), "language", lang, &langs, 150.0) {
                        self.lang = l;
                        self.cfg.language = Some(l.code().to_string());
                        self.save_cfg();
                    }
                    let themes: Vec<(theme::ThemeMode, String)> = theme::ThemeMode::ALL.iter().map(|m| (*m, i18n::theme_name(*m, lang).to_string())).collect();
                    if let Some(m) = combo_row(ui, p, i18n::theme_label(lang), "theme", self.cfg.theme, &themes, 150.0) {
                        self.cfg.theme = m;
                        theme::apply(&ctx, m);
                        self.save_cfg();
                    }
                    theme::divider(ui, p);
                    self.hotkey_block(ui, p, st);
                    theme::divider(ui, p);
                    let mut v = self.cfg.sound;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width() - 130.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new(t.sound).font(theme::reg(14.0)).color(p.text));
                            theme::hint(ui, t.sound_hint, p);
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::switch(ui, &mut v, p).changed() {
                                self.cfg.sound = v;
                                self.save_cfg();
                            }
                            if widgets::btn(ui, t.sound_play, Btn::Secondary, p).clicked() {
                                crate::sound::play_now();
                            }
                        });
                    });
                    ui.add_space(8.0);
                    let mut auto = self.autostart;
                    if switch_row(ui, p, if cfg!(windows) { t.autostart } else { t.autostart_os }, t.autostart_hint, &mut auto) {
                        let ok = platform::set_autostart(auto);
                        self.autostart = platform::autostart_enabled();
                        self.autostart_failed = !ok || self.autostart != auto;
                        self.cfg.autostart = self.autostart;
                        self.save_cfg();
                    }
                    if self.autostart_failed {
                        ui.label(RichText::new(t.autostart_failed).font(theme::reg(12.0)).color(p.negative));
                    }
                });

                let u = nimvoro::strings(lang);
                group(ui, p, u.updates, |ui| {
                    let mut v = self.cfg.check_updates;
                    if switch_row(ui, p, u.check_on_start, u.check_on_start_hint, &mut v) {
                        self.cfg.check_updates = v;
                        self.save_cfg();
                    }
                    if !update_ui::progress(ui, &self.updater, lang, p) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(self.updater.status_text(lang)).font(theme::reg(13.0)).color(p.text2));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if let UpdateState::Available { .. } = self.updater.state() {
                                    if widgets::btn(ui, u.update_btn, Btn::Primary, p).clicked() {
                                        self.save_cfg();
                                        if self.updater.apply_if(true) {
                                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                        }
                                    }
                                }
                                if widgets::btn(ui, u.check_now, Btn::Secondary, p).clicked() {
                                    let c = ctx.clone();
                                    self.updater.check(move || c.request_repaint());
                                }
                            });
                        });
                    }
                });

                ui.add_space(24.0);
                support::support_section(ui, lang);

                group(ui, p, t.about, |ui| {
                    ui.horizontal(|ui| {
                        theme::mark(ui, 32.0, widgets::mark_glyph, p);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new("Remindo").font(theme::medium(15.0)).color(p.text));
                            theme::meta(ui, &format!("{} {}", t.version, env!("CARGO_PKG_VERSION")), p);
                        });
                    });
                    ui.add_space(8.0);
                    theme::hint(ui, t.privacy_note, p);
                    ui.add_space(8.0);
                    if widgets::btn(ui, crate::REPO_URL, Btn::Link, p).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(crate::REPO_URL));
                    }
                });
            },
        );
    }

    /// The shortcut row: key caps, Change (record), Reset, the result, and the Wayland hint.
    fn hotkey_block(&mut self, ui: &mut Ui, p: &Pal, st: &DState) {
        let t = self.s();
        let ctx = ui.ctx().clone();
        let current = if st.hotkey.is_empty() { self.cfg.hotkey.clone() } else { st.hotkey.clone() };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 250.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(t.hotkey).font(theme::reg(14.0)).color(p.text));
                theme::hint(ui, if self.recording { t.hotkey_recording } else { t.hotkey_hint }, p);
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if self.recording {
                    if widgets::btn(ui, t.cancel, Btn::Secondary, p).clicked() {
                        self.recording = false;
                    }
                    theme::hint(ui, t.hotkey_cancel_hint, p);
                } else {
                    if current != crate::hotkey::Hotkey::default().to_string() && widgets::btn(ui, t.hotkey_reset, Btn::Link, p).clicked() {
                        self.set_hotkey(&ctx, crate::hotkey::Hotkey::default().to_string());
                    }
                    if widgets::btn(ui, t.hotkey_change, Btn::Secondary, p).clicked() {
                        self.recording = true;
                        self.hk_msg = None;
                    }
                    let parts = Hotkey::parse(&current).map(|h| h.parts()).unwrap_or_default();
                    for part in parts.iter().rev() {
                        theme::keycap(ui, part, p);
                    }
                }
            });
        });
        if self.recording {
            let events = ctx.input(|i| i.events.clone());
            for ev in events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                    if key == Key::Escape && !modifiers.any() {
                        self.recording = false;
                        break;
                    }
                    if let Some(h) = Hotkey::from_input(modifiers, key) {
                        self.recording = false;
                        self.set_hotkey(&ctx, h.to_string());
                        break;
                    }
                }
            }
            ctx.request_repaint();
        }
        if let Some((msg, ok)) = &self.hk_msg {
            ui.add_space(4.0);
            ui.label(RichText::new(msg).font(theme::reg(12.5)).color(if *ok { p.positive } else { p.negative }));
        }
        if st.hotkey_error.as_deref().is_some_and(|e| e != "wayland") {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(t.hotkey_down).font(theme::reg(12.5)).color(p.warning)).wrap());
        }
        if cfg!(unix) && std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland") {
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(t.hotkey_wayland).font(theme::reg(12.5)).color(p.text2)).wrap());
            ui.add_space(4.0);
            ui.label(RichText::new("remindo --new").font(theme::mono(13.0)).color(p.text));
        }
    }

    fn set_hotkey(&mut self, ctx: &egui::Context, spec: String) {
        let t = self.s();
        let old = self.state().1.hotkey;
        if self.demo {
            self.hk_msg = Some((t.hotkey_saved.to_string(), true));
            return;
        }
        let (out, ctx) = (self.hk_result.clone(), ctx.clone());
        let (saved, taken) = (t.hotkey_saved.to_string(), t.hotkey_taken.to_string());
        let cfg_hotkey = self.cfg.hotkey.clone();
        std::thread::spawn(move || {
            let reply = ask(&format!("hotkey {spec}")).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
            let result = if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
                (saved, true)
            } else {
                let keep = if old.is_empty() { cfg_hotkey } else { old };
                (taken.replace("{hk}", &spec).replace("{old}", &keep), false)
            };
            if let Ok(mut g) = out.lock() {
                *g = Some(result);
            }
            ctx.request_repaint();
        });
    }

    fn track_window(&mut self, ctx: &egui::Context) {
        let (inner, outer, minimized, maximized) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect, i.viewport().minimized, i.viewport().maximized));
        if minimized == Some(true) || maximized == Some(true) || self.shot.is_some() {
            return;
        }
        if let (Some(inner), Some(outer)) = (inner, outer) {
            let w = Window { w: inner.width(), h: inner.height(), x: Some(outer.min.x), y: Some(outer.min.y) };
            let same = self.cfg.window.is_some_and(|c| (c.w - w.w).abs() < 1.0 && (c.h - w.h).abs() < 1.0 && c.x == w.x && c.y == w.y);
            if !same && w.w > 300.0 && w.h > 300.0 && !self.demo {
                self.cfg.window = Some(w);
            }
        }
    }
}

trait FocusNew {
    fn request_focus_if_new(&self, ui: &Ui);
}

impl FocusNew for egui::Response {
    fn request_focus_if_new(&self, ui: &Ui) {
        // Focus the text field the first time it appears.
        if ui.memory(|m| m.focused().is_none()) {
            self.request_focus();
        }
    }
}

impl eframe::App for Settings {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        for cmd in bus::drain() {
            match cmd {
                Cmd::Focus => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Cmd::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        if self.updater.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.updater.should_exit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.track_window(ctx);
        if let Some(s) = &mut self.shot {
            s.drive(ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let p = Pal::of(ui.visuals().dark_mode);
        let (reachable, st) = self.state();
        egui::Panel::left("sidebar")
            .exact_size(216.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar).stroke(Stroke::NONE).inner_margin(Margin { left: 12, right: 12, top: 16, bottom: 12 }))
            .show(ui, |ui| self.sidebar(ui, &p, reachable, st.timers.len()));
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0.0, p.bg);
        ui.painter().vline(area.left(), area.y_range(), Stroke::new(1.0, p.line));
        let inner = Rect::from_min_max(area.min + vec2(28.0, 24.0), area.max - vec2(28.0, 8.0));
        let mut cui = ui.new_child(egui::UiBuilder::new().id_salt("central").max_rect(inner).layout(Layout::top_down(Align::Min)));
        cui.set_clip_rect(area);
        match self.nav {
            Nav::Timers => self.timers_view(&mut cui, &p, &st, reachable),
            Nav::History => self.history_view(&mut cui, &p, &st, reachable),
            Nav::Settings => self.settings_view(&mut cui, &p, &st, reachable),
        }
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        if !self.demo {
            self.cfg.save_to(&self.dir);
        }
    }
}

/// Starts the Settings window. A second one is never opened: the first one is told to come forward.
pub fn run(dir: PathBuf) -> eframe::Result<()> {
    let demo = super::demo_allowed();
    if !demo {
        let served = ipc::serve("settings", |m| {
            match m {
                "focus" => bus::push(Cmd::Focus),
                "close" => bus::push(Cmd::Close),
                _ => {}
            }
            "ok".into()
        });
        if !served {
            platform::allow_foreground();
            ipc::request("settings", "focus");
            return Ok(());
        }
    }
    let cfg = Config::load_from(&dir);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Remindo")
        .with_app_id("local.remindo")
        .with_min_inner_size([860.0, 580.0])
        .with_icon(std::sync::Arc::new(super::app_icon()));
    let mut centered = true;
    match cfg.window {
        Some(w) if w.w >= 860.0 && w.h >= 580.0 && !demo => {
            viewport = viewport.with_inner_size([w.w, w.h]);
            if let (Some(x), Some(y)) = (w.x, w.y) {
                if x > -20000.0 && y > -20000.0 && x < 20000.0 && y < 20000.0 {
                    viewport = viewport.with_position([x, y]);
                    centered = false;
                }
            }
        }
        // A tall window for the Settings screenshot, so the Support block is in the frame.
        _ if demo && std::env::var("REMINDO_NAV").is_ok_and(|n| n == "settings") => viewport = viewport.with_inner_size([980.0, 975.0]),
        _ => viewport = viewport.with_inner_size([980.0, 680.0]),
    }
    let options = eframe::NativeOptions { viewport, centered, ..Default::default() };
    eframe::run_native("Remindo", options, Box::new(move |cc| Ok(Box::new(Settings::new(cc, cfg, dir)))))
}
