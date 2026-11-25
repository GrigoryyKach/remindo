//! The quick input as a native Win32 window inside the daemon (Windows only).
//!
//! Created hidden once at daemon start and only shown on the hotkey, so there is no process to start and no GPU
//! context to build: the first key typed right after the hotkey already lands in the input. A topmost, rounded,
//! borderless window painted with GDI, with a real EDIT control (IME, Cyrillic, Ctrl+A, Ctrl+V and Ctrl+Backspace
//! work), the suite's palette, IBM Plex loaded privately from the embedded fonts, and the same preview line as the
//! egui popup (`crate::preview`, driven by `crate::parse`). Linux and `--popup-egui` use the egui popup instead.
//!
//! Everything here runs on the daemon's message-loop thread.

use crate::i18n::Lang;
use crate::strings;
use crate::theme::{self, Pal, ThemeMode};
use eframe::egui::Color32;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const W: f32 = 520.0;
const H: f32 = 126.0;
const WM_ACTIVATE_: u32 = 0x0006;
const WM_COMMAND_: u32 = 0x0111;
const WM_CTLCOLOREDIT_: u32 = 0x0133;
const EN_CHANGE_: u32 = 0x0300;
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SETMARGINS: u32 = 0x00D3;
const TIMER_SHOT: usize = 99;

static POPUP: AtomicIsize = AtomicIsize::new(0);
static EDIT: AtomicIsize = AtomicIsize::new(0);
static OLD_EDIT: AtomicIsize = AtomicIsize::new(0);
static SHOT: Mutex<Option<(String, String)>> = Mutex::new(None);

#[derive(Clone)]
struct St {
    scale: f32,
    dark: bool,
    lang: Lang,
    error: Option<String>,
    last: Option<String>,
    had_focus: bool,
    /// reg 18, preview 13.5, medium 14, mono 12, hint 12.5
    fonts: [isize; 5],
    fonts_scale: f32,
    input_brush: isize,
    /// Left edge of the input text, the preview and the hints (physical pixels).
    text_x: i32,
}

static ST: Mutex<St> = Mutex::new(St { scale: 1.0, dark: true, lang: Lang::En, error: None, last: None, had_focus: false, fonts: [0; 5], fonts_scale: 0.0, input_brush: 0, text_x: 0 });

fn st() -> St {
    ST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_st(f: impl FnOnce(&mut St)) {
    f(&mut ST.lock().unwrap_or_else(|e| e.into_inner()));
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rgb(c: Color32) -> u32 {
    c.r() as u32 | (c.g() as u32) << 8 | (c.b() as u32) << 16
}

fn px(v: f32, scale: f32) -> i32 {
    (v * scale).round() as i32
}

pub fn is_ready() -> bool {
    POPUP.load(Ordering::Relaxed) != 0
}

// ───────────────────────── setup ─────────────────────────

/// Loads IBM Plex privately (for this process only), then creates the hidden window and its EDIT control.
pub fn init() {
    theme::set_accent(crate::app::ACCENT);
    for font in [&include_bytes!("../assets/fonts/PlexSans-Regular.ttf")[..], &include_bytes!("../assets/fonts/PlexSans-Medium.ttf")[..], &include_bytes!("../assets/fonts/PlexMono-Regular.ttf")[..]] {
        let mut n = 0u32;
        // SAFETY: the data is a static slice; the system copies it.
        unsafe { AddFontMemResourceEx(font.as_ptr() as *const _, font.len() as u32, std::ptr::null(), &mut n) };
    }
    // SAFETY: standard window creation on this thread; strings are NUL-terminated.
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("RemindoQuickInput");
        let wc = WNDCLASSW { lpfnWndProc: Some(popup_proc), hInstance: hinst, lpszClassName: class.as_ptr(), hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let title = wide("Remindo");
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, px(W, 1.0), px(H, 1.0), std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        if hwnd.is_null() {
            return;
        }
        let edit_class = wide("EDIT");
        let empty = wide("");
        let edit = CreateWindowExW(0, edit_class.as_ptr(), empty.as_ptr(), WS_CHILD | WS_VISIBLE | 0x80 /* ES_AUTOHSCROLL */, 0, 0, 100, 24, hwnd, std::ptr::null_mut(), hinst, std::ptr::null());
        if edit.is_null() {
            DestroyWindow(hwnd);
            return;
        }
        SendMessageW(edit, EM_SETMARGINS, 3, 0); // EC_LEFTMARGIN | EC_RIGHTMARGIN = 0
        let old = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as isize);
        OLD_EDIT.store(old, Ordering::Relaxed);
        EDIT.store(edit as isize, Ordering::Relaxed);
        POPUP.store(hwnd as isize, Ordering::Relaxed);
    }
}

fn make_font(face: &str, height_px: i32) -> isize {
    let f = wide(face);
    // SAFETY: plain GDI call; the face name is NUL-terminated.
    unsafe { CreateFontW(-height_px, 0, 0, 0, 400, 0, 0, 0, 1 /* DEFAULT_CHARSET */, 0, 0, 5 /* CLEARTYPE_QUALITY */, 0, f.as_ptr()) as isize }
}

fn ensure_fonts(scale: f32) {
    let s = st();
    if (s.fonts_scale - scale).abs() < 0.001 && s.fonts[0] != 0 {
        return;
    }
    for f in s.fonts {
        if f != 0 {
            // SAFETY: deleting fonts this module created.
            unsafe { DeleteObject(f as _) };
        }
    }
    let fonts = [
        make_font("IBM Plex Sans", px(18.0, scale)),
        make_font("IBM Plex Sans", px(13.5, scale)),
        make_font("IBM Plex Sans Medium", px(14.0, scale)),
        make_font("IBM Plex Mono", px(12.0, scale)),
        make_font("IBM Plex Sans", px(12.5, scale)),
    ];
    set_st(|s| {
        s.fonts = fonts;
        s.fonts_scale = scale;
    });
}

fn system_is_dark() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v == 0)
        .unwrap_or(true)
}

// ───────────────────────── showing and hiding ─────────────────────────

/// Shows the quick input on the monitor with the cursor and puts the keyboard in it. Already open: only refocus.
pub fn show() {
    let hwnd = POPUP.load(Ordering::Relaxed) as HWND;
    let edit = EDIT.load(Ordering::Relaxed) as HWND;
    if hwnd.is_null() {
        return;
    }
    // SAFETY: plain Win32 calls on windows of this thread.
    unsafe {
        if IsWindowVisible(hwnd) != 0 {
            crate::platform::force_foreground(hwnd as isize);
            SetFocus(edit);
            return;
        }
    }
    let (lang, mode, dir) = crate::daemon::with(|d| (d.lang, d.cfg.theme, d.dir.clone())).unwrap_or((Lang::En, ThemeMode::System, crate::config::data_dir()));
    let dark = match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => system_is_dark(),
    };
    let scale = crate::platform::cursor_area().map(|a| a.scale).unwrap_or(1.0).max(0.5);
    ensure_fonts(scale);
    let last = std::fs::read_to_string(dir.join("last_input.txt")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let pal = Pal::of(dark);
    let shot_text = SHOT.lock().ok().and_then(|g| g.as_ref().map(|s| s.1.clone()));
    // SAFETY: plain Win32/GDI calls on windows and objects of this thread.
    unsafe {
        let old_brush = st().input_brush;
        if old_brush != 0 {
            DeleteObject(old_brush as _);
        }
        let brush = CreateSolidBrush(rgb(pal.input)) as isize;
        let s = st();
        let text_x = px(51.0, scale);
        set_st(|s| {
            s.scale = scale;
            s.dark = dark;
            s.lang = lang;
            s.error = None;
            s.last = last;
            s.had_focus = false;
            s.input_brush = brush;
            s.text_x = text_x;
        });
        let _ = s;
        let text = wide(shot_text.as_deref().unwrap_or(""));
        SetWindowTextW(edit, text.as_ptr());
        SendMessageW(edit, WM_SETFONT, st().fonts[0] as usize, 1);
        layout(hwnd, edit, scale);
        crate::platform::place(hwnd as isize, (W, H), crate::app::Anchor::Center, true);
        crate::platform::force_foreground(hwnd as isize);
        SetFocus(edit);
        InvalidateRect(hwnd, std::ptr::null(), 0);
        if SHOT.lock().map(|g| g.is_some()).unwrap_or(false) {
            SetTimer(hwnd, TIMER_SHOT, 700, None);
        }
    }
}

/// Sets the rounded shape and puts the EDIT control where the text goes, centred on the clock icon.
unsafe fn layout(hwnd: HWND, edit: HWND, scale: f32) {
    let (w, h) = (px(W, scale), px(H, scale));
    let r = px(16.0, scale);
    // SAFETY: GDI objects created and handed over (the region is owned by the window afterwards).
    unsafe {
        SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, r * 2, r * 2), 1);
        // The text line's optical centre (between cap height and x-height) sits on the centre of the field.
        let dc = GetDC(std::ptr::null_mut());
        let old = SelectObject(dc, st().fonts[0] as _);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        GetTextMetricsW(dc, &mut tm);
        SelectObject(dc, old);
        ReleaseDC(std::ptr::null_mut(), dc);
        let em = 18.0 * scale;
        let field_cy = px(33.0, scale) as f32;
        // (+2 px: measured against the rendered glyphs, GDI's ascent is a little larger than the ink above the baseline)
        let top = field_cy - (tm.tmAscent as f32 - 0.5 * 0.5 * (0.698 + 0.516) * em) + 2.0 * scale;
        let x = st().text_x;
        MoveWindow(edit, x, top.round() as i32, w - x - px(24.0, scale), tm.tmHeight, 1);
    }
}

pub fn hide() {
    let hwnd = POPUP.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        // SAFETY: plain Win32 call.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

fn edit_text(edit: HWND) -> String {
    // SAFETY: the buffer is sized from the control's own length.
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buf[..got])
    }
}

fn submit() {
    let edit = EDIT.load(Ordering::Relaxed) as HWND;
    let text = edit_text(edit).trim().to_string();
    if text.is_empty() {
        return;
    }
    let result = crate::daemon::with(|d| d.add_text(&text).map(|_| d.dir.clone()));
    match result {
        Some(Ok(dir)) => {
            let _ = crate::config::write_atomic(&dir.join("last_input.txt"), text.as_bytes());
            crate::daemon::update_tray();
            hide();
        }
        Some(Err(code)) => {
            let lang = st().lang;
            let msg = crate::preview::error_text(code, strings::TABLE.get(lang)).to_string();
            set_st(|s| s.error = Some(msg));
            invalidate();
        }
        None => {
            let lang = st().lang;
            set_st(|s| s.error = Some(strings::TABLE.get(lang).daemon_down.to_string()));
            invalidate();
        }
    }
}

fn invalidate() {
    let hwnd = POPUP.load(Ordering::Relaxed) as HWND;
    // SAFETY: plain Win32 call.
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}

// ───────────────────────── the EDIT control ─────────────────────────

fn ctrl_down() -> bool {
    // SAFETY: plain Win32 call.
    unsafe { GetKeyState(0x11) < 0 }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let old: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(OLD_EDIT.load(Ordering::Relaxed)) };
    match msg {
        WM_KEYDOWN => match wp as u32 {
            0x0D => {
                submit();
                return 0;
            }
            0x1B => {
                hide();
                return 0;
            }
            0x26 => {
                if let Some(last) = st().last {
                    let t = wide(&last);
                    // SAFETY: plain Win32 calls on this control.
                    unsafe {
                        SetWindowTextW(hwnd, t.as_ptr());
                        SendMessageW(hwnd, EM_SETSEL, last.encode_utf16().count(), last.encode_utf16().count() as isize);
                    }
                    set_st(|s| s.error = None);
                    invalidate();
                }
                return 0;
            }
            0x41 if ctrl_down() => {
                // SAFETY: plain Win32 call.
                unsafe { SendMessageW(hwnd, EM_SETSEL, 0, -1) };
                return 0;
            }
            0x08 if ctrl_down() => {
                delete_word(hwnd);
                return 0;
            }
            _ => {}
        },
        // Swallow the characters of the keys handled above (Enter, Escape, Ctrl+A, Ctrl+Backspace): no beep.
        WM_CHAR if matches!(wp as u32, 0x0D | 0x1B | 0x01 | 0x7F) => return 0,
        _ => {}
    }
    // SAFETY: the original window procedure of the EDIT control.
    let r = unsafe { CallWindowProcW(old, hwnd, msg, wp, lp) };
    if msg == WM_PAINT {
        // The placeholder, while the field is empty.
        // SAFETY: plain GDI calls with the control's own client DC.
        unsafe {
            if GetWindowTextLengthW(hwnd) == 0 {
                let s = st();
                let pal = Pal::of(s.dark);
                let dc = GetDC(hwnd);
                let old_font = SelectObject(dc, s.fonts[0] as _);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, rgb(pal.muted));
                let t = strings::TABLE.get(s.lang).placeholder;
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                rc.left += 1;
                let w = wide(t);
                DrawTextW(dc, w.as_ptr(), -1, &mut rc, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
                SelectObject(dc, old_font);
                ReleaseDC(hwnd, dc);
            }
        }
    }
    if msg == WM_SETFOCUS || msg == WM_KILLFOCUS {
        // keep the placeholder honest
    }
    r
}

fn delete_word(edit: HWND) {
    // SAFETY: plain Win32 calls on this control.
    unsafe {
        let sel = SendMessageW(edit, EM_GETSEL, 0, 0) as u32;
        let (start, end) = ((sel & 0xFFFF) as usize, (sel >> 16) as usize);
        if start != end {
            SendMessageW(edit, EM_REPLACESEL, 1, wide("").as_ptr() as isize);
            return;
        }
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
        let mut i = end.min(n);
        while i > 0 && (buf[i - 1] as u8 as char).is_whitespace() && buf[i - 1] < 128 {
            i -= 1;
        }
        while i > 0 && !(buf[i - 1] < 128 && (buf[i - 1] as u8 as char).is_whitespace()) {
            i -= 1;
        }
        SendMessageW(edit, EM_SETSEL, i, end as isize);
        SendMessageW(edit, EM_REPLACESEL, 1, wide("").as_ptr() as isize);
    }
}

// ───────────────────────── the window ─────────────────────────

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ACTIVATE_ => {
            let active = (wp & 0xFFFF) != 0;
            if active {
                set_st(|s| s.had_focus = true);
            } else if st().had_focus && SHOT.lock().map(|g| g.is_none()).unwrap_or(true) {
                // A click outside closes the input without a timer.
                hide();
            }
            0
        }
        WM_SETFOCUS => {
            // SAFETY: plain Win32 call.
            unsafe { SetFocus(EDIT.load(Ordering::Relaxed) as HWND) };
            0
        }
        WM_COMMAND_ => {
            if ((wp >> 16) as u32) == EN_CHANGE_ {
                set_st(|s| s.error = None);
                invalidate();
            }
            0
        }
        WM_CTLCOLOREDIT_ => {
            let s = st();
            let pal = Pal::of(s.dark);
            // SAFETY: wp is the control's DC.
            unsafe {
                SetTextColor(wp as HDC, rgb(pal.text));
                SetBkColor(wp as HDC, rgb(pal.input));
            }
            s.input_brush as LRESULT
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32);
            let r = start_rect(&st());
            if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
                submit();
            }
            0
        }
        WM_TIMER if wp == TIMER_SHOT => {
            // SAFETY: plain Win32 call.
            unsafe { KillTimer(hwnd, TIMER_SHOT) };
            save_shot(hwnd);
            crate::daemon::request_quit();
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn start_rect(s: &St) -> RECT {
    let sc = s.scale;
    let label = strings::TABLE.get(s.lang).start;
    let w = text_width(s.fonts[2], label) + px(32.0, sc);
    RECT { left: px(W - 13.0, sc) - w, top: px(85.0, sc), right: px(W - 13.0, sc), bottom: px(113.0, sc) }
}

fn text_width(font: isize, text: &str) -> i32 {
    // SAFETY: plain GDI calls on a screen DC.
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let old = SelectObject(dc, font as _);
        let w = wide(text);
        let mut size = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, w.as_ptr(), (w.len() - 1) as i32, &mut size);
        SelectObject(dc, old);
        ReleaseDC(std::ptr::null_mut(), dc);
        size.cx
    }
}

unsafe fn draw_text(dc: HDC, font: isize, color: Color32, rect: RECT, text: &str, flags: u32) {
    let mut rc = rect;
    let w = wide(text);
    // SAFETY: plain GDI calls.
    unsafe {
        let old = SelectObject(dc, font as _);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, rgb(color));
        DrawTextW(dc, w.as_ptr(), -1, &mut rc, flags | DT_SINGLELINE | DT_NOPREFIX | DT_VCENTER);
        SelectObject(dc, old);
    }
}

unsafe fn round_rect(dc: HDC, r: RECT, radius: i32, fill: Color32, line: Option<Color32>) {
    // SAFETY: GDI objects created and released here.
    unsafe {
        let pen = match line {
            Some(c) => CreatePen(PS_SOLID, 1, rgb(c)),
            None => CreatePen(PS_NULL, 0, 0),
        };
        let brush = CreateSolidBrush(rgb(fill));
        let (op, ob) = (SelectObject(dc, pen as _), SelectObject(dc, brush as _));
        RoundRect(dc, r.left, r.top, r.right, r.bottom, radius * 2, radius * 2);
        SelectObject(dc, op);
        SelectObject(dc, ob);
        DeleteObject(pen as _);
        DeleteObject(brush as _);
    }
}

/// Draws the whole window into `dc` (a memory DC of the window's size).
unsafe fn draw(dc: HDC, w: i32, h: i32, edit_empty: bool, text: &str) {
    let s = st();
    let sc = s.scale;
    let pal = Pal::of(s.dark);
    let rc = |l: f32, t: f32, r: f32, b: f32| RECT { left: px(l, sc), top: px(t, sc), right: px(r, sc), bottom: px(b, sc) };
    // SAFETY: GDI drawing into a DC we own.
    unsafe {
        round_rect(dc, RECT { left: 0, top: 0, right: w, bottom: h }, px(16.0, sc), pal.card, Some(pal.line_hi));
        // The input field and the clock.
        round_rect(dc, rc(13.0, 13.0, W - 13.0, 53.0), px(10.0, sc), pal.input, Some(pal.line_hi));
        let (cx, cy) = (px(33.0, sc), px(33.0, sc));
        let pen = CreatePen(PS_SOLID, ((1.5 * sc).round() as i32).max(1), rgb(pal.accent));
        let old_pen = SelectObject(dc, pen as _);
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        let r = px(7.4, sc);
        Ellipse(dc, cx - r, cy - r, cx + r, cy + r);
        MoveToEx(dc, cx, cy, std::ptr::null_mut());
        LineTo(dc, cx, cy - px(4.5, sc));
        MoveToEx(dc, cx, cy, std::ptr::null_mut());
        LineTo(dc, cx + px(3.4, sc), cy + px(2.0, sc));
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(pen as _);

        // The preview line, on the same left edge as the input text.
        let t = strings::TABLE.get(s.lang);
        let (line, warn, ok) = match &s.error {
            Some(e) => (e.clone(), true, false),
            None => crate::preview::line(text, s.lang),
        };
        let _ = edit_empty;
        let col = if warn { pal.warning } else if ok { pal.text2 } else { pal.muted };
        let mut prev = rc(0.0, 60.0, W - 13.0, 80.0);
        prev.left = s.text_x;
        draw_text(dc, s.fonts[1], col, prev, &line, DT_LEFT | DT_END_ELLIPSIS);

        // Key hints, starting on the same left edge.
        let mut x = s.text_x;
        let row_top = px(85.0, sc);
        let row_bottom = px(113.0, sc);
        let cap_h = px(20.0, sc);
        let cap_top = (row_top + row_bottom - cap_h) / 2;
        let hint = |dc: HDC, cap: &str, label: &str, x: &mut i32| {
            let cw = text_width(s.fonts[3], cap) + px(12.0, sc);
            round_rect(dc, RECT { left: *x, top: cap_top, right: *x + cw, bottom: cap_top + cap_h }, px(5.0, sc), pal.raised, Some(pal.line_hi));
            draw_text(dc, s.fonts[3], pal.text2, RECT { left: *x, top: cap_top, right: *x + cw, bottom: cap_top + cap_h }, cap, DT_CENTER);
            *x += cw + px(6.0, sc);
            let lw = text_width(s.fonts[4], label);
            draw_text(dc, s.fonts[4], pal.muted, RECT { left: *x, top: row_top, right: *x + lw + 4, bottom: row_bottom }, label, DT_LEFT);
            *x += lw + px(14.0, sc);
        };
        hint(dc, "Enter", t.key_start, &mut x);
        hint(dc, "Esc", t.key_close, &mut x);
        if s.last.is_some() {
            hint(dc, "\u{2191}", t.key_recall, &mut x);
        }

        // The Start button.
        let b = start_rect(&s);
        round_rect(dc, b, px(8.0, sc), pal.accent, None);
        draw_text(dc, s.fonts[2], pal.on_accent, b, t.start, DT_CENTER);
    }
}

fn paint(hwnd: HWND) {
    // SAFETY: standard BeginPaint/EndPaint with a double buffer.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, bmp as _);
        let text = edit_text(EDIT.load(Ordering::Relaxed) as HWND);
        draw(mem, w, h, text.is_empty(), &text);
        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp as _);
        DeleteDC(mem);
        EndPaint(hwnd, &ps);
    }
}

// ───────────────────────── screenshots (demo runs only) ─────────────────────────

/// Asks the next `show` to fill the input with `text` and save a PNG of the window to `path`, then quit the daemon.
pub fn request_shot(path: String, text: String) {
    if let Ok(mut g) = SHOT.lock() {
        *g = Some((path, text));
    }
}

fn save_shot(hwnd: HWND) {
    let Some((path, _)) = SHOT.lock().ok().and_then(|g| g.clone()) else { return };
    // SAFETY: the window prints itself (WM_PRINT) into our own bitmap: no screen capture.
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        let dc = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(dc);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: 0, ..std::mem::zeroed() };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        let old = SelectObject(mem, bmp as _);
        // paint the window, then the text of the EDIT control on top of it
        let edit = EDIT.load(Ordering::Relaxed) as HWND;
        let text = edit_text(edit);
        let s = st();
        let pal = Pal::of(s.dark);
        draw(mem, w, h, false, &text);
        let mut er: RECT = std::mem::zeroed();
        GetWindowRect(edit, &mut er);
        let mut pr: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut pr);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        let old_font = SelectObject(mem, s.fonts[0] as _);
        GetTextMetricsW(mem, &mut tm);
        SetBkMode(mem, TRANSPARENT as i32);
        SetTextColor(mem, rgb(pal.text));
        let mut trc = RECT { left: er.left - pr.left, top: er.top - pr.top, right: er.right - pr.left, bottom: er.bottom - pr.top };
        let tw = wide(&text);
        DrawTextW(mem, tw.as_ptr(), -1, &mut trc, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(mem, old_font);
        let px_count = (w * h) as usize;
        let data = std::slice::from_raw_parts(bits as *const u8, px_count * 4);
        let radius = 16.0 * s.scale;
        let mut out = Vec::with_capacity(px_count * 4);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                // analytic corner coverage for the transparent corners
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let dx = if fx < radius { radius - fx } else if fx > w as f32 - radius { fx - (w as f32 - radius) } else { 0.0 };
                let dy = if fy < radius { radius - fy } else if fy > h as f32 - radius { fy - (h as f32 - radius) } else { 0.0 };
                let d = (dx * dx + dy * dy).sqrt();
                let a = if dx > 0.0 && dy > 0.0 { (radius - d + 0.5).clamp(0.0, 1.0) } else { 1.0 };
                out.extend_from_slice(&[data[i + 2], data[i + 1], data[i], (a * 255.0) as u8]);
            }
        }
        let _ = image::save_buffer(path, &out, w as u32, h as u32, image::ColorType::Rgba8);
        SelectObject(mem, old);
        DeleteObject(bmp as _);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), dc);
    }
}
