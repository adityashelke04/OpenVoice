# Downloads LibriSpeech test-clean (346 MB, CC BY 4.0, https://www.openslr.org/12)
# into fixtures/librispeech for `ov bench`. Git ignores the directory.
#
# The MD5 is the one OpenSLR publishes in md5sum.txt beside the archive. If it
# ever stops matching, compare against that file before changing it here: a
# benchmark corpus that silently changed makes every comparison meaningless.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'fixtures\librispeech'
$ready = Join-Path $dest 'LibriSpeech\test-clean'
if (Test-Path $ready) { Write-Host "already present: $ready"; exit 0 }

New-Item -ItemType Directory -Force $dest | Out-Null
$archive = Join-Path $dest 'test-clean.tar.gz'
Invoke-WebRequest -Uri 'https://www.openslr.org/resources/12/test-clean.tar.gz' -OutFile $archive
$md5 = (Get-FileHash $archive -Algorithm MD5).Hash.ToLowerInvariant()
if ($md5 -ne '32fa31d27d2e1cad72775fee3f4849a9') {
    Remove-Item $archive
    throw "test-clean.tar.gz MD5 $md5 does not match OpenSLR's published checksum"
}
tar -xzf $archive -C $dest
Remove-Item $archive
Write-Host "ready: $ready"
