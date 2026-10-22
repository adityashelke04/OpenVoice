param([string]$ScriptPath = (Join-Path $PSScriptRoot 'fetch-model.ps1'))
$ErrorActionPreference = 'Stop'
$fixture = Join-Path ([System.IO.Path]::GetTempPath()) ('ov-fetch-script-' + [guid]::NewGuid().ToString('N'))
$scripts = Join-Path $fixture 'scripts'
$model = Join-Path $fixture 'models/parakeet-tdt-0.6b-v2'
New-Item -ItemType Directory -Path $scripts, $model | Out-Null
$source = Get-Content -LiteralPath $ScriptPath -Raw
$source = $source.Replace('Get-FileHash', 'Test-ArchiveHash').Replace('Invoke-WebRequest', 'Test-Download').Replace('tar -xjf', 'Test-Extract -xjf')
$source = $source.Replace('[System.IO.Path]::GetTempPath()', "'$($fixture.Replace("'", "''"))'")
Set-Content -LiteralPath (Join-Path $scripts 'fetch-model.ps1') -Value $source
Set-Content -LiteralPath (Join-Path $model 'previous.onnx') -Value 'valid previous weights'
# Only the socket/archive tools are replaced; run the real install script against a disposable checkout.
$global:testDownloads = 0
$global:testCompleteArchive = $false
function global:Test-Download { param($Uri, $OutFile, [switch]$UseBasicParsing) $global:testDownloads++; Set-Content -LiteralPath $OutFile -Value 'test archive' }
function global:Test-ArchiveHash {
    param($Path, $LiteralPath, $Algorithm)
    if ($Path) { return @{ Hash = '157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad' } }
    Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath -Algorithm $Algorithm
}
function global:Test-Extract {
    if ($global:testCompleteArchive) {
        $stage = $args[3]
        $release = Join-Path $stage 'sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8'
        New-Item -ItemType Directory -Path (Join-Path $release 'test_wavs') -Force | Out-Null
        foreach ($name in @('encoder.int8.onnx', 'decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt', 'test_wavs/0.wav')) {
            Set-Content -LiteralPath (Join-Path $release $name) -Value "verified archive $name"
        }
    }
    $global:LASTEXITCODE = 0
}
$failed = $false
try { & (Join-Path $scripts 'fetch-model.ps1') -Force } catch { $failed = $true; Write-Host $_ }
if (-not $failed) { throw 'Incomplete archive must fail installation.' }
if (-not (Test-Path -LiteralPath (Join-Path $model 'previous.onnx'))) { throw 'Failed extraction destroyed the previous model.' }
Write-Host 'PASS: incomplete extraction preserves previous model.'
$global:testCompleteArchive = $true
& (Join-Path $scripts 'fetch-model.ps1') -Force
if (-not (Test-Path -LiteralPath (Join-Path $model 'verified-files.json'))) { throw 'Verified install must have a file manifest.' }
$downloads = $global:testDownloads
& (Join-Path $scripts 'fetch-model.ps1')
if ($global:testDownloads -ne $downloads) { throw 'Verified unchanged model should not download again.' }
Set-Content -LiteralPath (Join-Path $model 'encoder.int8.onnx') -Value 'corrupted'
& (Join-Path $scripts 'fetch-model.ps1')
if ($global:testDownloads -ne ($downloads + 1)) { throw 'Corrupted model must be fetched again.' }
Write-Host 'PASS: verified install is reused; changed model bytes trigger replacement.'
