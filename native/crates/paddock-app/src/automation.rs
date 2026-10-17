//! 자동 점검용 실행 대본: 파일의 명령을 순서대로 실행해 터미널에 입력하고 화면 상태를 파일로 남긴다.
//! `automation` 기능으로 빌드한 개발용 실행 파일에만 들어간다(release 제외).
//!
//! 대본의 한 줄은 `명령 인자`이고 `#`로 시작하는 줄과 빈 줄은 건너뛴다.
//!
//! | 명령               | 하는 일                                                               |
//! |--------------------|-----------------------------------------------------------------------|
//! | `wait MS`          | 잠시 기다린다                                                         |
//! | `send TEXT`        | 선택한 터미널에 입력한다(`\r` `\n` `\t` `\e` `\\` `\xHH` 풀이)       |
//! | `sendto POS TEXT`  | POS번째(1부터) 탭의 터미널에 입력한다. 그 탭이 보이지 않아도 된다   |
//! | `key STROKE`       | 창에 키 누름(`enter`, `ctrl-c`, `a` …)을 넣는다. 사람이 누른 것과 같은 키·글자 경로를 탄다 |
//! | `text TEXT`        | TEXT의 글자를 하나씩 `key`로 넣는다                                   |
//! | `waittabs N [@MS]` | 탭이 N개가 될 때까지 기다린다                                         |
//! | `waitoverlay NAME [@MS]` | 창 위의 묻기 창이 NAME(`none`·`close-terminal`·`quit`)이 될 때까지 기다린다 |
//! | `waitfor TEXT [@MS]` | 선택한 터미널 화면 어딘가에 TEXT가 나타날 때까지 기다린다           |
//! | `waitline TEXT [@MS]` | 화면의 한 줄이 TEXT와 같아질 때까지 기다린다(입력한 명령줄에 속지 않는다) |
//! | `resize W H`       | 창 크기를 논리 px로 바꾸고 격자가 바뀔 때까지 기다린다                |
//! | `dump NAME`        | 선택한 터미널의 화면 격자를 `dump-NAME.txt`로 남긴다                  |
//! | `dumptab POS NAME` | POS번째 탭의 화면 격자를 `dump-NAME.txt`로 남긴다                     |
//! | `waitstate TEXT [@MS]` | 창 상태 글(`state-live.txt`와 같은 것)에 TEXT가 나타날 때까지 기다린다 |
//! | `state NAME`       | 탭 목록(id·이름·셸 pid·프로그램·활동·미확인)·선택·묻기 창·소리 요청을 `state-NAME.txt`로 남긴다 |
//! | `daemonlist NAME`  | 데몬이 쥔 터미널 목록을 `daemon-list-NAME.txt`로 남긴다               |
//! | `files NAME`      | 네이티브 설정·기존 앱 설정·state.json의 내용을 `files-NAME.txt`로 남긴다(설정을 바꿔도 다른 파일이 그대로인지 비교한다) |
//! | `act ACTION`       | 사이드바·메뉴 조작을 사람이 누른 것과 같은 창 동작으로 실행한다(아래 표) |
//! | `closewindow`      | 사람이 창 닫기를 누른 것과 같은 경로로 창 닫기를 요청한다             |
//!
//! 파일 탭: 창 상태 글에 `file-pane ratio= tabs= active=`(지금 보는 묶음의 파일 칸), 모든 묶음의 `file-tab 묶음 종류 경로 [selected] [shown]`,
//! 선택한 탭의 `file-view kind= state= (lines= language= size= readonly=yes | reason= size=) external-button=yes|no` 줄이 붙는다.
//!
//! 미리보기: 선택한 탭이 Markdown이면 `file-view kind=markdown state=markdown view=file|preview|both lines= size= content-hash= preview-width= images=읽음/전체`,
//! 이미지면 `kind=image|svg state=image size=가로x세로 decoded= bytes= zoom=fit|actual shown=`, PDF면 `kind=pdf state=pdf pages= visible=1,2 cached= zoom=fit|set percent= renders= render-ms= bytes=`가 붙는다.
//! `act mdview file|preview|both`(Markdown 보기 단추), `act imgzoom`(이미지 더블클릭), `act pdfzoom in|out|fit`(`+`·`−`), `act pdfscroll N`(N쪽 맨 위로 스크롤), `act textscroll Y`(텍스트·Markdown 원문 편집기를 Y px 아래로 스크롤).
//!
//! 파일 목록: 창 상태 글에 `files-folder=`·`files-expanded=`·`files-loaded=`·`files-watch=`·`files-read-ms=`·`files-reads=`·`files-read-off-window=` 줄과 행마다
//! `file-row 깊이 dir|file 이름 [expanded]`(안내는 `file-notice 깊이 글`) 줄이 붙는다.
//!
//! Git 표시·Source control: 파일 행 줄 끝에 ` mark=<글자|-> color=<이름>`(바뀐 파일·폴더만), `view=work|scm`,
//! `scm state=ready|reading|none|unavailable repo= branch= merge= staged= changes= untracked= runs= generation= badge=`, 변경 행마다 `scm-row <구획> <글자> <경로>`,
//! 입력칸 아래 한 줄 `scm-note <글>`, 되돌리기 확인 창의 `discard-prompt <제목 문장> | <안내> | <단추>`, 앱이 띄운 git·`wsl.exe` 수 `git-calls local= wsl=`가 붙는다.
//! 추적하지 않는 새 파일 탭의 `file-view`에는 ` untracked=yes`, 비교 탭에는 `kind=diff state=diff files= added= removed=`가 붙는다.
//!
//! `act`의 동작: `workadd`(Work ＋), `envmenu`(▾ 환경 목록), `menu terminal N`(N번째 탭의 우클릭 메뉴),
//! `menu group N`(N번째 Unassigned 묶음 행), `menu folder N`(N번째 폴더 행), `menuitem 이름`(열린 메뉴의 항목),
//! `foldertoggle N`(폴더 행 누름), `folderadd N`(폴더 행 ＋), `showmore N`(`Show N more`), `selectgroup N`(묶음 행 누름),
//! `closegroup N`(묶음 행 ×), `selecttab N`(N번째 탭의 터미널 행 누름), `tabadd`(탭 줄 ＋), `confirm`·`cancel`(묻기 창 버튼), `quitstop`(종료 묻기의 Stop all and quit),
//! `plusmenu`(탭 ＋: 등록 계정이 있으면 메뉴, 없으면 바로 새 터미널)·`pickermenu`(탭 줄 오른쪽 계정 고르기 메뉴),
//! `accounts`(Accounts 창 열기)와 그 창 안의 `acct provider claude|codex`·`acct name 글자`·`acct add`·`acct showform`·
//! `acct rename N 글자`·`acct remove N`·`acct open N`·`acct close`(N은 목록의 N번째 계정),
//! `splitright`·`splitdown`(칸 나누기 단추), `sidebar`(사이드바 접기 단추), `setsplit PATH SIZE…`(칸 경계를 끌어 비율을 바꾼 결과;
//! PATH는 분할의 자리를 `0.1`처럼 점으로 이은 자식 번호이고 루트는 `-`),
//! 빠른 설정 `quick open`·`quick close`·`quick expand 줄`·`quick choose 줄 값`(줄: `theme`·`terminalfont`·`interfacefont`·`shell`·`onquit`·`restore`·`permissions`)·
//! `quick step zoom|fontsize 증감`·`quick sound`(소리 켬/끔)·`quick quitstop`(Quit and Stop All Terminals),
//! 파일 탭 `openfile N`(파일 목록 N번째 행을 눌러 연다)·`openpath 경로`(목록에 없는 파일도 연다)·`closefile`(선택한 파일 탭 닫기)·`filetab N`(N번째 파일 탭 선택)·
//! `resetgap`(창 스레드가 막힌 시간의 최댓값 `ui-max-gap-ms=`을 0으로 되돌린다), `notice reveal|astext|default`(안내 탭의 단추)·`setfileratio 비율`(파일 칸 경계를 끌어 비율을 바꾼 결과),
//! Source control `view work|scm`(사이드바 보기)·`stage N`·`unstage N`·`discard N`(확인 창을 띄움 — 승인·취소는 `confirm`·`cancel`)·`scmrow N`(행을 눌러 비교 탭이나 파일 탭을 연다)·
//! `stageall`·`unstageall`·`scmtoggle merge|staged|changes`·`gitrefresh`·`commit 메시지`(N은 구획 머리를 뺀 변경 행의 번호: stage는 병합·변경 행, unstage는 스테이징 행, discard는 변경 행, scmrow는 모든 행),
//! 파일 목록 `files N`(N번째 행 누름: 폴더는 접고 펼침), `filesmenu N [항목]`(N번째 행의 우클릭 메뉴를 열고 항목 이름이 있으면 고름),
//! `newfile 이름`·`newfolder 이름`(머리줄의 새 파일·새 폴더 단추를 누르고 이름을 넣어 확인), `filestoggle`(머리줄 접기 표지), `filesrefresh`(새로 고침).
//!
//! 대본과 별개로, 창 상태(탭·선택·묻기 창)가 바뀔 때마다 `state-live.txt`를 새로 쓴다. 바깥 실행기가 지금 어느 탭이
//! 선택돼 있는지 읽는 데 쓴다.
//! | `shot NAME`        | 창 캡처를 요청한다(`shot-NAME.req`; 바깥 실행기가 PNG로 만든다)       |
//! | `mem NAME`         | 메모리 표본을 요청한다(`mem-NAME.req`)                                |
//! | `net NAME`         | 앱 프로세스의 TCP 연결 목록 표본을 요청한다(`net-NAME.req` → `net.log`) |
//! | `mark TEXT`        | 이벤트 기록에 표지를 남긴다                                           |
//! | `quit`             | 앱을 끝낸다                                                           |
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::TermMode;
use paddock_protocol::message::TerminalState;

use crate::startup::connect_daemon;
use crate::terminal::grid::Grid;
use crate::terminal::session::list_terminals;

/// `waitfor`·`waitline`이 기다리는 기본 최대 시간.
const DEFAULT_WAIT_MS: u64 = 15_000;

/// 창 크기 변경 요청이 격자에 반영되기를 기다리는 최대 시간.
const RESIZE_WAIT: Duration = Duration::from_secs(5);

/// `key`·`text`로 쌓은 키 누름이 처리되기를 기다리는 최대 시간.
const KEY_WAIT: Duration = Duration::from_secs(5);

/// `shot` 뒤에 바깥 실행기가 캡처를 마칠 때까지 기다리는 시간.
const SHOT_SETTLE: Duration = Duration::from_millis(2500);

/// `mem` 뒤에 바깥 실행기가 표본을 뽑을 때까지 기다리는 시간.
const MEM_SETTLE: Duration = Duration::from_millis(1500);

/// `--offscreen` 창과 가상 화면 왼쪽 끝 사이의 여유(논리 px).
const OFFSCREEN_MARGIN: f32 = 100.0;

/// `act` 명령이 여는 메뉴의 창 안 위치(논리 px). 사람이 누른 자리를 흉내 낼 수 없으므로 어느 창 크기에서도 안에 들어오는
/// 고정 자리에서 열어, 대본마다 같은 자리의 메뉴를 캡처하게 한다.
pub mod menu_anchor {
    pub const X: f32 = 300.0;
    pub const Y: f32 = 200.0;
}

/// 가상 화면(모든 모니터를 합친 영역)의 왼쪽 끝에서 창이 통째로 벗어나는 x 좌표(논리 px).
///
/// 화면 배율이 1보다 크면 논리 좌표가 실제 화면 좌표보다 작게 쓰이므로(음수일수록 더 왼쪽) 배율과 무관하게 화면 밖이다.
///
/// Examples
/// --------
/// | 가상 화면 왼쪽 끝 | 창 폭 | 결과  |
/// |-------------------|-------|-------|
/// | 0                 | 1000  | -1100 |
/// | -1920             | 1000  | -3020 |
pub fn offscreen_x_for(
    virtual_left: i32,
    width: f32,
) -> f32 {
    virtual_left as f32 - width - OFFSCREEN_MARGIN
}

/// 지금 모니터 배치에서 `width` 폭의 창이 화면 밖에 놓이는 x 좌표(논리 px).
pub fn offscreen_x(width: f32) -> f32 {
    #[cfg(windows)]
    // SAFETY: 인자 없는 시스템 조회이며 포인터를 쓰지 않는다.
    let virtual_left = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows_sys::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN,
        )
    };
    #[cfg(not(windows))]
    let virtual_left = 0;
    offscreen_x_for(virtual_left, width)
}

static OUT: OnceLock<PathBuf> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

/// 결과 파일을 남길 폴더를 정한다.
pub fn init(out: PathBuf) {
    let _ = std::fs::create_dir_all(&out);
    let _ = OUT.set(out);
    let _ = START.set(Instant::now());
}

/// 이벤트 기록(`events.log`)에 한 줄 남긴다(프로세스 시작 뒤 ms).
pub fn event(message: &str) {
    if let Some(out) = OUT.get() {
        let ms = START.get().map(|start| start.elapsed().as_millis()).unwrap_or(0);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(out.join("events.log")) {
            let _ = writeln!(file, "[{ms:>7} ms] {message}");
        }
    }
}

/// 대본의 `send` 인자를 바이트로 푼다. 알 수 없는 `\` 뒤 글자는 그대로 둔다.
///
/// Examples
/// --------
/// | 입력           | 결과 바이트        |
/// |----------------|--------------------|
/// | `ls\r`         | `ls` + `0x0d`      |
/// | `\e[A`         | `0x1b` + `[A`      |
/// | `\x41`         | `A`                |
/// | `a\\b`         | `a\b`              |
pub fn unescape(text: &str) -> Vec<u8> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                'r' => out.push(b'\r'),
                'n' => out.push(b'\n'),
                't' => out.push(b'\t'),
                'e' => out.push(0x1b),
                '\\' => out.push(b'\\'),
                'x' if i + 2 < chars.len() => {
                    let hex: String = chars[i + 1..i + 3].iter().collect();
                    out.push(u8::from_str_radix(&hex, 16).unwrap_or(b'?'));
                    i += 2;
                }
                other => {
                    out.push(b'\\');
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
            }
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(chars[i].encode_utf8(&mut buf).as_bytes());
        }
        i += 1;
    }
    out
}

/// 화면 격자를 크기·커서·대체 화면 여부와 줄 내용이 담긴 글로 만든다.
pub fn dump_text(grid: &Grid) -> String {
    let lines = grid.visible_text();
    let term = grid.term.lock();
    let cursor = term.grid().cursor.point;
    let mut text = format!(
        "size: {}x{}\nhistory: {}\ncursor: row={} col={}\nalt_screen: {}\n----\n",
        term.columns(),
        term.screen_lines(),
        term.history_size(),
        cursor.line.0,
        cursor.column.0,
        term.mode().contains(TermMode::ALT_SCREEN)
    );
    for line in &lines {
        text.push_str(line);
        text.push('\n');
    }
    // 화면 위로 밀려난 줄은 구분선 뒤에 오래된 순으로 붙인다(되살린 화면이 새 셸의 화면 지우기로 밀려난 것을 확인한다).
    text.push_str("---- scrollback ----\n");
    drop(term);
    for line in grid.history_text() {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// `TEXT [@MS]` 인자를 글자와 최대 대기 시간으로 나눈다.
fn split_timeout(rest: &str) -> (&str, u64) {
    match rest.rsplit_once(" @") {
        Some((needle, ms)) => (needle, ms.parse().unwrap_or(DEFAULT_WAIT_MS)),
        None => (rest, DEFAULT_WAIT_MS),
    }
}

/// 조건이 참이 될 때까지 기다린다. 시간이 지나면 false다.
fn wait_until(
    timeout_ms: u64,
    mut condition: impl FnMut() -> bool,
) -> bool {
    let started = Instant::now();
    let mut ok = condition();
    while !ok && started.elapsed() < Duration::from_millis(timeout_ms) {
        std::thread::sleep(Duration::from_millis(10));
        ok = condition();
    }
    ok
}

fn write_request(
    kind: &str,
    name: &str,
) {
    if let Some(out) = OUT.get() {
        let _ = std::fs::write(out.join(format!("{kind}-{name}.req")), b"");
    }
}

/// 대본이 보기 쪽에 부탁하는 일을 쌓아 두는 자리. 창 보기의 갱신 루프가 꺼내 처리한다.
pub type KeyQueue = Arc<Mutex<VecDeque<String>>>;

/// 대본이 보는 탭 하나.
pub struct ProbeTab {
    pub id: u32,
    pub name: String,
    pub shell_pid: u32,
    pub grid: Arc<Grid>,
    /// 데몬이 알린 폴더·프로그램·활동. 아직 못 받았으면 None.
    pub state: Option<TerminalState>,
    /// 완료 알림을 확인하지 않아 미확인 표시가 붙어 있다.
    pub unseen: bool,
    /// 이 터미널이 속한 작업 폴더. Unassigned 묶음의 터미널이면 None.
    pub folder: Option<String>,
    /// 이 터미널이 속한 Unassigned 묶음의 시작 터미널. 작업 폴더의 터미널이면 None.
    pub group: Option<u32>,
}

/// 대본이 보는 창 상태. 창 보기가 갱신 루프마다 채운다.
#[derive(Default)]
pub struct Probe {
    pub tabs: Vec<ProbeTab>,
    pub selected: Option<u32>,
    /// 창 위의 묻기 창 이름: `none`·`close-terminal`·`quit`.
    pub overlay: &'static str,
    /// 완료 소리를 요청한 터미널 번호(요청 순서). 자동 점검은 소리를 내지 않고 요청만 센다.
    pub sound_requests: Vec<u32>,
    /// 지금 탭 줄이 보여 주는 터미널(탭 순서).
    pub view: Vec<u32>,
    /// 열려 있는 메뉴 이름: `none`·`terminal`·`group`·`folder`·`environments`.
    pub menu: &'static str,
    /// 사이드바 행 요약(위에서 아래로). 폴더·터미널·`more`·묶음 행과 첫 실행 안내 표시다.
    pub rows: Vec<String>,
    /// Accounts 창이 보이는 계정·양식·오류 요약(창이 닫혀 있으면 비어 있다). 계정 폴더 경로나 로그인 값은 싣지 않는다.
    pub accounts: Vec<String>,
    /// 지금 보는 묶음의 칸 배치(JSON 한 줄). 보이는 터미널이 없으면 빈 글자.
    pub layout: String,
    /// 왼쪽 사이드바가 접혀 있다.
    pub sidebar_collapsed: bool,
    /// 그리는 동안 잰 본문 크기(화면 크기 단위). 사이드바를 접으면 폭이 사이드바만큼 늘어난다.
    pub body: (f32, f32),
    /// 탭 묶음마다 가려진 쪽: (묶음 번호, 왼쪽에 가려진 탭이 있음, 오른쪽에 가려진 탭이 있음).
    pub overflow: Vec<(usize, bool, bool)>,
    /// 지금 적용된 화면 모양과 빠른 설정 패널 상태(한 줄 요약).
    pub look: String,
    /// 파일 목록 요약(위에서 아래로): 목록 폴더·접힘·읽기 상태 줄과 행마다 한 줄.
    pub files: Vec<String>,
}

impl Probe {
    fn selected_grid(&self) -> Option<Arc<Grid>> {
        self.tabs.iter().find(|tab| Some(tab.id) == self.selected).map(|tab| tab.grid.clone())
    }

    fn grid_at(
        &self,
        position: usize,
    ) -> Option<Arc<Grid>> {
        position.checked_sub(1).and_then(|index| self.tabs.get(index)).map(|tab| tab.grid.clone())
    }
}

/// 대본과 창 보기가 주고받는 자리 모음.
#[derive(Clone)]
pub struct Hooks {
    pub probe: Arc<Mutex<Probe>>,
    /// 창 크기 요청(논리 px). 창 보기가 적용한다.
    pub pending_resize: Arc<Mutex<Option<(f32, f32)>>>,
    /// 창에 넣을 키 누름.
    pub pending_keys: KeyQueue,
    /// 창 닫기 요청. 창 보기가 사람이 닫을 때와 같은 경로로 처리한다.
    pub close_window: Arc<AtomicBool>,
    /// 대본이 부탁한 사이드바·메뉴 조작(`act`). 창 보기가 꺼내 처리한다.
    pub pending_actions: KeyQueue,
    /// 데몬 실행 파일. `daemonlist`가 데몬에 연결할 때 쓴다.
    pub daemon_exe: PathBuf,
}

impl Hooks {
    pub fn new(daemon_exe: PathBuf) -> Self {
        Self {
            probe: Arc::default(),
            pending_resize: Arc::default(),
            pending_keys: Arc::default(),
            close_window: Arc::default(),
            pending_actions: Arc::default(),
            daemon_exe,
        }
    }
}

/// 대본을 별도 스레드에서 실행한다.
pub fn run(
    path: PathBuf,
    hooks: Hooks,
) {
    std::thread::spawn(move || match std::fs::read_to_string(&path) {
        Err(error) => event(&format!("script read failed: {error}")),
        Ok(text) => {
            for raw in text.lines() {
                let line = raw.trim_end_matches('\r');
                if !line.trim().is_empty() && !line.starts_with('#') {
                    run_command(line, &hooks);
                }
            }
            event("script finished");
        }
    });
}

/// 키 누름을 쌓고, 보기가 모두 꺼내 처리할 때까지 기다린다.
fn push_keys(
    pending_keys: &KeyQueue,
    keys: Vec<String>,
) {
    pending_keys.lock().unwrap().extend(keys);
    wait_until(KEY_WAIT.as_millis() as u64, || pending_keys.lock().unwrap().is_empty());
}

/// `text` 명령의 글자 하나를 `key` 명령 이름으로 바꾼다.
///
/// Examples
/// --------
/// | 글자 | 이름    |
/// |------|---------|
/// | `a`  | `a`     |
/// | ` `  | `space` |
/// | `한` | `한`    |
fn key_name_of(c: char) -> String {
    if c == ' ' { "space".to_string() } else { c.to_string() }
}

/// `POS 나머지` 인자를 탭 위치(1부터)와 나머지 글자로 나눈다. 위치가 숫자가 아니면 None이다.
///
/// Examples
/// --------
/// | 인자           | 결과                 |
/// |----------------|----------------------|
/// | `2 seq 1 100`  | `Some((2, "seq 1 100"))` |
/// | `x foo`        | `None`               |
fn split_position(rest: &str) -> Option<(usize, &str)> {
    let (position, text) = rest.split_once(' ').unwrap_or((rest, ""));
    position.parse().ok().map(|position| (position, text))
}

fn write_file(
    name: &str,
    text: String,
) {
    if let Some(out) = OUT.get() {
        let _ = std::fs::write(out.join(name), text);
    }
}

/// `state` 명령이 남기는 글. 탭 목록(프로그램·활동·미확인 포함)과 선택·묻기 창 상태, 소리 요청이다.
fn state_text(probe: &Probe) -> String {
    let view = probe.view.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let mut text = format!(
        "selected: {}\noverlay: {}\nmenu: {}\nview: {}\n",
        probe.selected.map_or("none".to_string(), |id| id.to_string()),
        probe.overlay,
        probe.menu,
        if view.is_empty() { "-" } else { view.as_str() }
    );
    for (index, tab) in probe.tabs.iter().enumerate() {
        let program = tab.state.as_ref().and_then(|state| state.program.as_deref()).unwrap_or("-");
        let activity = tab
            .state
            .as_ref()
            .and_then(|state| state.activity)
            .map_or("-".to_string(), |activity| format!("{activity:?}").to_lowercase());
        text.push_str(&format!(
            "tab {} id={} name={} pid={} program={program} activity={activity} unseen={} folder={} group={} cwd={} account={}\n",
            index + 1,
            tab.id,
            tab.name,
            tab.shell_pid,
            if tab.unseen { "yes" } else { "no" },
            tab.folder.as_deref().unwrap_or("-"),
            tab.group.map_or("-".to_string(), |root| root.to_string()),
            tab.state.as_ref().and_then(|state| state.cwd.as_deref()).unwrap_or("-"),
            Some(crate::agent_view::account_label(tab.state.as_ref())).filter(|label| !label.is_empty()).unwrap_or("-".to_string())
        ));
    }
    text.push_str(&format!(
        "sidebar: {}\nbody: {}x{}\nlayout: {}\n",
        if probe.sidebar_collapsed { "collapsed" } else { "shown" },
        probe.body.0.round(),
        probe.body.1.round(),
        if probe.layout.is_empty() { "-" } else { probe.layout.as_str() }
    ));
    text.push_str(&format!("look: {}\n", if probe.look.is_empty() { "-" } else { probe.look.as_str() }));
    for (key, hidden_left, hidden_right) in &probe.overflow {
        text.push_str(&format!("overflow {key} left={hidden_left} right={hidden_right}\n"));
    }
    for row in &probe.rows {
        text.push_str(&format!("row {row}\n"));
    }
    for line in &probe.files {
        text.push_str(&format!("{line}\n"));
    }
    for row in &probe.accounts {
        text.push_str(&format!("acct {row}\n"));
    }
    for id in &probe.sound_requests {
        text.push_str(&format!("sound-requested {id}\n"));
    }
    text
}

static SOUND_REQUESTS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// 완료 소리 요청을 기록한다. 소리 대신 이 기록이 점검 결과다.
pub fn sound_requested(id: u32) {
    SOUND_REQUESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(id);
    event(&format!("sound-requested {id}"));
}

/// 지금까지의 소리 요청(요청 순서).
pub fn sound_requests() -> Vec<u32> {
    SOUND_REQUESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// 창 상태가 바뀌었을 때만 `state-live.txt`를 새로 쓴다. 창 보기의 갱신 루프가 매번 부른다.
pub fn publish_state(probe: &Probe) {
    static LAST: Mutex<String> = Mutex::new(String::new());
    let text = state_text(probe);
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if *last != text {
        write_file("state-live.txt", text.clone());
        *last = text;
    }
}

/// `daemonlist` 명령이 남기는 글. 데몬이 쥔 터미널마다 한 줄이다.
fn daemon_list_text(hooks: &Hooks) -> String {
    let listed = connect_daemon(&hooks.daemon_exe).and_then(|connection| list_terminals(&connection));
    match listed {
        Ok(list) => {
            let mut text = format!("terminals: {}\n", list.terminals.len());
            for terminal in &list.terminals {
                text.push_str(&format!(
                    "id={} pid={} alive={} exit_code={}\n",
                    terminal.id,
                    terminal.pid,
                    terminal.alive,
                    terminal.exit_code.map_or("-".to_string(), |code| code.to_string())
                ));
            }
            text
        }
        Err(error) => format!("error: {error}\n"),
    }
}

/// `files` 명령이 남기는 글. 설정을 바꿔도 네이티브 설정 파일에만 값이 더해지고 기존 앱 설정과 state.json은 그대로인지 비교하는 데 쓴다.
fn files_text() -> String {
    let read = |path: PathBuf| std::fs::read_to_string(path).unwrap_or_else(|_| "(missing)".to_string());
    format!(
        "== native settings ==\n{}\n== legacy settings ==\n{}\n== state.json ==\n{}\n",
        read(paddock_protocol::shared_config::native_settings_path()),
        read(paddock_protocol::shared_config::legacy_settings_path()),
        read(paddock_protocol::daemon_info::native_dir().join("state.json"))
    )
}

fn run_command(
    line: &str,
    hooks: &Hooks,
) {
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    let selected_grid = || hooks.probe.lock().unwrap().selected_grid();
    match command {
        "wait" => std::thread::sleep(Duration::from_millis(rest.trim().parse().unwrap_or(100))),
        "send" => {
            if let Some(grid) = selected_grid() {
                grid.input(unescape(rest));
            }
        }
        "sendto" => match split_position(rest) {
            Some((position, text)) => {
                let grid = hooks.probe.lock().unwrap().grid_at(position);
                match grid {
                    Some(grid) => grid.input(unescape(text)),
                    None => event(&format!("sendto: no tab at position {position}")),
                }
            }
            None => event(&format!("sendto: bad arguments {rest:?}")),
        },
        "key" => push_keys(&hooks.pending_keys, vec![rest.trim().to_string()]),
        "text" => push_keys(&hooks.pending_keys, rest.chars().map(key_name_of).collect()),
        "mark" => event(&format!("mark: {rest}")),
        "waittabs" => {
            let (needle, timeout_ms) = split_timeout(rest);
            let wanted: usize = needle.trim().parse().unwrap_or(0);
            let started = Instant::now();
            let found = wait_until(timeout_ms, || hooks.probe.lock().unwrap().tabs.len() == wanted);
            event(&format!(
                "waittabs {wanted}: {} after {} ms",
                if found { "found" } else { "TIMEOUT" },
                started.elapsed().as_millis()
            ));
        }
        "waitoverlay" => {
            let (needle, timeout_ms) = split_timeout(rest);
            let started = Instant::now();
            let found = wait_until(timeout_ms, || hooks.probe.lock().unwrap().overlay == needle.trim());
            event(&format!(
                "waitoverlay {:?}: {} after {} ms",
                needle.trim(),
                if found { "found" } else { "TIMEOUT" },
                started.elapsed().as_millis()
            ));
        }
        "waitfor" | "waitline" => {
            let (needle, timeout_ms) = split_timeout(rest);
            let started = Instant::now();
            let found = wait_until(timeout_ms, || {
                selected_grid().is_some_and(|grid| {
                    let lines = grid.visible_text();
                    if command == "waitfor" {
                        lines.iter().any(|text| text.contains(needle))
                    } else {
                        lines.iter().any(|text| text.trim() == needle)
                    }
                })
            });
            event(&format!(
                "{command} {needle:?}: {} after {} ms",
                if found { "found" } else { "TIMEOUT" },
                started.elapsed().as_millis()
            ));
        }
        "resize" => {
            let sizes: Vec<f32> = rest.split_whitespace().filter_map(|part| part.parse().ok()).collect();
            if let ([width, height], Some(grid)) = (&sizes[..], selected_grid()) {
                let before = {
                    let term = grid.term.lock();
                    (term.columns(), term.screen_lines())
                };
                let started = Instant::now();
                *hooks.pending_resize.lock().unwrap() = Some((*width, *height));
                let changed = wait_until(RESIZE_WAIT.as_millis() as u64, || {
                    let term = grid.term.lock();
                    (term.columns(), term.screen_lines()) != before
                });
                event(&if changed {
                    let term = grid.term.lock();
                    format!(
                        "relayout {}x{} -> {}x{} in {} ms",
                        before.0,
                        before.1,
                        term.columns(),
                        term.screen_lines(),
                        started.elapsed().as_millis()
                    )
                } else {
                    "relayout: no change within 5 s".to_string()
                });
            }
        }
        "dump" => {
            if let Some(grid) = selected_grid() {
                write_file(&format!("dump-{}.txt", rest.trim()), dump_text(&grid));
            }
        }
        "dumptab" => {
            if let Some((position, name)) = split_position(rest) {
                let grid = hooks.probe.lock().unwrap().grid_at(position);
                if let Some(grid) = grid {
                    write_file(&format!("dump-{}.txt", name.trim()), dump_text(&grid));
                }
            }
        }
        "files" => write_file(&format!("files-{}.txt", rest.trim()), files_text()),
        "state" => write_file(&format!("state-{}.txt", rest.trim()), state_text(&hooks.probe.lock().unwrap())),
        "daemonlist" => write_file(&format!("daemon-list-{}.txt", rest.trim()), daemon_list_text(hooks)),
        "waitstate" => {
            let (needle, timeout_ms) = split_timeout(rest);
            let started = Instant::now();
            let found = wait_until(timeout_ms, || state_text(&hooks.probe.lock().unwrap()).contains(needle));
            event(&format!(
                "waitstate {needle:?}: {} after {} ms",
                if found { "found" } else { "TIMEOUT" },
                started.elapsed().as_millis()
            ));
        }
        "act" => {
            hooks.pending_actions.lock().unwrap().push_back(rest.trim().to_string());
            wait_until(KEY_WAIT.as_millis() as u64, || hooks.pending_actions.lock().unwrap().is_empty());
        }
        "closewindow" => {
            event("close window requested");
            hooks.close_window.store(true, Ordering::Relaxed);
        }
        "shot" => {
            write_request("shot", rest.trim());
            event(&format!("shot requested: {}", rest.trim()));
            std::thread::sleep(SHOT_SETTLE);
        }
        "mem" => {
            write_request("mem", rest.trim());
            std::thread::sleep(MEM_SETTLE);
        }
        "net" => {
            write_request("net", rest.trim());
            std::thread::sleep(MEM_SETTLE);
        }
        "quit" => {
            event("quit");
            std::process::exit(0);
        }
        other => event(&format!("unknown script command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offscreen_window_is_left_of_the_whole_virtual_screen() {
        assert_eq!(offscreen_x_for(0, 1000.0), -1100.0);
        // 왼쪽에 모니터가 더 있으면 그 모니터의 왼쪽 끝도 벗어난다.
        let x = offscreen_x_for(-1920, 1000.0);
        assert!(x + 1000.0 < -1920.0, "{x}");
    }

    #[test]
    fn test_unescape_decodes_control_sequences() {
        assert_eq!(unescape("ls\\r"), b"ls\r");
        assert_eq!(unescape("\\e[A"), b"\x1b[A");
        assert_eq!(unescape("\\x41"), b"A");
        assert_eq!(unescape("a\\\\b"), b"a\\b");
        assert_eq!(unescape("한글"), "한글".as_bytes());
        assert_eq!(unescape("\\q"), b"\\q");
    }

    #[test]
    fn test_key_name_of_names_space_and_keeps_other_chars() {
        assert_eq!(key_name_of('a'), "a");
        assert_eq!(key_name_of(' '), "space");
        assert_eq!(key_name_of('한'), "한");
    }

    #[test]
    fn test_split_position_reads_one_based_tab_position_and_text() {
        assert_eq!(split_position("2 seq 1 100"), Some((2, "seq 1 100")));
        assert_eq!(split_position("3"), Some((3, "")));
        assert_eq!(split_position("x foo"), None);
    }

    #[test]
    fn test_state_text_lists_selection_overlay_and_tabs() {
        let probe = Probe {
            tabs: vec![ProbeTab {
                id: 4,
                name: "terminal 1".to_string(),
                shell_pid: 77,
                grid: Arc::new(Grid::new(10, 3, |_| {}, |_, _| {})),
                state: None,
                unseen: false,
                folder: Some("/repo".to_string()),
                group: None,
            }],
            selected: Some(4),
            overlay: "none",
            sound_requests: Vec::new(),
            view: vec![4],
            menu: "none",
            rows: vec!["folder key=/repo name=repo n=1".to_string()],
            accounts: Vec::new(),
            layout: "{\"type\":\"pane\"}".to_string(),
            sidebar_collapsed: true,
            body: (1000.4, 500.0),
            overflow: vec![(0, false, true)],
            look: "theme=Mermaid Dark".to_string(),
            files: vec!["files-folder=/repo".to_string(), "file-row 0 dir src".to_string()],
        };
        assert_eq!(
            state_text(&probe),
            "selected: 4\noverlay: none\nmenu: none\nview: 4\n\
             tab 1 id=4 name=terminal 1 pid=77 program=- activity=- unseen=no folder=/repo group=- cwd=- account=-\n\
             sidebar: collapsed\nbody: 1000x500\nlayout: {\"type\":\"pane\"}\n\
             look: theme=Mermaid Dark\n\
             overflow 0 left=false right=true\n\
             row folder key=/repo name=repo n=1\n\
             files-folder=/repo\nfile-row 0 dir src\n"
        );
    }

    #[test]
    fn test_c_d42_state_text_shows_agent_state_unseen_marks_and_sound_requests() {
        use paddock_core::agent_model::ActivityState;
        let state = TerminalState {
            id: 4,
            cwd: None,
            program: Some("claude".to_string()),
            is_agent: true,
            activity: Some(ActivityState::Working),
            finished: false,
            account: None,
        };
        let probe = Probe {
            tabs: vec![ProbeTab {
                id: 4,
                name: "terminal 1".to_string(),
                shell_pid: 77,
                grid: Arc::new(Grid::new(10, 3, |_| {}, |_, _| {})),
                state: Some(state),
                unseen: true,
                folder: None,
                group: Some(4),
            }],
            selected: None,
            overlay: "none",
            sound_requests: vec![4, 4],
            view: Vec::new(),
            menu: "none",
            rows: Vec::new(),
            accounts: Vec::new(),
            layout: String::new(),
            sidebar_collapsed: false,
            body: (0.0, 0.0),
            overflow: Vec::new(),
            look: String::new(),
            files: Vec::new(),
        };
        let text = state_text(&probe);
        assert!(
            text.contains("program=claude activity=working unseen=yes folder=- group=4 cwd=- account=-\n"),
            "{text}"
        );
        assert!(text.ends_with("sound-requested 4\nsound-requested 4\n"), "{text}");
    }

    #[test]
    fn test_split_timeout_reads_optional_limit() {
        assert_eq!(split_timeout("hello @500"), ("hello", 500));
        assert_eq!(split_timeout("hello world"), ("hello world", DEFAULT_WAIT_MS));
    }

    #[test]
    fn test_dump_text_lists_size_cursor_and_lines() {
        let grid = Grid::new(10, 3, |_| {}, |_, _| {});
        let mut parser: alacritty_terminal::vte::ansi::Processor = alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *grid.term.lock(), "한글".as_bytes());
        let text = dump_text(&grid);
        assert!(text.starts_with("size: 10x3\nhistory: 0\ncursor: row=0 col=4\nalt_screen: false\n----\n"), "{text}");
        assert!(text.contains("\n한글\n"), "{text}");
    }
}
