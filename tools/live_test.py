"""Drives private Remindo instances end to end without any global input. It starts the daemon with its own data
folder, asks it over IPC to open the quick input (the same code path the hotkey takes), measures how long the window
takes to appear, where it lands and who has the focus, fires timers, restarts the daemon to check persistence and
missed timers, and checks the shortcut conflict detection. Observation only: no keys and no clicks are sent to
anything, and only processes this script started are ever stopped.

    python tools/live_test.py target/release/Remindo.exe

The shortcut part registers the default shortcut for a few seconds (REMINDO_TEST_HOTKEY=1) and then lets it go.
"""
import ctypes, json, os, subprocess, sys, tempfile, time, shutil
from ctypes import wintypes

exe = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/debug/Remindo.exe")
image = os.path.basename(exe)
u = ctypes.windll.user32
CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
ok_all = True


def check(name, cond, detail=""):
    global ok_all
    ok_all &= bool(cond)
    print(("PASS " if cond else "FAIL ") + name + (f"  [{detail}]" if detail else ""))


def make_env(data, **extra):
    return dict(os.environ, REMINDO_DATA_DIR=data, **extra)


def ipc(env, *a):
    r = subprocess.run([exe, "--ipc", *a], env=env, capture_output=True, text=True, encoding="utf-8", timeout=15)
    return r.stdout.strip()


def pids():
    out = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    return {int(l.split('","')[1]) for l in out.splitlines() if '","' in l}


baseline = pids()


def mine():
    return pids() - baseline


def windows():
    ours = mine()
    found = []

    def cb(h, l):
        pid = wintypes.DWORD()
        u.GetWindowThreadProcessId(h, ctypes.byref(pid))
        if pid.value in ours and u.IsWindowVisible(h):
            cls = ctypes.create_unicode_buffer(60)
            u.GetClassNameW(h, cls, 60)
            if "Winit" in cls.value or "Pseudo" in cls.value:
                return True
            r = wintypes.RECT()
            u.GetWindowRect(h, ctypes.byref(r))
            found.append((pid.value, h, (r.left, r.top, r.right - r.left, r.bottom - r.top)))
        return True

    u.EnumWindows(CB(cb), 0)
    return found


def foreground_pid():
    pid = wintypes.DWORD()
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), ctypes.byref(pid))
    return pid.value


def cursor_monitor_work():
    pt = wintypes.POINT()
    u.GetCursorPos(ctypes.byref(pt))
    mon = u.MonitorFromPoint(pt, 2)

    class MI(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("mon", wintypes.RECT), ("work", wintypes.RECT), ("flags", wintypes.DWORD)]

    mi = MI()
    mi.cb = ctypes.sizeof(MI)
    u.GetMonitorInfoW(mon, ctypes.byref(mi))
    w = mi.work
    return (w.left, w.top, w.right - w.left, w.bottom - w.top)


def memory(pid):
    out = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    return "".join(c for c in out.strip().split('","')[-1] if c.isdigit())


def start_daemon(env):
    p = subprocess.Popen([exe, "--background"], env=env)
    for _ in range(60):
        if ipc(env, "ping") == "pong":
            return p
        time.sleep(0.1)
    raise SystemExit("daemon did not start")


def stop_all():
    for p in mine():
        subprocess.run(["taskkill", "/PID", str(p), "/F"], capture_output=True)


data1 = tempfile.mkdtemp(prefix="remindo-live-")
env1 = make_env(data1)
try:
    # ── the quick input: the native window inside the daemon, opened through the daemon's own hotkey handler ──
    d1 = start_daemon(env1)
    area = cursor_monitor_work()
    time.sleep(1.5)
    print("daemon memory with the native popup created (KB):", memory(d1.pid), "| cursor monitor work area:", area)
    dwin = u.FindWindowW("RemindoDaemonWindow", None)
    def daemon_windows():
        return [w for w in windows() if w[0] == d1.pid and w[2][2] > 100]
    qi = u.FindWindowW("RemindoQuickInput", None)
    times = []
    for i in range(5):
        t0 = time.perf_counter()
        u.PostMessageW(dwin, 0x0312, 1, 0)  # WM_HOTKEY to our own daemon window (not a global key press)
        while time.perf_counter() - t0 < 3:
            if u.IsWindowVisible(qi) and u.GetForegroundWindow() == qi:
                break
            time.sleep(0.0005)
        times.append((time.perf_counter() - t0) * 1000)
        if i == 0:
            time.sleep(0.5)
            ws = daemon_windows()
            rect = ws[0][2] if ws else (0, 0, 0, 0)
            cx, cy = rect[0] + rect[2] / 2, rect[1] + rect[3] / 2
            check("the quick input is on the monitor with the cursor, horizontally centred", abs(cx - (area[0] + area[2] / 2)) <= 2 and area[1] <= cy <= area[1] + area[3], f"{rect}")
            check("it is 520x126 at 100% scale", rect[2:] in ((520, 126), (650, 158), (780, 189)), f"{rect[2:]}")
            u.PostMessageW(dwin, 0x0312, 1, 0)
            time.sleep(0.5)
            check("a second hotkey press never opens a second window", len(daemon_windows()) == 1)
            check("the window is topmost", bool(u.GetWindowLongW(ws[0][1], -20) & 8))
        # close it again with Escape (sent to our own window only, after a PID check)
        if foreground_pid() == d1.pid:
            ki = ctypes.windll.user32.keybd_event
            ki(0x1B, 0, 0, 0); ki(0x1B, 0, 2, 0)
        time.sleep(0.4)
    print("hotkey to focused window, ms:", [round(t, 1) for t in times])
    check("hotkey to focused input is under 50 ms (median)", sorted(times)[2] < 50, f"{sorted(times)[2]:.1f} ms")
    check("the popup is hidden after Escape", len(daemon_windows()) == 0)
    print("daemon memory after showing it five times (KB):", memory(d1.pid))

    # ── `remindo --new` (what a Wayland shortcut runs) ──
    subprocess.run([exe, "--new"], env=env1, timeout=15)
    time.sleep(1.0)
    check("--new opens the quick input through the running daemon", len(daemon_windows()) == 1)
    if foreground_pid() == d1.pid:
        ki = ctypes.windll.user32.keybd_event
        ki(0x1B, 0, 0, 0); ki(0x1B, 0, 2, 0)
    time.sleep(0.4)

    # ── a timer that fires ──
    ipc(env1, "text", "in 3 seconds", "stove")
    ipc(env1, "text", "in 3 seconds", "tea")
    time.sleep(5.5)
    ws = windows()
    check("one finished-timer window stacks both timers", len(ws) == 1 and ws[0][2][3] > 250, f"{[w[2] for w in ws]}")
    if ws:
        r = ws[0][2]
        check("it is on the cursor's monitor", area[0] <= r[0] and r[0] + r[2] <= area[0] + area[2])
        check("it did not take the keyboard", foreground_pid() != ws[0][0])
    check("both alerts are waiting", len(json.loads(ipc(env1, "alerts"))) == 2)
    ipc(env1, "dismiss_all")
    time.sleep(2.5)
    check("the window ends when the last timer is closed", len(windows()) == 0)

    # ── persistence: restart, and a timer that came due while closed ──
    ipc(env1, "text", "in 1 hour", "long one")
    ipc(env1, "text", "in 4 seconds", "short one")
    ipc(env1, "quit")
    time.sleep(1.5)
    check("the daemon exits on quit", ipc(env1, "ping") == "not running")
    time.sleep(8.0)
    d1 = start_daemon(env1)
    time.sleep(2.0)
    state = json.loads(ipc(env1, "state"))
    labels = {t["label"] for t in state["timers"]}
    check("the active timer survived the restart", "long one" in labels)
    missed = [a for a in state["alerts"] if a["label"] == "short one"]
    check("the timer that came due while closed fired at start, marked missed", len(missed) == 1 and missed[0]["missed"])
    ipc(env1, "dismiss_all")
    time.sleep(2.0)
    ipc(env1, "quit")
    time.sleep(1.0)

    # ── the shortcut: Ctrl+Alt+R for a few seconds, then conflict detection ──
    data_a, data_b = tempfile.mkdtemp(prefix="remindo-hk-a-"), tempfile.mkdtemp(prefix="remindo-hk-b-")
    env_a, env_b = make_env(data_a, REMINDO_TEST_HOTKEY="1"), make_env(data_b, REMINDO_TEST_HOTKEY="1")
    start_daemon(env_a)
    time.sleep(0.5)
    st = json.loads(ipc(env_a, "state"))
    check("Ctrl+Alt+R is free on this PC", st["hotkey"] == "Ctrl+Alt+R" and st["hotkey_error"] is None, str(st["hotkey_error"]))
    start_daemon(env_b)
    time.sleep(0.5)
    st = json.loads(ipc(env_b, "state"))
    check("a second app taking the same shortcut is told it is taken", st["hotkey_error"] is not None, str(st["hotkey_error"]))
    r = json.loads(ipc(env_b, "hotkey", "Ctrl+Alt+Shift+F11"))
    check("changing to a free shortcut works", r.get("ok") is True)
    r = json.loads(ipc(env_b, "hotkey", "Ctrl+Alt+R"))
    check("changing to a taken shortcut is refused", r.get("ok") is False and r.get("err") == "taken", str(r))
    st = json.loads(ipc(env_b, "state"))
    check("the old shortcut is kept after a failed change", st["hotkey"] == "Ctrl+Alt+Shift+F11")
    ipc(env_b, "quit")
    ipc(env_a, "quit")
    time.sleep(1.0)
    shutil.rmtree(data_a, ignore_errors=True)
    shutil.rmtree(data_b, ignore_errors=True)
finally:
    stop_all()
    shutil.rmtree(data1, ignore_errors=True)
print("ALL PASS" if ok_all else "SOME FAILED")
sys.exit(0 if ok_all else 1)
