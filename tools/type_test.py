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
    return subprocess.run([exe, "--ipc", *a], env=env, capture_output=True, text=True, encoding="utf-8", timeout=15).stdout.strip()


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


def report(name, good, detail=""):
    global ok
    ok &= bool(good)
    print(("PASS " if good else "FAIL ") + name + (f"  [{detail}]" if detail else ""))


def open_popup(dwin, qi):
    """Opens the native popup the way the hotkey does and returns as soon as it has the focus."""
    t0 = time.perf_counter()
    u.PostMessageW(dwin, 0x0312, 1, 0)
    while time.perf_counter() - t0 < 3:
        if u.IsWindowVisible(qi) and u.GetForegroundWindow() == qi:
            return (time.perf_counter() - t0) * 1000
        time.sleep(0.0003)
    raise SystemExit("FAIL: the popup did not get the focus")


try:
    for _ in range(60):
        if ipc("ping") == "pong":
            break
        time.sleep(0.1)
    time.sleep(1.0)
    dwin = u.FindWindowW("RemindoDaemonWindow", None)
    qi = u.FindWindowW("RemindoQuickInput", None)
    pid = daemon.pid

    # 1. the very first key, sent the instant the window has the focus
    ms = open_popup(dwin, qi)
    print(f"focus after {ms:.1f} ms; the first key goes out right now")
    type_text(pid, "5m tea")          # the first character is sent without any pause after the focus
    press(pid, 0x0D, "Enter")
    time.sleep(0.5)
    t = json.loads(ipc("state"))["timers"]
    report("the first keystroke lands in the input (5m tea -> 300 s, tea)", len(t) == 1 and t[0]["secs"] == 300 and t[0]["label"] == "tea", str([(x["label"], x["secs"]) for x in t]))
    report("the popup closed after Enter", not u.IsWindowVisible(qi))

    # 2. Cyrillic, typed as real characters
    open_popup(dwin, qi)
    type_text(pid, "через 7 минут чай")
    press(pid, 0x0D, "Enter")
    time.sleep(0.5)
    t = json.loads(ipc("state"))["timers"]
    got = [x for x in t if x["secs"] == 420]
    report("Cyrillic text is typed and parsed", len(got) == 1 and got[0]["label"] == "чай", ascii(got))

    # 3. Ctrl+A and Ctrl+Backspace
    open_popup(dwin, qi)
    type_text(pid, "in 9 min junk words")
    guarded(pid, "Ctrl+Backspace"); send(0x11); send(0x08); send(0x08, up=True); send(0x11, up=True)
    send(0x11); send(0x08); send(0x08, up=True); send(0x11, up=True)
    type_text(pid, "ok")
    guarded(pid, "Ctrl+A"); send(0x11); send(0x41); send(0x41, up=True); send(0x11, up=True)
    type_text(pid, "in 2 min replaced")
    press(pid, 0x0D, "Enter")
    time.sleep(0.5)
    t = json.loads(ipc("state"))["timers"]
    report("Ctrl+Backspace and Ctrl+A edit the text", any(x["secs"] == 120 and x["label"] == "replaced" for x in t), str([(x["label"], x["secs"]) for x in t]))

    # 4. Escape closes without a timer; the Up arrow recalls the last entry; text without a time keeps it open
    n = len(json.loads(ipc("state"))["timers"])
    open_popup(dwin, qi)
    type_text(pid, "in 5 min must not exist")
    press(pid, 0x1B, "Escape")
    time.sleep(0.4)
    report("Escape closes without a timer", not u.IsWindowVisible(qi) and len(json.loads(ipc("state"))["timers"]) == n)
    open_popup(dwin, qi)
    press(pid, 0x26, "Up")
    time.sleep(0.2)
    press(pid, 0x0D, "Enter")
    time.sleep(0.5)
    t = json.loads(ipc("state"))["timers"]
    report("the Up arrow recalls the last entry", len(t) == n + 1 and t[-1]["label"] == "replaced")
    open_popup(dwin, qi)
    type_text(pid, "stove")
    press(pid, 0x0D, "Enter")
    time.sleep(0.4)
    report("Enter on text without a time keeps the popup open and creates nothing", u.IsWindowVisible(qi) and len(json.loads(ipc("state"))["timers"]) == n + 1)
    press(pid, 0x1B, "Escape")
    time.sleep(0.3)
finally:
    ipc("quit")
    time.sleep(1.0)
    for p in pids() - baseline:
        subprocess.run(["taskkill", "/PID", str(p), "/F"], capture_output=True)
    shutil.rmtree(data, ignore_errors=True)
print("ALL PASS" if ok else "SOME FAILED")
sys.exit(0 if ok else 1)
