# 사용: input-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -Out <결과 폴더> -Tests I1,I3,I5
# 실제 SendInput 키·마우스로 "대본이 합성 키로는 확인하지 못하는" 입력 경로를 시험한다(야간 묶음 전용 — 낮에 사용자 화면에서 돌리지 않는다).
#   I1: Windows가 Shift+숫자·기호 키를 바뀐 글자(`~`·`%`)로 넘기는 경로. Ctrl+Shift+`(아래 분할)·Ctrl+Shift+5(옆 분할)가 칸을 나눠야 한다.
#       아직 확인하지 않은 배열: AZERTY처럼 Shift+5가 `%`가 아닌 글자를 내는 배열은 Ctrl+Shift+5 분할이 안 잡힐 수 있다(미국 배열 기준으로만 시험한다).
#   I3: 칸 경계를 마우스로 누르면 이웃 칸이 선택·포커스되지 않아야 한다(경계에서 이벤트가 멈춘다). 누른 뒤에도 선택은 그대로다.
#   I5: 러시아어 배열(00000419)에서 Ctrl+B가 `и`로 와도 사이드바 접기가 동작해야 한다. 끝나면 배열을 되돌린다.
#   Q1: 빠른 설정 패널을 실제 마우스로 조작한다. 톱니 → 테마 선택상자 → Mermaid Light → 다시 선택상자 → Mermaid Dark → 소리 체크 → 배율 +.
#       단추 자리는 기본 창 크기(1552폭)에서 잰 좌표를 창 폭 비율로 늘려 쓴다(창이 다른 크기로 떴으면 CHECK로 남는다).
# 판정은 앱이 대본 실행 중 남기는 state-live.txt(선택·배치·사이드바)로 한다.
# 안전 규칙: run/ime-run.ps1과 같다 — 이 스크립트가 띄운 앱 창이 전경일 때만 입력을 보내고, 실제 사용자 폴더를 거부하며,
# 다른 paddock-daemon이 있으면 시작하지 않고, 끝나면 이 스크립트가 띄운 프로세스만 종료하고 키보드 배열을 되돌린다.
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$Out,
    [string[]]$Tests = @('I1')
)
Add-Type -ReferencedAssemblies System.Drawing,System.Windows.Forms -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Explicit, Size = 40)] public struct INP {
    [FieldOffset(0)] public uint type; [FieldOffset(8)] public ushort vk; [FieldOffset(10)] public ushort sc;
    [FieldOffset(12)] public uint fl; [FieldOffset(16)] public uint time; [FieldOffset(24)] public IntPtr ex; }
  [StructLayout(LayoutKind.Explicit, Size = 40)] public struct MINP {
    [FieldOffset(0)] public uint type; [FieldOffset(8)] public int dx; [FieldOffset(12)] public int dy;
    [FieldOffset(16)] public uint data; [FieldOffset(20)] public uint fl; [FieldOffset(24)] public uint time; [FieldOffset(32)] public IntPtr ex; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INP[] i, int sz);
  [DllImport("user32.dll", EntryPoint = "SendInput")] public static extern uint SendMouse(uint n, MINP[] i, int sz);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr ActivateKeyboardLayout(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint t);
  public static IntPtr T; public static int Fail = 0;
  // 전경이 시험 창이 아니면 입력을 보내지 않고 실패로 센다.
  public static void Key(ushort vk, bool up) {
    if (GetForegroundWindow() != T) { Fail++; return; }
    INP[] a = new INP[1]; a[0].type = 1; a[0].vk = vk; a[0].fl = up ? 2u : 0u; SendInput(1, a, 40); }
  public static void Tap(ushort vk) { Key(vk, false); System.Threading.Thread.Sleep(20); Key(vk, true); }
  public static void Chord(ushort[] mods, ushort vk) {
    foreach (ushort m in mods) { Key(m, false); System.Threading.Thread.Sleep(15); }
    Tap(vk); System.Threading.Thread.Sleep(15);
    for (int i = mods.Length - 1; i >= 0; i--) { Key(mods[i], true); System.Threading.Thread.Sleep(15); } }
  public static void Click(int x, int y) {
    if (GetForegroundWindow() != T) { Fail++; return; }
    SetCursorPos(x, y); System.Threading.Thread.Sleep(60);
    MINP[] a = new MINP[1]; a[0].type = 0; a[0].fl = 0x2; SendMouse(1, a, 40);
    System.Threading.Thread.Sleep(40); a[0].fl = 0x4; SendMouse(1, a, 40); }
  public static bool Focus(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    uint p; uint ft = GetWindowThreadProcessId(GetForegroundWindow(), out p); uint me = GetCurrentThreadId();
    ShowWindow(h, 9); AttachThreadInput(me, ft, true); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(me, ft, false);
    System.Threading.Thread.Sleep(200); return GetForegroundWindow() == h; }
}
'@
[void][W]::SetProcessDPIAware()
New-Item -ItemType Directory -Force $Out | Out-Null
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required (omitting it would run with the real user profile)" }
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
New-Item -ItemType Directory -Force $HomeDir | Out-Null
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$script = Join-Path $Out 'input.script'
[IO.File]::WriteAllText($script, "waittabs 1 @30000`nwait 900000`n")
$resultPath = Join-Path $Out "result-$($Tests -join '-').txt"
function Say($m) { $m | Out-File $resultPath -Append -Encoding utf8 }
$startedPids = @()
$proc = Start-Process -FilePath $Exe -ArgumentList "--script `"$script`" --out `"$Out`"" -PassThru
$startedPids += $proc.Id
$originalLayout = [IntPtr]::Zero
function State { Get-Content (Join-Path $Out 'state-live.txt') -Raw -ErrorAction SilentlyContinue }
function Ensure { for ($i = 0; $i -lt 5; $i++) { if ([W]::Focus($h)) { return $true }; Start-Sleep -Milliseconds 300 }; return $false }
try {
    for ($i = 0; $i -lt 100 -and $proc.MainWindowHandle -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $proc.Refresh() }
    $h = $proc.MainWindowHandle
    $wp = [uint32]0; [void][W]::GetWindowThreadProcessId($h, [ref]$wp)
    if ($h -eq [IntPtr]::Zero -or $wp -ne $proc.Id -or $proc.MainWindowTitle -ne 'Paddock') { Say "NO WINDOW (handle=$h pid=$wp)"; return }
    [W]::T = $h
    for ($i = 0; $i -lt 100 -and -not ((State) -match 'tab 1 '); $i++) { Start-Sleep -Milliseconds 300 }
    Start-Sleep -Milliseconds 1500
    $VK_CONTROL = 0xA2; $VK_SHIFT = 0xA0; $VK_OEM_3 = 0xC0; $VK_5 = 0x35; $VK_B = 0x42
    foreach ($test in $Tests) {
        if (-not (Ensure)) { Say "$test SKIP (could not bring the test window to the foreground)"; continue }
        [W]::Fail = 0
        switch ($test) {
            'I1' {
                $before = (State) -split "`n" | Where-Object { $_ -like 'layout:*' }
                [W]::Chord(@($VK_CONTROL, $VK_SHIFT), $VK_5); Start-Sleep -Milliseconds 1200
                $right = (State) -split "`n" | Where-Object { $_ -like 'layout:*' }
                [W]::Chord(@($VK_CONTROL, $VK_SHIFT), $VK_OEM_3); Start-Sleep -Milliseconds 1200
                $down = (State) -split "`n" | Where-Object { $_ -like 'layout:*' }
                $okRight = ($right -ne $before) -and ($right -match 'horizontal|"type":"split"')
                $okDown = ($down -ne $right)
                Say ("I1 " + $(if ($okRight -and $okDown -and [W]::Fail -eq 0) { 'PASS' } else { 'FAIL' }) + " before=[$before] afterCtrlShift5=[$right] afterCtrlShiftBackquote=[$down] inputFailures=$([W]::Fail)")
            }
            'I3' {
                # 옆 칸 둘을 만든 뒤 선택을 첫 칸으로 돌려 두고, 두 칸 사이 경계(본문 폭의 반쯤)를 눌러 선택이 그대로인지 본다.
                [W]::Chord(@($VK_CONTROL, $VK_SHIFT), $VK_5); Start-Sleep -Milliseconds 1200
                [W]::Chord(@($VK_CONTROL), 0x25); Start-Sleep -Milliseconds 500
                $selectedBefore = (State) -split "`n" | Where-Object { $_ -like 'selected:*' }
                $rect = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$rect)
                $bodyLine = (State) -split "`n" | Where-Object { $_ -like 'body:*' }
                $x = $rect.L + [int](($rect.R - $rect.L) * 0.62); $y = $rect.T + [int](($rect.B - $rect.T) * 0.5)
                [W]::Click($x, $y); Start-Sleep -Milliseconds 800
                $selectedAfter = (State) -split "`n" | Where-Object { $_ -like 'selected:*' }
                Say ("I3 " + $(if ($selectedBefore -eq $selectedAfter -and [W]::Fail -eq 0) { 'PASS' } else { 'CHECK' }) + " selectedBefore=[$selectedBefore] selectedAfter=[$selectedAfter] $bodyLine click=($x,$y) (클릭 자리가 경계가 아니면 CHECK — 경계 좌표는 body와 layout 비율로 다시 잡는다)")
            }
            'Q1' {
                $rect = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$rect)
                $scale = ($rect.R - $rect.L) / 1552.0
                $look = { (State) -split "`n" | Where-Object { $_ -like 'look:*' } }
                $at = { param($x, $y) [W]::Click($rect.L + [int]($x * $scale), $rect.T + [int]($y * $scale)); Start-Sleep -Milliseconds 700 }
                $steps = @()
                & $at 256 52;  $steps += @{ name = 'gear opens the panel'; ok = ((& $look) -match 'quick=open') }
                & $at 489 148; $steps += @{ name = 'theme box expands'; ok = ((& $look) -notmatch 'expanded=-') }
                & $at 332 215; $steps += @{ name = 'Mermaid Light applied'; ok = ((& $look) -match 'theme=Mermaid Light') }
                & $at 489 148; & $at 332 185; $steps += @{ name = 'Mermaid Dark applied'; ok = ((& $look) -match 'theme=Mermaid Dark') }
                & $at 555 405
                $settingsFile = Join-Path $HomeDir '.paddock\native\settings.json'
                $steps += @{ name = 'sound toggle written'; ok = ((Get-Content $settingsFile -Raw -ErrorAction SilentlyContinue) -match '"paddock.agentDoneSound":\s*false') }
                & $at 555 190; $steps += @{ name = 'zoom + applied'; ok = ((& $look) -match 'zoom=1\.2') }
                $failed = @($steps | Where-Object { -not $_.ok } | ForEach-Object { $_.name })
                Say ("Q1 " + $(if ($failed.Count -eq 0 -and [W]::Fail -eq 0) { 'PASS' } else { 'CHECK' }) + " failedSteps=[$($failed -join '; ')] windowWidth=$($rect.R - $rect.L) scale=$scale inputFailures=$([W]::Fail) look=[$(& $look)]")
            }
            'I5' {
                $originalLayout = [W]::GetKeyboardLayout(0)
                [void][W]::ActivateKeyboardLayout([W]::LoadKeyboardLayout('00000419', 1), 0)
                Start-Sleep -Milliseconds 500
                if (-not (Ensure)) { Say "I5 SKIP (focus lost after switching layout)"; continue }
                $before = (State) -split "`n" | Where-Object { $_ -like 'sidebar:*' }
                [W]::Chord(@($VK_CONTROL), $VK_B); Start-Sleep -Milliseconds 1000
                $after = (State) -split "`n" | Where-Object { $_ -like 'sidebar:*' }
                [void][W]::ActivateKeyboardLayout($originalLayout, 0)
                Say ("I5 " + $(if ($before -ne $after -and [W]::Fail -eq 0) { 'PASS' } else { 'FAIL' }) + " before=[$before] after=[$after] inputFailures=$([W]::Fail)")
            }
        }
    }
} finally {
    if ($originalLayout -ne [IntPtr]::Zero) { [void][W]::ActivateKeyboardLayout($originalLayout, 0) }
    $info = Join-Path $HomeDir '.paddock\native\daemon.json'
    if (Test-Path $info) { try { $d = (Get-Content $info -Raw | ConvertFrom-Json).pid; if ($d) { $startedPids += $d } } catch {} }
    foreach ($id in $startedPids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    Say "cleanup: stopped $($startedPids -join ',')"
}
