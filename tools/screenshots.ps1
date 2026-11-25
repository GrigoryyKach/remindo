# Takes the README screenshots from demo data. Nothing here touches real timers or the real hotkey:
# every run uses a throw-away data folder, the daemon is never started, and each window saves its own frame
# (no screen capture, no keys, no clicks).
#
#   powershell -File tools\screenshots.ps1 -Exe target\release\Remindo.exe -Out assets\screenshots
param(
    [string]$Exe = "target\release\Remindo.exe",
    [string]$Out = "assets\screenshots",
    [string]$Work = (Join-Path $env:TEMP "remindo-shots")
)
$ErrorActionPreference = "Stop"
$Exe = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path

# The script file may be read as ANSI by Windows PowerShell 5, so the Russian sample is built from code points:
# "1ч15м - плита"
$ruCompact = -join ([char[]](0x31, 0x447, 0x31, 0x35, 0x43C, 0x20, 0x2D, 0x20, 0x43F, 0x43B, 0x438, 0x442, 0x430))

# (theme, language, mode, name, optional text, optional settings page)
$runs = @(
    @{ theme = "dark";  lang = "en"; mode = "--popup";    name = "popup" },
    @{ theme = "light"; lang = "en"; mode = "--popup";    name = "popup" },
    @{ theme = "dark";  lang = "ru"; mode = "--popup";    name = "popup" },
    @{ theme = "dark";  lang = "uk"; mode = "--popup";    name = "popup" },
    @{ theme = "dark";  lang = "en"; mode = "--popup";    name = "popup-tomorrow"; text = "at 6 pm tomorrow call the dentist" },
    @{ theme = "dark";  lang = "ru"; mode = "--popup";    name = "popup-compact";  text = $ruCompact },
    @{ theme = "dark";  lang = "en"; mode = "--popup";    name = "popup-hint";     text = "stove" },
    @{ theme = "dark";  lang = "en"; mode = "--alerts";   name = "alerts" },
    @{ theme = "light"; lang = "en"; mode = "--alerts";   name = "alerts" },
    @{ theme = "dark";  lang = "ru"; mode = "--alerts";   name = "alerts" },
    @{ theme = "dark";  lang = "en"; mode = "--settings"; name = "timers";   nav = "timers" },
    @{ theme = "light"; lang = "en"; mode = "--settings"; name = "timers";   nav = "timers" },
    @{ theme = "dark";  lang = "ru"; mode = "--settings"; name = "timers";   nav = "timers" },
    @{ theme = "dark";  lang = "uk"; mode = "--settings"; name = "timers";   nav = "timers" },
    @{ theme = "dark";  lang = "en"; mode = "--settings"; name = "history";  nav = "history" },
    @{ theme = "dark";  lang = "en"; mode = "--settings"; name = "settings"; nav = "settings" },
    @{ theme = "light"; lang = "en"; mode = "--settings"; name = "settings"; nav = "settings" }
)
foreach ($r in $runs) {
    $data = Join-Path $Work "data-$($r.theme)-$($r.lang)"
    Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $data | Out-Null
    $env:REMINDO_DATA_DIR = $data
    $env:REMINDO_DEMO = "1"
    $env:REMINDO_THEME = $r.theme
    $env:REMINDO_LANG = $r.lang
    $env:REMINDO_SHOT = Join-Path $Out "$($r.theme)-$($r.lang)-$($r.name).png"
    foreach ($v in "REMINDO_SHOT_TEXT", "REMINDO_NAV") { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
    if ($r.text) { $env:REMINDO_SHOT_TEXT = $r.text }
    if ($r.nav) { $env:REMINDO_NAV = $r.nav }
    $p = Start-Process -FilePath $Exe -ArgumentList @($r.mode, "--demo") -PassThru
    if (-not $p.WaitForExit(60000)) { $p.Kill(); throw "timed out: $($r.mode) $($r.name)" }
}
foreach ($v in "REMINDO_DATA_DIR", "REMINDO_DEMO", "REMINDO_THEME", "REMINDO_LANG", "REMINDO_SHOT", "REMINDO_SHOT_TEXT", "REMINDO_NAV") { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue

# The native popup (Windows): the daemon shows it once, saves its own frame (WM_PRINT-free GDI redraw) and quits.
# Nothing is registered: a private data folder and no hotkey.
$ruCompact2 = -join ([char[]](0x31, 0x447, 0x31, 0x35, 0x43C, 0x20, 0x2D, 0x20, 0x43F, 0x43B, 0x438, 0x442, 0x430))
$ruText = -join ([char[]](0x447, 0x435, 0x440, 0x435, 0x437, 0x20, 0x31, 0x35, 0x20, 0x43C, 0x438, 0x43D, 0x443, 0x442, 0x20, 0x443, 0x440, 0x43E, 0x43D, 0x438, 0x442, 0x44C, 0x20, 0x43F, 0x440, 0x43E, 0x431, 0x43B, 0x435, 0x43C, 0x443))
$native = @(
    @{ theme = "dark";  lang = "en"; name = "native-popup"; text = "in 15 minutes turn off the stove" },
    @{ theme = "light"; lang = "en"; name = "native-popup"; text = "in 15 minutes turn off the stove" },
    @{ theme = "dark";  lang = "ru"; name = "native-popup"; text = $ruCompact2 },
    @{ theme = "dark";  lang = "en"; name = "native-popup-tomorrow"; text = "at 6 pm tomorrow gypsy ryg" },
    @{ theme = "dark";  lang = "en"; name = "native-popup-hint"; text = "stove" }
)
foreach ($r in $native) {
    $data = Join-Path $env:TEMP "remindo-shots-native"
    Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $data | Out-Null
    $env:REMINDO_DATA_DIR = $data
    $env:REMINDO_THEME = $r.theme
    $env:REMINDO_LANG = $r.lang
    $env:REMINDO_SHOT_TEXT = $r.text
    $env:REMINDO_NATIVE_SHOT = Join-Path $Out "$($r.theme)-$($r.lang)-$($r.name).png"
    $p = Start-Process -FilePath $Exe -ArgumentList @("--background") -PassThru
    if (-not $p.WaitForExit(30000)) { $p.Kill(); throw "timed out: native $($r.name)" }
}
foreach ($v in "REMINDO_DATA_DIR", "REMINDO_THEME", "REMINDO_LANG", "REMINDO_SHOT_TEXT", "REMINDO_NATIVE_SHOT") { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
Remove-Item -Recurse -Force (Join-Path $env:TEMP "remindo-shots-native") -ErrorAction SilentlyContinue
