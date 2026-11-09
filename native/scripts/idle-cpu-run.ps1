# 사용: idle-cpu-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -Out <결과 폴더> [-IdleSec 60] [-WarmupSec 20] [-EditTabs 0]
# 놀 때 앱 CPU를 점검 기능 없는 제품 빌드로 잰다(야간 묶음 전용 — 앱 창이 사용자 화면에 뜨고 키 입력을 쓴다).
#   준비: 자동 점검 기능 없이 만든 paddock-app.exe여야 한다(-SoundExe로 넘기는 빌드). 점검 빌드는 화면 상태 글을 계속 조립해 놀 때 CPU가 제품보다 높게 나온다.
#   과정: 기본 셸(WSL) 터미널에 실제 키로 임시 Git 저장소(/tmp/pdk-idle-<id>)를 만들고, 이름이 claude인 가짜 에이전트(`read -t`로 출력 없이 대기)를 돌려
#         그 폴더가 파일 목록·Source control 감시 상태가 되게 한다. 시작 출력이 가라앉도록 -WarmupSec 뒤 -IdleSec 동안 변경도 출력도 입력도 없이 둔다.
#   편집 장면(`-EditTabs N`, 덩어리 28): 준비 뒤 실제 키 Ctrl+Shift+P(빠른 열기)로 파일 N개를 열어 한 글자씩 고친 채(저장하지 않음) 편집기에 포커스를 둔 상태로 같은 구간을 잰다 — 캐럿 깜빡임이 놀 때 CPU를 올리지 않는지 본다.
#   판정: 구간의 앱 평균 CPU(코어 하나 대비 %) <= 2, 데몬 평균 CPU <= 1, 앱이 띄운 `wsl.exe` 호출(app.log의 `git: wsl.exe` 줄) 0이면 PASS. 하나라도 넘으면 FAIL.
#         넘으면 놀 때 다시 그리는 경로를 찾는 일을 새로 연다(D60).
# 안전 규칙: input-run.ps1과 같다 — 이 스크립트가 띄운 앱 창이 전경일 때만 키를 보내고, 실제 사용자 폴더를 거부하며, 다른 paddock-daemon이 있으면 시작하지 않고,
# 끝나면 임시 저장소를 지우고 이 스크립트가 띄운 프로세스만 종료한다.
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$Out,
    [int]$IdleSec = 60,
    [int]$WarmupSec = 20,
    [int]$EditTabs = 0
)
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class S {
  [StructLayout(LayoutKind.Explicit, Size = 40)] public struct INP {
    [FieldOffset(0)] public uint type; [FieldOffset(8)] public ushort vk; [FieldOffset(10)] public ushort sc;
    [FieldOffset(12)] public uint fl; [FieldOffset(16)] public uint time; [FieldOffset(24)] public IntPtr ex; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INP[] i, int sz);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  public static IntPtr T; public static int Fail = 0;
  // 전경이 시험 창이 아니면 키를 보내지 않고 실패로 센다.
  public static void Key(ushort vk, ushort scan, uint flags) {
    if (GetForegroundWindow() != T) { Fail++; return; }
    INP[] a = new INP[1]; a[0].type = 1; a[0].vk = vk; a[0].sc = scan; a[0].fl = flags; SendInput(1, a, 40); }
  public static void Tap(ushort vk) { Key(vk, 0, 0); System.Threading.Thread.Sleep(20); Key(vk, 0, 2); }
  // 글자를 유니코드 입력으로 보낸다(키보드 배열에 상관없이 같은 글자가 들어간다).
  public static void Text(string s) {
    foreach (char c in s) { Key(0, c, 4); System.Threading.Thread.Sleep(8); Key(0, c, 6); System.Threading.Thread.Sleep(8); } }
  public static bool Focus(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    uint p; uint ft = GetWindowThreadProcessId(GetForegroundWindow(), out p); uint me = GetCurrentThreadId();
    ShowWindow(h, 9); AttachThreadInput(me, ft, true); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(me, ft, false);
    System.Threading.Thread.Sleep(200); return GetForegroundWindow() == h; }
}
'@
New-Item -ItemType Directory -Force $Out | Out-Null
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required (omitting it would run with the real user profile)" }
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$native = Join-Path $HomeDir '.paddock\native'
New-Item -ItemType Directory -Force $native | Out-Null
$resultPath = Join-Path $Out 'result-IDLE-CPU.txt'
function Say($m) { $m | Out-File $resultPath -Append -Encoding utf8 }
$id = [Guid]::NewGuid().ToString('N').Substring(0, 8)
$wslDir = "/tmp/pdk-idle-$id"
$startedPids = @()
$proc = Start-Process -FilePath $Exe -ArgumentList "--out `"$Out`"" -PassThru
$startedPids += $proc.Id
try {
    for ($i = 0; $i -lt 100 -and $proc.MainWindowHandle -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $proc.Refresh() }
    $h = $proc.MainWindowHandle
    $windowPid = [uint32]0; [void][S]::GetWindowThreadProcessId($h, [ref]$windowPid)
    if ($h -eq [IntPtr]::Zero -or $windowPid -ne $proc.Id -or $proc.MainWindowTitle -ne 'Paddock') { Say "IDLE-CPU NO WINDOW (handle=$h pid=$windowPid)"; return }
    [S]::T = $h
    Start-Sleep -Seconds 5
    $focused = $false
    for ($i = 0; $i -lt 5 -and -not $focused; $i++) { $focused = [S]::Focus($h); if (-not $focused) { Start-Sleep -Milliseconds 300 } }
    if (-not $focused) { Say "IDLE-CPU SKIP (could not bring the test window to the foreground)"; return }
    # 임시 Git 저장소와 가짜 claude(bash 심볼릭 링크)를 만들고, 출력 없이 대기하는 claude를 돌려 폴더 묶음·파일 목록·Source control이 켜지게 한다.
    $setup = "unset HISTFILE; rm -rf $wslDir && mkdir -p $wslDir/proj && cd $wslDir/proj && git init -q -b main && git config user.name Tester && git config user.email t@example.com && echo one > a.js && for i in 1 2 3 4 5 6 7 8; do echo one > f$i.txt; done && git add -A && git commit -q -m base && echo two >> a.js && ln -sf /bin/bash $wslDir/claude && export PATH=${wslDir}:`$PATH"
    [S]::Text($setup); [S]::Tap(0x0D)
    Start-Sleep -Seconds 4
    [S]::Text("claude -c 'read -t 900'"); [S]::Tap(0x0D)
    Start-Sleep -Seconds ($WarmupSec)
    # 편집 장면: 빠른 열기로 파일을 열어 고친다(마지막 편집기에 포커스가 남는다).
    for ($n = 1; $n -le $EditTabs; $n++) {
        [S]::Key(0xA2, 0, 0); [S]::Key(0xA0, 0, 0); [S]::Tap(0x50); [S]::Key(0xA0, 0, 2); [S]::Key(0xA2, 0, 2)
        Start-Sleep -Milliseconds 800
        [S]::Text("f$n.txt"); Start-Sleep -Milliseconds 500; [S]::Tap(0x0D)
        Start-Sleep -Milliseconds 1200
        [S]::Text("x"); Start-Sleep -Milliseconds 400
    }
    $info = Join-Path $native 'daemon.json'
    $daemonPid = $null
    if (Test-Path $info) { try { $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid } catch {} }
    $daemon = if ($daemonPid) { Get-Process -Id $daemonPid -ErrorAction SilentlyContinue } else { $null }
    if ($proc.HasExited -or -not $daemon) { Say "IDLE-CPU FAIL (app or daemon is not running after warm-up)"; return }
    $appLog = Join-Path $native 'logs\app.log'
    $callsBefore = if (Test-Path $appLog) { @(Select-String -Path $appLog -Pattern 'git: wsl\.exe').Count } else { 0 }
    $proc.Refresh(); $daemon.Refresh()
    $appBefore = $proc.TotalProcessorTime.TotalSeconds; $daemonBefore = $daemon.TotalProcessorTime.TotalSeconds
    $startedAt = Get-Date
    Start-Sleep -Seconds $IdleSec
    $proc.Refresh(); $daemon.Refresh()
    $seconds = ((Get-Date) - $startedAt).TotalSeconds
    $appPercent = 100.0 * ($proc.TotalProcessorTime.TotalSeconds - $appBefore) / $seconds
    $daemonPercent = 100.0 * ($daemon.TotalProcessorTime.TotalSeconds - $daemonBefore) / $seconds
    $callsAfter = if (Test-Path $appLog) { @(Select-String -Path $appLog -Pattern 'git: wsl\.exe').Count } else { 0 }
    $calls = $callsAfter - $callsBefore
    $pass = ($appPercent -le 2.0) -and ($daemonPercent -le 1.0) -and ($calls -eq 0)
    Say ("IDLE-CPU editTabs=$EditTabs {0} window={1:N1}s app={2:N3}% (<= 2) daemon={3:N3}% (<= 1) wsl_calls={4} (== 0) inputFailures={5}" -f $(if ($pass) { 'PASS' } else { 'FAIL' }), $seconds, $appPercent, $daemonPercent, $calls, [S]::Fail)
} finally {
    # 임시 저장소는 이 시험이 만든 폴더 하나만 지운다.
    & wsl.exe -e rm -rf $wslDir 2>$null
    $info = Join-Path $native 'daemon.json'
    if (Test-Path $info) { try { $d = (Get-Content $info -Raw | ConvertFrom-Json).pid; if ($d) { $startedPids += $d } } catch {} }
    foreach ($p in $startedPids) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
    Say "cleanup: stopped $($startedPids -join ',')"
}
