<#
.SYNOPSIS
  Fetch and verify the Parakeet TDT 0.6B v2 weights into models/.

.DESCRIPTION
  ADR 0003 carries a correction admitting that a SHA-256 manifest for the
  Whisper weights was claimed in writing but never actually built. This script
  is that claim made real: the archive hash is pinned below, and a mismatch is
  a hard failure — because these bytes end up inside an installer that other
  people run.

  Idempotent. Re-running with a complete model already present verifies nothing
  is missing and exits without touching the network.

.PARAMETER Force
  Refetch even when the model is already present.
#>
[CmdletBinding()]
param([switch]$Force)

$ErrorActionPreference = 'Stop'

$Name   = 'sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8'
$Uri    = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/$Name.tar.bz2"
$Sha256 = '157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad'
$Root   = Split-Path -Parent $PSScriptRoot
$Dest   = Join-Path $Root 'models/parakeet-tdt-0.6b-v2'
# The four weights, plus the release's own sample clip. The clip is fetched
# rather than committed because .gitignore rightly refuses audio in this repo
# ("not ours to redistribute", and a voice app should not carry recordings), yet
# ov-asr's tests need real speech to decode. Fetching it keeps both true.
$Files  = @('encoder.int8.onnx', 'decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt')
$Extras = @{ 'test_wavs/0.wav' = 'test.wav' }

$Expected = $Files + $Extras.Values

if (-not $Force -and (Test-Path $Dest)) {
    $missing = $Expected | Where-Object { -not (Test-Path (Join-Path $Dest $_)) }
    if (-not $missing) {
        Write-Host "Model already present at $Dest"
        exit 0
    }
    Write-Host "Model at $Dest is incomplete (missing: $($missing -join ', ')); refetching."
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "$Name.tar.bz2"
if (Test-Path $tmp) {
    Write-Host "Reusing the already-downloaded archive at $tmp"
} else {
    Write-Host "Downloading $Uri (482 MB)..."
    # Progress rendering costs more than the download on some hosts.
    $prior = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try { Invoke-WebRequest -Uri $Uri -OutFile $tmp -UseBasicParsing }
    finally { $ProgressPreference = $prior }
}

$actual = (Get-FileHash -Path $tmp -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $Sha256) {
    throw "Checksum mismatch for $Name.tar.bz2`n  expected $Sha256`n  actual   $actual`n" +
          "The archive is left at $tmp for inspection. Delete it to force a clean redownload."
}
Write-Host 'Checksum OK.'

$staging = Join-Path ([System.IO.Path]::GetTempPath()) "ov-model-$(Get-Random)"
New-Item -ItemType Directory -Path $staging -Force | Out-Null
try {
    tar -xjf $tmp -C $staging
    if ($LASTEXITCODE -ne 0) { throw "tar failed to extract $tmp (exit $LASTEXITCODE)" }

    # The archive expands to a directory named after the upstream release asset.
    # Install under a stable name so nothing downstream — the resolver, the NSIS
    # hook, the tests — has to encode that filename.
    if (Test-Path $Dest) { Remove-Item $Dest -Recurse -Force }
    New-Item -ItemType Directory -Path $Dest -Force | Out-Null
    foreach ($f in $Files) {
        Copy-Item (Join-Path $staging "$Name/$f") (Join-Path $Dest $f)
    }
    foreach ($src in $Extras.Keys) {
        Copy-Item (Join-Path $staging "$Name/$src") (Join-Path $Dest $Extras[$src])
    }
} finally {
    if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
}

Remove-Item $tmp -Force
Write-Host "Model ready at $Dest"
