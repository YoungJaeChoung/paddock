# 사용: daemon-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -Out <결과 폴더> [-Offscreen] [-PrepareOnly]
# 호환 데몬 계속 쓰기(야간 항목 I49): 앱 업데이트(데몬 실행 파일만 달라짐)와 프로토콜 불일치 때 앱이 데몬을 어떻게 다루는지 실제 앱·데몬으로 본다.
# 앱 옆 paddock-daemon.exe(= -Exe 와 같은 폴더)에 한 바이트를 덧붙인 사본을 「이전 빌드의 데몬」으로 쓴다. 시험 훅(test-hooks)이 켜진 데몬이어야 마지막 두 단계가 돈다.
#   1  이전 앱 사본(옆에 덧붙인 데몬)으로 시작해 데몬에 터미널 1개를 남기고 앱만 끝낸다(데몬은 터미널이 있어 계속 산다)
#   2  지금 앱으로 시작: 묻지 않고 같은 데몬에 붙어 터미널이 살아 있고(daemon=outdated) 빠른 설정에 안내 줄이 보인다(캡처 panel-outdated)
#   3  앱만 끝냈다 다시 시작: 같은 데몬 pid, 여전히 daemon=outdated
#   4  Quit and Stop All Terminals: 데몬이 끝난다
#   5  다시 시작: 새 데몬(pid가 다름), daemon=same. 끝은 Quit and Stop All이라 데몬이 스스로 끝난다
#   6  터미널이 없는 이전 빌드의 데몬을 띄우고 10초 안에 앱 시작: 그 데몬이 교체된다(app.log `idle outdated daemon replaced`, daemon=same, pid가 다름). 끝은 Quit and Stop All
#   7  프로토콜 1이라고 인사하는 데몬(훅)으로 시작: 교체를 묻는 시작 화면(notice=protocol-mismatch, 캡처 protocol-mismatch), 데몬은 그대로
#   8  인사를 ERR로 거절하는 옛 v1 데몬(훅)으로 시작: 강제 종료 안내가 든 화면(notice=protocol-mismatch-v1, 캡처 protocol-mismatch-v1), 데몬은 그대로
# 판정은 앱이 남기는 state-live.txt(waitstate)·events.log의 TIMEOUT 수·app.log·daemon.json의 pid로 한다. 결과: <Out>\result-I49.txt (PASS / FAIL), 캡처는 <Out>\<단계>\*.png.
# -PrepareOnly: 앱을 띄우지 않고 사본과 대본만 만든다(구문 점검용).
# 안전 규칙: automation-run.ps1과 같다 — HomeDir이 실제 사용자 폴더면 거부하고, 다른 paddock-daemon이 떠 있으면 시작하지 않으며, 입력은 보내지 않는다.
# 끝내는 프로세스는 이 스크립트가 띄운 것(앱이 띄운 데몬 포함)뿐이고, 끝내기 직전에 프로세스 이름이 paddock-daemon이며 실행 파일이 이 실행의 두 데몬 파일(앞의 sibling·덧붙인 사본) 중 하나인지 다시 본다
# (끝난 뒤 pid가 다른 프로세스에 다시 배정됐을 수 있다). 중간에 예외·Ctrl+C로 멈춰도 finally가 남은 시험 데몬을 정리한다(사용자의 다음 네이티브 Paddock이 데몬 파이프를 쓸 수 있도록).
param(
    [string]$Exe,
    [string]$HomeDir = "",
    [string]$Out,
    [string[]]$Tests = @('I49'),
    [switch]$Offscreen,
    [switch]$PrepareOnly
)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required" }
New-Item -ItemType Directory -Force $HomeDir | Out-Null
if ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\')) { throw "HomeDir is the real user profile" }
if (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0) { throw "paddock-daemon is already running" }
New-Item -ItemType Directory -Force $Out | Out-Null
. (Join-Path $here 'test-env.ps1')
Use-TestHome $HomeDir
$native = Join-Path $HomeDir '.paddock\native'
$result = Join-Path $Out 'result-I49.txt'
Remove-Item $result -ErrorAction SilentlyContinue
$runStart = Get-Date
$started = New-Object System.Collections.ArrayList    # 아직 끝났다고 확인하지 못한, 이 스크립트가 띄운 데몬 pid
$everStarted = New-Object System.Collections.ArrayList # 이 실행이 띄운 모든 데몬 pid(끝난 것 포함, wsl.exe 자식 점검용)
$failed = 0
function Log($text) { "$(Get-Date -Format HH:mm:ss) $text" | Out-File (Join-Path $Out 'daemon-run.log') -Append -Encoding utf8 }
function Check($name, [bool]$ok, $detail = '') {
    if (-not $ok) { $script:failed++ }
    $line = "$(if ($ok) { 'PASS' } else { 'FAIL' }) $name $detail"
    $line | Out-File $result -Append -Encoding utf8
    Log $line
}
function Track($id) { if ($id -gt 0) { if (-not $started.Contains($id)) { [void]$started.Add($id) }; if (-not $everStarted.Contains($id)) { [void]$everStarted.Add($id) } } }
function DaemonPid { $info = Join-Path $native 'daemon.json'; if (Test-Path $info) { try { return [int](Get-Content $info -Raw | ConvertFrom-Json).pid } catch {} }; return 0 }
# 그 pid가 지금 paddock-daemon 프로세스인가(이름 확인 — pid가 다른 프로세스에 다시 배정됐으면 아니다).
function IsAlive($id) {
    if ($id -le 0) { return $false }
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    return ($null -ne $p) -and ($p.ProcessName -eq 'paddock-daemon')
}
# 이 실행의 데몬 파일(앞의 sibling·덧붙인 사본)에서 뜬 paddock-daemon 인가. 끝내도 되는 프로세스의 조건이다.
function IsMine($id) {
    if (-not (IsAlive $id)) { return $false }
    $path = (Get-Process -Id $id -ErrorAction SilentlyContinue).Path
    return ($path -ieq $sibling) -or ($path -ieq $oldDaemon)
}
function WaitGone($id, $seconds) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((IsAlive $id) -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 200 }
    $gone = -not (IsAlive $id)
    if ($gone) { $started.Remove($id) }
    return $gone
}
function Stop-Mine($id) {
    if ($started.Contains($id)) {
        if (IsMine $id) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue; [void](WaitGone $id 5) }
        else { $started.Remove($id) }
    }
}
# 데몬 실행 파일을 직접 띄워 daemon.json 에 자기 pid 를 쓸 때까지 기다린다. $hooks 의 환경 변수는 이 데몬에만 준다.
function Start-Daemon($daemonExe, $hooks = @{}) {
    foreach ($key in $hooks.Keys) { Set-Item "Env:$key" $hooks[$key] }
    try { $process = Start-Process -FilePath $daemonExe -WindowStyle Hidden -PassThru }
    finally { foreach ($key in $hooks.Keys) { Remove-Item "Env:$key" -ErrorAction SilentlyContinue } }
    Track $process.Id
    $end = (Get-Date).AddSeconds(10)
    while ((DaemonPid) -ne $process.Id -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 100 }
    Log "daemon started pid=$($process.Id) json=$(DaemonPid) exe=$daemonExe hooks=$($hooks.Keys -join ',')"
    return $process.Id
}
function Timeouts($phase) {
    $events = Join-Path $Out "$phase\events.log"
    if (-not (Test-Path $events)) { return -1 }
    return @(Select-String -Path $events -Pattern 'TIMEOUT').Count
}
function Run-Phase($phase, $appExe, $scriptText) {
    $path = Join-Path $Out "$phase.script"
    # 대본은 UTF-8(BOM 없음)이어야 앱이 읽는다.
    [IO.File]::WriteAllText($path, $scriptText + "`n", (New-Object Text.UTF8Encoding $false))
    if ($PrepareOnly) { Log "$phase prepared: $path"; return }
    & (Join-Path $here 'automation-run.ps1') -Exe $appExe -Script $path -Out (Join-Path $Out $phase) -HomeDir $HomeDir -TimeoutSec 90 -Offscreen:$Offscreen
    Log "$phase done timeouts=$(Timeouts $phase) daemon=$(DaemonPid)"
}
function State-Has($phase, $needle) {
    $file = Get-ChildItem (Join-Path $Out $phase) -Filter 'state-*.txt' -ErrorAction SilentlyContinue | Select-Object -First 1
    return ($null -ne $file) -and (Select-String -Path $file.FullName -Pattern $needle -SimpleMatch -Quiet)
}
function Live-Has($phase, $needle) {
    $file = Join-Path $Out "$phase\state-live.txt"
    return (Test-Path $file) -and (Select-String -Path $file -Pattern $needle -Quiet)
}
function Log-Has($pattern) {
    $log = Join-Path $native 'logs\app.log'
    return (Test-Path $log) -and (Select-String -Path $log -Pattern $pattern -Quiet)
}

$exeDir = Split-Path -Parent $Exe
$sibling = Join-Path $exeDir 'paddock-daemon.exe'
if (-not (Test-Path $sibling)) { throw "paddock-daemon.exe is not beside the app: $sibling" }
# 이전 빌드 흉내: 앱 사본 + 옆에 한 바이트를 덧붙인 데몬(앱이 기대하는 데몬과 지문이 다르다).
$old = Join-Path $Out 'old'
New-Item -ItemType Directory -Force $old | Out-Null
$oldApp = Join-Path $old 'paddock-app.exe'
$oldDaemon = Join-Path $old 'paddock-daemon.exe'
Copy-Item $Exe $oldApp -Force
$bytes = [IO.File]::ReadAllBytes($sibling) + [byte[]](0)
[IO.File]::WriteAllBytes($oldDaemon, $bytes)

$main = [IO.File]::ReadAllText((Join-Path $here 'cases\daemon-outdated.script'), [Text.Encoding]::UTF8)
$stopAll = "act quick open`nwaitstate quick=open @5000`nact quick quitstop`nwait 6000"
$p1 = "waittabs 1 @30000`nsend echo OLD-ONE\r`nwaitline OLD-ONE @20000`nstate old`nquit"
$p3 = "waittabs 1 @30000`nwaitstate daemon=outdated @10000`nwaitline NEW-TWO @20000`nstate again`nquit"
$p4 = "waittabs 1 @30000`nwaitstate daemon=outdated @10000`n$stopAll"
$p5 = "waittabs 1 @30000`nwaitstate daemon=same @10000`nstate fresh`nact quick open`nwaitstate quick=open @5000`nwait 500`nshot panel-same`nact quick quitstop`nwait 6000"
$p6 = "waittabs 1 @30000`nwaitstate daemon=same @10000`nstate idle-replaced`n$stopAll"
$p7 = "waitstate notice=protocol-mismatch @10000`nstate asks`nwait 500`nshot protocol-mismatch`nquit"
$p8 = "waitstate notice=protocol-mismatch-v1 @10000`nstate asks-v1`nwait 500`nshot protocol-mismatch-v1`nquit"

try {
    # 1: 이전 앱 + 이전 데몬 -> 데몬에 터미널 1개
    Run-Phase 'p1-old-app' $oldApp $p1
    $pidA = DaemonPid
    if (-not $PrepareOnly) {
        Track $pidA
        $pathA = (Get-Process -Id $pidA -ErrorAction SilentlyContinue).Path
        Check 'p1 the old build daemon holds the terminal' ((Timeouts 'p1-old-app') -eq 0 -and $pathA -ieq $oldDaemon) "pid=$pidA path=$pathA timeouts=$(Timeouts 'p1-old-app')"
    }
    # 2: 지금 앱 -> 묻지 않고 같은 데몬, 터미널 유지, 안내 줄
    Run-Phase 'p2-outdated' $Exe $main
    if (-not $PrepareOnly) {
        Check 'p2 same daemon, outdated, terminal kept' ((Timeouts 'p2-outdated') -eq 0 -and (DaemonPid) -eq $pidA -and (State-Has 'p2-outdated' 'daemon=outdated')) "timeouts=$(Timeouts 'p2-outdated') pid=$(DaemonPid) expected=$pidA"
        Check 'p2 hint panel captured' (Test-Path (Join-Path $Out 'p2-outdated\panel-outdated.png'))
        Check 'p2 app.log says the outdated daemon was kept and the window uses it' ((Log-Has 'outdated daemon kept') -and (Log-Has '\(using the previous terminal daemon\)'))
    }
    # 3: 앱만 다시 -> 같은 데몬
    Run-Phase 'p3-restart' $Exe $p3
    if (-not $PrepareOnly) { Check 'p3 restart keeps the same daemon' ((Timeouts 'p3-restart') -eq 0 -and (DaemonPid) -eq $pidA) "timeouts=$(Timeouts 'p3-restart') pid=$(DaemonPid) expected=$pidA" }
    # 4: Quit and Stop All Terminals -> 데몬 종료
    Run-Phase 'p4-stop-all' $Exe $p4
    if (-not $PrepareOnly) { Check 'p4 Quit and Stop All ends the old daemon' ((Timeouts 'p4-stop-all') -eq 0 -and (WaitGone $pidA 20)) "timeouts=$(Timeouts 'p4-stop-all')" }
    # 5: 다시 시작 -> 새 데몬. 끝은 Quit and Stop All이라 데몬이 스스로 끝난다.
    Run-Phase 'p5-fresh' $Exe $p5
    if (-not $PrepareOnly) {
        $pidB = DaemonPid
        Track $pidB
        $newOk = ((Timeouts 'p5-fresh') -eq 0) -and ($pidB -ne $pidA) -and ($pidB -gt 0) -and (State-Has 'p5-fresh' 'daemon=same')
        Check 'p5 restart starts a new daemon that matches the app' $newOk "timeouts=$(Timeouts 'p5-fresh') new=$pidB old=$pidA"
        Check 'p5 Quit and Stop All ends the new daemon by itself' (WaitGone $pidB 20) "pid=$pidB"
        # 6: 터미널이 없는 이전 빌드의 데몬 + 10초 안에 앱 시작 -> 교체
        $idleOld = Start-Daemon $oldDaemon
    }
    Run-Phase 'p6-idle-replaced' $Exe $p6
    if (-not $PrepareOnly) {
        $pidC = DaemonPid
        Track $pidC
        $idleOk = ((Timeouts 'p6-idle-replaced') -eq 0) -and ($pidC -ne $idleOld) -and (WaitGone $idleOld 10) -and (State-Has 'p6-idle-replaced' 'daemon=same') -and (Log-Has 'idle outdated daemon replaced')
        Check 'p6 an idle outdated daemon is replaced at start (not the 10 s idle exit)' $idleOk "timeouts=$(Timeouts 'p6-idle-replaced') new=$pidC idle-old=$idleOld"
        Check 'p6 Quit and Stop All ends the replacing daemon by itself' (WaitGone $pidC 20) "pid=$pidC"
        # 7: 프로토콜 1이라고 인사하는 데몬 -> 교체를 묻는 화면
        $hooked = Start-Daemon $sibling @{ PADDOCK_DAEMON_PROTOCOL_VERSION = '1' }
    }
    Run-Phase 'p7-protocol-mismatch' $Exe $p7
    if (-not $PrepareOnly) {
        $askOk = ((Timeouts 'p7-protocol-mismatch') -eq 0) -and (IsAlive $hooked) -and ((DaemonPid) -eq $hooked) -and (Live-Has 'p7-protocol-mismatch' '^notice=protocol-mismatch\s*$') -and (Log-Has 'protocol mismatch: daemon v1 app v')
        Check 'p7 protocol mismatch asks (notice, app.log) and the daemon is kept' $askOk "timeouts=$(Timeouts 'p7-protocol-mismatch') alive=$(IsAlive $hooked)"
        Check 'p7 screen captured' (Test-Path (Join-Path $Out 'p7-protocol-mismatch\protocol-mismatch.png'))
        Stop-Mine $hooked
        # 8: 인사를 ERR로 거절하는 v1 데몬 -> 강제 종료 안내가 든 화면
        $strict = Start-Daemon $sibling @{ PADDOCK_DAEMON_PROTOCOL_VERSION = '1'; PADDOCK_DAEMON_STRICT_HELLO = '1' }
    }
    Run-Phase 'p8-protocol-mismatch-v1' $Exe $p8
    if (-not $PrepareOnly) {
        $strictOk = ((Timeouts 'p8-protocol-mismatch-v1') -eq 0) -and (IsAlive $strict) -and ((DaemonPid) -eq $strict) -and (Live-Has 'p8-protocol-mismatch-v1' '^notice=protocol-mismatch-v1\s*$') -and (Log-Has 'refused the hello')
        Check 'p8 a rejecting v1 daemon is reported (notice, app.log) and kept' $strictOk "timeouts=$(Timeouts 'p8-protocol-mismatch-v1') alive=$(IsAlive $strict)"
        Check 'p8 screen captured' (Test-Path (Join-Path $Out 'p8-protocol-mismatch-v1\protocol-mismatch-v1.png'))
        Stop-Mine $strict
    }
} catch {
    Log "exception: $($_.Exception.Message)"
    if (-not $PrepareOnly) { Check 'run finished without an exception' $false $_.Exception.Message }
} finally {
    # 남은 시험 데몬 정리: 이 실행이 띄운 것 중 아직 안 끝난 것만(이름·실행 파일을 다시 확인한다).
    foreach ($id in @($started)) { Stop-Mine $id }
    if (-not $PrepareOnly) {
        $left = @(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue)
        Check 'no daemon is left' ($left.Count -eq 0) "left=$($left.Id -join ',')"
        # 이 실행의 데몬이 띄운 wsl.exe(터미널 셸)가 남지 않았는지: 부모가 이 실행의 데몬인 것만 본다(이름만으로 세지 않는다).
        $wsl = @(Get-CimInstance Win32_Process -Filter "Name='wsl.exe'" -ErrorAction SilentlyContinue | Where-Object { $everStarted.Contains([int]$_.ParentProcessId) -and $_.CreationDate -ge $runStart })
        Check 'no wsl.exe of this run is left' ($wsl.Count -eq 0) "left=$($wsl.ProcessId -join ',')"
        "I49 $(if ($failed -eq 0) { 'PASS' } else { "FAIL ($failed)" })" | Out-File $result -Append -Encoding utf8
    }
    Log 'done'
}
