# 사용: cpu.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutRoot <결과 폴더> [-Scenario cpu-idle.script]
# 터미널 4개가 출력 없이 놓인 60초 구간의 데몬 CPU(코어 하나 대비 %)와 그 구간의 wsl.exe 호출 수를 재고 합격 여부를 낸다.
#   합격: 평균 CPU <= 1% 이고 wsl.exe 호출 0. 불합격이면 종료 코드 1이다.
# 구간은 대본이 남기는 `mark cpu-window-start`·`cpu-window-end`(events.log)로 정한다. 호출 수는 daemon.log의 `probe: wsl.exe` 줄을 센다.
# 안전 규칙은 session-run.ps1을 따른다: 실제 사용자 폴더 거부, 다른 paddock-daemon이 있으면 시작 안 함, 키·마우스 입력 없음(창은 화면 밖·비활성).
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutRoot,
    [string]$Scenario = 'cpu-idle.script'
)
$limitPercent = 1.0
$realHome = [Environment]::GetFolderPath('UserProfile')
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
if (Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue) { throw "paddock-daemon is already running" }
New-Item -ItemType Directory -Force $OutRoot | Out-Null
$out = Join-Path $OutRoot 'run1'
$script = Join-Path $PSScriptRoot "cases\$Scenario"
$runner = Start-Process powershell -WindowStyle Hidden -PassThru -ArgumentList @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'automation-run.ps1'),
    '-Exe', $Exe, '-Script', $script, '-Out', $out, '-HomeDir', $HomeDir, '-TimeoutSec', '240', '-Offscreen')
$events = Join-Path $out 'events.log'
$info = Join-Path $HomeDir '.paddock\native\daemon.json'
function Wait-Mark($text) {
    $deadline = (Get-Date).AddSeconds(200)
    while ((Get-Date) -lt $deadline) {
        if ((Test-Path $events) -and (Select-String -Path $events -Pattern $text -Quiet)) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}
function Daemon-Cpu() {
    $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid
    return (Get-Process -Id $daemonPid).TotalProcessorTime.TotalSeconds
}
$result = @()
if (Wait-Mark 'cpu-window-start') {
    $startedAt = Get-Date; $startCpu = Daemon-Cpu
    if (Wait-Mark 'cpu-window-end') {
        $endedAt = Get-Date; $endCpu = Daemon-Cpu
        $seconds = ($endedAt - $startedAt).TotalSeconds
        $percent = 100.0 * ($endCpu - $startCpu) / $seconds
        $log = Join-Path $HomeDir '.paddock\native\logs\daemon.log'
        $calls = 0
        foreach ($line in Get-Content $log) {
            if ($line -match '^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+) probe: wsl\.exe read') {
                $at = [datetime]::ParseExact($Matches[1], 'yyyy-MM-dd HH:mm:ss.fff', $null)
                if ($at -ge $startedAt -and $at -le $endedAt) { $calls++ }
            }
        }
        $pass = ($percent -le $limitPercent) -and ($calls -eq 0)
        $result += ("window={0:N1}s daemon_cpu={1:N3}s average={2:N3}% (<= {3}) wsl_calls={4} (== 0) => {5}" -f $seconds, ($endCpu - $startCpu), $percent, $limitPercent, $calls, $(if ($pass) { 'PASS' } else { 'FAIL' }))
    } else { $result += 'FAIL: window end mark never appeared' }
} else { $result += 'FAIL: window start mark never appeared' }
$result | Tee-Object -FilePath (Join-Path $OutRoot 'cpu-result.txt')
if (-not $runner.HasExited) { $runner.WaitForExit(60000) | Out-Null }
if (Test-Path $info) {
    try {
        $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid
        $process = Get-Process -Id $daemonPid -ErrorAction SilentlyContinue
        if ($process -and $process.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $daemonPid -Force }
    } catch {}
}
if (-not ($result -match 'PASS')) { exit 1 }
