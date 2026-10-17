# 사용: coexist-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutDir <결과 폴더>
# 기존 앱(Theia) 백엔드와 네이티브 데몬의 공존 점검. 사용자의 실제 기존 앱 백엔드는 읽기만 한다 — 실제 ~/.paddock/config/backend.json의 내용·수정 시각을
# 앞뒤로 비교하고, 그 프로세스가 점검 내내 살아 있고 포트를 쥔 채인지 본다. 네이티브 앱·데몬은 임시 사용자 폴더에서만 돌려
# 두 쪽의 파일(기존 앱 ~/.paddock/config, 네이티브 ~/.paddock/native)과 파이프 이름이 겹치지 않는지 확인한다.
# 안전 규칙: session-run.ps1과 같다(실제 사용자 폴더 거부, 다른 paddock-daemon이 있으면 시작 안 함, 키·마우스 입력 없음, 띄운 프로세스만 종료).
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutDir
)
$realHome = [Environment]::GetFolderPath('UserProfile')
New-Item -ItemType Directory -Force $HomeDir | Out-Null
if ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\')) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$report = Join-Path $OutDir 'coexist-report.txt'
Remove-Item $report -ErrorAction SilentlyContinue
function Say($text) { $text | Out-File $report -Append -Encoding utf8; Write-Output $text }

# 1. 기존 앱 백엔드의 상태(읽기만).
$theiaFile = Join-Path $realHome '.paddock\config\backend.json'
if (-not (Test-Path $theiaFile)) { throw "no Theia backend.json at $theiaFile (the existing app is not running)" }
function Theia-State {
    $file = Get-Item $theiaFile
    $info = Get-Content $theiaFile -Raw | ConvertFrom-Json
    $process = Get-Process -Id $info.pid -ErrorAction SilentlyContinue
    $listening = @(Get-NetTCPConnection -LocalPort $info.port -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.OwningProcess -eq $info.pid }).Count -gt 0
    [pscustomobject]@{
        Hash = (Get-FileHash $theiaFile -Algorithm SHA256).Hash.Substring(0, 12)
        Modified = $file.LastWriteTimeUtc.ToString('o')
        Pid = $info.pid; Port = $info.port
        Alive = [bool]$process
        Listening = $listening
    }
}
$before = Theia-State
Say ("Theia before : pid {0} port {1} alive={2} listening={3} backend.json sha256={4} modified={5}" -f $before.Pid, $before.Port, $before.Alive, $before.Listening, $before.Hash, $before.Modified)

# 2. 네이티브 앱과 데몬을 임시 폴더에서 띄워 15초 동안 터미널을 쥐게 한다.
$scriptPath = Join-Path $OutDir 'coexist.script'
"waittabs 1 @30000`nwait 15000`nquit`n" | Out-File $scriptPath -Encoding utf8
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$app = Start-Process -FilePath $Exe -ArgumentList "--script `"$scriptPath`" --out `"$OutDir`" --offscreen" -PassThru
$info = Join-Path $HomeDir '.paddock\native\daemon.json'
$deadline = (Get-Date).AddSeconds(20)
while (-not (Test-Path $info) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
Start-Sleep -Seconds 5
$daemonInfo = Get-Content $info -Raw | ConvertFrom-Json
$during = Theia-State
$daemonProcess = Get-Process -Id $daemonInfo.pid -ErrorAction SilentlyContinue
$pipeNames = [IO.Directory]::GetFiles('\\.\pipe\') | Where-Object { $_ -like '*paddock*' }
Say ("Native daemon : pid {0} alive={1} endpoint {2}" -f $daemonInfo.pid, [bool]$daemonProcess, $daemonInfo.endpoint)
Say ("Theia during: pid {0} alive={1} listening={2}" -f $during.Pid, $during.Alive, $during.Listening)
Say ("pipes named paddock*: {0}" -f ($pipeNames -join ', '))
$ownedByNative = @(Get-NetTCPConnection -OwningProcess $app.Id, $daemonInfo.pid -ErrorAction SilentlyContinue)
Say ("TCP connections owned by the native app or daemon: {0} (Theia port {1} is not among them: {2})" -f $ownedByNative.Count, $during.Port, (@($ownedByNative | Where-Object { $_.LocalPort -eq $during.Port -or $_.RemotePort -eq $during.Port }).Count -eq 0))
$nativeFiles = Get-ChildItem (Join-Path $HomeDir '.paddock') -Recurse -File | ForEach-Object { $_.FullName.Substring($HomeDir.Length + 1) }
Say ("files the native side wrote under the temp home .paddock: {0}" -f ($nativeFiles -join '; '))
$app.WaitForExit(40000) | Out-Null
if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
if (Test-Path $info) {
    $process = Get-Process -Id (Get-Content $info -Raw | ConvertFrom-Json).pid -ErrorAction SilentlyContinue
    if ($process -and $process.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $process.Id -Force }
}

# 3. 끝난 뒤 기존 앱 백엔드가 그대로인지 본다.
$after = Theia-State
Say ("Theia after  : pid {0} alive={1} listening={2} backend.json sha256={3} modified={4}" -f $after.Pid, $after.Alive, $after.Listening, $after.Hash, $after.Modified)
$overlap = @($nativeFiles | Where-Object { $_ -notlike '.paddock\native\*' })
$pass = $before.Alive -and $during.Alive -and $after.Alive -and $during.Listening -and $after.Listening -and `
    $daemonProcess -and ($before.Hash -eq $after.Hash) -and ($before.Modified -eq $after.Modified) -and `
    ($overlap.Count -eq 0) -and ($daemonInfo.endpoint -like '\\.\pipe\paddock-*')
Say ("overlap (native files outside .paddock\native): {0}" -f $overlap.Count)
Say ("RESULT {0}" -f $(if ($pass) { 'PASS: both backends alive at the same time, nothing shared, the existing backend was not touched' } else { 'FAIL' }))
