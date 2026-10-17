# 시험 스크립트 공용: 앱·데몬·그 아래 셸이 사용자의 실제 폴더와 실제 설치 도구를 건드리지 않게 환경을 임시 홈으로 바꾼다.
#   - USERPROFILE·HOME·APPDATA·LOCALAPPDATA를 임시 홈 아래로 옮겨, 설정·기록·캐시가 모두 임시 홈에 쌓인다.
#   - PATH 맨 앞에 `<임시 홈>\bin`을 둬서, 시험이 그 폴더에 만든 가짜 도구(예: claude)가 사용자 PC에 설치된 같은 이름의 실제 도구보다 먼저 불린다.
# 사용: `. (Join-Path $PSScriptRoot 'test-env.ps1'); Use-TestHome $HomeDir`
function Use-TestHome {
    param([string]$HomeDir)
    $roaming = Join-Path $HomeDir 'AppData\Roaming'
    $local = Join-Path $HomeDir 'AppData\Local'
    $bin = Join-Path $HomeDir 'bin'
    foreach ($folder in $HomeDir, $roaming, $local, $bin) { New-Item -ItemType Directory -Force $folder | Out-Null }
    $env:USERPROFILE = $HomeDir
    $env:HOME = $HomeDir
    $env:APPDATA = $roaming
    $env:LOCALAPPDATA = $local
    $entries = @($env:PATH -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ine $bin.TrimEnd('\')) })
    $env:PATH = (@($bin) + $entries) -join ';'
}
