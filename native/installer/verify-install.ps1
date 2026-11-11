# 사용: verify-install.ps1 -Installer <PaddockNative-Setup-….exe> -ExeDir <설치 파일에 담은 실행 파일 폴더> [-Version <버전>] [-Smoke] [-AllowSmokeOnThisPc]
#
# 네이티브 설치 파일을 실제로 설치·제거해 보고, 같은 PC의 Electron Paddock과 피드백 사본이 그대로인지 확인한다.
#   V0 준비      실제 설치(PaddockNative 키·Paddock Native.lnk)가 있으면 멈춘다. Electron 쪽 키·폴더·바로 가기·프로세스를 기록한다
#   V1 무인 설치  공백이 든 임시 폴더에 /S /D=로 설치: 파일 4개, 해시, 제거 키 값, 시작 메뉴 바로 가기
#   V2 폴더 거부  Programs\Paddock…·PaddockFeedback…로 시작하는 폴더는 종료 코드 4이고 폴더가 생기지 않는다
#   V3 pen-mcp   설치된 paddock-pen-mcp.exe가 initialize·tools/list에 답하고 입력을 닫으면 끝난다
#   V4 앱 시작    (-Smoke, 조건부) 임시 홈에서 앱을 띄워 데몬·pen-mcp.json이 설치 폴더를 가리키는지 본다
#   V5 실행 중    (V4를 했을 때만) 앱 실행 중 재설치 → 3, 데몬만 남긴 재설치 → 0(옛 데몬은 replaced\1로), 데몬만 남긴 제거 → 5
#   V6 무인 제거  /S _?=로 제거: 키·바로 가기·실행 파일·replaced가 없다
#   V7 무손상     V0 기록과 비교한다
#   V8 정리       작업 폴더를 지운다
#
# 안전 규칙
#   - 작업 폴더는 실행마다 %TEMP%\pn-verify-<임의값>으로 새로 만들고, 지우는 것은 이 실행이 만든 그 아래 경로와
#     V2가 실수로 만든 두 시험 폴더뿐이다(Remove-OwnPath가 그 밖의 경로를 거부한다).
#   - 프로세스는 실행 파일 경로가 작업 폴더 아래인 것만 끝낸다. 이름으로 찾아 끝내지 않는다.
#   - V4·V5는 paddock-daemon이 하나도 없을 때만 한다(데몬 파이프가 사용자마다 하나라 다른 데몬과 섞인다).
#     그리고 CI이거나 -AllowSmokeOnThisPc를 줬을 때만 한다 — release 앱은 임시 홈에서도 WSL 쪽 공용 Claude 설정에 쓴다.
# 결과: 단계마다 PASS·FAIL·SKIP 한 줄. FAIL이 하나라도 있으면 종료 코드 1.
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$ExeDir,
    [string]$Version = '',
    [switch]$Smoke,
    [switch]$AllowSmokeOnThisPc
)
$ErrorActionPreference = 'Stop'

$Installer = (Resolve-Path -LiteralPath $Installer).Path
$ExeDir = (Resolve-Path -LiteralPath $ExeDir).Path
if (-not $Version) {
    if ([IO.Path]::GetFileName($Installer) -match '^PaddockNative-Setup-(.+)-x64\.exe$') { $Version = $Matches[1] }
    else { $Version = (Get-Item -LiteralPath (Join-Path $ExeDir 'paddock-app.exe')).VersionInfo.ProductVersion }
}

# -- 설치 파일이 쓰는 이름(paddock-native.nsi와 같아야 한다) --
$Names = @{
    UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PaddockNative'
    Uninstaller  = 'Uninstall Paddock Native.exe'
    Shortcut     = 'Paddock Native.lnk'
    Exes         = @('paddock-app.exe', 'paddock-daemon.exe', 'paddock-pen-mcp.exe')
}
# -- 읽기만 하는 Electron·피드백 앱 항목 --
$LocalAppData = [Environment]::GetFolderPath('LocalApplicationData')
$Electron = @{
    UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\9aa88f6f-06f5-5270-9789-4b07f3891133'
    AppKey       = 'HKCU:\Software\9aa88f6f-06f5-5270-9789-4b07f3891133'
    Folders      = @((Join-Path $LocalAppData 'Programs\Paddock'), (Join-Path $LocalAppData 'PaddockFeedback'))
    Shortcuts    = @((Join-Path ([Environment]::GetFolderPath('Programs')) 'Paddock.lnk'), (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Paddock.lnk'))
}
$StartMenuShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) $Names.Shortcut
$DesktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) $Names.Shortcut
$RealPenMcpConfig = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.paddock\config\claude\pen-mcp.json'
$GuardProbeFolders = @((Join-Path $LocalAppData 'Programs\PaddockGuardProbe'), (Join-Path $LocalAppData 'PaddockFeedbackGuardProbe'))

$Root = Join-Path ([IO.Path]::GetTempPath()) ('pn-verify-' + [Guid]::NewGuid().ToString('N').Substring(0, 12))
$AppDir = Join-Path $Root 'app dir'
$TestHome = Join-Path $Root 'home'
$n_failures = 0

function Say($text) { Write-Output $text }
function Record($step, $passed, $detail) {
    $verdict = if ($null -eq $passed) { 'SKIP' } elseif ($passed) { 'PASS' } else { 'FAIL' }
    if ($verdict -eq 'FAIL') { $script:n_failures++ }
    Say ('{0} {1}: {2}' -f $step, $verdict, $detail)
}

# 이 실행이 만든 경로만 지운다: 작업 폴더 아래이거나 V2의 두 시험 폴더여야 한다.
function Remove-OwnPath($path) {
    $full = [IO.Path]::GetFullPath($path)
    $underRoot = $full.StartsWith($Root + '\', [StringComparison]::OrdinalIgnoreCase) -or ($full -ieq $Root)
    $isGuardProbe = @($GuardProbeFolders | Where-Object { $_ -ieq $full }).Count -gt 0
    if (-not ($underRoot -or $isGuardProbe)) { throw "refusing to delete a path this run did not create: $full" }
    if (Test-Path -LiteralPath $full) { Remove-Item -LiteralPath $full -Recurse -Force }
}

# 실행 파일 경로가 작업 폴더 아래인 프로세스만 끝낸다.
function Stop-OwnProcesses {
    $prefix = $Root + '\'
    Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

# 프로세스를 띄운 실행 파일 경로. 띄운 뒤 그 파일을 옮겨도 처음 경로가 나온다 — 옮겨졌는지는 파일 쪽으로 본다.
function Process-Path($processId) {
    $process = Get-CimInstance Win32_Process -Filter "ProcessId = $processId" -ErrorAction SilentlyContinue
    if ($process) { $process.ExecutablePath } else { $null }
}

# 인자는 한 문자열로 넘긴다 — /D=와 _?=는 따옴표 없이 마지막에 와야 하고, 배열로 넘기면 셸마다 따옴표 처리가 다르다.
function Run-Setup($file, $arguments) {
    $process = Start-Process -FilePath $file -ArgumentList $arguments -Wait -PassThru
    $process.ExitCode
}

function Hash($path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
function Hashes-Match($folder) {
    @($Names.Exes | Where-Object { (Hash (Join-Path $folder $_)) -ne (Hash (Join-Path $ExeDir $_)) }).Count -eq 0
}

function Shortcut-Target($path) {
    (New-Object -ComObject WScript.Shell).CreateShortcut($path).TargetPath
}

# Electron·피드백 앱 쪽 상태를 한 줄씩의 목록으로 적는다. V0과 V7이 같은 함수로 기록해 그대로 비교한다.
function Electron-State {
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($key in $Electron.UninstallKey, $Electron.AppKey) {
        if (Test-Path $key) {
            $item = Get-ItemProperty -LiteralPath $key
            foreach ($property in ($item.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | Sort-Object Name)) {
                $lines.Add(('key {0} {1}={2}' -f $key, $property.Name, $property.Value))
            }
        } else { $lines.Add("key $key (none)") }
    }
    foreach ($folder in $Electron.Folders) {
        if (Test-Path -LiteralPath $folder) {
            foreach ($entry in (Get-ChildItem -LiteralPath $folder -Force | Sort-Object Name)) {
                $size = if ($entry.PSIsContainer) { '-' } else { $entry.Length }
                $lines.Add(('entry {0} {1} {2}' -f $entry.FullName, $size, $entry.LastWriteTimeUtc.ToString('o')))
            }
        } else { $lines.Add("folder $folder (none)") }
    }
    foreach ($shortcut in $Electron.Shortcuts) {
        if (Test-Path -LiteralPath $shortcut) { $lines.Add(('shortcut {0} {1}' -f $shortcut, (Get-Item -LiteralPath $shortcut).LastWriteTimeUtc.ToString('o'))) }
        else { $lines.Add("shortcut $shortcut (none)") }
    }
    if (Test-Path -LiteralPath $RealPenMcpConfig) { $lines.Add(('pen-mcp.json {0}' -f (Hash $RealPenMcpConfig))) }
    else { $lines.Add('pen-mcp.json (none)') }
    , $lines
}

function Electron-Pids {
    $prefixes = @($Electron.Folders | ForEach-Object { $_ + '\' })
    @(Get-CimInstance Win32_Process | Where-Object {
            $path = $_.ExecutablePath
            $path -and @($prefixes | Where-Object { $path.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
        } | ForEach-Object { $_.ProcessId })
}

# MCP 서버에 한 줄씩 JSON-RPC를 보내고 tools/list 응답의 도구 이름을 돌려준다. 입력을 닫은 뒤 끝나는지도 본다.
function Probe-PenMcp($exe) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $false
    $process = [System.Diagnostics.Process]::Start($info)
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"verify-install","version":"0"}}}')
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}')
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","id":2,"method":"tools/list"}')
    $process.StandardInput.Flush()
    $tools = @()
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        $read = $process.StandardOutput.ReadLineAsync()
        if (-not $read.Wait([int][Math]::Max(1, ($deadline - (Get-Date)).TotalMilliseconds))) { break }
        if ($read.Result -eq $null) { break }
        $message = $read.Result | ConvertFrom-Json
        if ($message.id -eq 2) { $tools = @($message.result.tools | ForEach-Object { $_.name }); break }
    }
    $process.StandardInput.Close()
    $exited = $process.WaitForExit(10000)
    if (-not $exited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    [pscustomobject]@{ Tools = $tools; Exited = $exited }
}

$installed = $false
try {
    # -- V0 --
    $existing = @()
    if (Test-Path $Names.UninstallKey) { $existing += $Names.UninstallKey }
    foreach ($shortcut in $StartMenuShortcut, $DesktopShortcut) { if (Test-Path -LiteralPath $shortcut) { $existing += $shortcut } }
    if ($existing.Count -gt 0) { throw "Paddock Native is already installed on this PC ($($existing -join ', ')); a test install would overwrite it" }
    foreach ($folder in $GuardProbeFolders) { if (Test-Path -LiteralPath $folder) { throw "$folder already exists; V2 could not tell whether Setup created it" } }
    New-Item -ItemType Directory -Force $Root | Out-Null
    $before = Electron-State
    $beforePids = Electron-Pids
    Record 'V0' $true ("recorded {0} Electron/feedback entries, {1} running pid(s): {2}; work folder {3}" -f $before.Count, $beforePids.Count, ($beforePids -join ','), $Root)

    # -- V1 --
    $code = Run-Setup $Installer "/S /D=$AppDir"
    $installed = Test-Path $Names.UninstallKey
    $files = @(Get-ChildItem -LiteralPath $AppDir -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.Name } | Sort-Object)
    $wantFiles = @($Names.Exes + $Names.Uninstaller | Sort-Object)
    $filesOk = ($files -join '|') -eq ($wantFiles -join '|')
    $hashesOk = $filesOk -and (Hashes-Match $AppDir)
    $uninstaller = Join-Path $AppDir $Names.Uninstaller
    $app = Join-Path $AppDir 'paddock-app.exe'
    $want = [ordered]@{
        DisplayName          = 'Paddock Native'
        DisplayVersion       = $Version
        Publisher            = 'Paddock'
        DisplayIcon          = "`"$app`",0"
        InstallLocation      = $AppDir
        UninstallString      = "`"$uninstaller`""
        QuietUninstallString = "`"$uninstaller`" /S"
        NoModify             = 1
        NoRepair             = 1
    }
    $mismatches = @()
    if ($installed) {
        $key = Get-ItemProperty -LiteralPath $Names.UninstallKey
        foreach ($name in $want.Keys) { if ("$($key.$name)" -ne "$($want[$name])") { $mismatches += "$name='$($key.$name)'" } }
        if (-not ($key.EstimatedSize -gt 0)) { $mismatches += "EstimatedSize='$($key.EstimatedSize)'" }
    } else { $mismatches += 'key missing' }
    $shortcutOk = (Test-Path -LiteralPath $StartMenuShortcut) -and ((Shortcut-Target $StartMenuShortcut) -ieq $app)
    $noDesktop = -not (Test-Path -LiteralPath $DesktopShortcut)
    Record 'V1' (($code -eq 0) -and $filesOk -and $hashesOk -and ($mismatches.Count -eq 0) -and $shortcutOk -and $noDesktop) `
        ("exit {0}; files [{1}]; hashes match={2}; key mismatches [{3}]; start menu shortcut ok={4}; no desktop shortcut={5}" -f $code, ($files -join ', '), $hashesOk, ($mismatches -join ', '), $shortcutOk, $noDesktop)

    # -- V2 --
    $guardDetails = @()
    $guardOk = $true
    foreach ($folder in $GuardProbeFolders) {
        $code = Run-Setup $Installer "/S /D=$folder"
        $created = Test-Path -LiteralPath $folder
        if ($created) { Remove-OwnPath $folder }
        if (($code -ne 4) -or $created) { $guardOk = $false }
        $guardDetails += ('{0}: exit {1}, folder created={2}' -f $folder, $code, $created)
    }
    Record 'V2' $guardOk ($guardDetails -join '; ')

    # -- V3 --
    $probe = Probe-PenMcp (Join-Path $AppDir 'paddock-pen-mcp.exe')
    $toolSet = ($probe.Tools | Sort-Object) -join ','
    Record 'V3' (($toolSet -eq 'pen_close,pen_open,pen_save') -and $probe.Exited) ("tools [{0}]; exited after stdin closed={1}" -f $toolSet, $probe.Exited)

    # -- V4 --
    $runningDaemons = @(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue)
    $smokeAllowed = $Smoke -and (($env:CI -eq 'true') -or $AllowSmokeOnThisPc) -and ($runningDaemons.Count -eq 0)
    $smokeRan = $false
    if (-not $smokeAllowed) {
        $why = if (-not $Smoke) { 'no -Smoke' } elseif ($runningDaemons.Count -gt 0) { "paddock-daemon already running (pid $(($runningDaemons | ForEach-Object { $_.Id }) -join ','))" } else { 'not CI and no -AllowSmokeOnThisPc' }
        Record 'V4' $null $why
        Record 'V5' $null 'V4 did not run'
    } else {
        $smokeRan = $true
        # 앱만 임시 홈 환경으로 띄우고, 설치·제거 프로그램은 원래 환경으로 돌린다.
        $saved = @{}
        foreach ($name in 'USERPROFILE', 'HOME', 'APPDATA', 'LOCALAPPDATA', 'PATH') { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
        . (Join-Path $PSScriptRoot '..\scripts\test-env.ps1')
        Use-TestHome $TestHome
        $appProcess = Start-Process -FilePath $app -PassThru
        foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }

        $daemonInfoPath = Join-Path $TestHome '.paddock\native\daemon.json'
        $deadline = (Get-Date).AddSeconds(30)
        while (-not (Test-Path -LiteralPath $daemonInfoPath) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
        $daemonInfo = if (Test-Path -LiteralPath $daemonInfoPath) { Get-Content -LiteralPath $daemonInfoPath -Raw | ConvertFrom-Json } else { $null }
        $daemonPath = if ($daemonInfo) { Process-Path $daemonInfo.pid } else { $null }
        Start-Sleep -Seconds 5
        $appAlive = -not $appProcess.HasExited
        $penMcpConfig = Join-Path $TestHome '.paddock\config\claude\pen-mcp.json'
        $deadline = (Get-Date).AddSeconds(15)
        while (-not (Test-Path -LiteralPath $penMcpConfig) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
        $penCommand = if (Test-Path -LiteralPath $penMcpConfig) { (Get-Content -LiteralPath $penMcpConfig -Raw | ConvertFrom-Json).mcpServers.'pen-open'.command } else { $null }
        $daemonOk = $daemonPath -ieq (Join-Path $AppDir 'paddock-daemon.exe')
        $penOk = $penCommand -ieq (Join-Path $AppDir 'paddock-pen-mcp.exe')
        $breakaway = if ($daemonInfo) { $daemonInfo.breakaway_denied } else { 'n/a' }
        Record 'V4' ($daemonOk -and $appAlive -and $penOk) ("daemon {0} at {1}; app alive={2}; pen-mcp.json command={3}; breakaway_denied={4}" -f $daemonInfo.pid, $daemonPath, $appAlive, $penCommand, $breakaway)

        # -- V5 --
        $v5 = @()
        $v5Ok = $true
        # ① 앱이 돌고 있으면 무인 재설치는 3으로 끝나고 아무것도 바꾸지 않는다.
        $code = Run-Setup $Installer "/S /D=$AppDir"
        $unchanged = Hashes-Match $AppDir
        if (($code -ne 3) -or -not $unchanged) { $v5Ok = $false }
        $v5 += "app running: exit $code, files unchanged=$unchanged"
        # ② 앱 창만 닫아 데몬을 남긴 채 다시 설치하면 옛 데몬은 replaced\1로 옮겨지고 계속 돈다.
        $appProcess.CloseMainWindow() | Out-Null
        if (-not $appProcess.WaitForExit(15000)) { Stop-Process -Id $appProcess.Id -Force }
        $oldDaemonAlive = $daemonInfo -and (Get-Process -Id $daemonInfo.pid -ErrorAction SilentlyContinue)
        if ($oldDaemonAlive) {
            $code = Run-Setup $Installer "/S /D=$AppDir"
            $movedPath = Join-Path $AppDir 'replaced\1\paddock-daemon.exe'
            $moved = Test-Path -LiteralPath $movedPath
            $stillAlive = [bool](Get-Process -Id $daemonInfo.pid -ErrorAction SilentlyContinue)
            $newDaemonOk = (Hash (Join-Path $AppDir 'paddock-daemon.exe')) -eq (Hash (Join-Path $ExeDir 'paddock-daemon.exe'))
            if (($code -ne 0) -or -not $moved -or -not $stillAlive -or -not $newDaemonOk) { $v5Ok = $false }
            $v5 += "daemon only: exit $code, replaced\1 has the daemon=$moved, old pid $($daemonInfo.pid) alive=$stillAlive, new daemon matches=$newDaemonOk"
            # ③ 데몬이 남아 있으면 제거는 5로 끝나고 아무것도 지우지 않는다.
            $code = Run-Setup (Join-Path $AppDir $Names.Uninstaller) "/S _?=$AppDir"
            $kept = (Hashes-Match $AppDir) -and (Test-Path -LiteralPath $movedPath) -and (Test-Path $Names.UninstallKey)
            if (($code -ne 5) -or -not $kept) { $v5Ok = $false }
            $v5 += "uninstall with the daemon running: exit $code, nothing removed=$kept"
        } else {
            $v5Ok = $false
            $v5 += 'the daemon exited with the app window, so the daemon-only cases could not run'
        }
        Stop-OwnProcesses
        Start-Sleep -Seconds 2
        Record 'V5' $v5Ok ($v5 -join '; ')
    }

    # -- V6 --
    $code = Run-Setup (Join-Path $AppDir $Names.Uninstaller) "/S _?=$AppDir"
    $keyGone = -not (Test-Path $Names.UninstallKey)
    $shortcutsGone = -not (Test-Path -LiteralPath $StartMenuShortcut) -and -not (Test-Path -LiteralPath $DesktopShortcut)
    $exesGone = @($Names.Exes | Where-Object { Test-Path -LiteralPath (Join-Path $AppDir $_) }).Count -eq 0
    $replacedGone = -not (Test-Path -LiteralPath (Join-Path $AppDir 'replaced'))
    $nativeDataKept = if ($smokeRan) { Test-Path -LiteralPath (Join-Path $TestHome '.paddock\native') } else { $true }
    # _?=로 돌린 제거 프로그램은 자기 자신과 폴더를 지우지 못한다 — 이 실행이 만든 경로이므로 여기서 지운다.
    # 제거가 실패해 키가 남았으면 지우지 않는다 — 아래 정리 단계가 그 제거 프로그램으로 다시 지운다.
    if ($keyGone) {
        $installed = $false
        Remove-OwnPath $uninstaller
        Remove-OwnPath $AppDir
    }
    Record 'V6' (($code -eq 0) -and $keyGone -and $shortcutsGone -and $exesGone -and $replacedGone -and $nativeDataKept) `
        ("exit {0}; key gone={1}; shortcuts gone={2}; exes gone={3}; replaced gone={4}; .paddock\native kept={5}" -f $code, $keyGone, $shortcutsGone, $exesGone, $replacedGone, $nativeDataKept)
} catch {
    Record 'ERROR' $false $_.Exception.Message
} finally {
    Stop-OwnProcesses
    # 중간에 멈췄으면 이 실행이 만든 설치를 되돌린다(V0에서 실제 설치가 없음을 확인했다).
    # 제거 프로그램으로 먼저 지우고, 그래도 남은 키·바로 가기는 이 실행의 설치 폴더를 가리킬 때만 직접 지운다.
    if ($installed -and (Test-Path $Names.UninstallKey)) {
        $location = (Get-ItemProperty -LiteralPath $Names.UninstallKey).InstallLocation
        if ($location -ieq $AppDir) {
            $leftover = Join-Path $AppDir $Names.Uninstaller
            if (Test-Path -LiteralPath $leftover) { Run-Setup $leftover "/S _?=$AppDir" | Out-Null }
            if (Test-Path $Names.UninstallKey) { Remove-Item -LiteralPath $Names.UninstallKey -Recurse -Force }
            foreach ($shortcut in $StartMenuShortcut, $DesktopShortcut) {
                if ((Test-Path -LiteralPath $shortcut) -and (Shortcut-Target $shortcut).StartsWith($Root + '\', [StringComparison]::OrdinalIgnoreCase)) {
                    Remove-Item -LiteralPath $shortcut -Force
                }
            }
            Say 'cleanup: removed the test install left by a failed step'
        }
    }
}

# -- V7 --
if ($before) {
    $after = Electron-State
    $changed = @(Compare-Object -ReferenceObject $before -DifferenceObject $after | ForEach-Object { '{0} {1}' -f $_.SideIndicator, $_.InputObject })
    $gonePids = @($beforePids | Where-Object { -not (Get-Process -Id $_ -ErrorAction SilentlyContinue) })
    # 사용자가 그사이 앱을 닫았을 수 있다 — 사라진 PID는 실패가 아니라 보고로 남긴다.
    Record 'V7' ($changed.Count -eq 0) ("changed entries [{0}]; Electron/feedback pids no longer running [{1}]" -f ($changed -join '; '), ($gonePids -join ','))
}

# -- V8 --
if (Test-Path -LiteralPath $Root) {
    Start-Sleep -Seconds 1
    Remove-OwnPath $Root
}
Record 'V8' (-not (Test-Path -LiteralPath $Root)) "work folder removed=$(-not (Test-Path -LiteralPath $Root))"

Say ("RESULT {0} ({1} failure(s))" -f $(if ($n_failures -eq 0) { 'PASS' } else { 'FAIL' }), $n_failures)
if ($n_failures -gt 0) { exit 1 }
