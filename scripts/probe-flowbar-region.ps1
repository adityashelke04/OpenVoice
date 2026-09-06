<#
.SYNOPSIS
  Compare the Flow Bar's window region against what it actually paints, row by row.

.DESCRIPTION
  The overlay window is clipped with `SetWindowRgn`, and that clip is aliased: a
  physical pixel is in the region or it is not. The bar's edges are painted
  antialiased. So every bug in this family looks the same from the outside — a
  hard staircase along a curve, or a light fringe outside one — and the only way
  to tell which is to put the two boundaries side by side.

  This reads the live region out of the window with `GetWindowRgn`, collapses it
  into one span per physical row, screenshots the desktop underneath it, and
  prints the pixels immediately inside and outside each row's span.

  How to read it:

    * The outermost pixel inside the span is a strong, saturated edge colour, and
      the pixel outside it is the desktop.
        -> The region is cutting inside the paint. Its staircase IS the edge the
           user sees. This is the bug fixed by `scanlines` in overlay.rs.
    * The outermost pixel inside the span is the desktop, shading into the edge
      colour a pixel or two further in.
        -> Correct. The region contains the paint and its own staircase falls on
           transparent pixels.
    * The outermost pixel inside the span is lighter than both the bar and the
      desktop.
        -> Unpainted webview surface: the region is reaching somewhere no element
           draws. This is the Flow Menu's old corner notches.

  Run it while the bar is in the state being investigated. The yellow loading and
  notice states are the ones that show this family of bug first, because they are
  the only ones that paint the border in a colour that contrasts with the pill.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\probe-flowbar-region.ps1
#>
param(
  # Where to write the screenshot the numbers were read from.
  [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public struct RECT { public int Left, Top, Right, Bottom; }

public static class Rgn {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll")] public static extern int GetWindowRgn(IntPtr h, IntPtr rgn);
  [DllImport("gdi32.dll")] public static extern IntPtr CreateRectRgn(int a, int b, int c, int d);
  [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr o);
  [DllImport("gdi32.dll")] public static extern uint GetRegionData(IntPtr rgn, uint count, IntPtr data);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out uint pid);

  public static List<IntPtr> ForProcess(uint want) {
    var f = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == want) f.Add(h); return true; }, IntPtr.Zero);
    return f;
  }

  // The region's rectangles, in window coordinates, as "l,t,r,b". GDI stores a
  // region as scanline rectangles, so a rounded box comes back as its staircase.
  public static List<string> Rects(IntPtr hwnd) {
    var found = new List<string>();
    IntPtr rgn = CreateRectRgn(0, 0, 1, 1);
    if (GetWindowRgn(hwnd, rgn) == 0) { DeleteObject(rgn); return found; }
    uint size = GetRegionData(rgn, 0, IntPtr.Zero);
    IntPtr buf = Marshal.AllocHGlobal((int)size);
    GetRegionData(rgn, size, buf);
    int count = Marshal.ReadInt32(buf, 8);   // RGNDATAHEADER.nCount
    const int head = 32;                     // sizeof(RGNDATAHEADER)
    for (int i = 0; i < count; i++) {
      found.Add(Marshal.ReadInt32(buf, head + i*16)      + "," +
                Marshal.ReadInt32(buf, head + i*16 + 4)  + "," +
                Marshal.ReadInt32(buf, head + i*16 + 8)  + "," +
                Marshal.ReadInt32(buf, head + i*16 + 12));
    }
    Marshal.FreeHGlobal(buf);
    DeleteObject(rgn);
    return found;
  }
}
"@

$proc = Get-Process -Name "OpenVoice", "openvoice" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { Write-Host "OpenVoice is not running." -ForegroundColor Red; exit 1 }

# The overlay is the tall, narrow one. The Hub is much wider.
$overlay = [IntPtr]::Zero
foreach ($h in [Rgn]::ForProcess([uint32]$proc.Id)) {
  $cn = New-Object System.Text.StringBuilder 256
  [void][Rgn]::GetClassName($h, $cn, 256)
  if ($cn.ToString() -ne "Tauri Window") { continue }
  if (-not [Rgn]::IsWindowVisible($h)) { continue }
  $r = New-Object RECT
  [void][Rgn]::GetWindowRect($h, [ref]$r)
  if (($r.Right - $r.Left) -lt 900 -and ($r.Bottom - $r.Top) -gt 100) { $overlay = $h; break }
}
if ($overlay -eq [IntPtr]::Zero) { Write-Host "Overlay window not found." -ForegroundColor Red; exit 1 }

$win = New-Object RECT
[void][Rgn]::GetWindowRect($overlay, [ref]$win)
$rects = [Rgn]::Rects($overlay)
if ($rects.Count -eq 0) { Write-Host "No region is set on the overlay." -ForegroundColor Red; exit 1 }

# One span per row, which is the shape the eye sees.
$rows = @{}
$minL = [int]::MaxValue; $minT = [int]::MaxValue; $maxR = 0; $maxB = 0
foreach ($s in $rects) {
  $p = $s.Split(",")
  $l = [int]$p[0]; $t = [int]$p[1]; $r = [int]$p[2]; $b = [int]$p[3]
  if ($l -lt $minL) { $minL = $l }
  if ($t -lt $minT) { $minT = $t }
  if ($r -gt $maxR) { $maxR = $r }
  if ($b -gt $maxB) { $maxB = $b }
  for ($y = $t; $y -lt $b; $y++) {
    if ($rows.ContainsKey($y)) {
      $cur = $rows[$y]
      $rows[$y] = @([Math]::Min($cur[0], $l), [Math]::Max($cur[1], $r))
    } else { $rows[$y] = @($l, $r) }
  }
}

Write-Host ""
Write-Host ("window    {0},{1}  {2}x{3}" -f $win.Left, $win.Top, ($win.Right-$win.Left), ($win.Bottom-$win.Top)) -ForegroundColor Cyan
Write-Host ("region    {0},{1}..{2},{3}   {4}x{5}   {6} scanline rects" -f $minL, $minT, $maxR, $maxB, ($maxR-$minL), ($maxB-$minT), $rects.Count) -ForegroundColor Cyan

# Screenshot the region's box plus a little desktop on every side, so the pixels
# outside the clip are in frame too.
$slack = 6
$bmp = New-Object System.Drawing.Bitmap (($maxR-$minL) + $slack*2), (($maxB-$minT) + $slack*2)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($win.Left + $minL - $slack, $win.Top + $minT - $slack, 0, 0, $bmp.Size)
$g.Dispose()
if (-not $OutFile) { $OutFile = Join-Path $env:TEMP "flowbar-region.png" }
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Host ("screenshot {0}" -f $OutFile) -ForegroundColor Cyan
Write-Host ""

function Hex($x, $y) {
  if ($x -lt 0 -or $y -lt 0 -or $x -ge $bmp.Width -or $y -ge $bmp.Height) { return "......" }
  $c = $bmp.GetPixel($x, $y)
  "{0:X2}{1:X2}{2:X2}" -f $c.R, $c.G, $c.B
}

Write-Host " row |  span (window x) | left:  out    | in ->                        | right:                    <- in | out" -ForegroundColor Yellow
foreach ($y in ($rows.Keys | Sort-Object)) {
  $l = $rows[$y][0]; $r = $rows[$y][1]
  $by = $y - $minT + $slack
  $left  = (-1..3   | ForEach-Object { Hex (($l - $minL + $slack) + $_) $by }) -join " "
  $right = (-4..0   | ForEach-Object { Hex (($r - $minL + $slack) + $_) $by }) -join " "
  $out   = Hex (($r - $minL + $slack)) $by
  Write-Host ("{0,4} | {1,5}..{2,-5} | {3} | {4} | {5}" -f $y, $l, $r, $left, $right, $out)
}

$bmp.Dispose()
