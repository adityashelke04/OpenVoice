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
    $missing = $Expected | Where-Object { -not (Test-Path -LiteralPath (Join-Path $Dest $_) -PathType Leaf) }
    $verified = $false
    $manifestPath = Join-Path $Dest 'verified-files.json'
    if (-not $missing -and (Test-Path -LiteralPath $manifestPath)) {
        try {
            $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
            $verified = $manifest.archiveSha256 -eq $Sha256
            foreach ($file in $Expected) {
                $hash = (Get-FileHash -LiteralPath (Join-Path $Dest $file) -Algorithm SHA256).Hash.ToLower()
                if ($manifest.files.$file -ne $hash) { $verified = $false }
            }
        } catch { $verified = $false }
    }
    if ($verified) {
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

$modelsRoot = [System.IO.Path]::GetFullPath((Join-Path $Root 'models'))
$Dest = [System.IO.Path]::GetFullPath($Dest)
if (-not $Dest.StartsWith($modelsRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Model destination escaped checkout.' }
New-Item -ItemType Directory -Path $modelsRoot -Force | Out-Null
$staging = Join-Path $modelsRoot ('.fetch-' + [guid]::NewGuid().ToString('N'))
$backup = Join-Path $modelsRoot ('.previous-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
try {
    tar -xjf $tmp -C $staging
    if ($LASTEXITCODE -ne 0) { throw "tar failed to extract $tmp (exit $LASTEXITCODE)" }

    # The archive expands to a directory named after the upstream release asset.
    # Install under a stable name so nothing downstream — the resolver, the NSIS
    # hook, the tests — has to encode that filename.
    $prepared = Join-Path $staging 'prepared'
    New-Item -ItemType Directory -Path $prepared | Out-Null
    foreach ($f in $Files) {
        Copy-Item -LiteralPath (Join-Path $staging "$Name/$f") -Destination (Join-Path $prepared $f)
    }
    foreach ($src in $Extras.Keys) {
        Copy-Item -LiteralPath (Join-Path $staging "$Name/$src") -Destination (Join-Path $prepared $Extras[$src])
    }
    $hashes = @{}
    foreach ($file in $Expected) {
        $hashes[$file] = (Get-FileHash -LiteralPath (Join-Path $prepared $file) -Algorithm SHA256).Hash.ToLower()
    }
    @{ archiveSha256 = $Sha256; files = $hashes } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $prepared 'verified-files.json')
    $hadPrevious = Test-Path -LiteralPath $Dest
    if ($hadPrevious) { Move-Item -LiteralPath $Dest -Destination $backup }
    try { Move-Item -LiteralPath $prepared -Destination $Dest }
    catch {
        if ($hadPrevious) { Move-Item -LiteralPath $backup -Destination $Dest }
        throw
    }
    $resolvedBackup = [System.IO.Path]::GetFullPath($backup)
    if (-not $resolvedBackup.StartsWith($modelsRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Backup directory escaped checkout.' }
    if (Test-Path -LiteralPath $resolvedBackup) { Remove-Item -LiteralPath $resolvedBackup -Recurse -Force }
} finally {
    $resolvedStage = [System.IO.Path]::GetFullPath($staging)
    if (-not $resolvedStage.StartsWith($modelsRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Staging directory escaped checkout.' }
    if (Test-Path -LiteralPath $resolvedStage) { Remove-Item -LiteralPath $resolvedStage -Recurse -Force }
}

Remove-Item -LiteralPath $tmp -Force
Write-Host "Model ready at $Dest"
