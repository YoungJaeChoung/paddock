# 사용: restore-run.ps1 -Exe <paddock-app.exe> -Out <결과 폴더> -HomeDir <임시 사용자 폴더> -Mode stop|kill|reboot|never [-Offscreen]
# 되살리기 시나리오(야간이 아니라 화면 밖 실행): 앱을 한 번 띄워 터미널을 만들고, 데몬이 사라지게 만든 뒤 앱을 다시 띄워 되살아나는지 본다.
#   stop   앱의 종료 묻기에서 Stop all and quit (데몬 종료 요청)
#   kill   데몬만 강제 종료한 뒤 앱을 끝낸다
#   reboot 앱을 먼저, 이어서 데몬을 강제 종료한다(컴퓨터가 다시 켜진 것과 같다)
#   never  restoreOnRestart=never 설정으로 같은 일을 했을 때 아무것도 되살아나지 않는다
# 안전 규칙: automation-run.ps1과 같다 — HomeDir이 실제 사용자 폴더면 거부하고, 이 HomeDir의 것이 아닌 paddock-daemon이 떠 있으면 시작하지 않으며,
# 키보드·마우스 입력은 보내지 않고, 이 스크립트가 띄운 앱·데몬 프로세스만 끝낸다. 창은 화면 밖에 활성화 없이 띄운다.
param(
    [string]$Exe,
    [string]$Out,
    [string]$HomeDir = "",
    [ValidateSet('stop','kill','reboot','never')][string]$Mode = 'stop',
    [switch]$Offscreen
)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required" }
New-Item -ItemType Directory -Force $HomeDir | Out-Null
if ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\')) { throw "HomeDir is the real user profile" }
if (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0) { throw "paddock-daemon is already running" }
New-Item -ItemType Directory -Force $Out | Out-Null
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$native = Join-Path $HomeDir '.paddock\native'
New-Item -ItemType Directory -Force $native | Out-Null
if ($Mode -eq 'never') { Set-Content (Join-Path $native 'settings.json') '{ "paddock.terminal.restoreOnRestart": "never" }' }
function DaemonPid { $info = Join-Path $native 'daemon.json'; if (Test-Path $info) { try { return [int](Get-Content $info -Raw | ConvertFrom-Json).pid } catch {} }; return 0 }
function Log($text) { "$(Get-Date -Format HH:mm:ss) $text" | Out-File (Join-Path $Out 'restore-run.log') -Append -Encoding utf8 }

# 1단계: 터미널을 만든다.
$tail = if ($Mode -in 'kill','reboot') { "mark ready-to-kill`nwait 600000" } else { "closewindow`nwaitoverlay quit @10000`nact quitstop`nwait 8000" }
$script1 = Join-Path $Out 'phase1.script'
# 대본은 UTF-8(BOM 없음)이어야 앱이 읽는다.
$prepare = [IO.File]::ReadAllText((Join-Path $here 'cases\restore-prepare.script'), [Text.Encoding]::UTF8)
[IO.File]::WriteAllText($script1, $prepare + "`n" + $tail + "`n", (New-Object Text.UTF8Encoding $false))
$args1 = "--script `"$script1`" --out `"$Out\phase1`"$(if ($Offscreen) { ' --offscreen' })"
$app = Start-Process -FilePath $Exe -ArgumentList $args1 -PassThru
Log "phase1 started pid=$($app.Id) mode=$Mode"
$started = Get-Date
if ($Mode -in 'kill','reboot') {
    $events = Join-Path $Out 'phase1\events.log'
    while ((-not (Test-Path $events) -or -not (Select-String -Path $events -Pattern 'ready-to-kill' -Quiet)) -and ((Get-Date) - $started).TotalSeconds -lt 120 -and -not $app.HasExited) { Start-Sleep -Milliseconds 200 }
    $daemon = DaemonPid
    Log "ready: app=$($app.Id) daemon=$daemon"
    if ($Mode -eq 'kill') { if ($daemon) { Stop-Process -Id $daemon -Force -ErrorAction SilentlyContinue }; Start-Sleep -Seconds 1; Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
    else { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue; Start-Sleep -Seconds 1; if ($daemon) { Stop-Process -Id $daemon -Force -ErrorAction SilentlyContinue } }
} else {
    while (-not $app.HasExited -and ((Get-Date) - $started).TotalSeconds -lt 120) { Start-Sleep -Milliseconds 200 }
    if (-not $app.HasExited) { Log "phase1 timeout"; Stop-Process -Id $app.Id -Force }
}
$end = (Get-Date).AddSeconds(15)
while (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0 -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 200 }
$left = @(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue)
Log "after phase1 daemons=$($left.Count) history=$((Get-ChildItem (Join-Path $native 'terminal-history') -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($native.Length) }) -join ',')"
if ($left.Count -gt 0) { throw "a daemon is still running after phase 1" }

# 확인용으로 되살리기 전의 기록 파일을 결과 폴더에 복사해 둔다(시험용 가짜 데이터뿐이다).
Copy-Item (Join-Path $native 'terminal-history') (Join-Path $Out 'history-before') -Recurse -ErrorAction SilentlyContinue

# 14일 지난 다른 데몬의 기록 폴더: 새 데몬이 시작할 때 지워져야 한다.
$stale = Join-Path $native 'terminal-history\stale-daemon'
New-Item -ItemType Directory -Force $stale | Out-Null
Set-Content (Join-Path $stale '1.json') '{}'
(Get-Item (Join-Path $stale '1.json')).LastWriteTime = (Get-Date).AddDays(-15)
(Get-Item $stale).LastWriteTime = (Get-Date).AddDays(-15)

# 2단계: 앱을 다시 띄워 되살아나는지 본다(화면 캡처는 automation-run.ps1이 한다).
$after = if ($Mode -eq 'never') { 'restore-after-never.script' } else { 'restore-after.script' }
& (Join-Path $here 'automation-run.ps1') -Exe $Exe -Script (Join-Path $here "cases\$after") -Out "$Out\phase2" -HomeDir $HomeDir -TimeoutSec 150 -Offscreen:$Offscreen
Log "stale folder exists after phase2: $(Test-Path $stale)"
# 되살린 화면 판정: 이전 화면(BACKMARK)·안내 줄·이어하기 명령이 새 터미널의 화면 또는 스크롤백에 있는지(Windows는 셸이 시작하며 화면을 지워 이전 화면이 스크롤백으로 밀린다).
$dump = Join-Path $Out 'phase2\dump-restored.txt'
if (Test-Path $dump) {
    $text = Get-Content $dump -Raw
    foreach ($needle in 'BACKMARK','Previous shell ended. Screen restored','claude --resume 0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10') { Log "dump has [$needle]: $($text.Contains($needle))" }
}
$end = (Get-Date).AddSeconds(25)
while (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0 -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 300 }
$daemon = DaemonPid
foreach ($p in @(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue)) { if ($p.Id -eq $daemon) { Stop-Process -Id $p.Id -Force } }
Log "history left: $((Get-ChildItem (Join-Path $native 'terminal-history') -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($native.Length) }) -join ',')"
Log "done"
