# 사용: attack-run.ps1 -DaemonExe <paddock-daemon.exe> -ProbeExe <probe.exe> -HomeDir <임시 사용자 폴더> -OutDir <결과 폴더>
# 데몬 접속 공격 재연(T1·T2). 다른 Windows 계정을 만들지 않으므로 같은 계정으로 할 수 있는 시도와 운영체제 값 조회만 한다 — 실제 다른 계정의 거부는 검증하지 않는다.
#   T1  데몬을 띄워 파이프 권한(소유자 SID 하나), 올바른 토큰 접속, 틀린 토큰 거부, 틀린 이름·파일 권한을 본다.
#   T2  같은 이름의 가짜 서버가 먼저 파이프를 쥐면 데몬이 뜨지 못하는지, 앱의 서버 확인이 시스템 서비스(다른 사용자)의 파이프를 인사 전에 거부하는지,
#       같은 사용자의 가짜 서버에는 가장(impersonation)이 신원 확인 수준에서 멈추는지 본다.
# 안전 규칙: HomeDir이 실제 사용자 폴더이면 거부, 다른 paddock-daemon이 이미 있으면 시작 안 함, 이 스크립트가 띄운 프로세스만 종료한다.
# 시스템 서비스의 파이프에는 연결만 열고 닫는다(데이터를 보내지 않는다 — 앱의 서버 확인이 인사 전에 끊는 것을 보이는 시도다).
param(
    [string]$DaemonExe,
    [string]$ProbeExe,
    [string]$HomeDir,
    [string]$OutDir
)
$realHome = [Environment]::GetFolderPath('UserProfile')
New-Item -ItemType Directory -Force $HomeDir | Out-Null
if ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\')) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
# probe는 UTF-8로 출력한다(운영체제 오류 문장 포함).
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$report = Join-Path $OutDir 'attack-report.txt'
Remove-Item $report -ErrorAction SilentlyContinue
function Say($text) { $text | Out-File $report -Append -Encoding utf8; Write-Output $text }
# probe를 한 번 실행하고 출력을 기록한다. 상대가 답하지 않는 시도(가짜 서버)는 TimeoutSec 뒤 끊는다.
function Probe {
    param([string[]]$ProbeArgs, [int]$TimeoutSec = 10)
    Say ("> probe " + ($ProbeArgs -join ' '))
    $out = Join-Path $OutDir 'probe-step.out'
    $argText = ($ProbeArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }) -join ' '
    $process = Start-Process -FilePath $ProbeExe -ArgumentList $argText -PassThru -WindowStyle Hidden -RedirectStandardOutput $out
    if (-not $process.WaitForExit($TimeoutSec * 1000)) {
        Stop-Process -Id $process.Id -Force
        Say "  (no answer within $TimeoutSec s; probe stopped)"
    }
    Get-Content $out -Encoding UTF8 | ForEach-Object { Say ("  " + $_) }
}
$infoPath = Join-Path $HomeDir '.paddock\native\daemon.json'
$started = @()

Say "=== T1: 다른 사용자가 파이프를 열거나 토큰을 추측하는 시도 (같은 계정으로 할 수 있는 것) ==="
$daemon = Start-Process -FilePath $DaemonExe -PassThru -WindowStyle Hidden
$started += $daemon.Id
$deadline = (Get-Date).AddSeconds(5)
while (-not (Test-Path $infoPath) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50 }
$info = Get-Content $infoPath -Raw | ConvertFrom-Json
Say "daemon pid $($daemon.Id) endpoint $($info.endpoint)"
# 데몬은 연결과 터미널이 모두 0이면 10초 뒤 끝난다 — 아래 시도는 그 안에 한다.
Probe @('dacl')
Probe @('connect')
Probe @('badtoken')
Probe @('connect', '--token', 'not-the-token')
Probe @('connect', '--endpoint', '\\.\pipe\paddock-0000000000000000', '--token', $info.token)
Say "> icacls (daemon.json, native folder)"
# icacls는 시스템 기본(OEM) 코드 페이지로 출력한다.
[Console]::OutputEncoding = [System.Text.Encoding]::GetEncoding([Globalization.CultureInfo]::CurrentCulture.TextInfo.OEMCodePage)
icacls $infoPath | Out-String | ForEach-Object { $_.TrimEnd() -split "`r?`n" | ForEach-Object { Say ("  " + $_) } }
icacls (Join-Path $HomeDir '.paddock\native') | Out-String | ForEach-Object { $_.TrimEnd() -split "`r?`n" | ForEach-Object { Say ("  " + $_) } }
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Stop-Process -Id $daemon.Id -Force -ErrorAction SilentlyContinue
Remove-Item $infoPath -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

Say ""
Say "=== T2: 가짜 서버가 파이프 이름을 먼저 쥔 경우 ==="
$fakeOut = Join-Path $OutDir 'fake-server.out'
Remove-Item $fakeOut -ErrorAction SilentlyContinue
$fake = Start-Process -FilePath $ProbeExe -ArgumentList 'fake-server' -PassThru -WindowStyle Hidden -RedirectStandardOutput $fakeOut
$started += $fake.Id
Start-Sleep -Milliseconds 800
Say "fake server pid $($fake.Id) (same user): $((Get-Content $fakeOut -Raw -Encoding UTF8).Trim())"
$fakeEndpoint = ((Get-Content $fakeOut -Raw -Encoding UTF8) -replace '(?s).*listening on\s+(\S+).*', '$1').Trim()

Say "-- (a) 진짜 데몬이 같은 이름으로 파이프를 만들려 할 때 (FIRST_PIPE_INSTANCE: 이미 있으면 실패해야 한다)"
$daemon2 = Start-Process -FilePath $DaemonExe -PassThru -WindowStyle Hidden
$started += $daemon2.Id
$daemon2.WaitForExit(5000) | Out-Null
Say "daemon exited=$($daemon2.HasExited) exit code=$(if ($daemon2.HasExited) { $daemon2.ExitCode } else { 'still running' })"
if (-not $daemon2.HasExited) { Stop-Process -Id $daemon2.Id -Force }
$logPath = Join-Path $HomeDir '.paddock\native\logs\daemon.log'
if (Test-Path $logPath) { Say "daemon.log tail:"; Get-Content $logPath -Tail 3 -Encoding UTF8 | ForEach-Object { Say ("  " + $_) } }
Remove-Item $infoPath -ErrorAction SilentlyContinue

Say "-- (b) 앱의 서버 확인(connect_verified)이 다른 사용자의 서버를 인사 전에 거부하는지: 시스템 서비스의 파이프를 대상으로"
foreach ($service in @('wkssvc', 'LanmanServer')) {
    $svc = Get-CimInstance Win32_Service -Filter "Name='$service'" -ErrorAction SilentlyContinue
    if ($svc -and $svc.ProcessId) { Probe @('owner', "$($svc.ProcessId)") ; break }
}
foreach ($pipe in @('\\.\pipe\wkssvc', '\\.\pipe\srvsvc', '\\.\pipe\atsvc')) {
    Probe @('connect', '--verified', '--endpoint', $pipe, '--token', 'x')
}
Say "-- (c) 같은 사용자의 서버 프로세스는 통과한다(대조군)"
Probe @('owner', "$($fake.Id)")

Say "-- (d) 같은 사용자의 가짜 서버에 앱처럼 접속하면 가장은 신원 확인 수준에서 멈춘다. 라이브러리 기본 접속은 가장 수준이 높다(대조군)"
Probe @('connect', '--endpoint', $fakeEndpoint, '--token', 'x') -TimeoutSec 3
Probe @('connect', '--plain', '--endpoint', $fakeEndpoint, '--token', 'x') -TimeoutSec 3
Start-Sleep -Milliseconds 500
Say "fake server output:"
Get-Content $fakeOut -Encoding UTF8 | ForEach-Object { Say ("  " + $_) }

foreach ($id in $started) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
Remove-Item $infoPath -ErrorAction SilentlyContinue
Say "done"
