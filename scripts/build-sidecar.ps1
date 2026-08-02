<#
.SYNOPSIS
  Freeze the ASR sidecar into a standalone folder with no Python dependency.

.DESCRIPTION
  Produces sidecar/dist/openvoice-asr/, which `cargo tauri build` then copies into
  the installer. On the machine that installs OpenVoice there is no Python, no
  virtualenv and no pip — this folder is the entire speech engine.

  The build is not considered finished until the frozen binary answers a real
  protocol request. PyInstaller reports success for a binary that dies on its
  first import, because a missing hidden import is invisible until run time; a
  green build that produces a broken engine is worse than a red one.

.PARAMETER Python
  Interpreter of the environment holding faster-whisper. Defaults to
  $env:OPENVOICE_PYTHON, then the repo's .venv.

.PARAMETER Clean
  Remove previous build and dist output first. Use after changing dependencies —
  PyInstaller caches aggressively and will happily reuse a stale analysis.

.EXAMPLE
  pwsh scripts/build-sidecar.ps1 -Clean
#>
[CmdletBinding()]
param(
    [string]$Python,
    [switch]$Clean
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$sidecar = Join-Path $repo 'sidecar'
$dist = Join-Path $sidecar 'dist\openvoice-asr'

function Resolve-Python {
    $candidates = @(
        $Python,
        $env:OPENVOICE_PYTHON,
        (Join-Path $repo '.venv\Scripts\python.exe'),
        (Join-Path $sidecar '.venv\Scripts\python.exe')
    )
    foreach ($c in $candidates) {
        if ($c -and (Test-Path $c)) { return (Resolve-Path $c).Path }
    }
    throw "No Python with faster-whisper found. Pass -Python, or set OPENVOICE_PYTHON."
}

$py = Resolve-Python
Write-Host "interpreter  $py"

# Check the environment here, with a sentence, rather than 200 lines into
# PyInstaller's analysis.
#
# These probes swallow their own ImportError and report on stdout instead of
# failing. That is not defensiveness for its own sake: in Windows PowerShell 5.1
# *any* stderr output from a native executable becomes an ErrorRecord, which under
# `$ErrorActionPreference = 'Stop'` aborts the script — so a Python traceback we
# fully expected would kill the build with an unrelated message. A probe that
# never writes to stderr cannot do that.
function Test-Import([string]$module) {
    $code = "try:`n import $module`n print('yes')`nexcept Exception:`n print('no')"
    return (& $py -c $code) -eq 'yes'
}

if (-not (Test-Import 'faster_whisper')) {
    throw "$py cannot import faster_whisper. Wrong environment?"
}

if (-not (Test-Import 'PyInstaller')) {
    Write-Host "installing PyInstaller into the build environment"
    # A uv-created virtualenv has no pip in it at all — uv installs into the
    # environment from outside. Both layouts are normal here, so try the one that
    # is actually present rather than assuming pip exists.
    $uv = Get-Command uv -ErrorAction SilentlyContinue
    if (Test-Import 'pip') {
        & $py -m pip install --quiet 'pyinstaller>=6.11'
    }
    elseif ($uv) {
        & $uv.Source pip install --python $py --quiet 'pyinstaller>=6.11'
    }
    else {
        throw "This environment has neither pip nor uv, so PyInstaller cannot be installed. Install it yourself into $py and re-run."
    }
    if (-not (Test-Import 'PyInstaller')) { throw "could not install PyInstaller" }
}

if ($Clean) {
    foreach ($d in @((Join-Path $sidecar 'build'), (Join-Path $sidecar 'dist'))) {
        if (Test-Path $d) {
            Write-Host "removing $d"
            Remove-Item $d -Recurse -Force
        }
    }
}

Push-Location $sidecar
try {
    Write-Host "freezing (this takes a few minutes)"
    & $py -m PyInstaller --noconfirm --log-level WARN 'openvoice-asr.spec'
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed" }
}
finally {
    Pop-Location
}

$exe = Join-Path $dist 'openvoice-asr.exe'
if (-not (Test-Path $exe)) { throw "expected $exe, which PyInstaller did not produce" }

# The real test. `probe` loads ctranslate2 and inspects the machine's devices,
# so it exercises the native libraries without needing model weights on disk —
# which is exactly the subset a freeze is most likely to have broken.
Write-Host "verifying the frozen binary answers its protocol"

# Start-Process with files on every handle, rather than a pipeline. The sidecar
# logs to stderr on startup ("ready ..."), and in Windows PowerShell 5.1 any
# stderr from a native command in a pipeline becomes an ErrorRecord that aborts
# this script — so a perfectly healthy sidecar would fail its own smoke test.
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ov-probe-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $inFile = Join-Path $tmp 'in.jsonl'
    $outFile = Join-Path $tmp 'out.jsonl'
    $errFile = Join-Path $tmp 'err.log'
    # No trailing newline needed: closing stdin ends the sidecar's read loop.
    [System.IO.File]::WriteAllText($inFile, '{"id":1,"op":"probe"}' + "`n")

    $proc = Start-Process -FilePath $exe -ArgumentList '--model', 'base.en' `
        -RedirectStandardInput $inFile -RedirectStandardOutput $outFile `
        -RedirectStandardError $errFile -NoNewWindow -PassThru
    if (-not $proc.WaitForExit(120000)) {
        $proc.Kill()
        throw "the frozen sidecar did not answer a probe within 120s"
    }

    $response = (Get-Content $outFile -ErrorAction SilentlyContinue | Select-Object -First 1)
    if (-not $response) {
        $diag = (Get-Content $errFile -ErrorAction SilentlyContinue) -join "`n"
        throw "the frozen sidecar produced no response to the probe request.`nstderr:`n$diag"
    }
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
try {
    $parsed = $response | ConvertFrom-Json
}
catch {
    throw "the frozen sidecar replied with something that is not JSON: $response"
}
if (-not $parsed.ok) { throw "probe failed: $response" }

# `probe` deliberately swallows a broken import into a diagnostic string rather
# than failing the request -- that's right for a settings-screen diagnostic, but
# it means `$parsed.ok` stays true even when the ASR library itself cannot be
# imported. This exact silent failure shipped once: excluding PyAV from the spec
# broke `import faster_whisper` (it imports `faster_whisper.audio`, which imports
# `av`, unconditionally, regardless of whether decode_audio is ever called), and
# this script's only check at the time was `.ok`. Check the field the bug
# actually appears in.
if ($parsed.faster_whisper -like '*unavailable*') {
    throw "faster_whisper cannot be imported in the frozen build: $($parsed.faster_whisper)"
}

$mb = [math]::Round((Get-ChildItem $dist -Recurse -File | Measure-Object Length -Sum).Sum / 1MB, 1)
Write-Host ""
Write-Host "ok  $dist"
Write-Host "    $mb MB, CPU engine only (CUDA is downloaded after install)"
Write-Host "    ctranslate2 $($parsed.ctranslate2), faster-whisper $($parsed.faster_whisper)"
Write-Host "    CUDA devices visible to this build: $($parsed.cuda_devices)"
