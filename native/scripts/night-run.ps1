# 사용: night-run.ps1 -Exe <paddock-app.exe> -Root <결과 폴더> -Go [-SoundExe <paddock-app.exe>] [-N 30] [-Only T1,I3,...]
# 야간 묶음: 대본(합성 키)으로는 확인하지 못하는 실제 입력 경로 항목을 한 번에 돌려 결과를 한 폴더에 모은다.
# 사용자가 자리에 없을 때만 돌린다 — 항목마다 화면 전경·키보드·마우스·한/영 상태를 쓰고, 소리 항목은 스피커로 소리가 난다.
#   -Exe       자동 점검 기능(automation)으로 빌드한 앱. IME·입력·빠른 설정 항목이 쓴다(앱이 남기는 state-live.txt로 판정).
#   -SoundExe  자동 점검 기능 없이 빌드한 앱. 소리 항목만 쓴다. 없으면 소리 항목은 건너뛴다(SKIP).
#   -Root      결과 폴더. 항목마다 items\<이름>\ 에 결과·로그·캡처가 남고, 요약은 summary.csv, 시작·끝 상태는 아래 파일에 남는다.
#   -Only      지정한 항목만 돌린다(예: -Only T8,Q1).  -Go  없으면 아무것도 하지 않고 항목 목록만 보인다.
# 항목(이름: 내용)
#   T-main  T1·T2·T3·T4a·T4b·T5·T7  한글 IME 기본 경로(조합 중 Enter·Space·Esc·Backspace, 다른 창 갔다 오기, 한/영 전환)
#   T5b     탭 전환 중 조합 확정(터미널 2개)
#   T6      한자 후보창 캡처(-Mode capture)
#   T8      이름 바꾸기 창에서 조합 중 Enter
#   T8b     터미널 조합 중 이름 바꾸기 창 열기
#   T9      Accounts 이름 입력칸에서 조합 중 Enter
#   I1      Windows가 바꿔 넘기는 Shift+숫자·기호 키로 분할 단축키
#   I3      칸 경계 마우스 누름 전파
#   I5      러시아어 배열에서 Ctrl+B 사이드바 접기
#   Q1      빠른 설정 패널 실제 마우스 조작(톱니·테마·소리·배율)
#   S1      완료 소리 실제 재생 1회(소리 크기 계로 소리가 난 시각을 확인)
# 안전 규칙
#   - 시작 전에 열린 창 목록·전경 창·입력 언어를 기록하고, 끝나면(중간에 멈춰도) 입력 언어와 전경 창을 되돌린 결과를 기록한다.
#   - 항목은 서로 독립이다: 하나가 실패하거나 예외가 나도 다음 항목을 계속하고, 결과는 summary.csv에 남긴다.
#   - 항목마다 새 임시 홈(Root\homes\<이름>)을 쓰며 실제 사용자 폴더는 건드리지 않는다. 다른 paddock-daemon이 떠 있으면 시작하지 않는다.
#   - 각 항목이 띄운 앱·데몬만 그 항목이 끝낼 때 종료한다.
param(
    [string]$Exe,
    [string]$Root,
    [string]$SoundExe = "",
    [int]$N = 30,
    [string[]]$Only = @(),
    [switch]$Go
)
$items = @(
    @{ name = 'T-main'; script = 'ime-run.ps1'; tests = @('T1', 'T2', 'T3', 'T4a', 'T4b', 'T5', 'T7') },
    @{ name = 'T5b';    script = 'ime-run.ps1'; tests = @('T5b') },
    @{ name = 'T6';     script = 'ime-run.ps1'; tests = @('T1'); mode = 'capture' },
    @{ name = 'T8';     script = 'ime-run.ps1'; tests = @('T8') },
    @{ name = 'T8b';    script = 'ime-run.ps1'; tests = @('T8b') },
    @{ name = 'T9';     script = 'ime-run.ps1'; tests = @('T9') },
    @{ name = 'I1';     script = 'input-run.ps1'; tests = @('I1') },
    @{ name = 'I3';     script = 'input-run.ps1'; tests = @('I3') },
    @{ name = 'Q1';     script = 'input-run.ps1'; tests = @('Q1') },
    @{ name = 'I5';     script = 'input-run.ps1'; tests = @('I5') },
    @{ name = 'S1';     script = 'sound-run.ps1'; tests = @(); sound = $true }
)
if ($Only.Count -gt 0) { $items = @($items | Where-Object { $Only -contains $_.name }) }
if (-not $Go) {
    "night-run.ps1 items (add -Go to run; it uses the screen, keyboard, mouse and speakers):"
    $items | ForEach-Object { "  {0,-7} {1} {2}" -f $_.name, $_.script, ($_.tests -join ',') }
    return
}
if ($Exe -eq "" -or $Root -eq "") { throw "Exe and Root are required" }
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class NightWin {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static string LayoutOf(IntPtr window) { uint p; uint t = GetWindowThreadProcessId(window, out p); return GetKeyboardLayout(t).ToInt64().ToString("X"); }
  public static string TitleOf(IntPtr window) { var s = new System.Text.StringBuilder(256); GetWindowText(window, s, 256); return s.ToString(); }
  // 입력 언어를 되돌리는 표준 방법: 그 창에 입력 언어 바꾸기 요청(WM_INPUTLANGCHANGEREQUEST)을 보낸다.
  public static void RequestLayout(IntPtr window, string hex) { PostMessage(window, 0x0050, IntPtr.Zero, new IntPtr(Convert.ToInt64(hex, 16))); }
}
'@
New-Item -ItemType Directory -Force $Root | Out-Null
$log = Join-Path $Root 'night-run.log'
$summary = Join-Path $Root 'summary.csv'
function Log($text) { "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $text" | Out-File $log -Append -Encoding utf8 }
# PowerShell 작은따옴표 글자로 만든다(안의 작은따옴표는 두 번 쓴다).
function Quote($text) { "'" + ($text -replace "'", "''") + "'" }
function Open-Windows {
    Get-Process | Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object Id |
        ForEach-Object { "{0,7} {1,-24} hwnd={2} title={3}" -f $_.Id, $_.ProcessName, $_.MainWindowHandle, $_.MainWindowTitle }
}
$foreground = [NightWin]::GetForegroundWindow()
$layoutBefore = [NightWin]::LayoutOf($foreground)
Open-Windows | Set-Content (Join-Path $Root 'windows-before.txt') -Encoding utf8
@("foreground hwnd=$foreground title=$([NightWin]::TitleOf($foreground))", "input layout (foreground thread)=$layoutBefore",
  "user language list=$((Get-WinUserLanguageList | ForEach-Object { $_.LanguageTag }) -join ',')") | Set-Content (Join-Path $Root 'input-language-before.txt') -Encoding utf8
"item,started,finished,exit_code,result" | Set-Content $summary -Encoding utf8
if (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0) { throw "paddock-daemon is already running" }
Log "start: items=$($items.name -join ',') exe=$Exe soundExe=$SoundExe n=$N"
try {
    foreach ($item in $items) {
        $started = Get-Date
        $itemOut = Join-Path $Root "items\$($item.name)"
        $itemHome = Join-Path $Root "homes\$($item.name)"
        New-Item -ItemType Directory -Force $itemOut | Out-Null
        $exitCode = -1; $result = ''
        try {
            if ($item.sound -and $SoundExe -eq "") { $result = 'SKIP (no -SoundExe)'; $exitCode = 0 }
            elseif (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0) { $result = 'SKIP (a paddock-daemon is running)' }
            else {
                $itemExe = if ($item.sound) { $SoundExe } else { $Exe }
                # 항목은 자식 PowerShell에서 돌려 서로의 형식·변수가 섞이지 않게 하고, 출력은 항목 폴더에 남긴다.
                # `-File`은 쉼표로 이은 목록을 글자 하나로 넘기므로 명령 글자로 만들어 `-Command`로 돌린다.
                $command = "& $(Quote (Join-Path $PSScriptRoot $item.script)) -Exe $(Quote $itemExe) -HomeDir $(Quote $itemHome) -Out $(Quote $itemOut)"
                if ($item.tests.Count -gt 0) { $command += " -Tests @($(($item.tests | ForEach-Object { Quote $_ }) -join ','))" }
                if ($item.mode) { $command += " -Mode $(Quote $item.mode)" }
                if ($item.script -eq 'ime-run.ps1') { $command += " -N $N" }
                & powershell.exe -NoProfile -ExecutionPolicy Bypass -Command $command *> (Join-Path $itemOut 'console.txt')
                $exitCode = $LASTEXITCODE
                $resultFile = Get-ChildItem $itemOut -Filter 'result-*.txt' -ErrorAction SilentlyContinue | Select-Object -First 1
                $result = if ($resultFile) { ((Get-Content $resultFile.FullName | Where-Object { $_ -match 'pass=|PASS|FAIL|CHECK|SKIP|NO-SOUND|NO WINDOW|NOT STARTED|captured' } | Select-Object -Last 3) -join ' / ') } else { 'NO RESULT FILE' }
            }
        } catch {
            $result = "EXCEPTION: $($_.Exception.Message)"
            Log "$($item.name) exception: $($_.Exception.Message)"
        }
        # 항목이 남긴 앱·데몬이 있으면 이 항목의 임시 홈이 가리키는 것만 정리한다(각 스크립트가 이미 정리했어야 한다).
        $info = Join-Path $itemHome '.paddock\native\daemon.json'
        if (Test-Path $info) { try { $d = (Get-Content $info -Raw | ConvertFrom-Json).pid; $p = Get-Process -Id $d -ErrorAction SilentlyContinue; if ($p -and $p.ProcessName -eq 'paddock-daemon') { Stop-Process -Id $d -Force; Log "$($item.name): stopped a leftover daemon pid=$d" } } catch {} }
        $line = '"{0}","{1:o}","{2:o}",{3},"{4}"' -f $item.name, $started, (Get-Date), $exitCode, ($result -replace '"', "'")
        Add-Content $summary $line -Encoding utf8
        Log "$($item.name) done exit=$exitCode result=$result"
        Start-Sleep -Seconds 3
    }
} finally {
    # 입력 언어와 전경 창을 시작 때로 되돌리고, 되돌려졌는지 기록한다.
    if ([NightWin]::IsWindow($foreground)) {
        [void][NightWin]::ShowWindow($foreground, 9)
        [void][NightWin]::SetForegroundWindow($foreground)
        Start-Sleep -Milliseconds 500
        [NightWin]::RequestLayout($foreground, $layoutBefore)
        Start-Sleep -Milliseconds 800
    }
    $layoutAfter = [NightWin]::LayoutOf([NightWin]::GetForegroundWindow())
    Open-Windows | Set-Content (Join-Path $Root 'windows-after.txt') -Encoding utf8
    @("foreground hwnd=$([NightWin]::GetForegroundWindow()) title=$([NightWin]::TitleOf([NightWin]::GetForegroundWindow()))", "input layout (foreground thread)=$layoutAfter",
      "input layout restored=$($layoutAfter -eq $layoutBefore)") | Set-Content (Join-Path $Root 'input-language-after.txt') -Encoding utf8
    Log "finished: input layout before=$layoutBefore after=$layoutAfter"
}
