//! The daemon's Windows message loop: the global hotkey (`RegisterHotKey`), sleep and resume, the tray refresh,
//! and a clean shutdown. One hidden top-level window receives it all; nothing polls.

use crate::hotkey::Hotkey;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage, RegisterClassW, TranslateMessage, MSG, WNDCLASSW,
};

const WM_HOTKEY: u32 = 0x0312;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_APP: u32 = 0x8000;
const WM_QUIT_REQUEST: u32 = WM_APP + 1;
const WM_TRAY_REFRESH: u32 = WM_APP + 2;
const WM_REBIND: u32 = WM_APP + 3;
const PBT_APMRESUMESUSPEND: usize = 0x0007;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
/// The id of the hotkey that is registered now (1 or 2: a new one is registered before the old one is let go).
static CURRENT_ID: AtomicU32 = AtomicU32::new(0);
static REBIND: Mutex<Option<(Hotkey, Sender<Result<(), String>>)>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Registers `h` under `id`. `Err` carries a reason the user can read.
fn register(hwnd: HWND, id: u32, h: &Hotkey) -> Result<(), String> {
    let Some(vk) = h.vk() else { return Err("unknown key".into()) };
    // SAFETY: plain Win32 call with a window of this thread.
    let ok = unsafe { RegisterHotKey(hwnd, id as i32, h.win_modifiers() | MOD_NOREPEAT, vk) };
    if ok != 0 {
        return Ok(());
    }
    // SAFETY: plain Win32 call.
    let err = unsafe { GetLastError() };
    Err(if err == ERROR_HOTKEY_ALREADY_REGISTERED { "taken".into() } else { format!("error {err}") })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            // The process just got the foreground right from the keypress: hand it to the window we are about to open.
            crate::platform::allow_foreground();
            crate::daemon::open_popup_async();
            0
        }
        WM_POWERBROADCAST => {
            if wp == PBT_APMRESUMEAUTOMATIC || wp == PBT_APMRESUMESUSPEND {
                // The monotonic clock may not count the sleep: look at the wall clock now.
                crate::daemon::wake();
            }
            1
        }
        WM_TRAY_REFRESH => {
            crate::tray::apply_pending();
            0
        }
        WM_REBIND => {
            if let Some((h, reply)) = REBIND.lock().ok().and_then(|mut g| g.take()) {
                let old = CURRENT_ID.load(Ordering::Relaxed);
                let new_id = if old == 1 { 2 } else { 1 };
                let r = register(hwnd, new_id, &h);
                if r.is_ok() {
                    if old != 0 {
                        // SAFETY: plain Win32 call.
                        unsafe { UnregisterHotKey(hwnd, old as i32) };
                    }
                    CURRENT_ID.store(new_id, Ordering::Relaxed);
                }
                let _ = reply.send(r);
            }
            0
        }
        WM_QUIT_REQUEST => {
            // SAFETY: plain Win32 call.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Runs the message loop until a quit is requested. `hotkey` is registered first (`None`: not at all, in a test
/// run); `setup` runs on this thread once the window exists (it creates the tray) and learns whether the hotkey
/// was taken.
pub fn run(hotkey: Option<Hotkey>, setup: impl FnOnce(Result<(), String>)) {
    // SAFETY: standard window creation on this thread; the class and title are NUL-terminated.
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("RemindoDaemonWindow");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let title = wide("Remindo");
        // A hidden top-level window (never shown), so it receives the power broadcasts too.
        let hwnd = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        if hwnd.is_null() {
            setup(Err("no window".into()));
            return;
        }
        MAIN_HWND.store(hwnd as isize, Ordering::Relaxed);
        let result = match &hotkey {
            Some(h) => {
                let r = register(hwnd, 1, h);
                if r.is_ok() {
                    CURRENT_ID.store(1, Ordering::Relaxed);
                }
                r
            }
            None => Ok(()),
        };
        setup(result);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let id = CURRENT_ID.swap(0, Ordering::Relaxed);
        if id != 0 {
            UnregisterHotKey(hwnd, id as i32);
        }
        MAIN_HWND.store(0, Ordering::Relaxed);
        DestroyWindow(hwnd);
    }
}

fn post(msg: u32) {
    let h = MAIN_HWND.load(Ordering::Relaxed);
    if h != 0 {
        // SAFETY: posting to a window of this process; failure is harmless.
        unsafe { PostMessageW(h as HWND, msg, 0, 0) };
    }
}

pub fn post_quit() {
    post(WM_QUIT_REQUEST);
}

pub fn request_tray_refresh() {
    post(WM_TRAY_REFRESH);
}

/// Registers a new shortcut and lets the old one go only when that worked. Any thread.
pub fn rebind(h: &Hotkey) -> Result<(), String> {
    if MAIN_HWND.load(Ordering::Relaxed) == 0 {
        return Err("not running".into());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(mut g) = REBIND.lock() {
        *g = Some((h.clone(), tx));
    }
    post(WM_REBIND);
    rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap_or_else(|_| Err("timeout".into()))
}
