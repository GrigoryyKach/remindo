//! Windows glue: starting the processes, the autostart value, and putting a window where it belongs.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GetCursorPos, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, SetWindowPos, ASFW_ANY, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Starts another Remindo process (a window, or the daemon) and returns its id. It runs on its own.
pub fn spawn(args: &[&str]) -> Option<u32> {
    let exe = std::env::current_exe().ok()?;
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    cmd.spawn().ok().map(|c| c.id())
}

/// The process that has the foreground right may hand it to any other one (called right before a window is opened).
pub fn allow_foreground() {
    // SAFETY: plain Win32 call.
    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
}

// ───────────────────────── autostart ─────────────────────────

const REAL_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const TEST_RUN_KEY: &str = "Software\\Remindo\\Test\\Run";
const RUN_VALUE: &str = "Remindo";

/// The registry key that holds the startup value. A test instance (own data folder) and the
/// `REMINDO_AUTOSTART_KEY` override use a private key, so a test never touches the user's real Run key.
fn run_key() -> String {
    if let Ok(k) = std::env::var("REMINDO_AUTOSTART_KEY") {
        if !k.is_empty() {
            return k;
        }
    }
    if crate::config::is_test_instance() { TEST_RUN_KEY.to_string() } else { REAL_RUN_KEY.to_string() }
}

/// The command stored in the value: the exe path in quotes and `--background`.
pub fn autostart_command(exe: &std::path::Path) -> String {
    format!("\"{}\" --background", exe.display())
}

/// The real registry state, not a remembered setting.
pub fn autostart_enabled() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(run_key()).and_then(|k| k.get_value::<String, _>(RUN_VALUE)).is_ok_and(|v| !v.is_empty())
}

/// Adds or removes the startup value. Returns whether the operation succeeded.
pub fn set_autostart(on: bool) -> bool {
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE},
        RegKey,
    };
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(run_key(), KEY_SET_VALUE | KEY_READ) else {
        return false;
    };
    if on {
        let Ok(exe) = std::env::current_exe() else { return false };
        key.set_value(RUN_VALUE, &autostart_command(&exe)).is_ok()
    } else {
        match key.delete_value(RUN_VALUE) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
}

// ───────────────────────── windows on screen ─────────────────────────

/// The work area (without the taskbar) of the monitor that holds the mouse cursor, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// DPI scale (1.0 = 96 dpi).
    pub scale: f32,
}

pub fn cursor_area() -> Option<Area> {
    // SAFETY: documented Win32 calls with valid out parameters.
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
            dx = 96;
        }
        let r = mi.rcWork;
        Some(Area { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top, scale: dx as f32 / 96.0 })
    }
}

/// The first visible top-level window of this process (our own egui window).
pub fn own_window() -> Option<isize> {
    struct Search {
        pid: u32,
        found: isize,
    }
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        // SAFETY: `lp` is the address of the `Search` below, alive for the whole enumeration.
        let s = unsafe { &mut *(lp as *mut Search) };
        let mut pid = 0u32;
        // SAFETY: plain Win32 calls.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == s.pid && IsWindowVisible(hwnd) != 0 {
                s.found = hwnd as isize;
                return 0;
            }
        }
        1
    }
    // SAFETY: the callback only reads the struct we pass.
    unsafe {
        let mut s = Search { pid: GetCurrentProcessId(), found: 0 };
        EnumWindows(Some(each), &mut s as *mut Search as LPARAM);
        (s.found != 0).then_some(s.found)
    }
}

/// Moves a window (given as `hwnd`) to the monitor with the cursor, topmost. `logical` is its size in egui points.
/// `activate = false` shows it without taking the focus from the program in front.
pub fn place(hwnd: isize, logical: (f32, f32), anchor: crate::app::Anchor, activate: bool) {
    let Some(area) = cursor_area() else { return };
    let (w, h) = ((logical.0 * area.scale).round() as i32, (logical.1 * area.scale).round() as i32);
    let x = area.x + (area.w - w) / 2;
    let y = match anchor {
        crate::app::Anchor::Center => area.y + ((area.h - h) as f32 * 0.38) as i32,
        crate::app::Anchor::Upper => area.y + (area.h as f32 * 0.16) as i32,
    };
    let flags = SWP_SHOWWINDOW | if activate { 0 } else { SWP_NOACTIVATE };
    // SAFETY: plain Win32 call on a window of this process.
    unsafe {
        SetWindowPos(hwnd as HWND, HWND_TOPMOST, x, y, w, h, flags);
    }
}

/// The window's outer rectangle (left, top, width, height) in physical pixels.
pub fn window_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    // SAFETY: plain Win32 call with a valid out parameter.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        (GetWindowRect(hwnd as HWND, &mut r) != 0).then_some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

/// Makes sure the keyboard goes to our window. The hotkey's process was allowed to take the foreground and handed
/// that right over (`allow_foreground`); when Windows still refuses, joining the input queue of the current
/// foreground window lets the call through. No key is sent.
pub fn force_foreground(hwnd: isize) -> bool {
    use windows_sys::Win32::System::Threading::AttachThreadInput;
    // SAFETY: documented Win32 calls on a window of this process.
    unsafe {
        let h = hwnd as HWND;
        if GetForegroundWindow() == h {
            return true;
        }
        BringWindowToTop(h);
        if SetForegroundWindow(h) != 0 {
            SetFocus(h);
            return GetForegroundWindow() == h;
        }
        let fg = GetForegroundWindow();
        let fg_thread = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, std::ptr::null_mut()) };
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
        BringWindowToTop(h);
        let ok = SetForegroundWindow(h) != 0;
        SetFocus(h);
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
        ok && GetForegroundWindow() == h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_goes_to_a_private_key_and_round_trips() {
        let _env = crate::config::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: tests in this module hold ENV_LOCK; the variable is only read by `run_key`.
        unsafe { std::env::set_var("REMINDO_AUTOSTART_KEY", "Software\\Remindo\\UnitTest\\Run") };
        assert!(!autostart_enabled());
        assert!(set_autostart(true));
        assert!(autostart_enabled());
        assert!(set_autostart(false));
        assert!(!autostart_enabled());
        // Removing what is not there is fine.
        assert!(set_autostart(false));
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all("Software\\Remindo\\UnitTest");
        unsafe { std::env::remove_var("REMINDO_AUTOSTART_KEY") };
    }

    #[test]
    fn command_quotes_the_path() {
        let c = autostart_command(std::path::Path::new(r"C:\Program Files\Remindo\Remindo.exe"));
        assert_eq!(c, "\"C:\\Program Files\\Remindo\\Remindo.exe\" --background");
    }
}
