"""Types into the quick input of a private Remindo instance, to prove that the focus really lands and that Enter
starts a timer. Synthetic input goes only into Remindo's own popup: before every single key the script checks that
the foreground window belongs to the popup process, and stops at the first mismatch. Nothing is registered globally.

    python tools/type_test.py target/release/Remindo.exe
"""
import ctypes, json, os, shutil, subprocess, sys, tempfile, time
from ctypes import wintypes

exe = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/debug/Remindo.exe")
image = os.path.basename(exe)
u = ctypes.windll.user32
data = tempfile.mkdtemp(prefix="remindo-type-")
env = dict(os.environ, REMINDO_DATA_DIR=data)


def ipc(*a):
    return subprocess.run([exe, "--ipc", *a], env=env, capture_output=True, text=True, timeout=15).stdout.strip()


def pids():
    out = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    return {int(l.split('","')[1]) for l in out.splitlines() if '","' in l}


def foreground_pid():
    pid = wintypes.DWORD()
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), ctypes.byref(pid))
    return pid.value


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD), ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wintypes.LONG), ("dy", wintypes.LONG), ("mouseData", wintypes.DWORD), ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT)]

    _anonymous_ = ("u",)
    _fields_ = [("type", wintypes.DWORD), ("u", _U)]


def send(vk=0, ch=None, up=False):
    flags = (2 if up else 0) | (4 if ch is not None else 0)
    i = INPUT(type=1)
    i.ki = KEYBDINPUT(wVk=vk, wScan=ord(ch) if ch is not None else 0, dwFlags=flags, time=0, dwExtraInfo=0)
    u.SendInput(1, ctypes.byref(i), ctypes.sizeof(INPUT))


def guarded(popup_pid, what):
    if foreground_pid() != popup_pid:
        raise SystemExit(f"STOP: the foreground window is not the popup (before {what}); nothing more is sent")


def type_text(popup_pid, text):
    for ch in text:
        guarded(popup_pid, repr(ch))
        send(ch=ch)
        send(ch=ch, up=True)
        time.sleep(0.02)


def press(popup_pid, vk, name):
    guarded(popup_pid, name)
    send(vk)
    send(vk, up=True)


baseline = pids()
ok = True
daemon = subprocess.Popen([exe, "--background"], env=env)
try:
    for _ in range(60):
        if ipc("ping") == "pong":
            break
        time.sleep(0.1)
    # 1. type a line and press Enter
    ipc("new")
    popup = None
    for _ in range(100):
        new = pids() - baseline - {daemon.pid}
        if new and foreground_pid() in new:
            popup = foreground_pid()
            break
        time.sleep(0.05)
    print("popup process with the focus:", popup)
    if popup is None:
        raise SystemExit("FAIL: the popup did not get the focus")
    type_text(popup, "in 20 minutes turn off the stove")
    time.sleep(0.3)
    press(popup, 0x0D, "Enter")
    time.sleep(1.2)
    state = json.loads(ipc("state"))
    t = state["timers"]
    good = len(t) == 1 and t[0]["label"] == "turn off the stove" and t[0]["secs"] == 1200
    print("PASS" if good else "FAIL", "Enter created the timer:", [(x["label"], x["secs"]) for x in t])
    ok &= good
    gone = popup not in pids()
    print("PASS" if gone else "FAIL", "the popup closed after Enter")
    ok &= gone

    # 2. Escape closes without a timer
    ipc("new")
    popup = None
    for _ in range(100):
        new = pids() - baseline - {daemon.pid}
        if new and foreground_pid() in new:
            popup = foreground_pid()
            break
        time.sleep(0.05)
    type_text(popup, "in 5 min should not be created")
    press(popup, 0x1B, "Escape")
    time.sleep(1.2)
    state = json.loads(ipc("state"))
    good = len(state["timers"]) == 1 and popup not in pids()
    print("PASS" if good else "FAIL", "Escape closed the popup without a timer; timers:", len(state["timers"]))
    ok &= good

    # 3. a line the parser rejects: Enter keeps the popup open and shows the hint
    ipc("new")
    popup = None
    for _ in range(100):
        new = pids() - baseline - {daemon.pid}
        if new and foreground_pid() in new:
            popup = foreground_pid()
            break
        time.sleep(0.05)
    type_text(popup, "stove")
    press(popup, 0x0D, "Enter")
    time.sleep(1.0)
    good = popup in pids() and len(json.loads(ipc("state"))["timers"]) == 1
    print("PASS" if good else "FAIL", "Enter on text without a time keeps the popup open and creates nothing")
    ok &= good
    press(popup, 0x1B, "Escape")
    time.sleep(1.0)
finally:
    ipc("quit")
    time.sleep(1.0)
    for p in pids() - baseline:
        subprocess.run(["taskkill", "/PID", str(p), "/F"], capture_output=True)
    shutil.rmtree(data, ignore_errors=True)
print("ALL PASS" if ok else "SOME FAILED")
sys.exit(0 if ok else 1)
