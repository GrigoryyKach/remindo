//! Linux glue: starting the processes, the autostart `.desktop` file, and the monitor under the mouse cursor.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Starts another Remindo process (a window, or the daemon) and returns its id. It runs on its own.
pub fn spawn(args: &[&str]) -> Option<u32> {
    let exe = std::env::current_exe().ok()?;
    Command::new(exe).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok().map(|c| c.id())
}

/// Nothing to hand over on X11 and Wayland: the compositor decides who gets focus.
pub fn allow_foreground() {}

// ───────────────────────── autostart ─────────────────────────

/// `~/.config/autostart/remindo.desktop`. A test instance (own data folder) writes into that folder instead,
/// so a test never touches the user's real autostart.
fn autostart_file() -> Option<PathBuf> {
    if crate::config::is_test_instance() {
        return Some(crate::config::data_dir().join("autostart-test").join("remindo.desktop"));
    }
    dirs::config_dir().map(|d| d.join("autostart").join("remindo.desktop"))
}

/// A `.desktop` `Exec` value: the path in double quotes with the reserved characters escaped.
fn exec_quote(path: &str) -> String {
    let mut s = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

fn entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Remindo\nComment=Quick timers from a hotkey\nExec={} --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exec_quote(exe)
    )
}

/// The real state of the file, not a remembered setting.
pub fn autostart_enabled() -> bool {
    autostart_file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|t| t.lines().any(|l| l.starts_with("Exec=") && l.ends_with("--background")))
}

/// Writes or removes the autostart file (`"<exe>" --background`). Returns whether it succeeded.
pub fn set_autostart(on: bool) -> bool {
    let Some(file) = autostart_file() else { return false };
    if on {
        let Ok(exe) = std::env::current_exe() else { return false };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(file, entry(&exe.display().to_string())).is_ok()
    } else {
        match std::fs::remove_file(file) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
}

// ───────────────────────── monitors ─────────────────────────

/// The monitor that holds the mouse cursor (X11 and XWayland), in pixels.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f32,
}

pub fn cursor_area() -> Option<Area> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let ptr = conn.query_pointer(root).ok()?.reply().ok()?;
    let monitors = conn.randr_get_monitors(root, true).ok()?.reply().ok()?.monitors;
    let under = monitors.iter().find(|m| ptr.root_x >= m.x && ptr.root_x < m.x + m.width as i16 && ptr.root_y >= m.y && ptr.root_y < m.y + m.height as i16);
    let m = under.or(monitors.first())?;
    Some(Area { x: m.x as i32, y: m.y as i32, w: m.width as i32, h: m.height as i32, scale: 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_entry_quotes_the_path() {
        let e = entry("/opt/my apps/Remindo");
        assert!(e.contains("Exec=\"/opt/my apps/Remindo\" --background\n"));
        assert_eq!(exec_quote("/a\"b$c"), "\"/a\\\"b\\$c\"");
    }

    #[test]
    fn autostart_round_trips_in_a_test_folder() {
        let _env = crate::config::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("remindo-autostart-{}", std::process::id()));
        unsafe { std::env::set_var("REMINDO_DATA_DIR", &dir) };
        assert!(!autostart_enabled());
        assert!(set_autostart(true));
        assert!(autostart_enabled());
        assert!(set_autostart(false));
        assert!(!autostart_enabled());
        assert!(set_autostart(false));
        unsafe { std::env::remove_var("REMINDO_DATA_DIR") };
        let _ = std::fs::remove_dir_all(dir);
    }
}
