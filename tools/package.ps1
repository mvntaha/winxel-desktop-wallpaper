# portable release: builds winxel.exe and lays it out with its assets in dist\Winxel\.
# the exe finds assets\ next to itself, so the folder can be copied anywhere.
#
#   powershell -NoProfile -File tools\package.ps1 [-Out dist\Winxel]
#
# PERSONAL USE:
param([string]$Out = "dist\Winxel")
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
. .\tools\env.ps1

# one job: rustc runs out of memory on this machine with parallel jobs.
cargo build --release -j 1
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$dest = Join-Path $root $Out
# refuse up front if the packaged exe is running: deleting would fail half-way and leave the folder without its assets.
$exe = Join-Path $dest "winxel.exe"
if (Test-Path $exe) {
    try { [System.IO.File]::Open($exe, 'Open', 'ReadWrite', 'None').Close() }
    catch { throw "$exe is in use (Winxel running from it?). Quit it from the tray, then package again." }
}
if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
New-Item -ItemType Directory -Force "$dest\assets\ready", "$dest\assets\fonts" | Out-Null

Copy-Item "target\release\winxel.exe" $dest
Copy-Item "assets\anim.json" "$dest\assets\"
# everything the app loads from ready\ (plates, sprites, masks, blink), not debug output.
Get-ChildItem "assets\ready" -File |
    Where-Object { $_.Name -notlike "check_*" -and $_.Name -ne "alignment.json" } |
    Copy-Item -Destination "$dest\assets\ready"
Copy-Item "assets\fonts\*" "$dest\assets\fonts\"

$files = Get-ChildItem $dest -Recurse -File
$mb = ($files | Measure-Object Length -Sum).Sum / 1MB
"Packaged {0} files, {1:N1} MB -> {2}" -f $files.Count, $mb, $dest
