# 사용: privacy-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutDir <결과 폴더> [-Minutes 5]
# 기록·저장 파일과 네트워크 점검(T4b): 터미널 4개로 앱을 Minutes분 돌리며 앱·데몬 프로세스의 TCP/UDP 소켓을 10초마다 센 뒤,
# 입력한 글자·토큰이 로그·state.json에 남지 않았는지와 폴더 권한을 본다.
#   - 입력 표지: 터미널에 `echo <표지>`를 입력한다. 표지가 셸 출력으로는 나오지만 어떤 파일에도 남지 않아야 한다.
#   - 토큰은 daemon.json에만 있어야 한다(로그·state.json에 있으면 실패).
# 안전 규칙: session-run.ps1과 같다(실제 사용자 폴더 거부, 다른 paddock-daemon이 있으면 시작 안 함, 키·마우스 입력 없음, 띄운 프로세스만 종료).
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutDir,
    [int]$Minutes = 5
)
$realHome = [Environment]::GetFolderPath('UserProfile')
New-Item -ItemType Directory -Force $HomeDir | Out-Null
if ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\')) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$marker = 'PADDOCK_SECRET_MARKER_' + (Get-Random -Maximum 99999999) + '_한글입력'
$scriptPath = Join-Path $OutDir 'privacy.script'
$wait = ($Minutes * 60 * 1000)
@"
waittabs 1 @30000
send echo $marker\r
waitfor $marker @20000
key ctrl-shift-t
waittabs 2 @20000
key ctrl-shift-t
waittabs 3 @20000
key ctrl-shift-t
waittabs 4 @20000
send echo $marker\r
wait $wait
quit
"@ | Out-File $scriptPath -Encoding utf8
$report = Join-Path $OutDir 'privacy-report.txt'
Remove-Item $report -ErrorAction SilentlyContinue
function Say($text) { $text | Out-File $report -Append -Encoding utf8; Write-Output $text }
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$app = Start-Process -FilePath $Exe -ArgumentList "--script `"$scriptPath`" --out `"$OutDir`" --offscreen" -PassThru
$info = Join-Path $HomeDir '.paddock\native\daemon.json'
$n_samples = 0; $n_sockets = 0
$started = Get-Date
$deadline = $started.AddSeconds($wait / 1000 + 90)
while (-not $app.HasExited -and (Get-Date) -lt $deadline) {
    $pids = @($app.Id)
    if (Test-Path $info) { try { $pids += (Get-Content $info -Raw | ConvertFrom-Json).pid } catch {} }
    if ($pids.Count -eq 2) {
        $tcp = @(Get-NetTCPConnection -OwningProcess $pids -ErrorAction SilentlyContinue)
        $udp = @(Get-NetUDPEndpoint -OwningProcess $pids -ErrorAction SilentlyContinue)
        $n_samples++; $n_sockets += $tcp.Count + $udp.Count
        if (($n_samples % 6) -eq 1) { Say ("{0:N0}s  app pid {1} daemon pid {2}: TCP {3}, UDP {4}" -f ((Get-Date) - $started).TotalSeconds, $pids[0], $pids[1], $tcp.Count, $udp.Count) }
    }
    Start-Sleep -Seconds 10
}
if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force; Say "timeout: app stopped" }
Say "samples=$n_samples sockets_seen=$n_sockets  => $(if ($n_samples -gt 0 -and $n_sockets -eq 0) { 'PASS (0 sockets)' } else { 'FAIL or no samples' })"

# 파일 점검: 이 임시 사용자 폴더의 .paddock 아래 모든 파일에서 표지·토큰을 찾는다.
# 데몬이 독점으로 쥔 잠금 파일(daemon.lock)은 읽을 수 없어 크기만 본다(데몬은 이 파일에 아무것도 쓰지 않는다).
function Read-Text($path) {
    try { [System.IO.File]::ReadAllText($path, [System.Text.Encoding]::UTF8) } catch { $null }
}
$files = Get-ChildItem (Join-Path $HomeDir '.paddock') -Recurse -File -ErrorAction SilentlyContinue
$token = $null
if (Test-Path $info) { $token = (Get-Content $info -Raw | ConvertFrom-Json).token }
Say "files under .paddock: $($files.Count)"
$n_bad = 0
foreach ($file in $files) {
    $text = Read-Text $file.FullName
    $name = $file.FullName.Substring($HomeDir.Length + 1)
    if ($null -eq $text) {
        Say ("  {0,-40} unreadable (held open by the daemon), size {1} bytes" -f $name, $file.Length)
        if ($file.Length -gt 0) { $n_bad++ }
    } else {
        $hasMarker = $text.Contains($marker)
        $hasToken = [bool]($token -and $text.Contains($token))
        Say ("  {0,-40} marker={1} token={2}" -f $name, $hasMarker, $hasToken)
        if ($hasMarker -or ($hasToken -and $file.Name -ne 'daemon.json')) { $n_bad++ }
    }
}
Say "typed marker or token outside daemon.json: $n_bad file(s) => $(if ($n_bad -eq 0) { 'PASS' } else { 'FAIL' })"
[Console]::OutputEncoding = [System.Text.Encoding]::GetEncoding([Globalization.CultureInfo]::CurrentCulture.TextInfo.OEMCodePage)
Say "icacls .paddock\native:"
icacls (Join-Path $HomeDir '.paddock\native') | Out-String | ForEach-Object { $_.TrimEnd() -split "`r?`n" | ForEach-Object { Say ("  " + $_) } }
if (Test-Path (Join-Path $HomeDir '.paddock\native\logs')) { icacls (Join-Path $HomeDir '.paddock\native\logs') | Out-String | ForEach-Object { $_.TrimEnd() -split "`r?`n" | ForEach-Object { Say ("  " + $_) } } }
if (Test-Path (Join-Path $HomeDir '.paddock\native\state.json')) { icacls (Join-Path $HomeDir '.paddock\native\state.json') | Out-String | ForEach-Object { $_.TrimEnd() -split "`r?`n" | ForEach-Object { Say ("  " + $_) } } }
if (Test-Path $info) {
    try {
        $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid
        $process = Get-Process -Id $daemonPid -ErrorAction SilentlyContinue
        if ($process -and $process.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $daemonPid -Force }
    } catch {}
}
