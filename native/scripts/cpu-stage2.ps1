# 사용: cpu-stage2.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutRoot <결과 폴더> [-Scenario cpu-stage2.script]
# 2단계 마감 CPU 총측정: 터미널 4개와 WSL 저장소 폴더(파일 목록·Source control·9P 폴링 켬)에서 놀 때 60초와 변경 중 60초의
# 앱 CPU·데몬 CPU(코어 하나 대비 %)와 `wsl.exe` 호출 수를 잰다.
#   호출 수는 세 가지로 센다: 앱 기록(app.log)의 `git: wsl.exe` 줄(앱이 띄운 git 전부), 바깥에서 30 ms마다 본 새 wsl.exe 중 부모가 앱인 것·데몬인 것
#   (데몬의 것은 터미널 감시의 `probe: wsl.exe`이며 1단계 동작이다). 변경 중에는 앱 호출 사이 최소 간격도 낸다.
#   합격: 놀 때 데몬 CPU <= 1%·앱 호출 0·데몬 호출 0, 변경 중 앱 호출이 60초 창에 맞춰 <= 21회·최소 간격 >= 3초. 앱 CPU <= 2%는 점검 빌드에서 판정하지 않고 수치만 낸다(D60).
# 구간은 대본이 남기는 `mark idle-start/idle-end/change-start/change-end`(events.log)로 정한다.
# 안전 규칙은 session-run.ps1을 따른다: 실제 사용자 폴더 거부, 다른 paddock-daemon이 있으면 시작 안 함, 키·마우스 입력 없음(창은 화면 밖·비활성).
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutRoot,
    [string]$Scenario = 'cpu-stage2.script'
)
$realHome = [Environment]::GetFolderPath('UserProfile')
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
if (Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue) { throw "paddock-daemon is already running" }
New-Item -ItemType Directory -Force $OutRoot | Out-Null
$out = Join-Path $OutRoot 'run1'
$script = Join-Path $PSScriptRoot "cases\$Scenario"
$runner = Start-Process powershell -WindowStyle Hidden -PassThru -ArgumentList @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'automation-run.ps1'),
    '-Exe', $Exe, '-Script', $script, '-Out', $out, '-HomeDir', $HomeDir, '-TimeoutSec', '330', '-Offscreen')
$events = Join-Path $out 'events.log'
$appLog = Join-Path $HomeDir '.paddock\native\logs\app.log'
$daemonLog = Join-Path $HomeDir '.paddock\native\logs\daemon.log'
$info = Join-Path $HomeDir '.paddock\native\daemon.json'
function Wait-Mark($text) {
    $deadline = (Get-Date).AddSeconds(300)
    while ((Get-Date) -lt $deadline) {
        if ((Test-Path $events) -and (Select-String -Path $events -Pattern $text -Quiet)) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}
function App-Process() {
    return Get-Process -Name paddock-app -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq (Resolve-Path $Exe).Path } | Select-Object -First 1
}
function Measure-Window($startMark, $endMark, $label) {
    if (-not (Wait-Mark $startMark)) { return "FAIL ${label}: start mark never appeared" }
    $app = App-Process
    $daemonPid = [int](Get-Content $info -Raw | ConvertFrom-Json).pid
    $daemon = Get-Process -Id $daemonPid
    $startedAt = Get-Date
    $startApp = $app.TotalProcessorTime.TotalSeconds
    $startDaemon = $daemon.TotalProcessorTime.TotalSeconds
    $known = @{}
    foreach ($p in Get-Process -Name wsl -ErrorAction SilentlyContinue) { $known[$p.Id] = $true }
    $byApp = @{}
    $byDaemon = @{}
    while (-not (Select-String -Path $events -Pattern $endMark -Quiet)) {
        foreach ($p in Get-Process -Name wsl -ErrorAction SilentlyContinue) {
            if (-not $known.ContainsKey($p.Id)) {
                $known[$p.Id] = $true
                $parent = (Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue).ParentProcessId
                if ($parent -eq $app.Id) { $byApp[$p.Id] = $true }
                elseif ($parent -eq $daemonPid) { $byDaemon[$p.Id] = $true }
            }
        }
        Start-Sleep -Milliseconds 30
    }
    $endedAt = Get-Date
    $app.Refresh(); $daemon.Refresh()
    $seconds = ($endedAt - $startedAt).TotalSeconds
    $appPercent = 100.0 * ($app.TotalProcessorTime.TotalSeconds - $startApp) / $seconds
    $daemonPercent = 100.0 * ($daemon.TotalProcessorTime.TotalSeconds - $startDaemon) / $seconds
    $logCalls = 0; $localCalls = 0; $times = @()
    foreach ($line in Get-Content $appLog) {
        if ($line -match '^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+) git: (wsl\.exe|local) (\S+)') {
            $at = [datetime]::ParseExact($Matches[1], 'yyyy-MM-dd HH:mm:ss.fff', $null)
            if ($at -ge $startedAt -and $at -le $endedAt) { if ($Matches[2] -eq 'wsl.exe') { $logCalls++; $times += $at } else { $localCalls++ } }
        }
    }
    $probeCalls = 0
    foreach ($line in Get-Content $daemonLog) {
        if ($line -match '^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+) probe: wsl\.exe read') {
            $at = [datetime]::ParseExact($Matches[1], 'yyyy-MM-dd HH:mm:ss.fff', $null)
            if ($at -ge $startedAt -and $at -le $endedAt) { $probeCalls++ }
        }
    }
    # status 호출끼리의 최소 간격(rev-parse 같은 같은 순간의 보조 호출은 한 묶음으로 본다: 0.5초 안은 같은 묶음).
    $gaps = @(); $lastGroup = $null
    foreach ($t in $times) { if ($lastGroup -eq $null -or ($t - $lastGroup).TotalSeconds -gt 0.5) { if ($lastGroup -ne $null) { $gaps += ($t - $lastGroup).TotalSeconds }; $lastGroup = $t } }
    $minGap = if ($gaps.Count -gt 0) { ($gaps | Measure-Object -Minimum).Minimum } else { -1 }
    return ("{0}: window={1:N1}s app_cpu={2:N2}% daemon_cpu={3:N3}% wsl_calls_log={4} per_60s={5:N1} min_gap_s={6:N2} wsl_children_of_app={7} wsl_children_of_daemon={8} daemon_probe_log={9} local_git_calls={10}" -f $label, $seconds, $appPercent, $daemonPercent, $logCalls, ($logCalls * 60.0 / $seconds), $minGap, $byApp.Count, $byDaemon.Count, $probeCalls, $localCalls)
}
$result = @()
$result += Measure-Window 'idle-start' 'idle-end' 'idle'
$result += Measure-Window 'change-start' 'change-end' 'change'
$result | Tee-Object -FilePath (Join-Path $OutRoot 'cpu-result.txt')
if (-not $runner.HasExited) { $runner.WaitForExit(60000) | Out-Null }
if (Test-Path $info) {
    try {
        $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid
        $process = Get-Process -Id $daemonPid -ErrorAction SilentlyContinue
        if ($process -and $process.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $daemonPid -Force }
    } catch {}
}
