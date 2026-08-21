<#
.SYNOPSIS
  Record what the Flow Bar actually LOOKS like, frame by frame, while you use it.

.DESCRIPTION
  The app's own logs proved the Flow Bar's window is moved and resized correctly
  every single time — every commanded rectangle is centred exactly where it should
  be. So the remaining bug is not in the geometry, it is in what gets painted
  inside a correctly placed window, and no amount of logging from inside the app
  can see that. This captures the screen instead.

  It grabs a fixed region of the desktop around the bar, at about 30 frames a
  second, and writes each frame as a PNG named with its elapsed time and the
  window's rectangle at that moment. Play them in order and the glitch is simply
  visible, with a timestamp on it — which turns "it jumps for a second or two"
  into a frame count and a measured duration.

  The region is fixed rather than following the window, deliberately: if the bar
  paints somewhere it should not, a region that followed it would crop out the
  evidence.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\capture-flowbar.ps1 -Seconds 20
  # then press and hold the hotkey a few times while it records
#>
param(
  [int]$Seconds = 20,
  # How much desktop to keep around the bar's resting rectangle.
  [int]$Pad = 160,
  [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public struct RECT { public int Left, Top, Right, Bottom; }

public static class Cap {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static List<IntPtr> ForProcess(uint want) {
    var f = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == want) f.Add(h); return true; }, IntPtr.Zero);
    return f;
  }
}
"@

$proc = Get-Process -Name "OpenVoice", "openvoice" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { Write-Host "OpenVoice is not running." -ForegroundColor Red; exit 1 }

$overlay = [IntPtr]::Zero
foreach ($h in [Cap]::ForProcess([uint32]$proc.Id)) {
  $cn = New-Object System.Text.StringBuilder 256
  [void][Cap]::GetClassName($h, $cn, 256)
  if ($cn.ToString() -ne "Tauri Window") { continue }
  $r = New-Object RECT
  [void][Cap]::GetWindowRect($h, [ref]$r)
  $w = $r.Right - $r.Left
  if ($w -gt 20 -and $w -lt 600) { $overlay = $h; break }
}
if ($overlay -eq [IntPtr]::Zero) { Write-Host "Overlay window not found." -ForegroundColor Red; exit 1 }

if (-not $OutDir) { $OutDir = Join-Path $env:TEMP "flowbar-frames" }
if (Test-Path $OutDir) { Remove-Item "$OutDir\*.png" -Force -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# The capture region is pinned to where the bar is sitting BEFORE anything happens,
# so a bar that paints outside its resting place is still inside the frame.
$rest = New-Object RECT
[void][Cap]::GetWindowRect($overlay, [ref]$rest)
$rx = $rest.Left - $Pad
$ry = $rest.Top - $Pad
$rw = ($rest.Right - $rest.Left) + $Pad * 2
$rh = ($rest.Bottom - $rest.Top) + $Pad * 2

Write-Host ""
Write-Host ("Overlay 0x{0:X} resting at {1},{2} {3}x{4}" -f [int64]$overlay, $rest.Left, $rest.Top, ($rest.Right-$rest.Left), ($rest.Bottom-$rest.Top)) -ForegroundColor Cyan
Write-Host ("Capturing {0},{1} {2}x{3} for {4}s into {5}" -f $rx, $ry, $rw, $rh, $Seconds, $OutDir) -ForegroundColor Cyan
Write-Host "PRESS AND HOLD THE HOTKEY a few times now." -ForegroundColor Yellow
Write-Host ""

$bmp = New-Object System.Drawing.Bitmap $rw, $rh
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$n = 0
$prevKey = ""

while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
  $r = New-Object RECT
  [void][Cap]::GetWindowRect($overlay, [ref]$r)
  $vis = [Cap]::IsWindowVisible($overlay)
  $key = "$($r.Left),$($r.Top),$($r.Right-$r.Left),$($r.Bottom-$r.Top),$vis"

  $gfx.CopyFromScreen($rx, $ry, 0, 0, $bmp.Size)
  $ms = [int]$sw.Elapsed.TotalMilliseconds
  # The window rectangle goes in the filename, so a frame that looks wrong can be
  # compared against where the window actually was at that instant without
  # cross-referencing anything.
  $mark = if ($key -ne $prevKey) { "CHANGE" } else { "..." }
  $name = "{0:D6}ms_{1}_{2}.png" -f $ms, $key.Replace(",", "-"), $mark
  $bmp.Save((Join-Path $OutDir $name), [System.Drawing.Imaging.ImageFormat]::Png)
  if ($key -ne $prevKey) {
    Write-Host ("{0,7}ms  rect {1}  {2}" -f $ms, $key, $(if ($vis) { "" } else { "(hidden)" }))
    $prevKey = $key
  }
  $n++
  Start-Sleep -Milliseconds 25
}

$gfx.Dispose(); $bmp.Dispose()
Write-Host ""
Write-Host "Captured $n frames into $OutDir" -ForegroundColor Cyan
