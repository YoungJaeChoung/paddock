# 사용: night-run.ps1 -Exe <paddock-app.exe> -Root <결과 폴더> -Go [-SoundExe <paddock-app.exe>] [-N 30] [-Only T1,I3,...]
# 실행 방법(사용자가 자리를 비우며 "야간 시험 돌려"라고 할 때 — 화면·키보드·마우스·스피커를 쓰므로 그동안 PC를 만지지 않는다)
#   1) 자동 점검 빌드와 점검 기능 없는 빌드를 둘 다 새로 만든다. 점검 빌드 `-Exe`: `make native-win`의 `paddock-app.exe`·`paddock-daemon.exe`(같은 폴더).
#      점검 기능 없는 빌드 `-SoundExe`: 같은 폴더 구성으로 `automation` 기능 없이 만든 `paddock-app.exe`·`paddock-daemon.exe`(소리·놀 때 CPU 항목 S1·IDLE-CPU만 쓴다. 없으면 두 항목은 SKIP).
#   2) 사용자의 `make feedback` 앱(Paddock·Electron)은 그대로 둔다 — 이 묶음은 임시 홈과 새 앱·데몬만 쓰고 자기가 띄운 것만 끝낸다.
#   3) Windows PowerShell에서(WSL 경로 말고 C:\ 아래로 복사한 사본을 쓴다):
#        powershell -NoProfile -ExecutionPolicy Bypass -File night-run.ps1 -Exe C:\Temp\night\app\paddock-app.exe -SoundExe C:\Temp\night\release\paddock-app.exe -Root C:\Temp\night\run-<날짜> -Go
#      `-Go` 없이 실행하면 항목 목록만 보인다. 일부만: `-Only I6,I7`. 항목은 모두 합쳐 약 50분(항목당 1~2분, IDLE-CPU 약 2분).
#   4) 끝나면 `<Root>\summary.csv`(항목·시작·끝·종료 코드·결과 한 줄)와 `<Root>\items\<항목>\`(결과 파일·캡처·console.txt)를 본다. 판정 낱말: PASS / FAIL(기능이 틀림) /
#      CHECK(좌표가 빗나갔거나 눈으로 봐야 함 — 캡처가 남음) / SKIP(조건 부족). 시작·끝 창 목록과 입력 언어는 `windows-*.txt`·`input-language-*.txt`.
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
# 2단계 항목(files-run.ps1 — 항목마다 새 임시 홈·시험 자료(Git 저장소)·앱. 통과 조건과 증거는 files-run.ps1 머리와 항목 폴더의 캡처)
#   I6      파일 행 우클릭 메뉴 열기·Esc 닫기·항목 눌러 파일 탭 열기
#   I7      파일 칸 경계를 마우스로 끌어 비율 바꾸기(PTY 키 0)
#   I8      실제 자판 Ctrl+Shift+W로 활성 파일 탭 닫기(PTY 키 0)
#   I9      실제 자판 Ctrl+Shift+F 열기·닫기·입력칸 Esc(편집기·터미널 포커스 양쪽, PTY·편집기 새지 않음)
#   I10     Source control 행 호버 `+`·행 클릭(비교 탭)·메시지 입력·Commit 단추
#   I11     되돌리기 확인 창에서 실제 Enter가 취소인지 승인인지
#   I12     WSL 폴더에서 창 비활성 → 활성 때 `wsl.exe` 정확히 1회
#   I13     Markdown 웹 링크는 기본 브라우저가 열려 시험용 수신기에 요청이 오고, file:·앵커 링크는 안 열림
#   I14     이미지 더블클릭으로 맞춤 ↔ 1:1
#   I15     Markdown File·Preview·Both 단추와 미리보기 글자 드래그 선택·Ctrl+C
#   I16     이미지 1:1이 이미지 한 픽셀 = 화면 한 픽셀(1.5배 이상 모니터에서 의미)
#   I17     PDF −·+·Open with default app 단추
#   I18     PDF 휠·스크롤바 끌기 때 빈 종이가 보이는 시간 ≤ 1초
#   I19     PDF 글자 선명도(1.5배 이상 모니터, 확대 캡처)
#   I20     Ctrl+올림 밑줄·손가락 커서, Ctrl+클릭 파일 탭 열기, Ctrl 없는 클릭·Ctrl 뗌
#   I21     검색 결과 행 클릭·파일 행 접기·결과 휠(2,000건)·옵션 단추
#   T10     커밋 메시지 입력칸 한글 조합 중 Enter(한 번만 확정·커밋 안 됨)
#   T11     검색 입력칸 한글 조합 중 Enter(한 번만 확정·PTY 안 샘)
#   I22~I26 (덩어리 23, files-run.ps1) 터미널 행 더블클릭 이름 바꾸기·행 끌어 순서·copy on selection·메뉴 ↑↓Enter·Ctrl+Shift+H 터미널 찾기
#   I27~I30 (덩어리 24, files-run.ps1) 탐색기 셸 복사 → 목록 Ctrl+V·비트맵/PNG 실제 Ctrl+V(목록·터미널)·OLE 끌어 놓기(SKIP: 사람)·실제 F2 이름 바꾸기와 Windows §4 결함 확인
#   T12     편집기 한글 조합 중 Enter(한 번만 확정·줄바꿈 하나) 뒤 Ctrl+S로 저장 — 디스크에 한글+CRLF
#   I31     (덩어리 25, files-run.ps1) 실제 자판 Ctrl+S: 파일 탭 활성이면 저장, 터미널 활성이면 저장 안 함
#   I32     (덩어리 25, files-run.ps1) 실제 자판 Ctrl+F·Ctrl+H·Ctrl+G: 파일 탭 활성이면 패널·창, 터미널 활성이면 셸로
#   T13     (덩어리 26, files-run.ps1) 검토 메모 입력창 한글 조합 중 Enter(한 번만 확정)
#   I33     (덩어리 26, files-run.ps1) 실제 자판 Ctrl+Alt+M: 파일 탭 활성이면 메모 창, 터미널 활성이면 셸로(입력 언어 기록)
#   I34     (덩어리 27, files-run.ps1) 실제 자판 Ctrl+Shift+P: 터미널·편집기 포커스 어디서나 빠른 열기, 평 Ctrl+P는 셸로
#   I35     (덩어리 27, files-run.ps1) 폴더 검색 바꾸기 줄 + 실제 마우스 `Replace All` + Enter 승인
#   (클립보드를 바꾸는 `act` 대본 files-paste·files-paste-wsl·terminal-paste-paths는 이 묶음 항목이 아니다 — 사용자 클립보드를 건드리므로 자리를 비운 야간에 session-run.ps1로 따로 돌린다. 끝에서 클립보드 글자를 되돌린다.)
#   I36     (덩어리 29, files-run.ps1) 실제 `codex app-server`로 상태 줄 Codex 게이지 값(사용자 Codex 로그인, 읽기만 — 사람 승인 뒤 CHECK 등급)
#   I37     (덩어리 30, files-run.ps1) 실제 마우스로 사용량 목록 열기 + 실제 Esc 닫기(`Open in new terminal`은 캡처 좌표로)
#   I38     (덩어리 30, files-run.ps1) 기본 CLI 로그인 연결 이름이 Electron 상태 줄 이름과 같은지(실제 홈이 필요해 SKIP — 사람이 한 번 견준다)
#   I39     (덩어리 31, files-run.ps1) 실제 WSL 부하로 Memory 90% 경고 한 번(사용자 PC 메모리를 채우므로 SKIP — 사람이 승인해 직접 한다)
#   I40     (덩어리 32, files-run.ps1) 실제 Windows node로 상태 줄 명령 한 번 실행 + 앱이 쓴 flag-settings.json을 사용자의 실제 파일과 바이트 대조(읽기만)
#   T14     (덩어리 32, files-run.ps1) WSL 래퍼 묻기 창에서 한글 조합 중 Enter가 `Ask me later`로만 동작(WSL 쓰기는 임시 폴더로 가둠)
#   I41     (덩어리 33, files-run.ps1) 실제 마우스로 계정 고르기 메뉴를 열어 `Continue with …`(가짜 claude, WSL 임시 홈에 가둠)
#   I43     (덩어리 36, files-run.ps1) 실제 마우스로 탭 메뉴 `Move to New Window`를 눌러 분리 창을 열고 그 창에 타이핑·한글 입력(CHECK)
#   T15     (덩어리 36, files-run.ps1) 창 사이 포커스 이동 중 한글 조합 30회(두 창을 사람이 오가야 해서 SKIP — 사용자 1회 확인)
#   I44     (덩어리 37, files-run.ps1) 실제 마우스로 탭을 창 밖에 끌어 놓아 분리 창을 만들고 창 밖 MouseUp 좌표를 events.log로 읽는다(CHECK — 되돌리기는 사람이)
#   I45     (덩어리 37, files-run.ps1) 실제 마우스 가장자리 분할·탭 줄 끼우기(CHECK — 키를 누른 뒤 놓기는 사람이)
#   I46     (덩어리 37, files-run.ps1) 배율이 다른 모니터 사이 분리 창 → 본 창 끌기(사용자 모니터 구성이 필요해 CHECK — 준비까지)
#   T16     (덩어리 37, files-run.ps1) 끌기 중 Esc 뒤 놓기는 아무 일도 없고 끌기 상태가 남지 않는다(Alt+Tab은 사람이)
#   I47     (덩어리 38, files-run.ps1) 실제 마우스로 파일 탭을 창 밖에 끌어 분리·탭 우클릭 `Move to New Window`·되돌리기(CHECK — 사람이)
#   T17     (덩어리 38, files-run.ps1) 분리 창 편집기의 실제 한글 IME 조합 뒤 Ctrl+S(SKIP — 사람이)
#   I48     (덩어리 38, files-run.ps1) 분리 창 찾기 띠의 Enter·Shift+Enter(CHECK — 사람이)
#   T18     (덩어리 38, files-run.ps1) 빠른 창 밖 끌기·누른 뒤 놓기·끌기 중 Esc(CHECK — 사람이)
#   I42     (덩어리 33, files-run.ps1) 실제 두 계정으로 `--resume <전사 경로>` 교차 재개(실제 계정 폴더에 쓰므로 SKIP — 사용자가 1회 확인)
#   I49     (덩어리 42, daemon-run.ps1) 호환 데몬 계속 쓰기: 데몬 파일만 달라진 앱 업데이트는 묻지 않고 같은 데몬·터미널에 붙고(빠른 설정 안내 줄), Quit and Stop All 뒤 새 데몬, 터미널 없는 이전 데몬은 시작 때 교체, 프로토콜 불일치는 교체를 묻는 화면(훅 데몬 필요 — 앱과 같은 폴더의 paddock-daemon.exe가 test-hooks 빌드여야 7·8단계가 선다)
#   OPEN    쉼표·공백·한글 경로의 미디어 파일을 안내 탭 `Open with default app`으로 열기
#   IDLE-CPU-EDIT 편집 탭 5개를 고친 채 놀 때도 같은 기준(덩어리 28, 실제 키 Ctrl+Shift+P로 파일을 연다)
#   IDLE-CPU 놀 때 앱 CPU ≤ 2 %·데몬 ≤ 1 %·`wsl.exe` 0(점검 기능 없는 빌드 `-SoundExe`, idle-cpu-run.ps1)
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
    @{ name = 'S1';     script = 'sound-run.ps1'; tests = @(); sound = $true },
    @{ name = 'I6';     script = 'files-run.ps1'; tests = @('I6') },
    @{ name = 'I7';     script = 'files-run.ps1'; tests = @('I7') },
    @{ name = 'I8';     script = 'files-run.ps1'; tests = @('I8') },
    @{ name = 'I9';     script = 'files-run.ps1'; tests = @('I9') },
    @{ name = 'I10';    script = 'files-run.ps1'; tests = @('I10') },
    @{ name = 'I11';    script = 'files-run.ps1'; tests = @('I11') },
    @{ name = 'I12';    script = 'files-run.ps1'; tests = @('I12') },
    @{ name = 'I13';    script = 'files-run.ps1'; tests = @('I13') },
    @{ name = 'I14';    script = 'files-run.ps1'; tests = @('I14') },
    @{ name = 'I15';    script = 'files-run.ps1'; tests = @('I15') },
    @{ name = 'I16';    script = 'files-run.ps1'; tests = @('I16') },
    @{ name = 'I17';    script = 'files-run.ps1'; tests = @('I17') },
    @{ name = 'I18';    script = 'files-run.ps1'; tests = @('I18') },
    @{ name = 'I19';    script = 'files-run.ps1'; tests = @('I19') },
    @{ name = 'I20';    script = 'files-run.ps1'; tests = @('I20') },
    @{ name = 'I21';    script = 'files-run.ps1'; tests = @('I21') },
    @{ name = 'T10';    script = 'files-run.ps1'; tests = @('T10') },
    @{ name = 'T11';    script = 'files-run.ps1'; tests = @('T11') },
    @{ name = 'I22';    script = 'files-run.ps1'; tests = @('I22') },
    @{ name = 'I23';    script = 'files-run.ps1'; tests = @('I23') },
    @{ name = 'I24';    script = 'files-run.ps1'; tests = @('I24') },
    @{ name = 'I25';    script = 'files-run.ps1'; tests = @('I25') },
    @{ name = 'I26';    script = 'files-run.ps1'; tests = @('I26') },
    @{ name = 'I27';    script = 'files-run.ps1'; tests = @('I27') },
    @{ name = 'I28';    script = 'files-run.ps1'; tests = @('I28') },
    @{ name = 'I29';    script = 'files-run.ps1'; tests = @('I29') },
    @{ name = 'I30';    script = 'files-run.ps1'; tests = @('I30') },
    @{ name = 'T12';    script = 'files-run.ps1'; tests = @('T12') },
    @{ name = 'I31';    script = 'files-run.ps1'; tests = @('I31') },
    @{ name = 'I32';    script = 'files-run.ps1'; tests = @('I32') },
    @{ name = 'T13';    script = 'files-run.ps1'; tests = @('T13') },
    @{ name = 'I33';    script = 'files-run.ps1'; tests = @('I33') },
    @{ name = 'I34';    script = 'files-run.ps1'; tests = @('I34') },
    @{ name = 'I35';    script = 'files-run.ps1'; tests = @('I35') },
    @{ name = 'I36';    script = 'files-run.ps1'; tests = @('I36') },
    @{ name = 'I37';    script = 'files-run.ps1'; tests = @('I37') },
    @{ name = 'I38';    script = 'files-run.ps1'; tests = @('I38') },
    @{ name = 'I39';    script = 'files-run.ps1'; tests = @('I39') },
    @{ name = 'I40';    script = 'files-run.ps1'; tests = @('I40') },
    @{ name = 'T14';    script = 'files-run.ps1'; tests = @('T14') },
    @{ name = 'I41';    script = 'files-run.ps1'; tests = @('I41') },
    @{ name = 'I42';    script = 'files-run.ps1'; tests = @('I42') },
    @{ name = 'I43';    script = 'files-run.ps1'; tests = @('I43') },
    @{ name = 'T15';    script = 'files-run.ps1'; tests = @('T15') },
    @{ name = 'I44';    script = 'files-run.ps1'; tests = @('I44') },
    @{ name = 'I45';    script = 'files-run.ps1'; tests = @('I45') },
    @{ name = 'I46';    script = 'files-run.ps1'; tests = @('I46') },
    @{ name = 'T16';    script = 'files-run.ps1'; tests = @('T16') },
    @{ name = 'I47';    script = 'files-run.ps1'; tests = @('I47') },
    @{ name = 'T17';    script = 'files-run.ps1'; tests = @('T17') },
    @{ name = 'I48';    script = 'files-run.ps1'; tests = @('I48') },
    @{ name = 'T18';    script = 'files-run.ps1'; tests = @('T18') },
    @{ name = 'I49';    script = 'daemon-run.ps1'; tests = @() },
    @{ name = 'OPEN';   script = 'files-run.ps1'; tests = @('OPEN') },
    @{ name = 'IDLE-CPU'; script = 'idle-cpu-run.ps1'; tests = @(); sound = $true },
    @{ name = 'IDLE-CPU-EDIT'; script = 'idle-cpu-run.ps1'; tests = @(); sound = $true; extra = '-EditTabs 5' }   # 편집 탭 5개를 고친 채 놀 때(캐럿 깜빡임, 덩어리 28)   # `sound = $true`: 점검 기능 없는 빌드(-SoundExe)를 쓰는 항목
)
if ($Only.Count -gt 0) { $items = @($items | Where-Object { $Only -contains $_.name }) }
if (-not $Go) {
    "night-run.ps1 items (add -Go to run; it uses the screen, keyboard, mouse and speakers):"
    $items | ForEach-Object { "  {0,-8} {1} {2}" -f $_.name, $_.script, ($_.tests -join ',') }
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
            if ($item.sound -and $SoundExe -eq "") { $result = 'SKIP (no -SoundExe: a build without the automation feature)'; $exitCode = 0 }
            elseif (@(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue).Count -gt 0) { $result = 'SKIP (a paddock-daemon is running)' }
            else {
                $itemExe = if ($item.sound) { $SoundExe } else { $Exe }
                # 항목은 자식 PowerShell에서 돌려 서로의 형식·변수가 섞이지 않게 하고, 출력은 항목 폴더에 남긴다.
                # `-File`은 쉼표로 이은 목록을 글자 하나로 넘기므로 명령 글자로 만들어 `-Command`로 돌린다.
                $command = "& $(Quote (Join-Path $PSScriptRoot $item.script)) -Exe $(Quote $itemExe) -HomeDir $(Quote $itemHome) -Out $(Quote $itemOut)"
                if ($item.tests.Count -gt 0) { $command += " -Tests @($(($item.tests | ForEach-Object { Quote $_ }) -join ','))" }
                if ($item.extra) { $command += " $($item.extra)" }
                if ($item.mode) { $command += " -Mode $(Quote $item.mode)" }
                if ($item.script -eq 'ime-run.ps1' -or $item.script -eq 'files-run.ps1') { $command += " -N $N" }
                & powershell.exe -NoProfile -ExecutionPolicy Bypass -Command $command *> (Join-Path $itemOut 'console.txt')
                $exitCode = $LASTEXITCODE
                $resultFile = Get-ChildItem $itemOut -Filter 'result-*.txt' -ErrorAction SilentlyContinue | Select-Object -First 1
                $result = if ($resultFile) { ((Get-Content $resultFile.FullName | Where-Object { $_ -match 'pass=|PASS|FAIL|CHECK|SKIP|NO-SOUND|NO WINDOW|NOT STARTED|SETUP-TIMEOUT|captured' } | Select-Object -Last 3) -join ' / ') } else { 'NO RESULT FILE' }
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
