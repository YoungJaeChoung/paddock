# 사용: sound-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -Out <결과 폴더> [-WaitSec 150]
# 완료 소리가 실제 스피커 장치로 한 번 나오는지 본다(야간 묶음 전용 — 소리가 울리고 키 입력을 쓴다).
#   준비: 일반 빌드(자동 점검 기능 없이 만든 paddock-app.exe)여야 한다. 자동 점검 빌드는 소리를 내지 않고 사건만 남기므로 이 시험이 늘 NO-SOUND가 된다.
#   과정: 임시 홈의 bin 폴더에 cmd.exe 복사본을 claude.exe로 두고, 앱 첫 터미널(PowerShell)에서 그것으로 출력이 5초 넘게 이어지는 반복을 돌린다.
#         곧바로 Ctrl+Shift+T로 다른 터미널을 열어 에이전트 터미널을 안 보는 상태로 만든다. 일이 끝나고 3초 조용하면 완료 소리가 나야 한다.
#   판정: 기본 출력 장치의 소리 크기 계(Windows 오디오 미터)가 시험 전 기준보다 뚜렷하게 올라가면 PASS, 끝까지 오르지 않으면 NO-SOUND(음소거·장치 없음이거나 소리가 안 났다).
#         사람이 귀로 듣는 확인은 따로 필요하다 — 결과 파일은 소리가 난 시각만 알려 준다.
# 안전 규칙: input-run.ps1과 같다 — 이 스크립트가 띄운 앱 창이 전경일 때만 키를 보내고, 실제 사용자 폴더를 거부하며, 다른 paddock-daemon이 있으면 시작하지 않고,
# 끝나면 이 스크립트가 띄운 프로세스만 종료한다.
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$Out,
    [int]$WaitSec = 150
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
  public static void Chord(ushort[] mods, ushort vk) {
    foreach (ushort m in mods) { Key(m, 0, 0); System.Threading.Thread.Sleep(15); }
    Tap(vk); System.Threading.Thread.Sleep(15);
    for (int i = mods.Length - 1; i >= 0; i--) { Key(mods[i], 0, 2); System.Threading.Thread.Sleep(15); } }
  // 글자를 유니코드 입력으로 보낸다(키보드 배열에 상관없이 같은 글자가 들어간다).
  public static void Text(string s) {
    foreach (char c in s) { Key(0, c, 4); System.Threading.Thread.Sleep(8); Key(0, c, 6); System.Threading.Thread.Sleep(8); } }
  public static bool Focus(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    uint p; uint ft = GetWindowThreadProcessId(GetForegroundWindow(), out p); uint me = GetCurrentThreadId();
    ShowWindow(h, 9); AttachThreadInput(me, ft, true); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(me, ft, false);
    System.Threading.Thread.Sleep(200); return GetForegroundWindow() == h; }
}
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator { int NotImpl1(); int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice device); }
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice { int Activate(ref Guid iid, int clsCtx, IntPtr activationParams, [MarshalAs(UnmanagedType.IUnknown)] out object result); }
[Guid("C02216F6-8C67-4B5B-9D00-D008E73E0064"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioMeterInformation { int GetPeakValue(out float peak); }
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorObject { }
// 기본 출력 장치가 지금 내보내는 소리의 최대 크기(0~1). 장치가 없으면 -1이다.
public class Meter {
  static IAudioMeterInformation meter;
  public static float Peak() {
    try {
      if (meter == null) {
        IMMDevice device; ((IMMDeviceEnumerator)new MMDeviceEnumeratorObject()).GetDefaultAudioEndpoint(0, 1, out device);
        Guid iid = new Guid("C02216F6-8C67-4B5B-9D00-D008E73E0064"); object result;
        device.Activate(ref iid, 23, IntPtr.Zero, out result); meter = (IAudioMeterInformation)result; }
      float peak; meter.GetPeakValue(out peak); return peak;
    } catch { return -1f; } }
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
# 첫 터미널이 PowerShell이 되게 하고, 소리는 켠 채로 둔다(임시 홈의 네이티브 설정). 가짜 claude는 cmd.exe 복사본이라 자식 프로세스 없이 출력만 낸다.
$native = Join-Path $HomeDir '.paddock\native'
New-Item -ItemType Directory -Force $native | Out-Null
[IO.File]::WriteAllText((Join-Path $native 'settings.json'), '{ "terminal.integrated.defaultProfile.windows": "PowerShell", "paddock.agentDoneSound": true }')
Copy-Item (Join-Path $env:SystemRoot 'System32\cmd.exe') (Join-Path $HomeDir 'bin\claude.exe') -Force
$resultPath = Join-Path $Out 'result-S1.txt'
function Say($m) { $m | Out-File $resultPath -Append -Encoding utf8 }
$startedPids = @()
$proc = Start-Process -FilePath $Exe -ArgumentList "--out `"$Out`"" -PassThru
$startedPids += $proc.Id
try {
    for ($i = 0; $i -lt 100 -and $proc.MainWindowHandle -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $proc.Refresh() }
    $h = $proc.MainWindowHandle
    $windowPid = [uint32]0; [void][S]::GetWindowThreadProcessId($h, [ref]$windowPid)
    if ($h -eq [IntPtr]::Zero -or $windowPid -ne $proc.Id -or $proc.MainWindowTitle -ne 'Paddock') { Say "NO WINDOW (handle=$h pid=$windowPid)"; return }
    [S]::T = $h
    Start-Sleep -Seconds 5
    $focused = $false
    for ($i = 0; $i -lt 5 -and -not $focused; $i++) { $focused = [S]::Focus($h); if (-not $focused) { Start-Sleep -Milliseconds 300 } }
    if (-not $focused) { Say "S1 SKIP (could not bring the test window to the foreground)"; return }
    # 시험 전 기준 소리 크기: 다른 프로그램이 내는 소리가 있으면 기준이 올라가 판정이 엄격해진다.
    $baseline = 0.0
    for ($i = 0; $i -lt 15; $i++) { $baseline = [Math]::Max($baseline, [Meter]::Peak() -as [double]); Start-Sleep -Milliseconds 100 }
    if ([Meter]::Peak() -lt 0) { Say "S1 SKIP (no default output device or the audio meter could not be read)"; return }
    $threshold = [Math]::Max($baseline * 3, 0.002)
    # 에이전트처럼 보이는 일을 시킨다: 이름이 claude인 프로그램이 출력을 5초 넘게 낸 뒤 끝난다.
    [S]::Text('& "$env:USERPROFILE\bin\claude.exe" /c "for /L %i in (1,1,120000) do @echo work %i"')
    [S]::Tap(0x0D)
    Start-Sleep -Milliseconds 800
    # 다른 터미널을 열어 에이전트 터미널이 안 보이게 한다(보는 중이면 소리를 내지 않는다).
    [S]::Chord(@(0xA2, 0xA0), 0x54)
    $sentAt = Get-Date
    $heardAt = $null; $maxPeak = 0.0
    while (((Get-Date) - $sentAt).TotalSeconds -lt $WaitSec -and -not $proc.HasExited) {
        $peak = [Meter]::Peak() -as [double]
        $maxPeak = [Math]::Max($maxPeak, $peak)
        if ($peak -gt $threshold -and -not $heardAt) { $heardAt = Get-Date; Start-Sleep -Milliseconds 1000; break }
        Start-Sleep -Milliseconds 50
    }
    if ($heardAt) { Say ("S1 PASS sound heard {0:N1}s after the command (peak {1:N4}, baseline {2:N4}). Listen once to confirm it is the two-note chime. inputFailures={3}" -f (($heardAt - $sentAt).TotalSeconds), $maxPeak, $baseline, [S]::Fail) }
    else { Say ("S1 NO-SOUND within {0}s (max peak {1:N4}, baseline {2:N4}). Muted output, a build with the automation feature, or no completion detected. inputFailures={3}" -f $WaitSec, $maxPeak, $baseline, [S]::Fail) }
} finally {
    $info = Join-Path $native 'daemon.json'
    if (Test-Path $info) { try { $d = (Get-Content $info -Raw | ConvertFrom-Json).pid; if ($d) { $startedPids += $d } } catch {} }
    foreach ($id in $startedPids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    Say "cleanup: stopped $($startedPids -join ',')"
}
