# Newcomer scenarios on Windows, with an empty environment: fresh HOME / USERPROFILE /
# LOCALAPPDATA / APPDATA and a PATH with only the Windows folders (curl.exe, tar.exe and
# cmd.exe are in System32; no yt-dlp, deno, node or ffmpeg).
#
#   ci/newbie.ps1 <itg-yt.exe>            # scenarios 1-4 (no YouTube access needed)
#   ci/newbie.ps1 <itg-yt.exe> youtube    # real YouTube download, after the above
#
# Scenario 5 (full run with a fake yt-dlp) is only in ci/newbie.sh: the fake yt-dlp is a
# shell script.
param(
    [Parameter(Mandatory = $true)] [string]$Bin,
    [string]$Mode = 'all'
)
$ErrorActionPreference = 'Stop'
$Bin = (Resolve-Path $Bin).Path
$Work = if ($env:NEWBIE_WORK) { $env:NEWBIE_WORK }
elseif ($env:RUNNER_TEMP) { Join-Path $env:RUNNER_TEMP 'newbie' }
else { Join-Path ([IO.Path]::GetTempPath()) 'newbie' }
$Url = 'https://www.youtube.com/watch?v=abc123'

function Fail([string]$msg) { Write-Host "FAIL: $msg"; exit 1 }
function Step([string]$msg) { Write-Host ''; Write-Host "=== $msg" }
function Contains([string]$text, [string]$s) {
    if (-not $text.Contains($s)) { Fail "expected '$s' in: $text" }
}
function Lacks([string]$text, [string]$s) {
    if ($text.Contains($s)) { Fail "unexpected '$s' in: $text" }
}

$HomeDir = Join-Path $Work 'home'
$Songs = Join-Path $Work 'Songs'
$Tools = Join-Path $HomeDir 'AppData\Local\itg-yt\bin'
$Output = Join-Path $HomeDir 'ITG-YouTube'

if ($Mode -ne 'youtube') {
    if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
    New-Item -ItemType Directory -Force -Path $HomeDir, $Songs | Out-Null
}

# The environment of this process is inherited by itg-yt.
$env:HOME = $HomeDir
$env:USERPROFILE = $HomeDir
$env:LOCALAPPDATA = Join-Path $HomeDir 'AppData\Local'
$env:APPDATA = Join-Path $HomeDir 'AppData\Roaming'
$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
$env:ITG_YT_RETRY_PAUSE_MS = '10'
foreach ($v in 'ITG_YT_DLP', 'ITG_FFMPEG', 'ITG_CHARTER_PYTHON') {
    Remove-Item "env:$v" -ErrorAction SilentlyContinue
}

# Runs itg-yt; sets $OUT, $ERR and $CODE.
function Itg([string[]]$ArgList) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new($Bin)
    foreach ($a in $ArgList) { $psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    $o = $p.StandardOutput.ReadToEndAsync()
    $e = $p.StandardError.ReadToEndAsync()
    $p.WaitForExit()
    $script:OUT = $o.Result
    $script:ERR = $e.Result
    $script:CODE = $p.ExitCode
    Write-Host "itg-yt $($ArgList -join ' ') -> exit $($script:CODE)"
}

if ($Mode -eq 'youtube') {
    Step 'real YouTube download with the tools installed by setup'
    Itg -ArgList @('--no-video', '--no-stems', '-m', '3', 'https://www.youtube.com/watch?v=jNQXAC9IVRw')
    Write-Host $ERR
    if ($CODE -ne 0) { Fail 'real download failed' }
    if (-not (Test-Path -LiteralPath $OUT.Trim())) { Fail "no .sm: $OUT" }
    Write-Host "OK: $OUT"
    exit 0
}

foreach ($t in 'yt-dlp', 'deno', 'node', 'ffmpeg') {
    if (Get-Command $t -ErrorAction SilentlyContinue) { Fail "$t is visible in the test PATH" }
}
foreach ($t in 'curl.exe', 'tar.exe', 'cmd.exe') {
    if (-not (Get-Command $t -ErrorAction SilentlyContinue)) { Fail "$t missing from System32" }
}

Step '1. nothing installed: exit code 2 before any download'
Itg -ArgList @('--no-stems', $Url)
Write-Host $ERR
if ($CODE -ne 2) { Fail "exit code $CODE, expected 2" }
Contains $ERR 'itg-yt setup'
Contains $ERR 'yt-dlp (downloads the videos from YouTube): not found'
Contains $ERR 'deno or node'
Contains $ERR 'ffmpeg (encodes the audio with libvorbis and the background video with libx264)'
if (Test-Path (Join-Path $HomeDir '.cache\itg-charter\youtube')) { Fail 'something was downloaded' }

Step '2. --no-video: libx264 is not required'
Itg -ArgList @('--no-video', '--no-stems', $Url)
if ($CODE -ne 2) { Fail "exit code $CODE, expected 2" }
Contains $ERR 'ffmpeg (encodes the audio with libvorbis)'
Lacks $ERR 'libx264'

Step '3. itg-yt setup'
Itg -ArgList @('setup', '--songs', $Songs)
Write-Host $OUT
if ($CODE -ne 0) { Fail "setup: exit code $CODE`n$ERR" }
foreach ($t in 'yt-dlp.exe', 'deno.exe', 'ffmpeg.exe', 'ffprobe.exe') {
    if (-not (Test-Path (Join-Path $Tools $t))) { Fail "$Tools\$t not installed" }
}
$link = Get-Item -Force (Join-Path $Songs 'YouTube')
if ($link.LinkType -ne 'Junction') { Fail "Songs\YouTube is not a junction ($($link.LinkType))" }
Write-Host "junction target: $($link.Target)"
if (-not ("$($link.Target)".TrimEnd('\') -like "*\home\ITG-YouTube")) { Fail "junction target $($link.Target)" }
if (-not (Test-Path $Output -PathType Container)) { Fail 'output folder not created' }

Step '3b. setup again: nothing downloaded'
$before = (Get-ChildItem $Tools | ForEach-Object { "$($_.Name) $($_.LastWriteTimeUtc.Ticks)" }) -join ';'
Itg -ArgList @('setup', '--songs', $Songs)
if ($CODE -ne 0) { Fail "second setup: exit code $CODE" }
Lacks $OUT 'downloading'
$after = (Get-ChildItem $Tools | ForEach-Object { "$($_.Name) $($_.LastWriteTimeUtc.Ticks)" }) -join ';'
if ($before -ne $after) { Fail "tools changed by the second setup: $before / $after" }

Step '3c. an existing YouTube folder is not replaced'
$songs2 = Join-Path $Work 'Songs2'
New-Item -ItemType Directory -Force -Path (Join-Path $songs2 'YouTube') | Out-Null
Set-Content -Path (Join-Path $songs2 'YouTube\keep.txt') -Value 'keep'
Itg -ArgList @('setup', '--songs', $songs2)
Write-Host $OUT
if ($CODE -ne 1) { Fail "exit code $CODE, expected 1" }
Contains $OUT 'left untouched'
$kept = Get-Item -Force (Join-Path $songs2 'YouTube')
if ($kept.LinkType) { Fail 'existing folder replaced by a link' }
if ((Get-Content (Join-Path $songs2 'YouTube\keep.txt')) -ne 'keep') { Fail 'existing folder modified' }

Step '4. setup --check: everything OK'
Itg -ArgList @('setup', '--check', '--songs', $Songs)
Write-Host $OUT
if ($CODE -ne 0) { Fail "check: exit code $CODE" }
Contains $OUT 'ok       yt-dlp'
Contains $OUT 'ok       deno or node'
Contains $OUT 'ok       ffmpeg'
Contains $OUT "ok       $Songs\YouTube"

Step '5. full run with a fake yt-dlp: skipped on Windows (see ci/newbie.sh)'

Write-Host ''
Write-Host 'All newcomer scenarios passed.'
