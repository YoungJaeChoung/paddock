# 사용: ime-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -Out <결과 폴더> -Tests T1,T3 [-N 30] [-Mode run|capture]
# 터미널의 한글 IME 시험: 실제 SendInput 키로 Microsoft 한국어 IME에 조합을 넣고, 셸이 받은 바이트로 판정한다.
# 안전 규칙
#   - 키·마우스는 이 스크립트가 띄운 앱 창(PID·창 핸들로 확인)이 전경일 때만 보낸다. 전경이 아니면 보내지 않고 다시 시도한다.
#   - HomeDir이 실제 사용자 폴더이면 거부하고, paddock-daemon 프로세스가 이미 떠 있으면 시작하지 않는다.
#   - -HomeDir로 USERPROFILE을 바꿔 실행해 사용자의 실제 ~/.paddock/native를 건드리지 않는다.
#   - 끝나면 한/영 상태를 되돌리고, 이 스크립트가 띄운 프로세스(앱·데몬·보조 창)만 종료한다.
# 판정: 셸에서 PTY 바이트를 그대로 파일에 남기는 기록기(python3, raw 모드)를 돌리고, 시험 한 번마다 늘어난 바이트를 기대값과 비교한다.
# T8(이름 바꾸기 창에서 조합 중 Enter)은 시험마다 대본이 우클릭 메뉴의 Rename terminal로 창을 다시 열어 두고, 창의 입력칸에
# 한글 조합 뒤 Enter를 보내면 마지막 글자와 Enter가 터미널(기록기)로 새지 않고 이름만 `한글`로 바뀌어야 한다.
# T8b(터미널 조합 중 Rename 열기)는 터미널에서 한글을 조합하는 도중 대본이 Rename 창을 열면, 조합 글자가 터미널(기록기)에
# `한글`로 확정돼 들어가고 입력칸의 이름은 원래 이름(`terminal 1`)으로 시작해 Enter로 닫아도 이름이 바뀌지 않아야 한다.
# T9(Accounts 창의 이름 입력칸에서 조합 중 Enter)는 T8과 같은 꼴이다: 시험마다 대본이 Accounts 창의 추가 양식을 열어 두고, 입력칸에
# 한글 조합 뒤 Enter를 보내면 계정 이름이 `한글`로 추가돼(`Claude · 한글` 터미널이 열림) 마지막 글자와 Enter가 터미널(기록기)로 새지 않아야 한다.
# 시험 사이에 대본이 추가된 계정을 지운다(같은 이름은 두 번 만들 수 없다).
# T5b(탭 전환 중 조합 확정)는 터미널 2개(각각 기록기 하나)로 시험한다: 탭 1에서 조합 중 Ctrl+→로 탭 2로 옮기면
# 조합 글자가 탭 1에만 확정으로 들어가고, 이어 누른 Enter는 탭 2에만 들어가야 한다.
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$Out,
    [string[]]$Tests = @('T1'),
    [int]$N = 30,
    [string]$Mode = 'run',
    [switch]$DebugConv
)
Add-Type -ReferencedAssemblies System.Drawing,System.Windows.Forms -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Explicit, Size = 40)] public struct INP {
    [FieldOffset(0)] public uint type; [FieldOffset(8)] public ushort vk; [FieldOffset(10)] public ushort sc;
    [FieldOffset(12)] public uint fl; [FieldOffset(16)] public uint time; [FieldOffset(24)] public IntPtr ex; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INP[] i, int sz);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint t);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("imm32.dll")] public static extern IntPtr ImmGetDefaultIMEWnd(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  public static IntPtr T; public static int Fail = 0; public static string Bad = "";
  // 전경이 시험 창이 아니면 키를 보내지 않고 실패로 센다.
  public static void Key(ushort vk, bool up) {
    if (GetForegroundWindow() != T) { Fail++; uint p; GetWindowThreadProcessId(GetForegroundWindow(), out p); Bad = "fgpid=" + p; return; }
    INP[] a = new INP[1]; a[0].type = 1; a[0].vk = vk; a[0].fl = up ? 2u : 0u; SendInput(1, a, 40); }
  public static void Tap(ushort vk) { Key(vk, false); System.Threading.Thread.Sleep(15); Key(vk, true); }
  public static void CtrlTap(ushort vk) { Key(0xA2, false); System.Threading.Thread.Sleep(15); Tap(vk); System.Threading.Thread.Sleep(15); Key(0xA2, true); }
  // 시험 창을 전경으로 올린다. 다른 창에 키를 보내지 않는 방법만 쓴다.
  public static bool Focus(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    uint p; uint ft = GetWindowThreadProcessId(GetForegroundWindow(), out p); uint me = GetCurrentThreadId();
    ShowWindow(h, 9); AttachThreadInput(me, ft, true); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(me, ft, false);
    System.Threading.Thread.Sleep(200); return GetForegroundWindow() == h; }
  public static long ConvMode(IntPtr h) {
    IntPtr ime = ImmGetDefaultIMEWnd(h); return SendMessage(ime, 0x283, (IntPtr)1, IntPtr.Zero).ToInt64(); }
  public static string Layout(IntPtr h) { uint p; uint t = GetWindowThreadProcessId(h, out p); return GetKeyboardLayout(t).ToInt64().ToString("X"); }
}
'@
[void][W]::SetProcessDPIAware()
New-Item -ItemType Directory -Force $Out | Out-Null
$imeDir = Join-Path $Out 'ime'; New-Item -ItemType Directory -Force $imeDir | Out-Null
$logPath = Join-Path $imeDir 'log.bin'; Remove-Item $logPath -ErrorAction SilentlyContinue
function ToWsl($p) { '/mnt/' + $p.Substring(0, 1).ToLower() + $p.Substring(2).Replace('\', '/') }
$loggerPy = Join-Path $imeDir 'pty-log.py'
[IO.File]::WriteAllText($loggerPy, "import os, sys, tty`nout = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_APPEND)`ntty.setraw(0)`nwhile True:`n    data = os.read(0, 4096)`n    if not data:`n        break`n    os.write(out, data)`n")
$logPathB = Join-Path $imeDir 'log-b.bin'; Remove-Item $logPathB -ErrorAction SilentlyContinue
$twoTabs = $Tests -contains 'T5b'
$script = Join-Path $imeDir 'ime.script'
$scriptText = "wait 2500`nsend python3 $(ToWsl $loggerPy) $(ToWsl $logPath)\r`n"
if ($twoTabs) {
    # 탭 2를 열어 두 번째 기록기를 돌리고, 시험을 시작하는 탭 1로 돌아온다.
    $scriptText += "wait 900`nkey ctrl-shift-t`nwaittabs 2 @10000`nwait 2500`nsend python3 $(ToWsl $loggerPy) $(ToWsl $logPathB)\r`nwait 900`nkey ctrl-left`n"
}
if ($Tests -contains 'T8b') {
    # 시험이 조합을 시작할 시간을 준 뒤 Rename 창을 열고, 시험이 Enter로 닫을 때까지(none) 기다린다.
    for ($n = 0; $n -lt $N; $n++) { $scriptText += "wait 2500`nact menu terminal 1`nact menuitem Rename terminal`nwaitoverlay rename @600000`nwaitoverlay none @600000`n" }
}
if ($Tests -contains 'T8') {
    # 시험 한 번마다 이름 바꾸기 창을 열고(rename), 시험이 Enter로 닫을 때까지(none) 기다린다.
    for ($n = 0; $n -lt $N; $n++) { $scriptText += "act menu terminal 1`nact menuitem Rename terminal`nwaitoverlay rename @600000`nwaitoverlay none @600000`n" }
}
if ($Tests -contains 'T9') {
    # 시험 한 번마다 Accounts 창의 추가 양식을 열고(form), 시험이 Enter로 추가해 창이 닫힐 때까지(none) 기다린 뒤 추가된 계정을 지운다.
    # 계정 터미널이 시험마다 하나씩 늘어나므로 횟수를 10번으로 제한한다.
    for ($n = 0; $n -lt [Math]::Min($N, 10); $n++) { $scriptText += "act accounts`nwaitoverlay accounts @600000`nact acct showform`nwaitoverlay none @600000`nwait 1500`nact accounts`nwaitoverlay accounts @600000`nact acct remove 1`nwait 500`nact acct close`nwaitoverlay none @600000`n" }
}
[IO.File]::WriteAllText($script, $scriptText + "wait 900000`n")
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required (omitting it would run with the real user profile)" }
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
$running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
New-Item -ItemType Directory -Force $HomeDir | Out-Null
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir

$startedPids = @()
$proc = Start-Process -FilePath $Exe -ArgumentList "--script `"$script`" --out `"$Out`"" -PassThru
$startedPids += $proc.Id
$helper = $null
$resultPath = Join-Path $Out "result-$($Tests -join '-').txt"
function Say($m) { $m | Out-File $resultPath -Append -Encoding utf8 }
function Cleanup {
    $info = Join-Path $HomeDir '.paddock\native\daemon.json'
    if (Test-Path $info) { try { $d = (Get-Content $info -Raw | ConvertFrom-Json).pid; if ($d) { $script:startedPids += $d } } catch {} }
    if ($script:helper) { $script:startedPids += $script:helper.Id }
    foreach ($id in $script:startedPids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    Say "cleanup: stopped $($script:startedPids -join ',')"
}
try {
    # 창이 뜰 때까지 기다린다. 창 핸들은 이 프로세스(PID)의 것만 쓴다.
    for ($i = 0; $i -lt 100 -and $proc.MainWindowHandle -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $proc.Refresh() }
    $h = $proc.MainWindowHandle
    $wp = [uint32]0; [void][W]::GetWindowThreadProcessId($h, [ref]$wp)
    if ($h -eq [IntPtr]::Zero -or $wp -ne $proc.Id -or $proc.MainWindowTitle -ne 'Paddock') { Say "NO WINDOW (handle=$h pid=$wp title=$($proc.MainWindowTitle))"; return }
    [W]::T = $h
    Say "app pid=$($proc.Id) hwnd=$h layout=$([W]::Layout($h))"
    for ($i = 0; $i -lt 100 -and -not (Test-Path $logPath); $i++) { Start-Sleep -Milliseconds 200 }
    if (-not (Test-Path $logPath)) { Say "LOGGER NOT STARTED"; return }
    if ($twoTabs) {
        for ($i = 0; $i -lt 100 -and -not (Test-Path $logPathB); $i++) { Start-Sleep -Milliseconds 200 }
        if (-not (Test-Path $logPathB)) { Say "LOGGER B NOT STARTED"; return }
        Start-Sleep -Milliseconds 1500
    }
    Start-Sleep -Milliseconds 500
    $startConv = [W]::ConvMode($h)

    function Ensure { for ($i = 0; $i -lt 5; $i++) { if ([W]::Focus($h)) { return $true }; Start-Sleep -Milliseconds 300 }; return $false }
    function Typ($keys) { foreach ($k in $keys) { [W]::Tap($k); Start-Sleep -Milliseconds (Get-Random -Min 30 -Max 50) } }
    function SetHangul($on) {
        $isOn = ([W]::ConvMode($h) -band 1) -eq 1
        if ($DebugConv) { Say "  setHangul want=$on conv=$([W]::ConvMode($h))" }
        if ($isOn -ne $on) { [W]::Tap(0x15); Start-Sleep -Milliseconds 250 }
        return ([W]::ConvMode($h) -band 1) -eq 1
    }
    function LogSize($path = $logPath) { (Get-Item $path).Length }
    function Delta($before, $path = $logPath) {
        $stream = New-Object IO.FileStream($path, 'Open', 'Read', 'ReadWrite')
        $bytes = New-Object byte[] $stream.Length
        [void]$stream.Read($bytes, 0, $bytes.Length)
        $stream.Dispose()
        if ($bytes.Length -le $before) { return '' }
        return [Text.Encoding]::UTF8.GetString($bytes, $before, $bytes.Length - $before)
    }
    function Show($s) { $s.Replace([string][char]0x1b, '<ESC>').Replace("`r", '<CR>').Replace([string][char]0x7f, '<DEL>').Replace("`n", '<LF>') }
    $HG = @(0x47, 0x4B, 0x53, 0x52, 0x4D, 0x46)   # ㅎ ㅏ ㄴ ㄱ ㅡ ㄹ
    $ENTER = 0x0D; $ESC = 0x1B; $BACK = 0x08; $SPACE = 0x20

    function StartHelper {
        $p = Start-Process powershell -ArgumentList '-NoProfile', '-Command', 'Add-Type -AssemblyName System.Windows.Forms; $f = New-Object Windows.Forms.Form; $f.Text = "ime-helper"; $f.Width = 300; $f.Height = 150; [Windows.Forms.Application]::Run($f)' -PassThru
        for ($i = 0; $i -lt 100 -and ($p.Refresh() -or $p.MainWindowHandle -eq [IntPtr]::Zero); $i++) { Start-Sleep -Milliseconds 200 }
        return $p
    }

    if ($Mode -eq 'capture') {
        if (-not (Ensure)) { Say 'FOCUS FAIL'; return }
        [void](SetHangul $true)
        Add-Type -AssemblyName System.Drawing
        $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r)
        function Shot($name) {
            $w = $r.R - $r.L; $ht = $r.B - $r.T
            $bmp = New-Object System.Drawing.Bitmap $w, $ht
            $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc(); [void][W]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
            $bmp.Save((Join-Path $Out "$name-window.png")); $bmp.Dispose()
            # 후보창처럼 별도 창은 PrintWindow에 안 담기므로 화면의 같은 영역도 찍는다.
            $bmp = New-Object System.Drawing.Bitmap ($w + 200), ($ht + 200)
            $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L - 100, $r.T - 100, 0, 0, $bmp.Size); $g.Dispose()
            $bmp.Save((Join-Path $Out "$name-screen.png")); $bmp.Dispose()
        }
        Typ @(0x47, 0x4B, 0x53); Start-Sleep -Milliseconds 400; Shot 'compose-han'
        [W]::Tap(0x19); Start-Sleep -Milliseconds 600; Shot 'compose-hanja'
        [W]::Tap($ESC); Start-Sleep -Milliseconds 300; [W]::Tap($ESC); Start-Sleep -Milliseconds 300; [W]::Tap($ENTER)
        Say "captured; log=$(Show (Delta 0))"
        return
    }

    foreach ($test in $Tests) {
        $pass = 0; $fail = 0; $skip = 0; $escSeen = 0
        $name = $test
        for ($i = 0; $i -lt $N; $i++) {
            $done = $false
            for ($try = 0; $try -lt 3 -and -not $done; $try++) {
                if (-not (Ensure)) { continue }
                [void](SetHangul $true)
                $f0 = [W]::Fail
                if ($twoTabs) {
                    # 이전 시도가 도중에 끊겼을 수 있으므로 남은 조합을 Enter로 비우고, 앱이 알려 주는 선택 탭이 1번이 되게 한다.
                    [W]::Tap($ENTER); Start-Sleep -Milliseconds 300
                    if ((Get-Content (Join-Path $Out 'state-live.txt') -ErrorAction SilentlyContinue | Select-Object -First 1) -ne 'selected: 1') { [W]::CtrlTap(0x25); Start-Sleep -Milliseconds 400 }
                    $beforeB = LogSize $logPathB
                }
                $before = LogSize
                switch ($test) {
                    'T1' { Typ $HG; [W]::Tap($ENTER); $expect = '^한글<CR>$' }
                    'T2' { Typ $HG; [W]::Tap($SPACE); Start-Sleep -Milliseconds 50; [W]::Tap($ENTER); $expect = '^한글 <CR>$' }
                    'T3' { Typ $HG; [W]::Tap($ESC); Start-Sleep -Milliseconds 100; [W]::Tap($ENTER); $expect = '^한글(<ESC>)?<CR>$' }
                    'T4a' { Typ $HG; [W]::Tap($BACK); Start-Sleep -Milliseconds 100; [W]::Tap($ENTER); $expect = '^한그<CR>$' }
                    'T4b' { Typ $HG; [W]::Tap($BACK); Start-Sleep -Milliseconds 60; [W]::Tap($BACK); Start-Sleep -Milliseconds 100; [W]::Tap($ENTER); $expect = '^한ㄱ<CR>$' }
                    'T5' {
                        Typ $HG
                        if (-not $helper -or $helper.HasExited) { $script:helper = StartHelper }
                        [void][W]::Focus($script:helper.MainWindowHandle); Start-Sleep -Milliseconds 400
                        [void](Ensure); Start-Sleep -Milliseconds 300
                        [W]::Tap($ENTER); $expect = '^한글<CR>$'
                    }
                    'T5b' {
                        # 탭 1에서 조합 중에 Ctrl+→로 탭 2로 옮긴다. 이어 Enter는 탭 2에 들어가고, 시험 끝에 탭 1로 돌아온다.
                        Typ $HG
                        [W]::CtrlTap(0x27); Start-Sleep -Milliseconds 400
                        [W]::Tap($ENTER); Start-Sleep -Milliseconds 300
                        [W]::CtrlTap(0x25); Start-Sleep -Milliseconds 300
                        $expect = '^한글$'
                    }
                    'T8' {
                        # 대본이 이름 바꾸기 창을 열 때까지 기다린 뒤 조합 중 Enter를 보낸다. 터미널에는 아무것도 가지 않아야 한다.
                        for ($w = 0; $w -lt 50 -and -not ((Get-Content (Join-Path $Out 'state-live.txt') -ErrorAction SilentlyContinue) -contains 'overlay: rename'); $w++) { Start-Sleep -Milliseconds 200 }
                        Typ $HG; [W]::Tap($ENTER); Start-Sleep -Milliseconds 400
                        $named = (Get-Content (Join-Path $Out 'state-live.txt') -ErrorAction SilentlyContinue | Where-Object { $_ -like 'tab 1 *' }) -match 'name=한글 '
                        $expect = if ($named) { '^$' } else { '^NAME-NOT-APPLIED$' }
                    }
                    'T8b' {
                        # 조합을 시작하고(Enter 없음) 창이 열리기를 기다린 뒤 입력칸에서 Enter만 보낸다. 조합 글자만 터미널에 들어가야 한다.
                        Typ $HG
                        for ($w = 0; $w -lt 50 -and -not ((Get-Content (Join-Path $Out 'state-live.txt') -ErrorAction SilentlyContinue) -contains 'overlay: rename'); $w++) { Start-Sleep -Milliseconds 200 }
                        [W]::Tap($ENTER); Start-Sleep -Milliseconds 400
                        $kept = (Get-Content (Join-Path $Out 'state-live.txt') -ErrorAction SilentlyContinue | Where-Object { $_ -like 'tab 1 *' }) -match 'name=terminal 1 '
                        $expect = if ($kept) { '^한글$' } else { '^NAME-CHANGED$' }
                    }
                    'T9' {
                        # 대본이 추가 양식을 열 때까지 기다린 뒤 조합 중 Enter를 보낸다. 계정이 `한글`로 추가되고 터미널에는 아무것도 가지 않아야 한다.
                        $live = Join-Path $Out 'state-live.txt'
                        for ($w = 0; $w -lt 50 -and -not ((Get-Content $live -Encoding UTF8 -ErrorAction SilentlyContinue) -match 'acct dialog form=true'); $w++) { Start-Sleep -Milliseconds 200 }
                        Typ $HG; [W]::Tap($ENTER); Start-Sleep -Milliseconds 800
                        $added = (Get-Content $live -Encoding UTF8 -ErrorAction SilentlyContinue) -match 'account=Claude . 한글'
                        $expect = if ($added) { '^$' } else { '^ACCOUNT-NOT-ADDED$' }
                    }
                    'T7' {
                        [void](SetHangul $false); Typ $HG; [W]::Tap($ENTER); Start-Sleep -Milliseconds 200
                        [void](SetHangul $true); Typ $HG; [W]::Tap($ENTER); $expect = '^gksrmf<CR>한글<CR>$'
                    }
                }
                Start-Sleep -Milliseconds 400
                $got = Show (Delta $before)
                if ($test -eq 'T5b') {
                    $gotB = Show (Delta $beforeB $logPathB)
                    # 탭 1에는 확정된 글자만, 탭 2에는 Enter만 들어가야 한다.
                    if ($gotB -ne '<CR>') { $got = "$got | tab2=$gotB" }
                }
                if ([W]::Fail -ne $f0) { Say "$name #$i retry (focus lost: $([W]::Bad)) got=$got"; Start-Sleep -Milliseconds 500; continue }
                $done = $true
                if ($got -match $expect) { $pass++ } else { $fail++; Say "$name #$i FAIL got=$got" }
                if ($got -match '<ESC>') { $escSeen++ }
            }
            if (-not $done) { $skip++; Say "$name #$i SKIPPED" }
        }
        Say "$name pass=$pass fail=$fail skipped=$skip esc_forwarded=$escSeen of $N"
    }
    [void](SetHangul (($startConv -band 1) -eq 1))
    Say "conv restored: start=$startConv now=$([W]::ConvMode($h)) fail_keys=$([W]::Fail)"
} finally {
    Cleanup
}
