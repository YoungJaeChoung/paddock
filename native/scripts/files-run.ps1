# 사용: files-run.ps1 -Exe <paddock-app.exe> -HomeDir <임시 사용자 폴더> -Out <결과 폴더> -Tests I6,I7,... [-N 10] [-PrepareOnly]
# 2단계(파일 목록·파일 탭·Git·Source control·Markdown·이미지·PDF·검색·터미널 경로 링크)에서 자동 점검 대본이 `act`로만 누르던 동작을
# 실제 SendInput 마우스·키로 시험한다(야간 묶음 전용 — 낮에 사용자 화면에서 돌리지 않는다).
# 시험마다 새 임시 홈(HomeDir\<시험 이름>)·새 시험 자료(HomeDir\<시험 이름>\proj, Git 저장소)·새 앱으로 돈다. 앱은 자동 점검 대본이 자료 폴더로 들어가
# 가짜 claude(PING.EXE 복사본)를 돌려 폴더 묶음·파일 목록이 서게 한 뒤(`mark night-ready`) 이 스크립트가 실제 입력을 보내고, 판정은 앱이 남기는 state-live.txt로 한다.
#   I6   파일 행 우클릭 → 메뉴(Open)가 열리고 Esc로 닫히며, 항목을 누르면 파일 탭이 열린다.
#   I7   파일 칸 경계를 마우스로 끌면 파일 칸 비율(file-pane ratio=)이 바뀌고 키가 PTY로 새지 않는다.
#   I8   실제 자판 Ctrl+Shift+W가 활성 파일 탭을 닫고 PTY 바이트가 0이다.
#   I9   실제 자판 Ctrl+Shift+F가 터미널·편집기 어디에 포커스가 있어도 검색 보기를 열고 닫으며(입력칸 Esc 포함) PTY·편집기로 새지 않는다.
#   I10  Source control 행 호버 단추(+)·행 클릭(비교 탭)·커밋 메시지 입력·Commit 단추를 실제 마우스로 누른다(커밋 제목이 입력한 글과 같아야 한다).
#   I11  되돌리기 확인 창에서 실제 Enter가 취소·승인 중 무엇을 하는지 기록한다(취소면 PASS, 승인이면 CHECK).
#   I12  WSL 저장소 폴더에서 창을 실제로 비활성 → 활성으로 돌릴 때 앱이 띄우는 `wsl.exe`(git-calls wsl=)가 정확히 1 늘어난다.
#   I13  Markdown 미리보기의 웹 링크를 실제로 누르면 기본 브라우저가 열려 시험용 수신기에 요청이 오고, `file:`·앵커 링크는 아무것도 열지 않는다.
#   I14  이미지 더블클릭(실제 마우스 두 번 누름)이 맞춤 ↔ 1:1을 바꾼다.
#   I15  Markdown 탭 줄의 File·Preview·Both 단추를 실제로 누르고, 미리보기에서 글자를 끌어 선택해 Ctrl+C로 복사한다.
#   I16  이미지 1:1이 이미지 픽셀 하나 = 화면 픽셀 하나인지(1픽셀 격자 그림의 흑백 교대가 화면에서 그대로인지). 화면 배율 1.5 이상 모니터에서 의미가 있다(DPI를 결과에 적는다).
#   I17  PDF 탭의 −·+ 단추를 실제로 누르고 `Open with default app`이 기본 앱 창을 띄운다.
#   I18  PDF 위에서 실제 마우스 휠·스크롤바 끌기로 쪽을 옮길 때 빈 종이가 보이는 시간(글자가 다시 보이기까지)이 1초 이내이다.
#   I19  화면 배율 1.5 이상 모니터에서 PDF 글자 선명도(가로 기울기 상위 1 %)와 확대 캡처를 남긴다(눈으로 보는 확인을 겸한다).
#   I20  Ctrl을 누른 채 터미널의 파일 경로에 마우스를 올리면 밑줄·손가락 커서, Ctrl+클릭이 그 줄로 파일 탭을 열고, Ctrl 없는 클릭은 열지 않으며 Ctrl을 떼면 밑줄이 사라진다.
#   I21  검색 결과 행을 실제 마우스로 눌러 파일 탭이 그 줄로 열리고, 옵션 단추(Aa)·결과 목록 휠(2,000건)·파일 행 접기가 동작한다.
#   T10  Source control 커밋 메시지 입력칸에서 한글 조합 중 Enter가 마지막 글자와 함께 한 번만 확정되고 커밋되지 않는다(이어 Commit 단추로 커밋한 제목이 `한글`).
#   T11  검색 입력칸에서 한글 조합 중 Enter가 마지막 글자와 함께 한 번만 확정되고(query=한글) 터미널(PTY)로 새지 않는다.
#   I22  (덩어리 23) Work 목록의 터미널 행을 실제 마우스로 더블클릭하면 이름 바꾸기 창이 열린다(첫 누름은 선택만).
#   I23  (덩어리 23) Work 목록의 터미널 행을 실제 마우스로 끌어 같은 폴더의 다른 행 뒤에 놓으면 행 순서가 바뀐다(끌기 표식은 캡처로 본다).
#   I24  (덩어리 23) `Copy on selection`을 켜고 터미널 글자를 실제 마우스로 끌어 선택하면 선택이 끝나는 순간 클립보드에 그 글자가 복사된다(클립보드를 되돌린다).
#   I25  (덩어리 23) 터미널 행 우클릭 메뉴에서 실제 자판 ↓·↓·↓·Enter가 강조를 옮겨 Close terminal 확인 창을 연다(Esc로 메뉴가 닫힘).
#   I26  (덩어리 23) 실제 자판 Ctrl+Shift+H가 터미널 찾기 띠를 열고 친 글자는 PTY로 가지 않으며 Esc가 닫는다. 띠가 닫힌 뒤 Ctrl+H는 PTY로 간다.
#   I27  (덩어리 24) 탐색기 셸이 하는 복사(`InvokeVerb('copy')`, CF_HDROP)로 클립보드에 올린 파일을 파일 목록 폴더 행을 누르고 실제 Ctrl+V로 받는다(원본 유지, 다시 붙이면 `copy`).
#   I28  (덩어리 24) 비트맵(CF_DIB)·PNG를 클립보드에 올리고 실제 Ctrl+V로 파일 목록(`image.png`)과 터미널(`pasted-images\paste-….png` 경로)에 붙인다(클립보드를 되돌린다).
#   I29  (덩어리 24) 탐색기에서 파일 목록·터미널로 실제 끌어 놓기(OLE). 사람이 끌어야 하므로 항상 SKIP이고 점검 방법만 결과에 적는다.
#   I30  (덩어리 24, 덩어리 25에 9P 대소문자 쌍 `MoveFileExW` 확인 추가) 파일 행을 누른 뒤 실제 F2로 이름을 바꾸고, 터미널에 키가 가지 않는다. 폴더 이름을 바꾼 뒤 두 단계 아래에 만든 파일이 1초 안에 목록에 나타나는지(Windows에서 §4 결함 재현 여부)를 결과에 적는다.
#   T12  (덩어리 25) 편집기에서 한글 조합 중 Enter가 마지막 글자와 함께 한 번만 확정되고 줄바꿈이 하나만 들어간다. 이어 실제 Ctrl+S로 저장하면 디스크에 `한글`+CRLF가 그대로 쓰인다.
#   I31  (덩어리 25) 실제 자판 Ctrl+S: 파일 탭이 활성이면 저장하고(키가 PTY로 안 감) 터미널이 활성이면 저장하지 않는다(파일은 그대로, 키는 셸의 것).
#   I32  (덩어리 25) 실제 자판 Ctrl+F·Ctrl+H·Ctrl+G: 파일 탭이 활성이면 찾기 패널·바꾸기 패널·줄로 가기 창이 열리고 PTY로 안 가며, 터미널이 활성이면 열리지 않고 Ctrl+F·Ctrl+H가 PTY로 간다.
#   T13  (덩어리 26) 검토 메모 입력창에서 한글 조합 중 Enter가 마지막 글자와 함께 한 번만 확정되어 메모 글이 `한글`이 된다(창이 닫히고 메모 1개).
#   I33  (덩어리 26) 실제 자판 Ctrl+Alt+M(파일 탭 활성)이 메모 창을 열고 Esc가 닫는다. 터미널이 활성이면 창이 열리지 않고 키가 셸로 간다. 입력 언어(한/영)를 결과에 적는다.
#   I34  (덩어리 27) 실제 자판 Ctrl+Shift+P: 터미널이 활성이어도(PTY 0)·편집기에 포커스가 있어도 빠른 열기가 열리고 Esc가 닫는다. 터미널 활성에서 평 Ctrl+P는 셸로 간다.
#   I35  (덩어리 27) 폴더 검색에서 바꾸기 줄을 열고 실제 마우스로 `Replace All`을 눌러 확인 창에서 Enter로 승인하면 결과의 파일이 디스크에서 바뀐다(좌표는 캡처에서 잰 값 — 빗나가면 CHECK).
#   I36  (덩어리 29) 사용자의 실제 Codex 로그인으로 `codex app-server`를 조회해 상태 줄 Codex 게이지가 값을 보인다(5h 또는 week). 읽기만 하지만 CLI가 토큰을 새로 고칠 수 있어 사람이 승인한 야간에만 돌린다. 캡처를 Electron 상태 줄과 눈으로 견준다(CHECK).
#   I37  (덩어리 30) 실제 마우스로 상태 줄 사용량 묶음을 눌러 목록을 열고 실제 Esc로 닫는다. `Open in new terminal`을 실제로 눌러 탭이 늘고 상태 줄 선택이 그 계정이 되는지는 캡처에서 잰 좌표로 누른다(빗나가면 CHECK).
#   I38  (덩어리 30) 기본 CLI 로그인 연결 이름이 Electron 상태 줄 이름과 같은지. 실제 계정 폴더·로그인 파일이 필요해 시험 홈으로는 못 하므로 항상 SKIP이고 사람이 실제 홈으로 한 번 견주는 방법만 결과에 적는다(앱은 읽기만 한다).
#   I39  (덩어리 31) 실제 WSL에 임시 부하를 걸어 Memory 사용률이 90%를 넘으면 경고 띠가 한 번 뜨고 `Go to`가 그 터미널로 가는지. 사용자 PC 메모리를 채우는 일이라 항상 SKIP이고 사람이 승인해 직접 하는 방법만 결과에 적는다.
#   I40  (덩어리 32) 실제 Windows node로 앱이 쓴 상태 줄 명령을 한 번 실행한다(가짜 입력 → 임시 홈의 사용량 파일). 이어 앱이 쓴 `flag-settings.json`을 실제 설정 폴더 경로로 바꿔 사용자의 실제 파일(읽기만)과 바이트를 견준다(다르면 CHECK).
#   T14  (덩어리 32) WSL 래퍼 묻기 창이 떠 있는 동안 한글 조합 중 Enter를 눌러도 기본 단추가 `Ask me later`라서 설정이 `ask` 그대로이고 임시 WSL 홈의 `.bashrc`에 줄이 없다(`PADDOCK_TEST_WSL_HOME`으로 WSL 쓰기를 임시 폴더에 가둔다).
#   I41  (덩어리 33) 실제 마우스로 탭 줄 오른쪽 계정 고르기를 눌러 `Continue this conversation with` 메뉴를 열고 항목을 눌러 같은 탭에서 다른 계정으로 대화를 이어 연다(가짜 claude, WSL 임시 홈 — `PADDOCK_TEST_WSL_HOME`). 메뉴·항목 좌표는 캡처에서 잰 값이라 빗나가면 CHECK다.
#   I42  (덩어리 33) 사용자의 실제 계정 두 개로 `claude --resume <전사 경로>` 교차 재개가 지금 Claude Code 판에서도 되는지. 실제 계정 폴더에 쓰므로 자동으로 돌리지 않고 항상 SKIP이며 사람이 1회 확인하는 방법만 결과에 적는다.
#   OPEN 기본 앱 열기: 쉼표·공백·한글이 든 경로의 미디어 파일을 안내 탭의 `Open with default app`으로 열면 그 파일을 쓰는 앱 창이 뜬다.
# 판정: PASS / FAIL(기능이 틀림) / CHECK(좌표가 빗나갔거나 눈으로 봐야 함 — 캡처가 항목 폴더에 남는다) / SKIP. 단추·경계 좌표는 기본 창 크기(1552폭)의 캡처에서 잰
# 값을 창 폭(논리 크기는 앱이 알리는 body 폭 + 289)에 맞춰 늘려 쓴다. 창 모양이 다르면 CHECK로 남고 좌표 상수(아래 $G)를 고친다.
# -PrepareOnly: 앱을 띄우지 않고 시험 자료와 앱 대본(HomeDir\<시험>\night.script)만 만든다(구문·대본 점검용, 입력을 보내지 않는다).
# 안전 규칙: input-run.ps1과 같다 — 이 스크립트가 띄운 앱 창이 전경일 때만 입력을 보내고, 실제 사용자 폴더를 거부하며, 다른 paddock-daemon이 있으면 시작하지 않고,
# 끝나면 이 스크립트가 띄운 프로세스(앱·데몬·가짜 claude·보조 창·시험용 수신기)만 종료하고 한/영 상태와 클립보드(글자)를 되돌린다.
# 시험 자료는 HomeDir 아래에만 만들고(I12만 WSL /tmp/pdk-night-<id> 하나, 끝에 지움), 실제 사용자 폴더·설정은 쓰지 않는다.
param(
    [string]$Exe,
    [string]$HomeDir,
    [string]$Out,
    [string[]]$Tests = @('I6'),
    [int]$N = 10,
    [switch]$PrepareOnly
)
Add-Type -ReferencedAssemblies System.Drawing,System.Windows.Forms -TypeDefinition @'
using System; using System.Drawing; using System.Drawing.Imaging; using System.Runtime.InteropServices;
public class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct CURSORINFO { public int cbSize; public int flags; public IntPtr hCursor; public int x, y; }
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
  [DllImport("user32.dll")] public static extern bool GetCursorInfo(ref CURSORINFO c);
  [DllImport("user32.dll")] public static extern IntPtr LoadCursor(IntPtr inst, int id);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("imm32.dll")] public static extern IntPtr ImmGetDefaultIMEWnd(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static IntPtr T; public static int Fail = 0;
  // 전경이 시험 창이 아니면 입력을 보내지 않고 실패로 센다.
  public static void Key(ushort vk, ushort scan, uint flags) {
    if (GetForegroundWindow() != T) { Fail++; return; }
    INP[] a = new INP[1]; a[0].type = 1; a[0].vk = vk; a[0].sc = scan; a[0].fl = flags; SendInput(1, a, 40); }
  public static void Tap(ushort vk) { Key(vk, 0, 0); System.Threading.Thread.Sleep(20); Key(vk, 0, 2); }
  public static void Down(ushort vk) { Key(vk, 0, 0); System.Threading.Thread.Sleep(30); }
  public static void Up(ushort vk) { Key(vk, 0, 2); System.Threading.Thread.Sleep(30); }
  public static void Chord(ushort[] mods, ushort vk) {
    foreach (ushort m in mods) { Key(m, 0, 0); System.Threading.Thread.Sleep(15); }
    Tap(vk); System.Threading.Thread.Sleep(15);
    for (int i = mods.Length - 1; i >= 0; i--) { Key(mods[i], 0, 2); System.Threading.Thread.Sleep(15); } }
  // 글자를 유니코드 입력으로 보낸다(키보드 배열·한/영 상태에 상관없이 같은 글자가 들어간다).
  public static void Text(string s) {
    foreach (char c in s) { Key(0, c, 4); System.Threading.Thread.Sleep(10); Key(0, c, 6); System.Threading.Thread.Sleep(10); } }
  static void Btn(uint flag, int data) {
    if (GetForegroundWindow() != T) { Fail++; return; }
    MINP[] a = new MINP[1]; a[0].type = 0; a[0].fl = flag; a[0].data = unchecked((uint)data); SendMouse(1, a, 40); }
  public static void Move(int x, int y) { SetCursorPos(x, y); System.Threading.Thread.Sleep(80); }
  public static void Click(int x, int y) { SetCursorPos(x, y); System.Threading.Thread.Sleep(80); Btn(0x2, 0); System.Threading.Thread.Sleep(40); Btn(0x4, 0); }
  public static void RightClick(int x, int y) { SetCursorPos(x, y); System.Threading.Thread.Sleep(80); Btn(0x8, 0); System.Threading.Thread.Sleep(40); Btn(0x10, 0); }
  public static void DoubleClick(int x, int y) { Click(x, y); System.Threading.Thread.Sleep(90); Btn(0x2, 0); System.Threading.Thread.Sleep(40); Btn(0x4, 0); }
  public static void Drag(int x1, int y1, int x2, int y2) {
    SetCursorPos(x1, y1); System.Threading.Thread.Sleep(120); Btn(0x2, 0);
    for (int i = 1; i <= 14; i++) { SetCursorPos(x1 + (x2 - x1) * i / 14, y1 + (y2 - y1) * i / 14); System.Threading.Thread.Sleep(25); }
    System.Threading.Thread.Sleep(80); Btn(0x4, 0); }
  // 끌다가 중간에서 Esc를 누르고 끝까지 끈 뒤 뗀다(덩어리 37 T16). 뗄 때 아무 일도 없어야 한다.
  public static void DragEsc(int x1, int y1, int x2, int y2) {
    SetCursorPos(x1, y1); System.Threading.Thread.Sleep(120); Btn(0x2, 0);
    for (int i = 1; i <= 14; i++) { SetCursorPos(x1 + (x2 - x1) * i / 14, y1 + (y2 - y1) * i / 14); System.Threading.Thread.Sleep(25); if (i == 7) { Tap(0x1B); System.Threading.Thread.Sleep(60); } }
    System.Threading.Thread.Sleep(80); Btn(0x4, 0); }
  // 휠 한 칸은 120이다. 양수는 위로, 음수는 아래로 굴린다.
  public static void Wheel(int x, int y, int notches) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(80);
    int step = notches < 0 ? -120 : 120;
    for (int i = 0; i < Math.Abs(notches); i++) { Btn(0x800, step); System.Threading.Thread.Sleep(30); } }
  public static bool HandCursor() {
    CURSORINFO c = new CURSORINFO(); c.cbSize = Marshal.SizeOf(typeof(CURSORINFO)); GetCursorInfo(ref c);
    return c.hCursor == LoadCursor(IntPtr.Zero, 32649); }
  public static bool Focus(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    uint p; uint ft = GetWindowThreadProcessId(GetForegroundWindow(), out p); uint me = GetCurrentThreadId();
    ShowWindow(h, 9); AttachThreadInput(me, ft, true); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(me, ft, false);
    System.Threading.Thread.Sleep(200); return GetForegroundWindow() == h; }
  public static long ConvMode(IntPtr h) { IntPtr ime = ImmGetDefaultIMEWnd(h); return SendMessage(ime, 0x283, (IntPtr)1, IntPtr.Zero).ToInt64(); }
  // 창 전체(제목 줄 포함)를 그림으로 찍는다. 자동 점검 실행기와 같은 PrintWindow다.
  public static Bitmap Grab(IntPtr h) {
    RECT r; GetWindowRect(h, out r); Bitmap b = new Bitmap(r.R - r.L, r.B - r.T);
    Graphics g = Graphics.FromImage(b); IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 2); g.ReleaseHdc(dc); g.Dispose(); return b; }
}
// 캡처 그림의 픽셀 분석(밝기·교대·기울기). 사각형은 그림 픽셀 좌표다.
public class Px {
  static byte[] Bytes(Bitmap b, out int stride) {
    BitmapData d = b.LockBits(new Rectangle(0, 0, b.Width, b.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
    stride = d.Stride; byte[] a = new byte[stride * b.Height]; Marshal.Copy(d.Scan0, a, 0, a.Length); b.UnlockBits(d); return a; }
  static Rectangle Clip(Bitmap b, int x, int y, int w, int h) { return Rectangle.Intersect(new Rectangle(0, 0, b.Width, b.Height), new Rectangle(x, y, w, h)); }
  // 두 그림이 사각형 안에서 서로 다른 픽셀 수
  public static int Diff(Bitmap a, Bitmap b, int x, int y, int w, int h) {
    Rectangle r = Clip(a, x, y, w, h); r = Rectangle.Intersect(r, new Rectangle(0, 0, b.Width, b.Height));
    int sa, sb; byte[] pa = Bytes(a, out sa), pb = Bytes(b, out sb); int n = 0;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i < r.Right; i++) {
      int oa = j * sa + i * 4, ob = j * sb + i * 4;
      if (Math.Abs(pa[oa] - pb[ob]) + Math.Abs(pa[oa + 1] - pb[ob + 1]) + Math.Abs(pa[oa + 2] - pb[ob + 2]) > 24) n++; }
    return n; }
  // {어두운 픽셀(밝기 < 100), 흰 픽셀(밝기 >= 245), 전체}
  public static int[] Count(Bitmap a, int x, int y, int w, int h) {
    Rectangle r = Clip(a, x, y, w, h); int s; byte[] p = Bytes(a, out s); int dark = 0, white = 0;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i < r.Right; i++) {
      int o = j * s + i * 4; int v = (p[o] + p[o + 1] + p[o + 2]) / 3; if (v < 100) dark++; else if (v >= 245) white++; }
    return new int[] { dark, white, r.Width * r.Height }; }
  // 가로로 이웃한 두 픽셀이 모두 순수 흑(<= 20)·순수 백(>= 235)인 쌍의 수와, 그중 서로 색이 다른 쌍의 수
  public static int[] Alternation(Bitmap a, int x, int y, int w, int h) {
    Rectangle r = Clip(a, x, y, w, h); int s; byte[] p = Bytes(a, out s); int pairs = 0, alt = 0;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i + 1 < r.Right; i++) {
      int o = j * s + i * 4, q = o + 4; int va = (p[o] + p[o + 1] + p[o + 2]) / 3, vb = (p[q] + p[q + 1] + p[q + 2]) / 3;
      bool pa = va <= 20 || va >= 235, pb = vb <= 20 || vb >= 235;
      if (pa && pb) { pairs++; if ((va <= 20) != (vb <= 20)) alt++; } }
    return new int[] { pairs, alt }; }
  // 가로로 이웃한 픽셀 밝기 차이의 상위 1 %값(0~255). 글자 가장자리가 또렷할수록 크다.
  public static int GradientP99(Bitmap a, int x, int y, int w, int h) {
    Rectangle r = Clip(a, x, y, w, h); int s; byte[] p = Bytes(a, out s); int[] hist = new int[256]; int total = 0;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i + 1 < r.Right; i++) {
      int o = j * s + i * 4, q = o + 4; int d = Math.Abs((p[o] + p[o + 1] + p[o + 2]) / 3 - (p[q] + p[q + 1] + p[q + 2]) / 3); if (d > 8) { hist[d]++; total++; } }
    int cut = total / 100, acc = 0; for (int v = 255; v >= 0; v--) { acc += hist[v]; if (acc > cut) return v; } return 0; }
  // 글자 획 픽셀의 바깥 사각형: 어두운 픽셀(밝기 < 100) 양옆 6픽셀 안에 흰 픽셀(>= 245)이 모두 있는 것(종이 위의 글자 획). 바탕의 어두운 띠·종이 가장자리는 세지 않는다. 없으면 null.
  public static Rectangle? TextBounds(Bitmap a, int x, int y, int w, int h, out int count) {
    Rectangle r = Clip(a, x, y, w, h); int s; byte[] p = Bytes(a, out s); int minX = int.MaxValue, minY = int.MaxValue, maxX = -1, maxY = -1; count = 0;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i < r.Right; i++) {
      int o = j * s + i * 4; if ((p[o] + p[o + 1] + p[o + 2]) / 3 >= 100) continue;
      bool left = false, right = false;
      for (int k = 1; k <= 6 && !left; k++) { if (i - k < r.Left) break; int q = j * s + (i - k) * 4; if ((p[q] + p[q + 1] + p[q + 2]) / 3 >= 245) left = true; }
      for (int k = 1; k <= 6 && !right; k++) { if (i + k >= r.Right) break; int q = j * s + (i + k) * 4; if ((p[q] + p[q + 1] + p[q + 2]) / 3 >= 245) right = true; }
      if (left && right) { count++; if (i < minX) minX = i; if (i > maxX) maxX = i; if (j < minY) minY = j; if (j > maxY) maxY = j; } }
    if (maxX < 0) return null; return new Rectangle(minX, minY, maxX - minX + 1, maxY - minY + 1); }
  // 가장 왼쪽 위의 어두운 픽셀 묶음 자리(사각형 안). 없으면 null.
  public static Rectangle? FirstDark(Bitmap a, int x, int y, int w, int h) {
    Rectangle r = Clip(a, x, y, w, h); int s; byte[] p = Bytes(a, out s); int minX = int.MaxValue, minY = int.MaxValue, maxX = -1, maxY = -1;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i < r.Right; i++) {
      int o = j * s + i * 4; if ((p[o] + p[o + 1] + p[o + 2]) / 3 < 100) { if (i < minX) minX = i; if (i > maxX) maxX = i; if (j < minY) minY = j; if (j > maxY) maxY = j; } }
    if (maxX < 0) return null; return new Rectangle(minX, minY, maxX - minX + 1, maxY - minY + 1); }
  // 순수 흑(<= 5)·순수 백(>= 250) 픽셀이 있는 자리의 바깥 사각형. 없으면 null.
  public static Rectangle? PureBounds(Bitmap a, int x, int y, int w, int h) {
    Rectangle r = Clip(a, x, y, w, h); int s; byte[] p = Bytes(a, out s); int minX = int.MaxValue, minY = int.MaxValue, maxX = -1, maxY = -1;
    for (int j = r.Top; j < r.Bottom; j++) for (int i = r.Left; i < r.Right; i++) {
      int o = j * s + i * 4; int v = (p[o] + p[o + 1] + p[o + 2]) / 3; if (v <= 5 || v >= 250) { if (i < minX) minX = i; if (i > maxX) maxX = i; if (j < minY) minY = j; if (j > maxY) maxY = j; } }
    if (maxX < 0) return null; return new Rectangle(minX, minY, maxX - minX + 1, maxY - minY + 1); }
}
'@
[void][W]::SetProcessDPIAware()
New-Item -ItemType Directory -Force $Out | Out-Null
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required (omitting it would run with the real user profile)" }
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
if (-not $PrepareOnly) {
    $running = Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue
    if ($running) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
}
New-Item -ItemType Directory -Force $HomeDir | Out-Null
. (Join-Path $PSScriptRoot 'test-env.ps1')
$resultPath = Join-Path $Out "result-$($Tests -join '-').txt"
function Say($m) { $m | Out-File $resultPath -Append -Encoding utf8 }

# 좌표 상수: 기본 창 크기(1552폭) 캡처에서 잰 값(프레임 px). 가로는 창 왼쪽, 세로는 창 위쪽 기준이고 `*R`은 창 오른쪽 기준 거리다.
$G = @{
    FilesHeaderBase = 153; TerminalRowStep = 28; FirstRowOffset = 28; RowStep = 26; FileNameX = 80   # 파일 목록: 머리줄 y = 153 + 28 × 터미널 행 수, 첫 행 = 머리 + 28, 행 간격 26
    MenuOpenDx = 40; MenuOpenDy = 27                                                                  # 파일 행 우클릭 메뉴의 Open 항목(눌린 자리 기준)
    BodyLeft = 281; BodyExtra = 289; PaneTop = 70; PaneBottomMargin = 30; PaneRightMargin = 14        # 본문 왼쪽·창 논리 폭 = body 폭 + 289·파일 칸 위·아래·오른쪽 여백
    TabsY = 49; MdFileR = 382; MdPreviewR = 338; MdBothR = 292                                         # 탭 줄 y와 Markdown 단추 중심(창 오른쪽에서의 거리, 저장소 폴더라 브랜치 표시가 있는 배치)
    PdfMinusR = 427; PdfPlusR = 402; PdfDefaultR = 325                                                 # PDF 탭 줄 −·+·Open with default app 중심(같은 배치)
    ScmMessage = @(145, 150); ScmCommit = @(148, 210); ScmStageAll = @(95, 225)                        # Source control 입력칸·Commit 단추(스테이징 뒤)·Changes 머리의 모두 스테이징(+)
    ScmChangesRow1Y = 247; ScmStagedRow1Y = 263; ScmActionX = 266                                      # 변경만 있을 때 첫 행 y·스테이징 뒤 Staged 첫 행 y·행 호버 단추(+) 중심
    SearchInputDy = 35; SearchOptCaseX = 213; SearchFirstFileDy = 116; SearchMatchStep = 22; SearchInputX = 100
    TermLinkRowY = 103; TermTextLeft = 20.5; TermCellWidth = 7.9                      # 터미널 글자 칸(둘째 열 본문 왼쪽 = 본문 가운데 + 20)
    MdLinkY = @(175, 213, 251); MdParagraphY = 137                                                           # Markdown 미리보기: 링크 셋의 y와 본문 문단 y(보정 값)
    WorkRowX = 100; WorkFirstRowY = 148; TermSelectY = 121    # Work 터미널 행(첫 터미널 행 중심 y·행 간격은 TerminalRowStep)·터미널에 찍힌 한 줄 y(보정 값)
    NoticeDefaultDx = 73; NoticeDefaultDy = 45                                                          # 안내 탭의 `Open with default app` 단추(파일 칸 가운데 기준, 보정 값)
}

# -- 시험 자료·앱 대본 ----------------------------------------------------------------------------------------------------------------
function Escape-Script($text) { $text -replace '\\', '\\' }   # 대본 `send`는 역슬래시를 풀이하므로 두 번 쓴다.
function ForSlash($path) { $path -replace '\\', '/' }

function New-Fixture($root, $port) {
    $proj = Join-Path $root 'proj'
    foreach ($dir in @('src', 'img', 'media')) { New-Item -ItemType Directory -Force (Join-Path $proj $dir) | Out-Null }
    Set-Content (Join-Path $proj 'a.txt') 'one'
    Set-Content (Join-Path $proj 'src\b.txt') 'two'
    Set-Content (Join-Path $proj 'src\a.js') 'const answer = 42;'
    Set-Content (Join-Path $proj 'notes.txt') (1..200 | ForEach-Object { "notes line $_" })
    Set-Content (Join-Path $proj 'a-first.txt') (1..30 | ForEach-Object { if ($_ -eq 12) { 'the needle is on line twelve' } else { "line $_ of a-first" } })
    Set-Content (Join-Path $proj 'z-big.txt') (1..2500 | ForEach-Object { "line needle $_" })
    $readme = @(
        '# Night Markdown', '',
        'Paragraph text for selecting: the quick brown fox jumps over the lazy dog.', '',
        "[Open web link](http://127.0.0.1:$port/web-link)", '',
        '[Open file link](file:///C:/Windows/win.ini)', '',
        '[Jump to section](#section)', '',
        '## Section', '', 'Second section.')
    Set-Content (Join-Path $proj 'README.md') $readme -Encoding UTF8
    Add-Type -AssemblyName System.Drawing
    # 맞춤·더블클릭 확인용 네 색 그림(400×300)과, 1:1 픽셀 대응 확인용 1픽셀 흑백 격자(400×300)
    $pic = New-Object System.Drawing.Bitmap 400, 300
    $g = [System.Drawing.Graphics]::FromImage($pic)
    $g.FillRectangle([System.Drawing.Brushes]::IndianRed, 0, 0, 200, 150); $g.FillRectangle([System.Drawing.Brushes]::SteelBlue, 200, 0, 200, 150)
    $g.FillRectangle([System.Drawing.Brushes]::SteelBlue, 0, 150, 200, 150); $g.FillRectangle([System.Drawing.Brushes]::MediumSeaGreen, 200, 150, 200, 150)
    $g.Dispose(); $pic.Save((Join-Path $proj 'img\pic.png'), [System.Drawing.Imaging.ImageFormat]::Png); $pic.Dispose()
    $grid = New-Object System.Drawing.Bitmap 400, 300
    for ($y = 0; $y -lt 300; $y++) { for ($x = 0; $x -lt 400; $x++) { $grid.SetPixel($x, $y, $(if ((($x + $y) % 2) -eq 0) { [System.Drawing.Color]::White } else { [System.Drawing.Color]::Black })) } }
    $grid.Save((Join-Path $proj 'img\grid.png'), [System.Drawing.Imaging.ImageFormat]::Png); $grid.Dispose()
    Copy-Item (Join-Path $PSScriptRoot 'assets\sample-3p.pdf') (Join-Path $proj 'sample-3p.pdf')
    # 쉼표·공백·한글이 든 파일 이름의 미디어 파일: 8 kHz 8비트 모노 무음 0.25초 WAV
    $samples = New-Object byte[] 2000; for ($i = 0; $i -lt $samples.Length; $i++) { $samples[$i] = 128 }
    $wav = New-Object System.Collections.Generic.List[byte]
    $wav.AddRange([Text.Encoding]::ASCII.GetBytes('RIFF')); $wav.AddRange([BitConverter]::GetBytes([int](36 + $samples.Length)))
    $wav.AddRange([Text.Encoding]::ASCII.GetBytes('WAVEfmt ')); $wav.AddRange([BitConverter]::GetBytes([int]16)); $wav.AddRange([BitConverter]::GetBytes([int16]1)); $wav.AddRange([BitConverter]::GetBytes([int16]1))
    $wav.AddRange([BitConverter]::GetBytes([int]8000)); $wav.AddRange([BitConverter]::GetBytes([int]8000)); $wav.AddRange([BitConverter]::GetBytes([int16]1)); $wav.AddRange([BitConverter]::GetBytes([int16]8))
    $wav.AddRange([Text.Encoding]::ASCII.GetBytes('data')); $wav.AddRange([BitConverter]::GetBytes([int]$samples.Length)); $wav.AddRange($samples)
    [IO.File]::WriteAllBytes((Join-Path $proj 'media\소리, 시험 file.wav'), $wav.ToArray())
    # 저장소: 모두 한 번 커밋한 뒤 변경 둘(수정)과 새 파일 하나(미추적)를 만든다 → Source control 변경 3.
    & git -C $proj init -q -b main 2>$null
    & git -C $proj config user.name Tester 2>$null; & git -C $proj config user.email t@example.com 2>$null
    & git -C $proj add -A 2>$null; & git -C $proj commit -q -m base 2>$null
    Set-Content (Join-Path $proj 'a.txt') 'changed'; Set-Content (Join-Path $proj 'src\b.txt') 'changed2'; Set-Content (Join-Path $proj 'new.txt') 'fresh'
    return $proj
}

# 앱 대본: 시험 자료 폴더로 들어가 가짜 claude를 돌려 폴더 묶음·파일 목록이 서게 한 뒤 $extra를 실행하고 `night-ready` 표지를 남기고 기다린다.
function Build-Script($proj, $extra, $wslRoot) {
    if ($wslRoot) {
        $lines = @(
            'waittabs 1 @30000',
            "send unset HISTFILE; rm -rf $wslRoot && mkdir -p $wslRoot/proj/src && cd $wslRoot/proj && git init -q -b main && git config user.name Tester && git config user.email t@example.com && echo one > a.js && echo two > src/b.js && git add -A && git commit -q -m base && echo x >> a.js && ln -sf /bin/bash $wslRoot/claude && export PATH=${wslRoot}:`$PATH\r",
            'send echo READY-NIGHT\r', 'waitline READY-NIGHT @30000',
            "send claude -c 'read -t 900'\r", 'waitstate program=claude @20000', 'waitstate row folder @20000', 'waitstate files-loaded=true @15000', 'waitstate scm state=ready @30000')
    } else {
        $p = Escape-Script $proj
        $lines = @(
            'waittabs 1 @30000',
            'waitfor PS @30000', 'wait 800',
            "send `$p = '$p'; Copy-Item C:\\Windows\\System32\\PING.EXE `"`$env:USERPROFILE\\bin\\claude.exe`" -Force; cd `$p\r",
            'send echo READY-NIGHT\r', 'waitline READY-NIGHT @30000',
            'send claude.exe -n 3000 127.0.0.1\r', 'waitstate program=claude @20000', 'waitstate row folder @20000', 'waitstate files-loaded=true @15000')
    }
    $lines += $extra
    $lines += @('wait 1500', 'mark night-ready', 'wait 3000000')
    return ($lines -join "`n") + "`n"
}

# 키가 PTY로 가는지 보는 둘째 터미널: 받은 키를 파일에 한 줄씩 남기는 읽기 루프(PTY 바이트가 0이면 파일이 비어 있다).
function Logger-Lines($keysLog) {
    return @('key ctrl-shift-c', 'waittabs 2 @20000', 'wait 1500',
        "send while (`$true) { `$k = [Console]::ReadKey(`$true); Add-Content -LiteralPath '$(Escape-Script $keysLog)' -Value ('GOT-' + [int]`$k.Key + ' ' + [int]`$k.Modifiers) }\r", 'wait 1500')
}
function ProjPath($name) { return (Join-Path (Join-Path $HomeDir $name) 'proj') }   # Start-Case 전에도 시험 자료 경로를 알아야 하는 대본 줄이 쓴다.
function OpenPath-Lines($path, $waitFor) { return @("act openpath $(ForSlash $path)", "waitstate $waitFor @15000", 'wait 1500') }

# -- 시험 한 건의 생명주기 -------------------------------------------------------------------------------------------------------------
$script:proc = $null; $script:h = [IntPtr]::Zero; $script:caseOut = ''; $script:caseHome = ''; $script:proj = ''; $script:keys = ''
$script:wslRoot = ''; $script:conv0 = $null; $script:helper = $null; $script:listener = $null; $script:fr = $null
$script:clip0 = $null; $script:clipSaved = $false

function State { Get-Content (Join-Path $script:caseOut 'state-live.txt') -Raw -Encoding UTF8 -ErrorAction SilentlyContinue }
function StateLines { return @((State) -split "`r?`n") }
function WaitState($pattern, $seconds = 5) {
    for ($i = 0; $i -lt ($seconds * 5); $i++) { if ((State) -match $pattern) { return $true }; Start-Sleep -Milliseconds 200 }
    return $false
}
function Ensure { for ($i = 0; $i -lt 5; $i++) { if ([W]::Focus($script:h)) { return $true }; Start-Sleep -Milliseconds 300 }; return $false }
function KeyLines { if (Test-Path $script:keys) { return @(Get-Content $script:keys) } else { return @() } }
function Shot($name) { $b = [W]::Grab($script:h); $b.Save((Join-Path $script:caseOut "$name.png")); return $b }
function SetHangul($on) {
    $isOn = ([W]::ConvMode($script:h) -band 1) -eq 1
    if ($isOn -ne $on) { [W]::Tap(0x15); Start-Sleep -Milliseconds 250 }
    return ([W]::ConvMode($script:h) -band 1) -eq 1
}
function Typ($codes) { foreach ($k in $codes) { [W]::Tap($k); Start-Sleep -Milliseconds (Get-Random -Min 30 -Max 50) } }
$HG = @(0x47, 0x4B, 0x53, 0x52, 0x4D, 0x46)   # ㅎ ㅏ ㄴ ㄱ ㅡ ㄹ
$VK_CONTROL = 0xA2; $VK_SHIFT = 0xA0; $VK_ENTER = 0x0D; $VK_ESC = 0x1B

# 프레임 → 화면 좌표. 배율은 창 논리 폭(body 폭 + 289) 대 실제 창 폭이다.
function Get-Frame {
    $rect = New-Object W+RECT; [void][W]::GetWindowRect($script:h, [ref]$rect)
    $scale = 1.0
    $total = BodyTotal
    if ($total -gt 0) { $scale = ($rect.R - $rect.L) / ($total + $G.BodyExtra) }
    $script:fr = @{ L = $rect.L; T = $rect.T; R = $rect.R; B = $rect.B; S = $scale }
}
function Sx($x) { return [int]($script:fr.L + $x * $script:fr.S) }
function Sy($y) { return [int]($script:fr.T + $y * $script:fr.S) }
function SxR($offset) { return [int]($script:fr.R - $offset * $script:fr.S) }
# 앱이 알리는 body는 터미널 열들의 영역이다(파일 칸이 열려 있으면 그 폭을 뺀 값). 전체 본문 폭 = body ÷ (1 − 파일 칸 비율).
function BodySize { if ((State) -match '(?m)^body: (\d+)x(\d+)') { return @([double]$Matches[1], [double]$Matches[2]) } else { return @(1263.0, 1004.0) } }
function FileRatio { if ((State) -match 'file-pane ratio=(\d\.\d+)') { return [double]$Matches[1] } else { return 0.5 } }
function HasFilePane { return [bool]((State) -match 'file-pane ratio=\S+ tabs=[1-9]') }
function BodyTotal { $b = BodySize; if (HasFilePane) { return $b[0] / (1 - (FileRatio)) } else { return $b[0] } }
function PaneLeft { $b = BodySize; return $G.BodyLeft + $b[0] }   # 파일 칸 왼쪽 경계(프레임 px)
function HeaderY { $n = ([regex]::Matches((State), '(?m)^row terminal ')).Count; return $G.FilesHeaderBase + $G.TerminalRowStep * $n }
function RowY($name) {
    $rows = @(StateLines | Where-Object { $_ -match '^file-row ' })
    for ($i = 0; $i -lt $rows.Count; $i++) { if ($rows[$i] -match " $([regex]::Escape($name))( expanded)?$") { return (HeaderY) + $G.FirstRowOffset + $G.RowStep * $i } }
    return -1
}
# 파일 칸 본문 사각형(그림 픽셀): x, y, 폭, 높이
function PaneRect {
    $b = BodySize
    $x0 = (PaneLeft) + 2; $x1 = $G.BodyLeft + (BodyTotal) - $G.PaneRightMargin
    $y0 = $G.PaneTop; $y1 = 66 + $b[1] - $G.PaneBottomMargin
    $s = $script:fr.S
    return @([int]($x0 * $s), [int]($y0 * $s), [int](($x1 - $x0) * $s), [int](($y1 - $y0) * $s))
}
function PaneCenter { $b = BodySize; return @((((PaneLeft) + $G.BodyLeft + (BodyTotal)) / 2), (66 + $b[1] / 2)) }
function NewWindowsSince($before) { return @(Get-Process | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero -and $before -notcontains $_.Id } | ForEach-Object { "{0}|{1}|{2}" -f $_.Id, $_.ProcessName, $_.MainWindowTitle }) }

function Start-Case($name, $extra, [bool]$useWsl = $false, [bool]$logger = $false) {
    $script:caseHome = Join-Path $HomeDir $name
    $script:caseOut = Join-Path $Out $name
    New-Item -ItemType Directory -Force $script:caseOut | Out-Null
    Use-TestHome $script:caseHome
    $native = Join-Path $script:caseHome '.paddock\native'
    New-Item -ItemType Directory -Force $native | Out-Null
    if (-not $useWsl) { [IO.File]::WriteAllText((Join-Path $native 'settings.json'), '{ "terminal.integrated.defaultProfile.windows": "PowerShell" }') }
    $port = if ($script:listener) { $script:listener.LocalEndpoint.Port } else { 9 }
    $script:proj = New-Fixture $script:caseHome $port
    $script:keys = Join-Path $script:caseOut 'keys.log'
    Remove-Item $script:keys -ErrorAction SilentlyContinue
    $lines = @()
    if ($logger) { $lines += Logger-Lines $script:keys }
    $lines += $extra
    $script:wslRoot = if ($useWsl) { '/tmp/pdk-night-' + [Guid]::NewGuid().ToString('N').Substring(0, 8) } else { '' }
    $scriptPath = Join-Path $script:caseHome 'night.script'
    [IO.File]::WriteAllText($scriptPath, (Build-Script $script:proj $lines $script:wslRoot), (New-Object Text.UTF8Encoding($false)))
    if ($PrepareOnly) { Say "$name prepared: $scriptPath proj=$($script:proj)"; return $false }
    $script:proc = Start-Process -FilePath $Exe -ArgumentList "--script `"$scriptPath`" --out `"$($script:caseOut)`"" -PassThru
    for ($i = 0; $i -lt 100 -and $script:proc.MainWindowHandle -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $script:proc.Refresh() }
    $script:h = $script:proc.MainWindowHandle
    $windowPid = [uint32]0; [void][W]::GetWindowThreadProcessId($script:h, [ref]$windowPid)
    if ($script:h -eq [IntPtr]::Zero -or $windowPid -ne $script:proc.Id -or $script:proc.MainWindowTitle -ne 'Paddock') { Say "$name NO WINDOW (handle=$($script:h) pid=$windowPid)"; return $false }
    [W]::T = $script:h
    # 대본이 끝(night-ready)에 닿을 때까지 기다린다.
    $events = Join-Path $script:caseOut 'events.log'
    $ready = $false
    for ($i = 0; $i -lt 600 -and -not $ready; $i++) { if ((Test-Path $events) -and (Select-String -Path $events -Pattern 'night-ready' -Quiet)) { $ready = $true } else { Start-Sleep -Milliseconds 200 } }
    if (-not $ready) { Say "$name SETUP-TIMEOUT (the app script never reached night-ready; see $($script:caseOut))"; return $false }
    Start-Sleep -Milliseconds 800
    if (-not (Ensure)) { Say "$name SKIP (could not bring the test window to the foreground)"; return $false }
    $script:conv0 = [W]::ConvMode($script:h) -band 1
    [W]::Fail = 0
    Get-Frame
    return $true
}

function Stop-Case {
    if ($script:conv0 -ne $null -and $script:h -ne [IntPtr]::Zero) { try { [void](SetHangul ($script:conv0 -eq 1)) } catch {} }
    $script:conv0 = $null
    if ($script:helper -and -not $script:helper.HasExited) { Stop-Process -Id $script:helper.Id -Force -ErrorAction SilentlyContinue }
    $script:helper = $null
    $ids = @()
    if ($script:proc) { $ids += $script:proc.Id }
    $info = if ($script:caseHome) { Join-Path $script:caseHome '.paddock\native\daemon.json' } else { '' }
    if ($info -and (Test-Path $info)) { try { $d = (Get-Content $info -Raw | ConvertFrom-Json).pid; if ($d) { $ids += $d } } catch {} }
    foreach ($id in $ids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    # 가짜 claude(PING.EXE 복사본)는 이 시험의 bin 폴더에서 뜬 것만 끝낸다.
    if ($script:caseHome) {
        Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith((Join-Path $script:caseHome 'bin'), [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
    }
    if ($script:wslRoot) { & wsl.exe -e rm -rf $script:wslRoot 2>$null; $script:wslRoot = '' }
    if ($script:clipSaved) { try { if ($script:clip0) { Set-Clipboard -Value $script:clip0 } else { Set-Clipboard -Value $null } } catch {}; $script:clipSaved = $false }
    if ($script:listener) { try { $script:listener.Stop() } catch {}; $script:listener = $null }
    Say "cleanup: stopped $($ids -join ',')"
    $script:proc = $null; $script:h = [IntPtr]::Zero
}

# -- 시험 ------------------------------------------------------------------------------------------------------------------------------
function Verdict($test, $failures, $checks, $detail) {
    # failures: 기능이 틀림, checks: 좌표가 빗나갔을 수 있거나 눈으로 봐야 함.
    $word = if ($failures.Count -gt 0) { 'FAIL' } elseif ($checks.Count -gt 0) { 'CHECK' } else { 'PASS' }
    $notes = @($failures + $checks) -join '; '
    Say ("$test $word" + $(if ($notes) { " [$notes]" } else { '' }) + " $detail inputFailures=$([W]::Fail)")
}
$script:requests = @()
function Pump-Listener {
    while ($script:listener -and $script:listener.Pending()) {
        $c = $script:listener.AcceptTcpClient(); $c.ReceiveTimeout = 1500
        try {
            $s = $c.GetStream(); $buf = New-Object byte[] 2048; $n = $s.Read($buf, 0, 2048)
            $script:requests += ([Text.Encoding]::ASCII.GetString($buf, 0, $n) -split "`r?`n")[0]
            $resp = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 200 OK`r`nContent-Type: text/plain`r`nContent-Length: 2`r`nConnection: close`r`n`r`nok"); $s.Write($resp, 0, $resp.Length)
        } catch {}
        $c.Close()
    }
}
function StartHelper {
    $p = Start-Process powershell -ArgumentList '-NoProfile', '-Command', 'Add-Type -AssemblyName System.Windows.Forms; $f = New-Object Windows.Forms.Form; $f.Text = "night-helper"; $f.Width = 300; $f.Height = 150; [Windows.Forms.Application]::Run($f)' -PassThru
    for ($i = 0; $i -lt 100 -and ($p.Refresh() -or $p.MainWindowHandle -eq [IntPtr]::Zero); $i++) { Start-Sleep -Milliseconds 200 }
    return $p
}

# 사용량 시험 자료: 임시 홈 `$root`에 가짜 Claude 계정 둘과 기본 Claude 기록·로그인(첫 계정과 같음), 가짜 claude.exe를 만든다.
function New-UsageFixture($root) {
    $config = Join-Path $root '.paddock\config'
    $now = [int][double]::Parse((Get-Date -UFormat %s)); $five = $now + 18000; $week = $now + 259200
    $names = @('Alpha One', 'Beta Two'); $ids = @('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2')
    New-Item -ItemType Directory -Force (Join-Path $config 'usage'), (Join-Path $root 'bin'), (Join-Path $root '.paddock\native') | Out-Null
    Set-Content (Join-Path $config 'usage\state.json') '{"claude":"on"}'
    Set-Content (Join-Path $config 'usage\claude.json') ('{"rate_limits":{"five_hour":{"used_percentage":23,"resets_at":' + $five + '},"seven_day":{"used_percentage":61,"resets_at":' + $week + '}},"updated_at":' + $now + '}')
    Set-Content (Join-Path $root '.claude.json') '{"oauthAccount":{"accountUuid":"login-1"}}'
    $items = @()
    for ($i = 0; $i -lt 2; $i++) {
        $dir = Join-Path $config "agent-profiles\$($ids[$i])\claude"
        New-Item -ItemType Directory -Force (Join-Path $dir 'paddock-usage') | Out-Null
        Set-Content (Join-Path $dir 'paddock-usage\claude.json') ('{"rate_limits":{"five_hour":{"used_percentage":' + (11 + $i) + ',"resets_at":' + $five + '}},"updated_at":' + $now + '}')
        Set-Content (Join-Path $dir '.claude.json') ('{"oauthAccount":{"accountUuid":"login-' + ($i + 1) + '"}}')
        $items += '{"id": "' + $ids[$i] + '", "provider": "claude", "label": "' + $names[$i] + '", "runtime": "native"}'
    }
    Set-Content (Join-Path $config 'account-profiles.json') ('{"version": 1, "profiles": [' + ($items -join ',') + ']}')
    Copy-Item C:\Windows\System32\PING.EXE (Join-Path $root 'bin\claude.exe') -Force
}

function Invoke-Test($test) {
    $failures = @(); $checks = @()
    switch ($test) {
        'I6' {
            if (-not (Start-Case 'I6' @())) { return }
            $y = RowY 'README.md'
            if ($y -lt 0) { Say 'I6 FAIL (README.md row is not listed)'; return }
            $px = Sx $G.FileNameX; $py = Sy $y
            [W]::RightClick($px, $py)
            $menuOpen = WaitState '(?m)^menu: entry' 3
            [void](Shot 'I6-menu')
            if (-not $menuOpen) { $failures += 'right-click did not open the file menu' }
            [W]::Tap($VK_ESC)
            if ($menuOpen -and -not (WaitState '(?m)^menu: none' 3)) { $checks += 'Esc did not close the menu' }
            if ($menuOpen) {
                [W]::RightClick($px, $py); [void](WaitState '(?m)^menu: entry' 3)
                [W]::Click($px + [int]($G.MenuOpenDx * $script:fr.S), $py + [int]($G.MenuOpenDy * $script:fr.S))
                if (-not (WaitState 'file-tab .*README\.md' 4)) { $checks += 'Open item click did not open README.md (item offset?)' }
            }
            [void](Shot 'I6-after')
            Verdict 'I6' $failures $checks ''
        }
        'I7' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I7') 'notes.txt') 'kind=text state=text'
            if (-not (Start-Case 'I7' $extra $false $true)) { return }
            Get-Frame
            $ratio0 = FileRatio; $b = BodySize
            $bx = Sx (PaneLeft); $by = Sy (66 + $b[1] / 2)
            [void](Shot 'I7-before')
            [W]::Drag($bx, $by, $bx - [int](200 * $script:fr.S), $by)
            Start-Sleep -Milliseconds 800
            $ratio1 = FileRatio
            [void](Shot 'I7-after')
            if ($ratio1 -le $ratio0 + 0.05) { $checks += "file pane ratio $ratio0 -> $ratio1 (boundary x=$bx missed? drag left should widen the file pane)" }
            if ((KeyLines).Count -gt 0) { $failures += "keys reached the PTY: $((KeyLines) -join ',')" }
            Verdict 'I7' $failures $checks "ratio $ratio0 -> $ratio1"
        }
        'I8' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I8') 'notes.txt') 'kind=text state=text'
            if (-not (Start-Case 'I8' $extra $false $true)) { return }
            $had = (State) -match '(?m)^file-tab .*notes\.txt'
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x57)
            for ($i = 0; $i -lt 10 -and ((State) -match '(?m)^file-tab '); $i++) { Start-Sleep -Milliseconds 200 }
            $closed = -not ((State) -match '(?m)^file-tab ')
            [void](Shot 'I8-after')
            if (-not $had) { $checks += 'the file tab was not open before the key' }
            if (-not $closed) { $failures += 'Ctrl+Shift+W did not close the active file tab' }
            if ((KeyLines).Count -gt 0) { $failures += "keys reached the PTY: $((KeyLines) -join ',')" }
            Verdict 'I8' $failures $checks ''
        }
        'I9' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I9') 'a.txt') 'kind=text state=text'
            if (-not (Start-Case 'I9' $extra $false $true)) { return }
            $before = (Get-FileHash (Join-Path $script:proj 'a.txt')).Hash
            $open = '(?m)^search open=y'; $shut = '(?m)^search open=n'
            $steps = @()
            # 단계 1: 파일(편집기) 탭이 활성일 때
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); $steps += @{ name = 'editor: Ctrl+Shift+F opens'; ok = (WaitState $open 3) }
            [W]::Tap($VK_ESC); $steps += @{ name = 'editor: Esc in the input closes'; ok = (WaitState $shut 3) }
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); [void](WaitState $open 3)
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); $steps += @{ name = 'editor: second Ctrl+Shift+F closes'; ok = (WaitState $shut 3) }
            # 단계 2: 터미널(키 기록기)이 활성일 때 — 터미널 본문을 눌러 포커스를 옮긴다.
            $b = BodySize
            [W]::Click((Sx ($G.BodyLeft + $b[0] * 0.75)), (Sy (66 + $b[1] / 2))); Start-Sleep -Milliseconds 500
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); $steps += @{ name = 'terminal: Ctrl+Shift+F opens'; ok = (WaitState $open 3) }
            [W]::Tap($VK_ESC); $steps += @{ name = 'terminal: Esc in the input closes'; ok = (WaitState $shut 3) }
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); [void](WaitState $open 3)
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); $steps += @{ name = 'terminal: second Ctrl+Shift+F closes'; ok = (WaitState $shut 3) }
            [void](Shot 'I9-after')
            $bad = @($steps | Where-Object { -not $_.ok } | ForEach-Object { $_.name })
            if ($bad.Count -gt 0) { $failures += "failed steps: $($bad -join ' / ')" }
            if ((KeyLines).Count -gt 0) { $failures += "keys reached the PTY: $((KeyLines) -join ',')" }
            if ((Get-FileHash (Join-Path $script:proj 'a.txt')).Hash -ne $before) { $failures += 'a.txt changed' }
            Verdict 'I9' $failures $checks ''
        }
        'I10' {
            $extra = @('act view scm', 'waitstate view=scm @5000', 'waitstate scm state=ready @15000')
            if (-not (Start-Case 'I10' $extra)) { return }
            [void](SetHangul $false)
            $x = Sx 60
            [W]::Move($x, (Sy $G.ScmChangesRow1Y)); Start-Sleep -Milliseconds 400; [void](Shot 'I10-hover')
            [W]::Click((Sx $G.ScmActionX), (Sy $G.ScmChangesRow1Y))
            if (-not (WaitState 'staged=1 ' 4)) {
                $checks += "the row + button click did not stage (button x=$($G.ScmActionX)?); staged with the header + instead"
                [W]::Click((Sx $G.ScmStageAll[0]), (Sy $G.ScmStageAll[1])); [void](WaitState 'staged=[1-9] ' 4)
            }
            [void](Shot 'I10-staged')
            [W]::Click($x, (Sy $G.ScmStagedRow1Y))   # 스테이징된 첫 행
            if (-not (WaitState '(?m)^file-tab .* diff ' 5)) { $checks += 'clicking the staged row did not open a diff tab' }
            [W]::Click((Sx $G.ScmMessage[0]), (Sy $G.ScmMessage[1])); Start-Sleep -Milliseconds 300
            [W]::Text('night commit'); Start-Sleep -Milliseconds 300
            [W]::Click((Sx $G.ScmCommit[0]), (Sy $G.ScmCommit[1]))
            $committed = WaitState 'staged=0 ' 8
            Start-Sleep -Milliseconds 500
            $subject = (& git -C $script:proj log -1 --format=%s 2>$null)
            [void](Shot 'I10-committed')
            if (-not $committed -or $subject -ne 'night commit') { $failures += "commit result: staged0=$committed subject=[$subject]" }
            Verdict 'I10' $failures $checks ''
        }
        'I11' {
            $extra = @('act view scm', 'waitstate view=scm @5000', 'waitstate scm state=ready @15000', 'act discard 1', 'waitstate overlay: discard @5000')
            if (-not (Start-Case 'I11' $extra)) { return }
            $beforeText = (Get-Content (Join-Path $script:proj 'a.txt') -Raw).Trim()
            [void](Shot 'I11-prompt')
            [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 1500
            $gone = (State) -match '(?m)^overlay: none'
            $afterText = (Get-Content (Join-Path $script:proj 'a.txt') -Raw).Trim()
            [void](Shot 'I11-after')
            if (-not $gone) { $checks += 'Enter did not close the confirmation window' }
            elseif ($afterText -ne $beforeText) { $checks += "Enter DISCARDED the change ($beforeText -> $afterText): the default selection approves" }
            Verdict 'I11' $failures $checks "enter=$(if ($afterText -ne $beforeText) { 'discard' } elseif ($gone) { 'cancel' } else { 'ignored' })"
        }
        'I12' {
            if (-not (Start-Case 'I12' @() $true)) { return }
            $script:helper = StartHelper
            Start-Sleep -Seconds 6
            $read = { if ((State) -match 'git-calls local=\d+ wsl=(\d+)') { [int]$Matches[1] } else { -1 } }
            $n0 = & $read
            [void][W]::Focus($script:helper.MainWindowHandle); Start-Sleep -Milliseconds 1500
            $n1 = & $read
            [void](Ensure); Start-Sleep -Milliseconds 3000
            $n2 = & $read
            Start-Sleep -Seconds 5
            $n3 = & $read
            [void](Shot 'I12-after')
            if ($n0 -lt 0) { $failures += 'git-calls line is missing' }
            elseif ($n1 -ne $n0) { $failures += "deactivating the window ran git ($n0 -> $n1)" }
            elseif ($n2 - $n1 -ne 1) { $failures += "reactivating ran git $($n2 - $n1) times (expected 1)" }
            elseif ($n3 -ne $n2) { $failures += "an extra git run followed ($n2 -> $n3)" }
            Verdict 'I12' $failures $checks "git-calls wsl: before=$n0 deactivated=$n1 reactivated=$n2 later=$n3"
        }
        'I13' {
            $script:listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0); $script:listener.Start()
            $script:requests = @()
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I13') 'README.md') 'kind=markdown state=markdown view=preview'
            if (-not (Start-Case 'I13' $extra)) { return }
            $before = @(Get-Process | ForEach-Object { $_.Id })
            $px = Sx ((PaneLeft) + 60)
            [void](Shot 'I13-before')
            [W]::Click($px, (Sy $G.MdLinkY[0]))
            $got = $false
            for ($i = 0; $i -lt 75 -and -not $got; $i++) { Pump-Listener; if (@($script:requests | Where-Object { $_ -match '/web-link' }).Count -gt 0) { $got = $true } else { Start-Sleep -Milliseconds 200 } }
            if (-not $got) { $checks += 'the web link click did not reach the receiver (link position?)' }
            $afterWeb = @($script:requests).Count
            [W]::Click($px, (Sy $G.MdLinkY[1])); Start-Sleep -Milliseconds 2500   # file: 링크
            [W]::Click($px, (Sy $G.MdLinkY[2])); Start-Sleep -Milliseconds 1500   # 앵커 링크
            Pump-Listener
            $notepad = @(Get-Process -Name notepad -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id })
            if ($notepad.Count -gt 0) { $failures += 'the file: link opened Notepad' }
            if (@($script:requests).Count -ne $afterWeb) { $failures += 'the file:/anchor link sent a request' }
            [void](Shot 'I13-after')
            Verdict 'I13' $failures $checks "requests=[$(@($script:requests) -join ' | ')] (a browser tab for the web link is left open for you to close)"
        }
        'I14' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I14') 'img\pic.png') 'kind=image state=image'
            if (-not (Start-Case 'I14' $extra)) { return }
            $c = PaneCenter
            if ((State) -notmatch 'zoom=fit') { $checks += 'zoom is not fit at the start' }
            [W]::DoubleClick((Sx $c[0]), (Sy $c[1])); $actual = WaitState 'zoom=actual' 3
            [void](Shot 'I14-actual')
            [W]::DoubleClick((Sx $c[0]), (Sy $c[1])); $fit = WaitState 'zoom=fit' 3
            if (-not $actual) { $failures += 'double-click did not switch to 1:1' }
            if (-not $fit) { $failures += 'second double-click did not return to fit' }
            Verdict 'I14' $failures $checks ''
        }
        'I15' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I15') 'README.md') 'kind=markdown state=markdown view=preview'
            if (-not (Start-Case 'I15' $extra)) { return }
            $script:clip0 = $null; try { $script:clip0 = Get-Clipboard -Raw } catch {}; $script:clipSaved = $true
            $ty = Sy $G.TabsY
            [W]::Click((SxR $G.MdFileR), $ty); $f = WaitState 'view=file' 3
            [W]::Click((SxR $G.MdBothR), $ty); $b = WaitState 'view=both' 3
            [W]::Click((SxR $G.MdPreviewR), $ty); $p = WaitState 'view=preview' 3
            if (-not ($f -and $b -and $p)) { $checks += "mode buttons: file=$f both=$b preview=$p (button x?)" }
            Set-Clipboard -Value 'SENTINEL-NIGHT'
            $x0 = (PaneLeft); $y = Sy $G.MdParagraphY
            [W]::Drag((Sx ($x0 + 30)), $y, (Sx ($x0 + 330)), $y); Start-Sleep -Milliseconds 300
            [W]::Chord(@($VK_CONTROL), 0x43); Start-Sleep -Milliseconds 500
            $clip = ''; try { $clip = Get-Clipboard -Raw } catch {}
            [void](Shot 'I15-selected')
            if ($clip -match 'SENTINEL-NIGHT' -or [string]::IsNullOrWhiteSpace($clip)) { $checks += 'Ctrl+C copied nothing from the preview selection (drag position?)' }
            Verdict 'I15' $failures $checks "copied=[$($clip.Trim())]"
        }
        'I16' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I16') 'img\grid.png') 'kind=image state=image'
            if (-not (Start-Case 'I16' $extra)) { return }
            $c = PaneCenter
            [W]::DoubleClick((Sx $c[0]), (Sy $c[1])); $actual = WaitState 'zoom=actual' 3
            Start-Sleep -Milliseconds 800
            $bmp = Shot 'I16-actual'
            $r = PaneRect
            $alt = [Px]::Alternation($bmp, $r[0], $r[1], $r[2], $r[3])
            $bounds = [Px]::PureBounds($bmp, $r[0], $r[1], $r[2], $r[3])
            $dpi = [W]::GetDpiForWindow($script:h)
            $bmp.Dispose()
            $ratio = if ($alt[0] -gt 0) { $alt[1] / [double]$alt[0] } else { 0 }
            $width = if ($bounds) { $bounds.Width } else { 0 }
            if (-not $actual) { $checks += 'double-click did not switch to 1:1' }
            if ($alt[0] -lt 1000) { $failures += "the black-white grid is not visible (pure pairs=$($alt[0]) — smoothed or not drawn)" }
            elseif ($ratio -lt 0.95) { $failures += "grid pixels are not 1:1 (alternation $([Math]::Round($ratio, 3)))" }
            if ($width -ne 0 -and [Math]::Abs($width - 400) -gt 2) { $failures += "grid is $width screen px wide (expected 400)" }
            if ($dpi -lt 144) { $checks += "display scale is $([Math]::Round($dpi / 96.0, 2))x (< 1.5x: not the condition this item targets)" }
            Verdict 'I16' $failures $checks "dpi=$dpi alternation=$([Math]::Round($ratio, 3)) pairs=$($alt[0]) gridWidth=$width"
        }
        'I17' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I17') 'sample-3p.pdf') 'kind=pdf state=pdf pages=3'
            if (-not (Start-Case 'I17' $extra)) { return }
            $ty = Sy $G.TabsY
            $percent = { if ((State) -match 'percent=(\d+)') { [int]$Matches[1] } else { -1 } }
            $p0 = & $percent
            [W]::Click((SxR $G.PdfPlusR), $ty); Start-Sleep -Milliseconds 800; $p1 = & $percent
            [W]::Click((SxR $G.PdfMinusR), $ty); Start-Sleep -Milliseconds 800; $p2 = & $percent
            [void](Shot 'I17-zoom')
            if (-not ($p1 -gt $p0)) { $checks += "+ did not raise the zoom ($p0 -> $p1; button x?)" }
            if (-not ($p2 -lt $p1)) { $checks += "- did not lower the zoom ($p1 -> $p2; button x?)" }
            $procs = @(Get-Process | ForEach-Object { $_.Id })
            [W]::Click((SxR $G.PdfDefaultR), $ty); Start-Sleep -Seconds 6
            $new = NewWindowsSince $procs
            [void](Shot 'I17-default')
            if ($new.Count -eq 0) { $checks += 'Open with default app opened no new window within 6 s (a viewer that reuses a running window shows no new process — look at the screen)' }
            Verdict 'I17' $failures $checks "zoom $p0 -> $p1 -> $p2 newWindows=[$($new -join ' ; ')] (close the opened PDF viewer yourself)"
        }
        'I18' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I18') 'sample-3p.pdf') 'kind=pdf state=pdf pages=3'
            if (-not (Start-Case 'I18' $extra)) { return }
            $r = PaneRect
            $measure = {
                param($act)
                $sw = [Diagnostics.Stopwatch]::StartNew(); & $act; $sw.Restart()
                $blank = 0; $textAt = -1
                while ($sw.ElapsedMilliseconds -lt 3000) {
                    $b = [W]::Grab($script:h); $n = 0; [void][Px]::TextBounds($b, $r[0], $r[1], $r[2], $r[3], [ref]$n); $b.Dispose()
                    if ($n -ge 30) { $textAt = [int]$sw.ElapsedMilliseconds; break } else { $blank++ }
                }
                return @($textAt, $blank)
            }
            $c = PaneCenter
            $visible0 = if ((State) -match 'visible=([\d,]*)') { $Matches[1] } else { '?' }
            $wheel = & $measure { [W]::Wheel((Sx $c[0]), (Sy $c[1]), -40) }
            $visible1 = if ((State) -match 'visible=([\d,]*)') { $Matches[1] } else { '?' }
            [void](Shot 'I18-wheel')
            $top = & $measure { [W]::Wheel((Sx $c[0]), (Sy $c[1]), 80) }
            $sx = SxR 16
            $drag = & $measure { [W]::Drag($sx, (Sy 120), $sx, (Sy 700)) }
            $visible2 = if ((State) -match 'visible=([\d,]*)') { $Matches[1] } else { '?' }
            [void](Shot 'I18-scrollbar')
            if ($visible1 -eq $visible0) { $checks += "wheel did not change the visible pages ($visible0)" }
            if ($visible2 -eq $visible1 -and $visible1 -eq $visible0) { $checks += 'scrollbar drag did not change the visible pages (thumb position?)' }
            foreach ($m in @(@('wheel', $wheel), @('wheel-back', $top), @('scrollbar', $drag))) { if ($m[1][0] -lt 0 -or $m[1][0] -gt 1000) { $failures += "$($m[0]): text visible after $($m[1][0]) ms (blank frames $($m[1][1])), limit 1000 ms" } }
            Verdict 'I18' $failures $checks "visible $visible0 -> $visible1 -> $visible2 textAfterMs wheel=$($wheel[0]) wheelBack=$($top[0]) scrollbar=$($drag[0]) blankFrames=$($wheel[1])/$($top[1])/$($drag[1])"
        }
        'I19' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I19') 'sample-3p.pdf') 'kind=pdf state=pdf pages=3'
            if (-not (Start-Case 'I19' $extra)) { return }
            Start-Sleep -Seconds 2
            $bmp = Shot 'I19-pdf'
            $r = PaneRect
            $n = 0; $dark = [Px]::TextBounds($bmp, $r[0], $r[1], $r[2], $r[3], [ref]$n)
            $grad = if ($dark) { [Px]::GradientP99($bmp, $dark.X - 4, $dark.Y - 4, $dark.Width + 8, $dark.Height + 8) } else { 0 }
            $dpi = [W]::GetDpiForWindow($script:h)
            if ($dark) {
                $crop = New-Object System.Drawing.Bitmap 480, 240
                $g = [System.Drawing.Graphics]::FromImage($crop); $g.InterpolationMode = 'NearestNeighbor'; $g.PixelOffsetMode = 'Half'
                $g.DrawImage($bmp, (New-Object System.Drawing.Rectangle 0, 0, 480, 240), (New-Object System.Drawing.Rectangle $dark.X, $dark.Y, 120, 60), [System.Drawing.GraphicsUnit]::Pixel); $g.Dispose()
                $crop.Save((Join-Path $script:caseOut 'I19-text-4x.png')); $crop.Dispose()
            }
            $bmp.Dispose()
            if (-not $dark) { $failures += 'no text pixels found in the PDF pane' }
            elseif ($grad -lt 150) { $failures += "text edges look soft (gradient p99=$grad, expected >= 150)" }
            $checks += 'look at I19-text-4x.png once: glyph edges should be crisp, not blurred (the metric alone cannot tell)'
            if ($dpi -lt 144) { $checks += "display scale is $([Math]::Round($dpi / 96.0, 2))x (< 1.5x: not the condition this item targets)" }
            Verdict 'I19' $failures $checks "dpi=$dpi gradientP99=$grad"
        }
        'I20' {
            $extra = @('key ctrl-shift-c', 'waittabs 2 @20000', 'wait 1500', "send function prompt { '> ' }\r", 'wait 500', 'send cls\r', 'wait 800',
                'send Write-Host "see src/a.js:12 here"\r', 'waitline see src/a.js:12 here @20000', 'waitstate a.js 12 @10000')
            if (-not (Start-Case 'I20' $extra)) { return }
            $b = BodySize
            $cellX = (Sx ($G.BodyLeft + $b[0] / 2 + $G.TermTextLeft + $G.TermCellWidth * 9.5))
            $cellY = Sy $G.TermLinkRowY
            $s = $script:fr.S
            $region = @([int]((($G.BodyLeft + $b[0] / 2 + $G.TermTextLeft + $G.TermCellWidth * 3.5) - 0) * $s), [int](($G.TermLinkRowY - 10) * $s), [int]($G.TermCellWidth * 12 * $s), [int](20 * $s))
            [W]::Move($cellX, $cellY); Start-Sleep -Milliseconds 500
            $base = [W]::Grab($script:h); $hand0 = [W]::HandCursor()
            [W]::Click($cellX, $cellY); Start-Sleep -Milliseconds 800
            $plainOpened = (State) -match '(?m)^file-tab '
            [W]::Down($VK_CONTROL); [W]::Move($cellX + 1, $cellY); Start-Sleep -Milliseconds 500
            $hover = [W]::Grab($script:h); $hand1 = [W]::HandCursor()
            $hover.Save((Join-Path $script:caseOut 'I20-ctrl-hover.png'))
            $underline = [Px]::Diff($base, $hover, $region[0], $region[1], $region[2], $region[3])
            [W]::Up($VK_CONTROL); Start-Sleep -Milliseconds 500
            [W]::Move($cellX, $cellY); Start-Sleep -Milliseconds 400
            $released = [W]::Grab($script:h); $hand2 = [W]::HandCursor()
            $left = [Px]::Diff($base, $released, $region[0], $region[1], $region[2], $region[3])
            [W]::Down($VK_CONTROL); [W]::Move($cellX + 1, $cellY); Start-Sleep -Milliseconds 300
            [W]::Click($cellX, $cellY); Start-Sleep -Milliseconds 300; [W]::Up($VK_CONTROL)
            $opened = WaitState '(?m)^file-tab .*a\.js' 5
            $line12 = WaitState 'file-view .*line=12' 5
            [void](Shot 'I20-opened')
            $base.Dispose(); $hover.Dispose(); $released.Dispose()
            if ($plainOpened) { $failures += 'a plain click (no Ctrl) opened a file tab' }
            if ($hand0) { $failures += 'hand cursor shows without Ctrl' }
            if (-not $hand1) { $checks += 'no hand cursor with Ctrl held over the link (link position?)' }
            if ($underline -lt 20) { $checks += "no underline change with Ctrl held (changed pixels=$underline; link position?)" }
            if ($left -gt 20 -or $hand2) { $failures += "underline/hand remained after releasing Ctrl (changed pixels=$left hand=$hand2)" }
            if (-not $opened) { $failures += 'Ctrl+click did not open a.js' } elseif (-not $line12) { $failures += 'a.js opened but not at line 12' }
            Verdict 'I20' $failures $checks "underlinePixels=$underline afterReleasePixels=$left"
        }
        'I21' {
            $extra = @('act search needle', 'waitstate capped=y state=done @30000')
            if (-not (Start-Case 'I21' $extra)) { return }
            $hy = HeaderY
            $before = [W]::Grab($script:h)
            # 옵션 단추: Aa 켬 → options=case, 다시 누르면 꺼짐
            [W]::Click((Sx $G.SearchOptCaseX), (Sy ($hy + $G.SearchInputDy))); $caseOn = WaitState 'options=case' 4
            [W]::Click((Sx $G.SearchOptCaseX), (Sy ($hy + $G.SearchInputDy))); $caseOff = WaitState 'options=-' 4
            if (-not ($caseOn -and $caseOff)) { $checks += "Aa button: on=$caseOn off=$caseOff (button x=$($G.SearchOptCaseX)?)" }
            [void](WaitState 'capped=y state=done' 15)
            # 결과 목록 휠: 2,000건에서 목록이 움직인다.
            $listX = Sx 140; $listTop = Sy ($hy + $G.SearchFirstFileDy - 10)
            $shot0 = [W]::Grab($script:h)
            [W]::Wheel($listX, (Sy ($hy + 200)), -15); Start-Sleep -Milliseconds 600
            $shot1 = [W]::Grab($script:h)
            $moved = [Px]::Diff($shot0, $shot1, 0, $listTop, [int](280 * $script:fr.S), [int](600 * $script:fr.S))
            $shot1.Save((Join-Path $script:caseOut 'I21-wheel.png'))
            if ($moved -lt 2000) { $checks += "result list did not move with the wheel (changed pixels=$moved)" }
            [W]::Wheel($listX, (Sy ($hy + 200)), 60); Start-Sleep -Milliseconds 600
            # 파일 행 접기: 첫 파일 행의 접기 표지
            $fileY = Sy ($hy + $G.SearchFirstFileDy)
            $shot2 = [W]::Grab($script:h)
            [W]::Click((Sx 22), $fileY); Start-Sleep -Milliseconds 600
            $shot3 = [W]::Grab($script:h)
            $folded = [Px]::Diff($shot2, $shot3, 0, $listTop, [int](280 * $script:fr.S), [int](300 * $script:fr.S))
            $shot3.Save((Join-Path $script:caseOut 'I21-folded.png'))
            if ($folded -lt 300) { $checks += "clicking the first file row's chevron changed the list by only $folded px (fold position?)" }
            [W]::Click((Sx 22), $fileY); Start-Sleep -Milliseconds 600
            # 결과 행: 첫 일치 줄(a-first.txt 12번째 줄)을 눌러 파일 탭이 그 줄로 열린다.
            [W]::Click((Sx 100), (Sy ($hy + $G.SearchFirstFileDy + $G.SearchMatchStep)))
            $tab = WaitState '(?m)^file-tab .*a-first\.txt' 5
            $line = WaitState 'file-view .*line=12' 5
            [void](Shot 'I21-row')
            $before.Dispose(); $shot0.Dispose(); $shot2.Dispose(); $shot3.Dispose()
            if (-not $tab) { $checks += 'clicking the first result row did not open a-first.txt (row position?)' } elseif (-not $line) { $failures += 'a-first.txt opened but not at line 12' }
            Verdict 'I21' $failures $checks ''
        }
        'T10' {
            $extra = @('act view scm', 'waitstate view=scm @5000', 'waitstate scm state=ready @15000')
            if (-not (Start-Case 'T10' $extra)) { return }
            $pass = 0; $fail = 0; $skip = 0
            for ($i = 0; $i -lt [Math]::Min($N, 10); $i++) {
                # 변경 하나를 만들어 스테이징한다(조합 중 Enter가 커밋하면 곧 드러난다).
                Set-Content (Join-Path $script:proj 'a.txt') "iteration $i $(Get-Random)"
                & git -C $script:proj add -- a.txt 2>$null
                if (-not (WaitState 'staged=1 ' 10)) { $skip++; Say "T10 #$i SKIPPED (staged change did not appear)"; continue }
                $countBefore = [int](& git -C $script:proj rev-list --count HEAD 2>$null)
                if (-not (Ensure)) { $skip++; continue }
                [void](SetHangul $true)
                [W]::Click((Sx $G.ScmMessage[0]), (Sy $G.ScmMessage[1])); Start-Sleep -Milliseconds 300
                Typ $HG; [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 700
                $stillStaged = (State) -match 'staged=1 '
                $countMid = [int](& git -C $script:proj rev-list --count HEAD 2>$null)
                [W]::Click((Sx $G.ScmCommit[0]), (Sy $G.ScmCommit[1]))
                [void](WaitState 'staged=0 ' 8); Start-Sleep -Milliseconds 400
                $subject = ((& git -C $script:proj log -1 --format=%B 2>$null) -join '').Trim()
                if ($stillStaged -and $countMid -eq $countBefore -and $subject -eq '한글') { $pass++ } else { $fail++; Say "T10 #$i FAIL stillStagedAfterEnter=$stillStaged commitsBefore=$countBefore afterEnter=$countMid subject=[$subject]" }
            }
            [void](Shot 'T10-after')
            Say "T10 $(if ($fail -eq 0 -and $pass -gt 0) { 'PASS' } else { 'FAIL' }) pass=$pass fail=$fail skipped=$skip of $([Math]::Min($N, 10)) inputFailures=$([W]::Fail)"
        }
        'T11' {
            $extra = @('key ctrl-shift-f', 'waitstate search open=y @5000')
            if (-not (Start-Case 'T11' $extra $false $true)) { return }
            $pass = 0; $fail = 0; $skip = 0
            $hy = HeaderY
            for ($i = 0; $i -lt [Math]::Min($N, 10); $i++) {
                if (-not (Ensure)) { $skip++; continue }
                if ((State) -notmatch '(?m)^search open=y') { [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x46); [void](WaitState '(?m)^search open=y' 3) }
                [void](SetHangul $true)
                $runs0 = if ((State) -match 'runs=(\d+)') { [int]$Matches[1] } else { 0 }
                [W]::Click((Sx $G.SearchInputX), (Sy ((HeaderY) + $G.SearchInputDy))); Start-Sleep -Milliseconds 300
                [W]::Chord(@($VK_CONTROL), 0x41); Start-Sleep -Milliseconds 100
                Typ $HG; [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 900
                $ok = ((State) -match 'query=한글 ') -and (((State) -match 'runs=(\d+)') -and ([int]$Matches[1] -eq $runs0 + 1))
                if ($ok -and (KeyLines).Count -eq 0) { $pass++ } else { $fail++; Say "T11 #$i FAIL query/runs=[$((StateLines | Where-Object { $_ -like 'search open*' }) -join ' ')] runsBefore=$runs0 keys=[$((KeyLines) -join ',')]" }
            }
            [void](Shot 'T11-after')
            Say "T11 $(if ($fail -eq 0 -and $pass -gt 0) { 'PASS' } else { 'FAIL' }) pass=$pass fail=$fail skipped=$skip of $([Math]::Min($N, 10)) inputFailures=$([W]::Fail)"
        }

        'I22' {
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000', 'key ctrl-shift-t', 'waittabs 3 @20000', 'wait 1500')
            if (-not (Start-Case 'I22' $extra)) { return }
            $x = Sx $G.WorkRowX; $y = Sy ($G.WorkFirstRowY + 2 * $G.TerminalRowStep)
            [W]::Click($x, $y); Start-Sleep -Milliseconds 700
            $afterOne = (State) -match '(?m)^overlay: rename'
            [W]::DoubleClick($x, $y)
            $opened = WaitState '(?m)^overlay: rename' 3
            [void](Shot 'I22-rename')
            [W]::Tap($VK_ESC)
            if ($afterOne) { $failures += 'a single click opened the rename window' }
            if (-not $opened) { $checks += 'double click did not open the rename window (row coordinates?)' }
            Verdict 'I22' $failures $checks ''
        }
        'I23' {
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000', 'key ctrl-shift-t', 'waittabs 3 @20000', 'wait 1500')
            if (-not (Start-Case 'I23' $extra)) { return }
            $order0 = @(StateLines | Where-Object { $_ -match '^row terminal ' })
            $x = Sx $G.WorkRowX
            [W]::Drag($x, (Sy $G.WorkFirstRowY), $x, (Sy ($G.WorkFirstRowY + 2 * $G.TerminalRowStep + 10)))
            Start-Sleep -Milliseconds 800
            $order1 = @(StateLines | Where-Object { $_ -match '^row terminal ' })
            [void](Shot 'I23-after')
            if (($order0 -join '|') -eq ($order1 -join '|')) { $checks += 'the row order did not change (drag coordinates?)' }
            Verdict 'I23' $failures $checks "before=[$($order0 -join ' / ')] after=[$($order1 -join ' / ')]"
        }
        'I24' {
            $extra = @('act quick copyonselection', 'waitstate copy-on-selection=on @5000', 'send echo SELECT-THIS-WORD\r', 'waitline SELECT-THIS-WORD @10000', 'wait 1000')
            if (-not (Start-Case 'I24' $extra)) { return }
            $script:clip0 = try { Get-Clipboard -Raw } catch { $null }; $script:clipSaved = $true
            Set-Clipboard -Value 'sentinel'
            $y = Sy $G.TermSelectY
            [W]::Drag((Sx 303), $y, (Sx 480), $y)
            Start-Sleep -Milliseconds 600
            $copied = try { Get-Clipboard -Raw } catch { '' }
            [void](Shot 'I24-after')
            if ($copied -eq 'sentinel') { $checks += 'the clipboard did not change after the drag (row coordinates?)' }
            elseif ($copied -notmatch 'SELECT|THIS|WORD|echo') { $failures += "unexpected clipboard text: [$copied]" }
            Verdict 'I24' $failures $checks "clipboard=[$copied]"
        }
        'I25' {
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000', 'key ctrl-shift-t', 'waittabs 3 @20000', 'wait 1500')
            if (-not (Start-Case 'I25' $extra)) { return }
            [W]::RightClick((Sx $G.WorkRowX), (Sy ($G.WorkFirstRowY + $G.TerminalRowStep)))
            $opened = WaitState '(?m)^menu: terminal' 3
            foreach ($i in 1..3) { [W]::Tap(0x28); Start-Sleep -Milliseconds 150 }
            $highlight = WaitState '(?m)^menu-highlight=2' 2
            [void](Shot 'I25-menu')
            [W]::Tap($VK_ENTER)
            $confirm = WaitState '(?m)^overlay: close-terminal' 3
            [W]::Tap($VK_ESC)
            if (-not $opened) { $checks += 'right click did not open the terminal menu (row coordinates?)' }
            elseif (-not $highlight) { $failures += 'three Down keys did not highlight the third item' }
            elseif (-not $confirm) { $failures += 'Enter on the highlighted Close terminal did not open the confirm window' }
            Verdict 'I25' $failures $checks ''
        }
        'I26' {
            if (-not (Start-Case 'I26' @() $false $true)) { return }
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x48)
            $open = WaitState 'term-find open=y' 3
            [W]::Text('needle'); Start-Sleep -Milliseconds 500
            [void](Shot 'I26-bar')
            $typed = (State) -match 'term-find open=y .*query=needle'
            [W]::Tap($VK_ESC)
            $closed = WaitState 'term-find open=n' 3
            $leaked = @(KeyLines).Count
            [W]::Chord(@($VK_CONTROL), 0x48); Start-Sleep -Milliseconds 600
            $afterCtrlH = @(KeyLines).Count
            if (-not $open) { $failures += 'Ctrl+Shift+H did not open the find bar' }
            if (-not $typed) { $failures += 'typed letters did not reach the find input' }
            if (-not $closed) { $failures += 'Esc did not close the find bar' }
            if ($leaked -gt 0) { $failures += "keys reached the PTY while the bar was open: $((KeyLines) -join ',')" }
            if ($afterCtrlH -le $leaked) { $failures += 'plain Ctrl+H did not reach the PTY' }
            Verdict 'I26' $failures $checks ''
        }
        'I27' {
            if (-not (Start-Case 'I27' @())) { return }
            $proj = $script:proj
            $shell = New-Object -ComObject Shell.Application
            $shell.Namespace($proj).ParseName('a-first.txt').InvokeVerb('copy')
            Start-Sleep -Milliseconds 500
            $y = RowY 'src'
            if ($y -lt 0) { Say 'I27 FAIL (the src row is not listed)'; return }
            [W]::Click((Sx $G.FileNameX), (Sy $y)); Start-Sleep -Milliseconds 500
            [W]::Chord(@($VK_CONTROL), 0x56)
            Start-Sleep -Milliseconds 1500
            $first = Test-Path (Join-Path $proj 'src\a-first.txt')
            [W]::Chord(@($VK_CONTROL), 0x56)
            Start-Sleep -Milliseconds 1500
            $second = Test-Path (Join-Path $proj 'src\a-first copy.txt')
            [void](Shot 'I27-after')
            if (-not (Test-Path (Join-Path $proj 'a-first.txt'))) { $failures += 'the original disappeared' }
            if (-not $first) { $failures += 'Ctrl+V did not copy the file into src' }
            if (-not $second) { $failures += 'the second Ctrl+V did not make the "copy" name' }
            Verdict 'I27' $failures $checks ''
        }
        'I28' {
            if (-not (Start-Case 'I28' @() $false $true)) { return }
            $proj = $script:proj
            $script:clip0 = try { Get-Clipboard -Raw } catch { $null }; $script:clipSaved = $true
            Add-Type -AssemblyName System.Windows.Forms, System.Drawing
            $bmp = New-Object System.Drawing.Bitmap 20, 10
            [Windows.Forms.Clipboard]::SetImage($bmp)
            $y = RowY 'src'
            if ($y -lt 0) { Say 'I28 FAIL (the src row is not listed)'; return }
            [W]::Click((Sx $G.FileNameX), (Sy $y)); Start-Sleep -Milliseconds 500
            [W]::Chord(@($VK_CONTROL), 0x56); Start-Sleep -Milliseconds 1500
            $png = Join-Path $proj 'src\image.png'
            $isPng = (Test-Path $png) -and (([IO.File]::ReadAllBytes($png) | Select-Object -First 4) -join ',') -eq '137,80,78,71'
            # 터미널: 키를 받는 쪽을 터미널로 돌리고(터미널 칸을 누름) 붙인 뒤 경로가 입력줄에 보이는지 본다.
            $before = @(Get-ChildItem (Join-Path $env:USERPROFILE '.paddock\native\pasted-images') -ErrorAction SilentlyContinue).Count
            [W]::Click((Sx 600), (Sy 400)); Start-Sleep -Milliseconds 300
            [W]::Chord(@($VK_CONTROL), 0x56); Start-Sleep -Milliseconds 1500
            $after = @(Get-ChildItem (Join-Path $env:USERPROFILE '.paddock\native\pasted-images') -ErrorAction SilentlyContinue).Count
            [void](Shot 'I28-after')
            if (-not $isPng) { $failures += 'the bitmap was not written as a PNG named image.png in src' }
            if ($after -le $before) { $failures += 'pasting the bitmap into the terminal saved no pasted-images file' }
            Verdict 'I28' $failures $checks 'Kakao Talk / screenshot tools: paste one by hand and add the format to the result (CHECK)'
        }
        'I29' {
            Say 'I29 SKIP (an OLE drag from a real Explorer window needs a person: drag a file from Explorer onto a folder row, onto the empty list area and onto a terminal; the files must appear with copy names, never overwrite, and the terminal must get the quoted path)'
        }
        'I30' {
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000', 'wait 1500')
            if (-not (Start-Case 'I30' $extra $false $true)) { return }
            $proj = $script:proj
            $y = RowY 'src'
            if ($y -lt 0) { Say 'I30 FAIL (the src row is not listed)'; return }
            [W]::Click((Sx $G.FileNameX), (Sy $y)); Start-Sleep -Milliseconds 500
            [W]::Tap(0x71)
            $opened = WaitState 'rename-target=entry:.*src' 3
            [void](Shot 'I30-dialog')
            [W]::Text('lib'); [W]::Tap($VK_ENTER)
            Start-Sleep -Milliseconds 1500
            $renamed = (Test-Path (Join-Path $proj 'lib\b.txt')) -and -not (Test-Path (Join-Path $proj 'src'))
            # §4 결함: 이름이 바뀐 폴더 두 단계 아래에 파일을 만들면 1초 안에 목록에 나타나는가. 새 폴더 `lib\x\y`를 펼쳐 두고 파일을 만든다.
            New-Item -ItemType Directory -Force (Join-Path $proj 'lib\x\y') | Out-Null
            Start-Sleep -Milliseconds 800
            $started = Get-Date
            Set-Content (Join-Path $proj 'lib\x\y\new-after-rename.txt') 'x'
            $seen = $false
            for ($i = 0; $i -lt 10 -and -not $seen; $i++) { Start-Sleep -Milliseconds 200; $seen = (State) -match 'files-reads=' }
            $reads = ((State) -split "`n" | Where-Object { $_ -like 'files-reads=*' }) -join ''
            [void](Shot 'I30-after')
            if (-not $opened) { $checks += 'F2 did not open the rename window for the selected row' }
            if (-not $renamed) { $failures += 'the folder was not renamed to lib' }
            if ((KeyLines).Count -gt 0) { $failures += "keys reached the PTY: $((KeyLines) -join ',')" }
            # 9P(WSL 공유 폴더)의 대소문자만 다른 두 파일: `MoveFileExW`를 덮어쓰기 플래그 없이 부르면 거절되고 두 파일이 그대로여야 한다(앱의 이름 바꾸기가 쓰는 호출).
            $wslDir = '\\\\wsl.localhost\\Ubuntu\\tmp\\pdk-night-case'
            wsl.exe -e sh -c 'rm -rf /tmp/pdk-night-case && mkdir -p /tmp/pdk-night-case && printf one > /tmp/pdk-night-case/a.txt && printf two > /tmp/pdk-night-case/A.txt' | Out-Null
            Add-Type -Namespace Pdk -Name Move -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, SetLastError = true)] public static extern bool MoveFileExW(string from, string to, uint flags);' -ErrorAction SilentlyContinue
            $moved = [Pdk.Move]::MoveFileExW("$wslDir\a.txt", "$wslDir\A.txt", 0)
            $a = (wsl.exe -e cat /tmp/pdk-night-case/a.txt); $b = (wsl.exe -e cat /tmp/pdk-night-case/A.txt)
            wsl.exe -e rm -rf /tmp/pdk-night-case | Out-Null
            if ($moved -or $a -ne 'one' -or $b -ne 'two') { $failures += "9P case-only rename overwrote or moved (moved=$moved a=$a A=$b)" }
            Verdict 'I30' $failures $checks "windows-section4: created lib\x\y\new-after-rename.txt $([int]((Get-Date) - $started).TotalMilliseconds) ms ago, $reads (compare files-reads before/after in state-live.txt)"
        }
        'T12' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'T12') 'a.txt') 'file-view kind=text state=text'
            if (-not (Start-Case 'T12' $extra)) { return }
            $file = Join-Path $script:proj 'a.txt'
            $pass = 0; $fail = 0; $skip = 0
            for ($i = 0; $i -lt [Math]::Min($N, 10); $i++) {
                if (-not (Ensure)) { $skip++; continue }
                [void](SetHangul $true)
                [W]::Chord(@($VK_CONTROL), 0x24); Start-Sleep -Milliseconds 200
                Typ $HG; [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 700
                [W]::Chord(@($VK_CONTROL), 0x53); Start-Sleep -Milliseconds 900
                $text = [IO.File]::ReadAllText($file)
                $want = ('한글' + "`r`n") * ($i + 1) + 'one'
                if ($text.StartsWith($want) -and (State) -match 'unsaved=no') { $pass++ } else { $fail++; Say "T12 #$i FAIL file=[$($text.Replace("`r", '\r').Replace("`n", '\n'))] want-prefix=[$($want.Replace("`r", '\r').Replace("`n", '\n'))]" }
            }
            [void](Shot 'T12-after')
            Say "T12 $(if ($fail -eq 0 -and $pass -gt 0) { 'PASS' } else { 'FAIL' }) pass=$pass fail=$fail skipped=$skip of $([Math]::Min($N, 10)) inputFailures=$([W]::Fail)"
        }
        'I31' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I31') 'a.txt') 'file-view kind=text state=text'
            if (-not (Start-Case 'I31' $extra $false $true)) { return }
            $file = Join-Path $script:proj 'a.txt'
            # 파일 탭이 활성: 글자를 치고 Ctrl+S로 저장한다.
            [W]::Text('x'); Start-Sleep -Milliseconds 400
            $edited = WaitState 'unsaved=yes' 3
            [W]::Chord(@($VK_CONTROL), 0x53)
            $saved = WaitState 'unsaved=no' 3
            $onDisk = ([IO.File]::ReadAllText($file)).StartsWith('x')
            $keysFile = @(KeyLines).Count
            # 터미널이 활성: 다시 고친 뒤 터미널을 눌러 Ctrl+S를 보내면 저장되지 않는다(키는 셸의 것).
            [W]::Click((Sx 600), (Sy 400)); Start-Sleep -Milliseconds 300
            [W]::Chord(@($VK_CONTROL), 0x53); Start-Sleep -Milliseconds 600
            [void](Shot 'I31-terminal')
            $stillSame = ([IO.File]::ReadAllText($file)).StartsWith('x') -and -not ([IO.File]::ReadAllText($file)).StartsWith('xx')
            if (-not $edited) { $failures += 'typing did not mark the file unsaved' }
            if (-not $saved) { $failures += 'Ctrl+S did not save the file' }
            if (-not $onDisk) { $failures += 'the typed letter is not on disk' }
            if ($keysFile -gt 0) { $failures += "Ctrl+S reached the PTY while the file tab was active: $((KeyLines) -join ',')" }
            if (-not $stillSame) { $failures += 'the file changed while the terminal was active' }
            Verdict 'I31' $failures $checks 'PowerShell ReadKey never reports Ctrl+S, so the byte reaching the shell is checked by file-keys-wsl.script (day run)'
        }
        'I32' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I32') 'notes.txt') 'file-view kind=text state=text'
            if (-not (Start-Case 'I32' $extra $false $true)) { return }
            # 파일 탭이 활성: 세 키가 각각 패널·창을 연다.
            [W]::Chord(@($VK_CONTROL), 0x46); $find = WaitState 'find-open=yes' 3
            [void](Shot 'I32-find'); [W]::Tap($VK_ESC); Start-Sleep -Milliseconds 400
            [W]::Chord(@($VK_CONTROL), 0x48); Start-Sleep -Milliseconds 600
            [void](Shot 'I32-replace'); [W]::Tap($VK_ESC); Start-Sleep -Milliseconds 400
            [W]::Chord(@($VK_CONTROL), 0x47); $goto = WaitState '(?m)^overlay: goto-line' 3
            [void](Shot 'I32-goto'); [W]::Tap($VK_ESC); Start-Sleep -Milliseconds 400
            $leakedFile = @(KeyLines).Count
            # 터미널이 활성: 열리지 않고 Ctrl+F·Ctrl+H가 셸로 간다.
            [W]::Click((Sx 600), (Sy 400)); Start-Sleep -Milliseconds 300
            [W]::Chord(@($VK_CONTROL), 0x46); Start-Sleep -Milliseconds 400
            [W]::Chord(@($VK_CONTROL), 0x48); Start-Sleep -Milliseconds 400
            [W]::Chord(@($VK_CONTROL), 0x47); Start-Sleep -Milliseconds 400
            $noPanel = (State) -notmatch 'find-open=yes' -and (State) -notmatch '(?m)^overlay: goto-line'
            $toShell = @(KeyLines).Count - $leakedFile
            if (-not $find) { $failures += 'Ctrl+F did not open the find panel on a file tab' }
            if (-not $goto) { $failures += 'Ctrl+G did not open the go-to-line window on a file tab' }
            if ($leakedFile -gt 0) { $failures += "keys reached the PTY while the file tab was active: $((KeyLines) -join ',')" }
            if (-not $noPanel) { $failures += 'a panel or window opened while the terminal was active' }
            if ($toShell -lt 2) { $failures += "Ctrl+F/Ctrl+H did not both reach the shell (saw $toShell key lines)" }
            Verdict 'I32' $failures $checks ''
        }
        'T13' {
            $extra = (OpenPath-Lines (Join-Path (ProjPath 'T13') 'notes.txt') 'file-view kind=text state=text') + @('act selectlines 2 2', 'wait 500')
            if (-not (Start-Case 'T13' $extra)) { return }
            $pass = 0; $fail = 0; $skip = 0
            for ($i = 0; $i -lt [Math]::Min($N, 10); $i++) {
                if (-not (Ensure)) { $skip++; continue }
                [void](SetHangul $true)
                [W]::Chord(@($VK_CONTROL, 0xA4), 0x4D); [void](WaitState '(?m)^overlay: review-note' 3)
                Typ $HG; [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 900
                $notes = @(StateLines | Where-Object { $_ -like 'review-note *' })
                $want = $i + 1
                if (((State) -match "review-notes=$want") -and ($notes[-1] -match '한글$') -and ((State) -notmatch '(?m)^overlay: review-note')) { $pass++ } else { $fail++; Say "T13 #$i FAIL notes=[$($notes -join ' | ')]" }
            }
            [void](Shot 'T13-after')
            Say "T13 $(if ($fail -eq 0 -and $pass -gt 0) { 'PASS' } else { 'FAIL' }) pass=$pass fail=$fail skipped=$skip of $([Math]::Min($N, 10)) inputFailures=$([W]::Fail)"
        }
        'I33' {
            $extra = (OpenPath-Lines (Join-Path (ProjPath 'I33') 'notes.txt') 'file-view kind=text state=text') + @('act selectlines 2 2', 'wait 500')
            if (-not (Start-Case 'I33' $extra $false $true)) { return }
            $lang = if ((([W]::ConvMode($script:h)) -band 1) -eq 1) { 'hangul' } else { 'english' }
            [W]::Chord(@($VK_CONTROL, 0xA4), 0x4D)
            $opened = WaitState '(?m)^overlay: review-note' 3
            [void](Shot 'I33-dialog'); [W]::Tap($VK_ESC)
            $closed = WaitState '(?m)^overlay: none' 3
            $leakedFile = @(KeyLines).Count
            [W]::Click((Sx 600), (Sy 400)); Start-Sleep -Milliseconds 300
            [W]::Chord(@($VK_CONTROL, 0xA4), 0x4D); Start-Sleep -Milliseconds 600
            $noWindow = (State) -notmatch '(?m)^overlay: review-note'
            if (-not $opened) { $failures += 'Ctrl+Alt+M did not open the note window on a file tab' }
            if (-not $closed) { $failures += 'Esc did not close the note window' }
            if ($leakedFile -gt 0) { $failures += "keys reached the PTY while the file tab was active: $((KeyLines) -join ',')" }
            if (-not $noWindow) { $failures += 'the note window opened while the terminal was active' }
            Verdict 'I33' $failures $checks "inputLanguage=$lang keysToShellWhenTerminalActive=$(@(KeyLines).Count - $leakedFile) altGrNote=(on layouts where AltGr+M types a character - Ctrl+Alt+M is text there and the note window must NOT open; a CHECK on such a layout is expected, record the layout name)"
        }
        'I34' {
            $extra = OpenPath-Lines (Join-Path (ProjPath 'I34') 'notes.txt') 'file-view kind=text state=text'
            if (-not (Start-Case 'I34' $extra $false $true)) { return }
            # 편집기에 포커스가 있을 때(파일을 연 직후).
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x50)
            $fromEditor = WaitState '(?m)^overlay: quick-open' 3
            [void](Shot 'I34-editor'); [W]::Tap($VK_ESC)
            $closed = WaitState '(?m)^overlay: none' 3
            $leakedEditor = @(KeyLines).Count
            # 터미널이 활성일 때.
            [W]::Click((Sx 600), (Sy 400)); Start-Sleep -Milliseconds 300
            [W]::Chord(@($VK_CONTROL, $VK_SHIFT), 0x50)
            $fromTerminal = WaitState '(?m)^overlay: quick-open' 3
            [void](Shot 'I34-terminal'); [W]::Tap($VK_ESC)
            [void](WaitState '(?m)^overlay: none' 3)
            $leakedTerminal = @(KeyLines).Count - $leakedEditor
            [W]::Chord(@($VK_CONTROL), 0x50); Start-Sleep -Milliseconds 600
            $plain = @(KeyLines).Count - $leakedEditor - $leakedTerminal
            $noList = (State) -notmatch '(?m)^overlay: quick-open'
            if (-not $fromEditor) { $failures += 'Ctrl+Shift+P did not open the list while the editor had focus' }
            if (-not $fromTerminal) { $failures += 'Ctrl+Shift+P did not open the list while the terminal was active' }
            if (-not $closed) { $failures += 'Esc did not close the list' }
            if ($leakedEditor -gt 0 -or $leakedTerminal -gt 0) { $failures += "Ctrl+Shift+P reached the PTY: $((KeyLines) -join ',')" }
            if ($plain -lt 1) { $failures += 'plain Ctrl+P did not reach the shell' }
            if (-not $noList) { $failures += 'plain Ctrl+P opened the list' }
            Verdict 'I34' $failures $checks ''
        }
        'I35' {
            $extra = @('key ctrl-shift-f', 'waitstate search open=y @5000', 'act search needle', 'waitstate state=done @20000', 'act replacetext thread', 'wait 800')
            if (-not (Start-Case 'I35' $extra)) { return }
            $file = Join-Path $script:proj 'a-first.txt'
            $before = [IO.File]::ReadAllText($file)
            [void](Shot 'I35-before')
            [W]::Click((Sx 241), (Sy ((HeaderY) + 122))); Start-Sleep -Milliseconds 700
            $asked = WaitState '(?m)^overlay: replace-all' 3
            [void](Shot 'I35-confirm')
            [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 2500
            $after = [IO.File]::ReadAllText($file)
            if (-not $asked) { $checks += 'the Replace All button was not hit (coordinates?) - see I35-before.png' }
            if ($asked -and $after -eq $before) { $failures += 'confirming Replace All did not change the file' }
            if ($asked -and $after -match 'needle') { $failures += "a match is left in a-first.txt: $after" }
            Verdict 'I35' $failures $checks ''
        }
        'I36' {
            # 시험 홈은 임시지만 Codex 설정 폴더만 사용자의 실제 폴더를 가리킨다(`codex app-server`가 실제 로그인을 쓰도록).
            $caseHome = Join-Path $HomeDir 'I36'
            New-Item -ItemType Directory -Force (Join-Path $caseHome '.paddock\native') | Out-Null
            [IO.File]::WriteAllText((Join-Path $caseHome '.paddock\native\state.json'), '{"version":3,"daemon":"seed","usage_selection":{"codex":{"provider":"codex"}}}')
            $realCodex = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex'
            $env:CODEX_HOME = $realCodex
            $started = Start-Case 'I36' @('waitstate codex=Default: @60000', 'wait 1000', 'state i36')
            Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue
            if (-not $started) { return }
            [void](Shot 'I36-status-bar')
            $state = Get-Content (Join-Path $script:caseOut 'state-i36.txt') -Raw -ErrorAction SilentlyContinue
            if ($state -notmatch '(?m)^usage .*codex=Default:\S*=\d+') { $failures += 'the Codex gauge shows no value (is Codex signed in? see state-i36.txt)' }
            $checks += 'compare I36-status-bar.png with the Codex gauge in the Electron status bar (same account, same values?)'
            Verdict 'I36' $failures $checks ''
        }
        'I37' {
            $caseHome = Join-Path $HomeDir 'I37'
            New-UsageFixture $caseHome
            $extra = @('waitstate claude=Alpha One @30000')
            if (-not (Start-Case 'I37' $extra)) { return }
            [void](Shot 'I37-before')
            [W]::Click((SxR 150), ($script:fr.B - [int](14 * $script:fr.S))); Start-Sleep -Milliseconds 800
            $opened = WaitState '(?m)^usage-popup open=yes' 4
            [void](Shot 'I37-popup')
            if (-not $opened) { $checks += 'clicking the status bar usage group did not open the list (coordinates?) - see I37-before.png' }
            if ($opened) {
                [W]::Tap($VK_ESC)
                $closed = WaitState '(?m)^usage-popup open=no' 3
                if (-not $closed) { $failures += 'Esc did not close the usage list' }
                if (@(KeyLines).Count -gt 0) { $failures += "the keys reached the PTY: $((KeyLines) -join ',')" }
            }
            Verdict 'I37' $failures $checks 'Open in new terminal is clicked by hand: see I37-popup.png'
        }
        'I38' {
            # 실제 계정 폴더·로그인 파일은 시험 홈에 둘 수 없다(앱은 링크로 바뀐 폴더를 읽지 않는다). 사람이 실제 홈으로 한 번 확인한다.
            Say 'I38 SKIP (reads the real account folders and login files, which a test home cannot hold: start the app normally with its real HOME, open a default claude terminal, and compare the account name in its status bar with the Electron status bar; the app only reads those files)'
        }
        'I39' {
            # 실제 WSL에 부하를 거는 일은 사용자 PC 메모리를 채우므로 시험 스크립트가 하지 않는다. 사람이 승인해 직접 한다.
            Say 'I39 SKIP (fills real WSL memory, so a person runs it: open Paddock with a WSL terminal, run `stress-ng --vm 1 --vm-bytes 85% --timeout 60s` or `tail /dev/zero` in WSL until the Memory group passes 90%, then check that one warning band appears, Go to <terminal> selects the heaviest WSL terminal of that window, and the band does not come back until usage falls below 80% and rises again; stop the load afterwards)'
        }
        'I40' {
            $caseHome = Join-Path $HomeDir 'I40'
            New-Item -ItemType Directory -Force (Join-Path $caseHome '.paddock\native') | Out-Null
            $started = Start-Case 'I40' @('waitstate usage-setup computer state=on @60000', 'state i40')
            if (-not $started) { return }
            $configDir = Join-Path $script:caseHome '.paddock\config'
            $flagFile = Join-Path $configDir 'claude\flag-settings.json'
            $appText = [IO.File]::ReadAllText($flagFile)
            $command = ($appText | ConvertFrom-Json).statusLine.command
            if (-not $command) { $failures += 'the app wrote no status line command (node missing?)' }
            if ($command) {
                $fake = '{"model":{"display_name":"I40"},"rate_limits":{"five_hour":{"used_percentage":12,"resets_at":9999999999}}}'
                $env:CLAUDE_CONFIG_DIR = $null
                $output = $fake | & cmd.exe /c $command
                $usageFile = Join-Path $configDir 'usage\claude.json'
                if (-not (Test-Path $usageFile) -or ((Get-Content $usageFile -Raw) -notmatch '"used_percentage":12')) { $failures += 'the status line command did not write the fake usage into the test home' }
                if ("$output" -notmatch 'I40') { $failures += "the status line command printed [$output]" }
            }
            # 앱이 쓴 파일의 설정 폴더 경로만 실제 것으로 바꾸어 사용자의 실제 파일(읽기만)과 바이트를 견준다.
            $realFlag = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.paddock\config\claude\flag-settings.json'
            if (Test-Path $realFlag) {
                $testJson = $configDir.Replace('\', '\\'); $realJson = (Split-Path (Split-Path $realFlag)).Replace('\', '\\')
                $expected = $appText.Replace($testJson, $realJson)
                if ([IO.File]::ReadAllText($realFlag) -ne $expected) { $checks += 'the real flag-settings.json differs from what this app writes for the same paths (memory notice on, or the Electron app writes different bytes?) - compare by hand' }
            } else { $checks += 'no real flag-settings.json to compare' }
            Verdict 'I40' $failures $checks ''
        }
        'T14' {
            # WSL 쓰기는 임시 폴더로 가둔다(자동 점검 빌드는 PADDOCK_TEST_WSL_HOME이 있을 때만 WSL 안에 쓴다).
            $wslHome = '/tmp/pdk-night-t14-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
            & wsl.exe --exec sh -c 'mkdir -p "$1"' sh $wslHome
            $env:PADDOCK_TEST_WSL_HOME = $wslHome
            try {
                $started = Start-Case 'T14' @('waitstate overlay: wsl-launcher @60000')
                if (-not $started) { return }
                $pass = 0; $fail = 0
                [void](SetHangul $true)
                Typ $HG; [W]::Tap($VK_ENTER); Start-Sleep -Milliseconds 900
                $rcLines = (& wsl.exe --exec sh -c 'grep -c paddock-claude-wrapper "$1/.bashrc" 2>/dev/null; true' sh $wslHome | Select-Object -First 1)
                if (((State) -match 'wsl-launcher=ask') -and ("$rcLines" -notmatch '^[1-9]')) { $pass++ } else { $fail++; Say "T14 FAIL settings=[$((State) -split "`n" | Where-Object { $_ -like 'usage-setup-settings*' })] bashrcLines=[$rcLines]" }
                [void](Shot 'T14-after')
                Say "T14 $(if ($fail -eq 0 -and $pass -gt 0) { 'PASS' } else { 'FAIL' }) pass=$pass fail=$fail inputFailures=$([W]::Fail)"
            } finally {
                Remove-Item Env:PADDOCK_TEST_WSL_HOME -ErrorAction SilentlyContinue
                if ($wslHome -like '/tmp/pdk-night-t14-*') { & wsl.exe --exec rm -rf $wslHome }
            }
        }
        'I41' {
            # 계정 A·B와 가짜 claude(bash)는 WSL 임시 홈에 둔다(앱은 PADDOCK_TEST_WSL_HOME이 있을 때만 WSL 계정을 열고 그 셸의 HOME을 이 폴더로 덮는다).
            $wslHome = '/tmp/pdk-night-i41-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
            $caseHome = Join-Path $HomeDir 'I41'
            New-Item -ItemType Directory -Force $caseHome | Out-Null
            # wsl.exe는 인자의 역슬래시를 지우므로 슬래시로 바꿔 경로를 넘긴다.
            $fixture = (& wsl.exe wslpath -a ((Join-Path $PSScriptRoot 'assets\account-switch-wsl-fixture.sh') -replace '\\', '/') | Select-Object -First 1)
            $wslCase = (& wsl.exe wslpath -a ($caseHome -replace '\\', '/') | Select-Object -First 1)
            & wsl.exe --exec env "WSLTEST=$wslHome" bash $fixture $wslCase
            $env:PADDOCK_TEST_WSL_HOME = $wslHome
            try {
                $extra = @('act plusmenu', 'waitstate menu: new-terminal @5000', 'act menuitem Claude · A', 'waitfor STARTED @60000', 'waitstate account-switch switching=no conversation=aaaaaaaa @30000')
                if (-not (Start-Case 'I41' $extra)) { return }
                [void](Shot 'I41-before')
                # 계정 고르기(탭 줄 오른쪽 끝, 높이는 탭 줄)를 실제로 눌러 메뉴를 연다.
                [W]::Click((SxR 90), (Sy 50)); Start-Sleep -Milliseconds 700
                $opened = WaitState '(?m)^account-menu kind=continue' 3
                [void](Shot 'I41-menu')
                if (-not $opened) { $checks += 'clicking the account picker did not open the Continue menu (coordinates?) - see I41-before.png' }
                if ($opened) { $checks += 'click Continue with B by hand if the menu item coordinates differ: see I41-menu.png' }
                Verdict 'I41' $failures $checks ''
            } finally {
                Remove-Item Env:PADDOCK_TEST_WSL_HOME -ErrorAction SilentlyContinue
                if ($wslHome -like '/tmp/pdk-night-i41-*') { & wsl.exe --exec rm -rf $wslHome }
            }
        }
        'I43' {
            # 덩어리 36: 실제 마우스로 탭 우클릭 메뉴의 `Move to New Window`를 눌러 분리 창을 열고, 그 창에 타이핑·한글이 들어가는지 본다(가짜 셸 없이 기본 셸).
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000', 'act menu terminal 2', 'waitstate menu: terminal @5000')
            if (-not (Start-Case 'I43' $extra)) { return }
            [void](Shot 'I43-menu')
            # 메뉴는 자동 점검 고정 자리(300,200)에 열려 있다. 항목 높이는 캡처에서 잰 값이라 빗나가면 CHECK다.
            [W]::Click((Sx 380), (Sy 269)); Start-Sleep -Milliseconds 1500
            $opened = WaitState '(?m)^windows .* detached=2' 5
            [void](Shot 'I43-after')
            if (-not $opened) { $checks += 'clicking Move to New Window did not open the detached window (coordinates?) - see I43-menu.png' }
            else { $checks += 'type English and Hangul into the new window by hand (focus is on it): the text must appear in that window and not in the main window' }
            Verdict 'I43' $failures $checks ''
        }
        'T15' {
            # 덩어리 36: 창 사이 포커스 이동 중 한글 조합(D27 감수 T5b 30회). 두 창을 실제로 오가야 해서 사람이 확인한다.
            Say 'T15 SKIP (needs two real windows and focus moves: open a detached window with Move to New Window, type Hangul in the main window without finishing the syllable, click the detached window, then the reverse, 30 times; the half-typed syllable must land in the window it was typed in exactly once and never in the other window or twice)'
        }
        'I44' {
            # 덩어리 37: 실제 마우스로 탭을 창 밖에 끌어 놓아 분리 창을 만든다(36 첫 항목 ③: 창 밖 MouseUp이 창 밖 좌표로 오는지도 events.log로 읽는다).
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000')
            if (-not (Start-Case 'I44' $extra)) { return }
            [void](Shot 'I44-before')
            # 탭 줄의 둘째 탭 가운데(탭 폭 150, 첫 탭은 x 44부터)에서 창 오른쪽 밖 200px까지 끈다.
            [W]::Drag((Sx 270), (Sy $G.TabsY), (SxR -200), (Sy 300)); Start-Sleep -Milliseconds 1500
            $opened = WaitState '(?m)^windows .* detached=2' 5
            [void](Shot 'I44-after')
            if (-not $opened) { $checks += 'dragging tab 2 outside the window did not open a detached window (coordinates? or the outside MouseUp never arrived: see events.log for "tab-drag outside-release") - see I44-before.png' }
            else { $checks += 'by hand: drag the tab of the new window onto an edge of the main window (the blue box must show) and release: it returns as a new pane and the detached window closes; events.log has "tab-drag outside-release x= y=" with x beyond the window width' }
            Verdict 'I44' $failures $checks ''
        }
        'I45' {
            # 덩어리 37: 실제 마우스로 칸 가장자리 분할과 탭 줄 끼우기. 가장자리로 끌면 칸이 둘, 첫 탭 앞에 끌어 놓으면 순서가 바뀐다.
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000', 'key ctrl-shift-t', 'waittabs 3 @20000')
            if (-not (Start-Case 'I45' $extra)) { return }
            [void](Shot 'I45-before')
            [W]::Drag((Sx 420), (Sy $G.TabsY), (SxR 20), (Sy 300)); Start-Sleep -Milliseconds 1500
            $split = WaitState '(?m)^strip col1 tabs=3' 5
            [void](Shot 'I45-split')
            if (-not $split) { $checks += 'dragging tab 3 to the right edge of the body did not split the pane (coordinates?) - see I45-split.png' }
            else {
                [W]::Drag((Sx 270), (Sy $G.TabsY), (Sx 70), (Sy $G.TabsY)); Start-Sleep -Milliseconds 1500
                $moved = WaitState '(?m)^strip col0 tabs=2,1' 5
                [void](Shot 'I45-insert')
                if (-not $moved) { $checks += 'dropping tab 2 on the left half of tab 1 did not put it before tab 1 (coordinates?) - see I45-insert.png' }
            }
            $checks += 'by hand: press a non-Escape key (for example Shift) during a drag, then release without moving the mouse: the tab must still be placed (gpui may report the release as outside the window)'
            Verdict 'I45' $failures $checks ''
        }
        'I46' {
            # 덩어리 37: 배율이 다른 모니터 사이의 화면 좌표 변환(분리 창 → 본 창). 사용자 모니터 구성에 달려 자동으로 하지 않는다(CHECK, 준비까지).
            Say 'I46 CHECK (needs two monitors with different scaling: open a detached window with Move to New Window, move it to the other monitor, drag its tab onto the middle and an edge of the main window and onto a main-window tab row; the blue box must be where the pointer is (no offset) and the tab must land there; if the offset is large, drag back by pane only and use the menu for the tab row)'
        }
        'I47' {
            # 덩어리 38: 실제 마우스로 파일 탭을 창 밖으로 끌어 분리하고, 탭 우클릭 메뉴 `Move to New Window`로도 분리한다. 대본은 합성 좌표라 사람이 확인한다.
            Say 'I47 CHECK (needs a real mouse: open a text file from the file list, drag its tab outside the main window - a detached file window must open with the same text and the tab dot appears after typing; then right-click the file tab and pick Move to New Window; drag the detached tab back onto the main window or use its menu Move Back to Main Window)'
        }
        'T17' {
            # 덩어리 38: 분리 창 편집기의 실제 한글 IME 조합(preedit). 합성 입력은 확정 글자만 넣으므로 사람이 확인한다.
            Say 'T17 SKIP (needs the real Korean IME: detach a text file tab, type Hangul in the detached editor without finishing the syllable, then press Ctrl+S - the half-typed syllable must be committed once in the file, and the detached window title/tab dot must stay correct)'
        }
        'I48' {
            # 덩어리 38: 분리 창 터미널 찾기 띠의 Enter(다음 일치)·Shift+Enter(이전). 합성 Enter는 본 창 띠에서도 일치를 넘기지 않아 사람이 확인한다.
            Say 'I48 CHECK (needs real keys: detach a terminal, press Ctrl+Shift+H in the detached window, type a word that appears twice, then Enter and Shift+Enter - the N of M count must move next/previous; Esc closes the bar and keys go to the terminal)'
        }
        'T18' {
            # 덩어리 38: 첫 움직임이 곧바로 창 밖인 빠른 끌기와, 탭을 누른 뒤 움직이지 않고 놓기. 끌기 상태가 안 생기는 경우(gpui 이동 이벤트가 창 안에서 없음)를 사람이 가린다.
            Say 'T18 CHECK (needs a real mouse: flick a file tab and a terminal tab straight out of the window in one fast motion - a detached window should open, if nothing happens that is the known limit (use the tab menu); press a tab and release without moving - nothing must change; during a file tab drag press Esc then release - nothing must change)'
        }
        'T16' {
            # 덩어리 37: 끌기 중 Esc 뒤 놓으면 아무 일도 없고 끌기 상태가 남지 않는다(실제 gpui 끌기가 멈추는지도 본다). Alt+Tab(캡처 해제)은 사람이 한다.
            $extra = @('key ctrl-shift-t', 'waittabs 2 @20000')
            if (-not (Start-Case 'T16' $extra)) { return }
            [void](Shot 'T16-before')
            [W]::DragEsc((Sx 270), (Sy $G.TabsY), (SxR -200), (Sy 300)); Start-Sleep -Milliseconds 1500
            $none = WaitState '(?m)^windows .* detached=-' 3
            $clean = WaitState '(?m)^drag=none' 3
            $preview = WaitState '(?m)^dock-preview=none' 3
            [void](Shot 'T16-after')
            if (-not ($none -and $clean -and $preview)) { $failures += "after Esc and a release outside the window: detached-none=$none drag=none=$clean dock-preview=none=$preview" }
            $checks += 'by hand: start a drag, press Alt+Tab (capture release), come back: no blue box, no floating label, drag=none in state; a following click on a tab selects it normally'
            Verdict 'T16' $failures $checks ''
        }
        'I42' {
            # 실제 두 계정 폴더에 쓰는 시험이라 자동으로 하지 않는다(규칙: 실제 계정 자료는 읽기만).
            Say 'I42 SKIP (writes the real account folders, so a person runs it once: with two real Claude accounts in Accounts, start claude in account A, ask something, then pick Continue with B in the account picker; check that Claude in the same tab shows the earlier conversation and that `claude --resume <transcript path>` works in this Claude Code version; the app never reads conversation text)'
        }
        'OPEN' {
            $media = Join-Path (ProjPath 'OPEN') 'media\소리, 시험 file.wav'
            $extra = OpenPath-Lines $media 'file-view kind=media state=notice'
            if (-not (Start-Case 'OPEN' $extra)) { return }
            $procs = @(Get-Process | ForEach-Object { $_.Id })
            $c = PaneCenter
            [void](Shot 'OPEN-notice')
            [W]::Click((Sx ($c[0] + $G.NoticeDefaultDx)), (Sy ($c[1] + $G.NoticeDefaultDy)))
            Start-Sleep -Seconds 8
            $new = NewWindowsSince $procs
            $withName = @(Get-CimInstance Win32_Process | Where-Object { $procs -notcontains $_.ProcessId -and $_.CommandLine -and $_.CommandLine -match '시험 file' } | ForEach-Object { $_.Name })
            $titled = @($new | Where-Object { $_ -match '시험 file' })
            [void](Shot 'OPEN-after')
            $errors = @($new | Where-Object { $_ -match '\|(Windows|Error|오류)' })
            if ($errors.Count -gt 0) { $failures += "an error window appeared: $($errors -join ' ; ')" }
            if ($titled.Count -eq 0 -and $withName.Count -eq 0) { $checks += 'no new window or process names the file (the default player may not show the file name — look at the screen and OPEN-after.png)' }
            Verdict 'OPEN' $failures $checks "newWindows=[$($new -join ' ; ')] processesWithFileName=[$($withName -join ',')] (close the opened player yourself)"
        }
        default { Say "$test SKIP (unknown test name)" }
    }
}

foreach ($test in $Tests) {
    try { Invoke-Test $test } catch { Say "$test FAIL (exception: $($_.Exception.Message))" } finally { Stop-Case }
}
