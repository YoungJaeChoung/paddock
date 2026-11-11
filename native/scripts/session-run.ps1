# 사용: session-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutRoot <결과 폴더> -Scripts <대본1>,<대본2> [-IdleWaitSec 0] [-TimeoutSec 120]
# 같은 사용자 폴더로 앱을 대본 순서대로 다시 실행한다(앱을 닫았다 다시 여는 시험). 대본마다 결과는 OutRoot\runN 에 남는다.
# 안전 규칙
#   - HomeDir이 실제 사용자 폴더이면 거부한다. 실제 ~/.paddock 에는 아무것도 만들지 않는다.
#   - paddock-daemon 프로세스가 이미 떠 있으면 시작하지 않는다(파이프 이름이 사용자마다 하나라 시험이 섞인다).
#   - 키·마우스 입력은 보내지 않는다. 끝나면 이 HomeDir의 daemon.json 이 가리키는 데몬만 종료한다.
#   -IdleWaitSec 를 주면 마지막 앱이 끝난 뒤 데몬이 스스로 끝나기까지 걸린 시간을 idle.log 에 남긴다.
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutRoot,
    [string[]]$Scripts,
    [int]$IdleWaitSec = 0,
    [int]$TimeoutSec = 120,
    [switch]$Offscreen
)
$realHome = [Environment]::GetFolderPath('UserProfile')
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
New-Item -ItemType Directory -Force $OutRoot | Out-Null
$runner = Join-Path $PSScriptRoot 'automation-run.ps1'
$index = 0
foreach ($script in $Scripts) {
    $index++
    & $runner -Exe $Exe -Script $script -Out (Join-Path $OutRoot "run$index") -HomeDir $HomeDir -TimeoutSec $TimeoutSec -Offscreen:$Offscreen
}
$info = Join-Path $HomeDir '.paddock\native\daemon.json'
if ($IdleWaitSec -gt 0) {
    $started = Get-Date
    while ((Test-Path $info) -and ((Get-Date) - $started).TotalSeconds -lt $IdleWaitSec) { Start-Sleep -Milliseconds 200 }
    $gone = -not (Test-Path $info)
    ("daemon_json_removed={0} after_seconds={1:N1}" -f $gone, ((Get-Date) - $started).TotalSeconds) | Out-File (Join-Path $OutRoot 'idle.log') -Encoding utf8
}
if (Test-Path $info) {
    try {
        $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid
        $process = Get-Process -Id $daemonPid -ErrorAction SilentlyContinue
        if ($process -and $process.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $daemonPid -Force; "stopped daemon pid=$daemonPid" | Out-File (Join-Path $OutRoot 'cleanup.log') -Encoding utf8 }
    } catch {}
}
