<#
.SYNOPSIS
  Reproduce the Flow Bar's position glitch without a human at the keyboard.

.DESCRIPTION
  Holds the hotkey with SendInput while polling GetWindowRect on the overlay's
  HWND, and prints the resulting timeline. This is what turns "it jumps to the top
  left sometimes" into a table with numbers and durations in it.

  The point of driving the key from here rather than by hand is repeatability: the
  hold and the gap are exact, so two runs can be compared, and the press timings
  appear in the same timeline as the movements they caused.

  Right Ctrl is sent as an extended key, which is how the real key reports itself
  and therefore how `ov-input`'s hook recognises it. No microphone is needed — the
  session will start, capture nothing and end, which exercises every window
  transition the bug lives in.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\repro-flowbar.ps1
  powershell -ExecutionPolicy Bypass -File scripts\repro-flowbar.ps1 -Presses 6 -HoldMs 900
#>
param(
  # How many press/release cycles to perform.
  [int]$Presses = 5,
  # How long to hold the key down each time.
  [int]$HoldMs = 1200,
  # Gap between releasing and pressing again. Short values are the "spamming"
  # case the bug is worst in.
  [int]$GapMs = 900,
  [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public struct RECT { public int Left, Top, Right, Bottom; }

[StructLayout(LayoutKind.Sequential)]
public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }

[StructLayout(LayoutKind.Explicit, Size = 40)]
public struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT ki; }

public static class Ov {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint c);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] i, int size);

  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out uint pid);

  public static List<IntPtr> ForProcess(uint want) {
    var f = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == want) f.Add(h); return true; }, IntPtr.Zero);
    return f;
  }

  const uint KEYEVENTF_KEYUP = 0x0002, KEYEVENTF_EXTENDEDKEY = 0x0001;
  const ushort VK_RCONTROL = 0xA3, VK_ESCAPE = 0x1B;

  static void Send(ushort vk, ushort scan, bool extended, bool down) {
    var i = new INPUT[1];
    i[0].type = 1; // INPUT_KEYBOARD
    i[0].ki.wVk = vk;
    i[0].ki.wScan = scan;
    i[0].ki.dwFlags = (extended ? KEYEVENTF_EXTENDEDKEY : 0u) | (down ? 0u : KEYEVENTF_KEYUP);
    SendInput(1, i, Marshal.SizeOf(typeof(INPUT)));
  }
  public static void KeyDown() { Send(VK_RCONTROL, 0x1D, true, true); }
  public static void KeyUp()   { Send(VK_RCONTROL, 0x1D, true, false); }

  // Discard whatever the synthetic press started, so this diagnostic cannot type
  // into whichever window happens to have the caret. Escape is the app's own
  // cancel path, so nothing is ever transcribed or injected.
  public static void Escape() {
    Send(VK_ESCAPE, 0x01, false, true);
    Send(VK_ESCAPE, 0x01, false, false);
  }
}
"@

$proc = Get-Process -Name "OpenVoice", "openvoice" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { Write-Host "OpenVoice is not running." -ForegroundColor Red; exit 1 }

$overlay = [IntPtr]::Zero
foreach ($h in [Ov]::ForProcess([uint32]$proc.Id)) {
  $cn = New-Object System.Text.StringBuilder 256
  [void][Ov]::GetClassName($h, $cn, 256)
  if ($cn.ToString() -ne "Tauri Window") { continue }
  $r = New-Object RECT
  [void][Ov]::GetWindowRect($h, [ref]$r)
  $w = $r.Right - $r.Left
  if ($w -gt 20 -and $w -lt 600) { $overlay = $h; break }
}
if ($overlay -eq [IntPtr]::Zero) { Write-Host "Overlay window not found." -ForegroundColor Red; exit 1 }

$child = [Ov]::GetWindow($overlay, 5)  # GW_CHILD
$ex = [Ov]::GetWindowLong($overlay, -20)

Write-Host ""
Write-Host ("Overlay HWND 0x{0:X}  PID {1}  exstyle 0x{2:X8}" -f [int64]$overlay, $proc.Id, $ex) -ForegroundColor Cyan
Write-Host ("WS_EX_NOACTIVATE {0}   WS_EX_TOOLWINDOW {1}" -f `
  $(if ($ex -band 0x08000000) { "SET" } else { "MISSING" }), `
  $(if ($ex -band 0x00000080) { "SET" } else { "MISSING" })) -ForegroundColor Cyan
Write-Host "$Presses presses, holding ${HoldMs}ms, ${GapMs}ms apart. Do not touch the keyboard." -ForegroundColor Cyan
Write-Host ""
Write-Host "    time | event         |      x      y |    w     h |     dx     dy |   dwell | vis"
Write-Host "---------+---------------+---------------+------------+---------------+---------+----"

$lines = New-Object System.Collections.Generic.List[string]
function Emit($s) { Write-Host $s; $lines.Add($s) }

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$last = $null; $lastAt = 0.0
$pending = ""

function Sample {
  $r = New-Object RECT
  if (-not [Ov]::GetWindowRect($script:overlay, [ref]$r)) { return }
  $x = $r.Left; $y = $r.Top; $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
  $key = "$x,$y,$w,$h"
  if ($key -eq $script:last -and $script:pending -eq "") { return }
  $now = $script:sw.Elapsed.TotalMilliseconds
  $dx = "      "; $dy = "      "; $dwell = "        "
  if ($null -ne $script:last) {
    $p = $script:last.Split(",")
    $dx = "{0,6}" -f ($x - [int]$p[0]); $dy = "{0,6}" -f ($y - [int]$p[1])
    $dwell = "{0,6}ms" -f [int]($now - $script:lastAt)
  }
  $vis = if ([Ov]::IsWindowVisible($script:overlay)) { " y" } else { " n" }
  Emit ("{0,7}ms | {1,-13} | {2,6} {3,6} | {4,4} {5,5} | {6} {7} | {8} |{9}" -f `
    [int]$now, $script:pending, $x, $y, $w, $h, $dx, $dy, $dwell, $vis)
  $script:pending = ""
  $script:last = $key; $script:lastAt = $now
}

function Spin([int]$ms) {
  $until = $sw.Elapsed.TotalMilliseconds + $ms
  while ($sw.Elapsed.TotalMilliseconds -lt $until) { Sample; Start-Sleep -Milliseconds 3 }
}

Spin 400
for ($i = 1; $i -le $Presses; $i++) {
  $pending = "DOWN #$i"; [Ov]::KeyDown(); Sample
  Spin $HoldMs
  $pending = "UP #$i"; [Ov]::KeyUp(); Sample
  Spin 60
  # Nothing this script starts is allowed to reach the user's documents.
  $pending = "ESC #$i"; [Ov]::Escape(); Sample
  Spin ($GapMs - 60)
}
# The tail matters: a correction that arrives a second after the last release is
# the whole point of the recording.
Spin 3000

Write-Host ""
Write-Host "Done. $($lines.Count) events." -ForegroundColor Cyan
if ($OutFile) { $lines | Set-Content -Path $OutFile -Encoding utf8; Write-Host "Written to $OutFile" -ForegroundColor Cyan }
