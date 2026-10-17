# 사용: memory.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -OutRoot <결과 폴더> [-Scenario memory-budget.script] [-Visible]
# 네이티브 앱과 데몬의 Private 메모리(MB, 셸 제외)를 표준 시나리오로 재고 예산과 견줘 합격 여부를 낸다.
#   시나리오 memory-budget.script: 시작 → 터미널 4개(1개 보임) 20초 뒤 → 4개에 각각 5만 줄 출력 뒤
#   scrollback-50k.script: 터미널 1개에 5만 줄(앱 Private 증가 ≤ 40 MB와 입력 지연을 본다)
# 예산(앱 ≤ 180 · 데몬 ≤ 40 · 합 ≤ 220)은 시나리오의 모든 표본에서 가장 큰 값으로 판정한다. 결과는 OutRoot\memory-result.txt 에도 남고, 불합격이면 종료 코드 1이다.
# 안전 규칙은 session-run.ps1을 따른다: 실제 사용자 폴더 거부, 다른 paddock-daemon이 있으면 시작 안 함, 키·마우스 입력 없음.
# 기본은 창을 화면 밖에 활성화 없이 띄운다(-Visible이면 화면 안).
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$OutRoot,
    [string]$Scenario = 'memory-budget.script',
    [switch]$Visible
)
$budget = @{ app = 180; daemon = 40; total = 220 }
$script = Join-Path $PSScriptRoot "cases\$Scenario"
& (Join-Path $PSScriptRoot 'session-run.ps1') -Exe $Exe -HomeDir $HomeDir -OutRoot $OutRoot -Scripts $script -TimeoutSec 240 -Offscreen:(-not $Visible)
$log = Join-Path $OutRoot 'run1\mem.log'
if (-not (Test-Path $log)) { throw "no memory samples at $log" }
$samples = [ordered]@{}
foreach ($line in Get-Content $log) {
    $line = $line.TrimStart([char]0xFEFF)
    if ($line -match '^(\S+) (paddock-app|paddock-daemon) pid=\d+ PrivateMB=([\d.,]+)') {
        $label = $Matches[1]; $mb = [double]($Matches[3] -replace ',', '')
        if (-not $samples.Contains($label)) { $samples[$label] = @{ app = 0.0; daemon = 0.0 } }
        $samples[$label][$(if ($Matches[2] -eq 'paddock-app') { 'app' } else { 'daemon' })] = $mb
    }
}
$result = @()
$result += "scenario=$Scenario"
$maxApp = 0.0; $maxDaemon = 0.0; $maxTotal = 0.0
foreach ($label in $samples.Keys) {
    $app = $samples[$label].app; $daemon = $samples[$label].daemon
    $result += ("{0,-24} app={1,6:N1} MB  daemon={2,5:N1} MB  total={3,6:N1} MB" -f $label, $app, $daemon, ($app + $daemon))
    $maxApp = [Math]::Max($maxApp, $app); $maxDaemon = [Math]::Max($maxDaemon, $daemon); $maxTotal = [Math]::Max($maxTotal, $app + $daemon)
}
$pass = ($maxApp -le $budget.app) -and ($maxDaemon -le $budget.daemon) -and ($maxTotal -le $budget.total)
$result += ("max app={0:N1} (<= {1})  daemon={2:N1} (<= {3})  total={4:N1} (<= {5})  => {6}" -f $maxApp, $budget.app, $maxDaemon, $budget.daemon, $maxTotal, $budget.total, $(if ($pass) { 'PASS' } else { 'FAIL' }))
$result | Tee-Object -FilePath (Join-Path $OutRoot 'memory-result.txt')
if (-not $pass) { exit 1 }
