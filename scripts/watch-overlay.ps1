<#
.SYNOPSIS
  Record what the Flow Bar's window actually does, from outside the app.

.DESCRIPTION
  Polls GetWindowRect on the overlay's HWND at high frequency and prints one line
  every time the rectangle changes. Nothing here trusts the app: the app's own
  logs say what it *asked* for, and the whole class of bug being chased is the gap
  between that and what the window actually did. This is the ground truth to
  compare them against.

  Finding the window: the overlay is the OpenVoice process's window with
  WS_EX_NOACTIVATE and WS_EX_TOOLWINDOW set (applied in main.rs::configure_overlay),
  which distinguishes it from the Hub.

  Reading the output: each line is the time since start, the window's top-left and
  size, and the deltas from the previous line. A jump up and to the left shows as
  negative dx and dy. What matters is the DWELL — the gap to the next line. A
  wrong rectangle held for one or two frames (under ~50ms) is a compositor
  artefact; one held for hundreds of milliseconds is a logic bug, and the line
  after it tells you what corrected it.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\watch-overlay.ps1
  powershell -ExecutionPolicy Bypass -File scripts\watch-overlay.ps1 -Seconds 60 -OutFile trace.txt
#>
param(
  # How long to record for. The default is long enough to press the hotkey a
  # dozen times without hurrying.
  [int]$Seconds = 45,
  # Optional file to tee the timeline into, for pasting somewhere.
  [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public struct RECT { public int Left, Top, Right, Bottom; }

public static class Win {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint c);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern int GetClassName(IntPtr h, StringBuilder s, int max);
}
"@

$GWL_EXSTYLE      = -20
$WS_EX_NOACTIVATE = 0x08000000
$GW_CHILD         = 5

# `MainWindowHandle` only ever reports one window per process and the Hub wins it,
# so the overlay has to be found by enumerating the process's top-level windows.
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class Enumer {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);

  public static List<IntPtr> ForProcess(uint want) {
    var found = new List<IntPtr>();
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == want) found.Add(h);
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@

$proc = Get-Process -Name "OpenVoice", "openvoice" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
  Write-Host "OpenVoice is not running. Start it first, then run this again." -ForegroundColor Red
  exit 1
}

# Matched on class and size rather than on WS_EX_NOACTIVATE.
#
# The obvious filter is that ex-style, since the overlay is the window that has
# it — except it turns out not to, on a running build, which is a bug in its own
# right and exactly the sort of thing this script exists to notice. Matching on
# something the bug cannot move: the overlay is the small `Tauri Window`, the Hub
# is the large one.
$overlay = [IntPtr]::Zero
foreach ($h in [Enumer]::ForProcess([uint32]$proc.Id)) {
  $cn = New-Object System.Text.StringBuilder 256
  [void][Win]::GetClassName($h, $cn, 256)
  if ($cn.ToString() -ne "Tauri Window") { continue }
  $r = New-Object RECT
  [void][Win]::GetWindowRect($h, [ref]$r)
  $w = $r.Right - $r.Left
  if ($w -gt 20 -and $w -lt 600) { $overlay = $h; break }
}

if ($overlay -eq [IntPtr]::Zero) {
  Write-Host "Could not find the overlay window (no visible WS_EX_NOACTIVATE window in PID $($proc.Id))." -ForegroundColor Red
  exit 1
}

$child = [Win]::GetWindow($overlay, $GW_CHILD)

Write-Host ""
Write-Host "Recording the Flow Bar for $Seconds seconds. HWND 0x$("{0:X}" -f [int64]$overlay), PID $($proc.Id)." -ForegroundColor Cyan
Write-Host "Press and hold the hotkey a few times now. Ctrl+C stops early."
Write-Host ""
Write-Host "    time |      x      y |    w     h |     dx     dy |   dwell | vis | webview"
Write-Host "---------+---------------+------------+---------------+---------+-----+--------"

$lines = New-Object System.Collections.Generic.List[string]
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$last = $null
$lastAt = 0.0

while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
  $r = New-Object RECT
  if ([Win]::GetWindowRect($overlay, [ref]$r)) {
    $x = $r.Left; $y = $r.Top
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $key = "$x,$y,$w,$h"
    if ($key -ne $last) {
      $now = $sw.Elapsed.TotalMilliseconds
      $dx = ""; $dy = ""; $dwell = ""
      if ($null -ne $last) {
        $p = $last.Split(",")
        $dx = "{0,6}" -f ($x - [int]$p[0])
        $dy = "{0,6}" -f ($y - [int]$p[1])
        $dwell = "{0,6}ms" -f [int]($now - $lastAt)
      }
      $vis = if ([Win]::IsWindowVisible($overlay)) { " y " } else { " n " }

      # The webview child's client size. If this disagrees with the parent's, the
      # surface is lagging the window rather than the app placing it wrongly.
      $cw = ""
      if ($child -ne [IntPtr]::Zero) {
        $cr = New-Object RECT
        if ([Win]::GetClientRect($child, [ref]$cr)) { $cw = "$($cr.Right)x$($cr.Bottom)" }
      }

      $line = "{0,7}ms | {1,6} {2,6} | {3,4} {4,5} | {5} {6} | {7} |{8}| {9}" -f `
        [int]$now, $x, $y, $w, $h, $dx, $dy, $dwell, $vis, $cw
      Write-Host $line
      $lines.Add($line)
      $last = $key
      $lastAt = $now
    }
  }
  Start-Sleep -Milliseconds 4
}

Write-Host ""
Write-Host "Done. $($lines.Count) rectangle changes recorded." -ForegroundColor Cyan

if ($OutFile) {
  $lines | Set-Content -Path $OutFile -Encoding utf8
  Write-Host "Written to $OutFile" -ForegroundColor Cyan
}
