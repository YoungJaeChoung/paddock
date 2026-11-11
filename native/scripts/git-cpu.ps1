# 사용: git-cpu.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutRoot <결과 폴더> [-Scenario git-cpu.script]
# WSL 저장소를 연 상태에서 놀 때(변경·출력 없음) 60초와 변경 중 60초의 앱 CPU(코어 하나 대비 %)와 `wsl.exe` 호출 수를 잰다.
#   호출 수는 두 가지로 센다: 앱 기록(app.log)의 `git: wsl.exe` 줄(앱이 띄운 것 전부)과, 바깥에서 30 ms마다 본 새 wsl.exe 중 부모가 이 앱인 것.
#   데몬이 띄우는 wsl.exe(터미널 감시)와 사용자가 따로 띄운 다른 Paddock의 것은 세지 않는다(부모로 가른다).
#   합격: 놀 때 호출 0·앱 CPU <= 2%, 변경 중 60초당 호출 <= 20(구간 길이로 60초에 맞춰 나눈 값). 불합격이면 종료 코드 1이다.
# 구간은 대본이 남기는 `mark idle-start/idle-end/change-start/change-end`(events.log)로 정한다.
# 안전 규칙은 session-run.ps1을 따른다: 실제 사용자 폴더 거부, 다른 paddock-daemon이 있으면 시작 안 함, 키·마우스 입력 없음(창은 화면 밖·비활성).
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutRoot,
    [string]$Scenario = 'git-cpu.script',
    [switch]$IdleOnly
)
$realHome = [Environment]::GetFolderPath('UserProfile')
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
if (Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue) { throw "paddock-daemon is already running" }
New-Item -ItemType Directory -Force $OutRoot | Out-Null
$out = Join-Path $OutRoot 'run1'
$script = Join-Path $PSScriptRoot "cases\$Scenario"
$runner = Start-Process powershell -WindowStyle Hidden -PassThru -ArgumentList @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'automation-run.ps1'),
    '-Exe', $Exe, '-Script', $script, '-Out', $out, '-HomeDir', $HomeDir, '-TimeoutSec', '300', '-Offscreen')
$events = Join-Path $out 'events.log'
$appLog = Join-Path $HomeDir '.paddock\native\logs\app.log'
function Wait-Mark($text) {
    $deadline = (Get-Date).AddSeconds(280)
    while ((Get-Date) -lt $deadline) {
        if ((Test-Path $events) -and (Select-String -Path $events -Pattern $text -Quiet)) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}
function App-Process() {
    # 이 실행의 앱: 실행 파일 경로가 -Exe와 같은 프로세스.
    return Get-Process -Name paddock-app -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq (Resolve-Path $Exe).Path } | Select-Object -First 1
}
function Measure-Window($startMark, $endMark, $label) {
    if (-not (Wait-Mark $startMark)) { return "FAIL ${label}: start mark never appeared" }
    $app = App-Process
    $daemonPid = 0
    $info = Join-Path $HomeDir '.paddock\native\daemon.json'
    if (Test-Path $info) { try { $daemonPid = [int](Get-Content $info -Raw | ConvertFrom-Json).pid } catch {} }
    $startedAt = Get-Date
    $startCpu = $app.TotalProcessorTime.TotalSeconds
    $known = @{}
    foreach ($p in Get-Process -Name wsl -ErrorAction SilentlyContinue) { $known[$p.Id] = $true }
    $byApp = @{}
    $byDaemon = @{}
    while (-not (Select-String -Path $events -Pattern $endMark -Quiet)) {
        foreach ($p in Get-Process -Name wsl -ErrorAction SilentlyContinue) {
            if (-not $known.ContainsKey($p.Id)) {
                # 새 wsl.exe 하나를 누가 띄웠는지(부모)를 한 번만 본다. 다른 Paddock(사용자 앱)의 것은 세지 않는다.
                $known[$p.Id] = $true
                $parent = (Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue).ParentProcessId
                if ($parent -eq $app.Id) { $byApp[$p.Id] = $true }
                elseif ($parent -eq $daemonPid) { $byDaemon[$p.Id] = $true }
            }
        }
        Start-Sleep -Milliseconds 30
    }
    $endedAt = Get-Date
    $app.Refresh()
    $seconds = ($endedAt - $startedAt).TotalSeconds
    $percent = 100.0 * ($app.TotalProcessorTime.TotalSeconds - $startCpu) / $seconds
    $logCalls = 0
    $localCalls = 0
    foreach ($line in Get-Content $appLog) {
        if ($line -match '^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+) git: (wsl\.exe|local) (\S+)') {
            $at = [datetime]::ParseExact($Matches[1], 'yyyy-MM-dd HH:mm:ss.fff', $null)
            if ($at -ge $startedAt -and $at -le $endedAt) { if ($Matches[2] -eq 'wsl.exe') { $logCalls++ } else { $localCalls++ } }
        }
    }
    return ("{0}: window={1:N1}s app_cpu={2:N2}% wsl_calls_log={3} per_60s={4:N1} wsl_children_of_app={5} wsl_children_of_daemon={6} local_git_calls={7}" -f $label, $seconds, $percent, $logCalls, ($logCalls * 60.0 / $seconds), $byApp.Count, $byDaemon.Count, $localCalls)
}
$result = @()
$result += Measure-Window 'idle-start' 'idle-end' 'idle'
if (-not $IdleOnly) { $result += Measure-Window 'change-start' 'change-end' 'change' }
$result | Tee-Object -FilePath (Join-Path $OutRoot 'git-cpu-result.txt')
if (-not $runner.HasExited) { $runner.WaitForExit(60000) | Out-Null }
$info = Join-Path $HomeDir '.paddock\native\daemon.json'
if (Test-Path $info) {
    try {
        $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid
        $process = Get-Process -Id $daemonPid -ErrorAction SilentlyContinue
        if ($process -and $process.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $daemonPid -Force }
    } catch {}
}
$idleOk = $result[0] -match 'wsl_calls_log=0 per_60s=0.0 wsl_children_of_app=0 .*local_git_calls=0' -and ([double]([regex]::Match($result[0], 'app_cpu=([\d.]+)%').Groups[1].Value) -le 2.0)
$changePer60 = if ($IdleOnly) { 0.0 } else { [double]([regex]::Match($result[1], 'per_60s=([\d.]+)').Groups[1].Value) }
if (-not ($idleOk -and $changePer60 -le 20.0)) { "VERDICT FAIL" | Tee-Object -FilePath (Join-Path $OutRoot 'git-cpu-result.txt') -Append; exit 1 }
"VERDICT PASS" | Tee-Object -FilePath (Join-Path $OutRoot 'git-cpu-result.txt') -Append
