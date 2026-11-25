"""Lists the visible windows of the Remindo processes (read only): pid, title, rectangle, extended style, and the
foreground process. Used to check where the windows land and who has the focus; it sends no input."""
import ctypes, sys
from ctypes import wintypes
u = ctypes.windll.user32
k = ctypes.windll.kernel32
names = {}
def pids_of(image):
    import subprocess
    out = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    return {int(l.split('","')[1]) for l in out.splitlines() if '","' in l}
pids = pids_of(sys.argv[1] if len(sys.argv) > 1 else "Remindo.exe")
fg = u.GetForegroundWindow(); fgpid = wintypes.DWORD(); u.GetWindowThreadProcessId(fg, ctypes.byref(fgpid))
print("remindo pids:", sorted(pids), "| foreground pid:", fgpid.value, "| foreground is remindo:", fgpid.value in pids)
CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
def cb(h, l):
    pid = wintypes.DWORD(); u.GetWindowThreadProcessId(h, ctypes.byref(pid))
    if pid.value in pids and u.IsWindowVisible(h):
        r = wintypes.RECT(); u.GetWindowRect(h, ctypes.byref(r))
        buf = ctypes.create_unicode_buffer(100); u.GetWindowTextW(h, buf, 100)
        cls = ctypes.create_unicode_buffer(100); u.GetClassNameW(h, cls, 100)
        owner = u.GetWindow(h, 4)
        ex = u.GetWindowLongW(h, -20) & 0xffffffff
        print(f"class {cls.value} owner {owner} pid {pid.value} hwnd {h} title {buf.value!r} rect ({r.left},{r.top}) {r.right-r.left}x{r.bottom-r.top} exstyle {ex:#x} topmost {bool(ex & 8)} noactivate {bool(ex & 0x08000000)} toolwindow {bool(ex & 0x80)} foreground {h == fg}")
    return True
u.EnumWindows(CB(cb), 0)
