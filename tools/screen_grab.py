"""Opens the native quick input of a private daemon (WM_HOTKEY to our own daemon window), fills the EDIT control with
WM_SETTEXT, and grabs the pixels of that window from the screen. Our own window only; no keys, no clicks."""
import ctypes, os, subprocess, sys, tempfile, time, shutil
from ctypes import wintypes
from PIL import ImageGrab
exe = os.path.abspath(sys.argv[1]); out = sys.argv[2]; text = sys.argv[3] if len(sys.argv) > 3 else "in 15 minutes turn off the stove"
theme = sys.argv[4] if len(sys.argv) > 4 else "dark"
u = ctypes.windll.user32
data = tempfile.mkdtemp(prefix="remindo-grab-")
env = dict(os.environ, REMINDO_DATA_DIR=data, REMINDO_THEME=theme)
d = subprocess.Popen([exe, "--background"], env=env)
try:
    time.sleep(2)
    dwin = u.FindWindowW("RemindoDaemonWindow", None); qi = u.FindWindowW("RemindoQuickInput", None)
    u.PostMessageW(dwin, 0x0312, 1, 0); time.sleep(0.6)
    edit = u.FindWindowExW(qi, None, "EDIT", None)
    u.SendMessageW(edit, 0x000C, 0, ctypes.c_wchar_p(text)); time.sleep(0.5)
    r = wintypes.RECT(); u.GetWindowRect(qi, ctypes.byref(r))
    ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom), all_screens=True).save(out)
    print("saved", out, (r.left, r.top, r.right - r.left, r.bottom - r.top))
finally:
    subprocess.run([exe, "--ipc", "quit"], env=env, capture_output=True); time.sleep(1); d.kill(); shutil.rmtree(data, ignore_errors=True)
