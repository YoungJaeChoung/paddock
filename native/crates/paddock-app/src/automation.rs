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
//! | `waitnostate TEXT [@MS]` | 창 상태 글에 TEXT가 0.6초 동안 계속 보이지 않을 때까지 기다린다(「없음」 고정). 그렇게 되지 않으면 TIMEOUT |
//! | `nolinestart POS PREFIX` | POS번째 탭의 화면·스크롤백에 PREFIX로 시작하는 줄(`#`는 숫자 한 글자)이 없어야 통과한다. 있으면 TIMEOUT로 기록한다 |
//! | `waitstate TEXT [@MS]` | 창 상태 글(`state-live.txt`와 같은 것)에 TEXT가 나타날 때까지 기다린다 |
//! | `state NAME`       | 탭 목록(id·이름·셸 pid·프로그램·활동·미확인)·선택·묻기 창·소리 요청을 `state-NAME.txt`로 남긴다 |
//! | `daemonlist NAME`  | 데몬이 쥔 터미널 목록을 `daemon-list-NAME.txt`로 남긴다               |
//! | `files NAME`      | 네이티브 설정·기존 앱 설정·state.json의 내용을 `files-NAME.txt`로 남긴다(설정을 바꿔도 다른 파일이 그대로인지 비교한다) |
//! | `act ACTION`       | 사이드바·메뉴 조작을 사람이 누른 것과 같은 창 동작으로 실행한다(아래 표) |
//! | `closewindow`      | 사람이 창 닫기를 누른 것과 같은 경로로 창 닫기를 요청한다             |
//! | `keywin ID STROKE` | 터미널 ID의 분리 창에 키 누름을 넣는다(`key`와 같은 경로를 그 창에서 탄다) |
//! | `textwin ID TEXT`  | TEXT의 글자를 하나씩 `keywin`으로 분리 창에 넣는다                    |
//! | `shotwin ID NAME`  | 터미널 ID의 분리 창 캡처를 요청한다(`shotwin-ID-NAME.req` → `NAME.png`) |
//!
//! 분리 창: 창 상태 글에 `windows main=active|inactive main-activations=N detached=<터미널 번호,…|->`와 분리 창마다
//! `dwin ID title= size=WxH active=yes|no activations=N unseen=yes|no`, 탭마다 `place=main|detached`가 붙는다. `activations`는 그 창을 앞으로 가져오라고
//! 요청한 횟수다(화면 밖 실행은 창을 실제로 활성화하지 않고 요청만 센다). 동작은 `act movetowindow ID`(`Move to New Window`)·`act moveback ID`(`Move Back to Main Window`)·
//! `act activatewindow ID`(Work 행 `other window`을 누른 것)·`act windowclose ID`(분리 창 닫기)·`act detachedclose ID`(분리 창 탭의 ×)·`act resizewin ID W H`(분리 창 크기)·
//! `act detachsize W H`(이후 분리 창을 그 크기로, 화면 안에서 연다. `0 0`이면 기본), `act startwin ID 계정`(번호로 고른 터미널에서 계정으로 시작 — 선택한 탭이 아니어도 된다), `act detachedmenu ID`(분리 창 탭 우클릭 메뉴 열기)·`act detachedmenuitem ID 이름`(그 메뉴의 항목).
//!
//! 탭 끌기(합성 좌표 — 실제 마우스는 쓰지 않는다): 창 상태 글에 `dock-preview=<left|right|top|bottom|after|before>:<기준 탭>|none`(놓을 자리 미리보기)과
//! `drag=<끌고 있는 터미널 번호>|none`이 붙는다. `act dragtab ID pane REF left|right|top|bottom|center`(REF가 든 칸의 가장자리·가운데에 놓기)·
//! `act dragtab ID strip COL`(COL열 맨 위 칸의 탭 줄 빈 자리에 놓기 = 그 칸 끝)·`act dragtab ID tab REF before|after`(REF 탭 앞·뒤에 끼우기)·
//! `act dragtab ID outside`(창 밖에서 뗌 = 새 분리 창)·`act dragtab ID cancel`(Esc 뒤 창 밖에서 뗌 = 아무 일 없음), `act dragback ID pane REF MODE`·
//! `act dragback ID strip COL`(분리 창에서 시작한 끌기를 화면 좌표로 바꿔 본 창에 놓기), 놓지 않고 미리보기만 보려면 `act dragtab ID hold pane REF MODE`·`hold strip COL`·`hold tab REF before|after`(탭 줄의 끼움 선)(`act dragtab ID cancel`로 푼다).
//!
//! 파일 분리 창(경로에 공백이 없는 파일): 창 이름은 `file_<파일 이름>`(공백·`-`는 `_`) — `keywin`·`textwin`·`shotwin`의 창 번호 자리에 쓴다. 동작은
//! `act movetowindow file 키`(탭을 새 창으로)·`act moveback file 키`·`act windowclose file 키`·`act detachedclose file 키`(탭의 ×)·`act detachedmenu file 키`·
//! `act detachedmenuitem file 키 이름`·`act activatewindow file 키`·`act dragfile 번호 [inside]`(보는 묶음 파일 칸의 번호 탭을 창 밖에 놓기, `inside`는 칸 위에 놓기 = 아무 일 없음).
//! 상태 글에는 다른 창에 있는 탭이 `file-tab 묶음 종류 경로 place=detached`로(칸의 탭은 끝에 `place=main`),
//! `dfile 창이름 unsaved=yes|no title= size=WxH active= activations=N`과 `dfile-view 창이름 <file-view와 같은 요약>`으로 보인다.
//!
//! 파일 탭: 창 상태 글에 `file-pane ratio= tabs= active=`(지금 보는 묶음의 파일 칸), 모든 묶음의 `file-tab 묶음 종류 경로 [selected] [shown]`,
//! 선택한 탭의 `file-view kind= state= (lines= language= size= readonly=yes|no | reason= size=) unsaved=yes|no disk-changed=yes|no find-open=yes|no external-button=yes|no` 줄이 붙는다.
//! 편집: 창 상태 글에 `unsaved-files=N`·`disk-changed-files=N`·`status-save=unsaved|saved|-`·`goto-input=<글자|->`, 묻기 창 이름 `close-file`·`quit-unsaved`·`overwrite-file`·`goto-line`이 붙고,
//! `act save`·`act savechoice save|dont|cancel`·`act reload`·`act keepmine`·`act gotoline N`·`act findreplace 글`·`act focusfile`(편집기를 눌러 포커스를 준 것과 같다)을 받는다.
//! `external-button`은 바깥 텍스트 편집기 단추가 보이는지다(텍스트·Markdown·변경 비교 탭만 `yes`).
//! `content-hash`는 편집기 전체 글자를 훑어 비싸므로 `state`·`waitstate`·`waitnostate`가 읽는 동안에만 채워지고, 그 밖의 `state-live.txt`에서는 `-`다.
//!
//! 미리보기: 선택한 탭이 Markdown이면 `file-view kind=markdown state=markdown view=file|preview|both lines= size= content-hash= preview-width= images=읽음/전체`,
//! 이미지면 `kind=image|svg state=image size=가로x세로 decoded= bytes= zoom=fit|actual full=yes|no drawn-tiles=그려진 원본 조각 수 shown=`, PDF면 `kind=pdf state=pdf pages= visible=1,2 cached= zoom=fit|set percent= renders= render-ms= bytes=`가 붙는다.
//! `act mdview file|preview|both`(Markdown 보기 단추), `act imgzoom`(이미지 더블클릭), `act imgscroll X Y`(1:1 이미지를 왼쪽 위에서 X·Y 논리 px 떨어진 곳이 보이게 스크롤), `act pdfzoom in|out|fit`(`+`·`−`), `act pdfscroll N`(N쪽 맨 위로 스크롤), `act textscroll Y`(텍스트·Markdown 원문 편집기를 Y px 아래로 스크롤).
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
//! `foldertoggle N`(폴더 행 누름), `repotoggle N`(N번째 저장소 묶음 행 누름), `folderadd N`(폴더 행 ＋), `showmore N`(`Show N more`), `selectgroup N`(묶음 행 누름),
//! `closegroup N`(묶음 행 ×), `selecttab N`(N번째 탭의 터미널 행 누름), `tabadd`(탭 줄 ＋), `confirm`·`cancel`(묻기 창 버튼), `quitstop`(종료 묻기의 Stop all and quit),
//! `plusmenu`(탭 ＋: 등록 계정이 있으면 메뉴, 없으면 바로 새 터미널)·`pickermenu`(탭 줄 오른쪽 계정 고르기 메뉴),
//! `accounts`(Accounts 창 열기)와 그 창 안의 `acct provider claude|codex`·`acct name 글자`·`acct add`·`acct showform`·
//! `acct rename N 글자`·`acct remove N`·`acct open N`·`acct close`(N은 목록의 N번째 계정),
//! `splitright`·`splitdown`(칸 나누기 단추), `sidebar`(사이드바 접기 단추), `setsplit PATH SIZE…`(칸 경계를 끌어 비율을 바꾼 결과;
//! PATH는 분할의 자리를 `0.1`처럼 점으로 이은 자식 번호이고 루트는 `-`),
//! 빠른 설정 `quick open`·`quick close`·`quick expand 줄`·`quick choose 줄 값`(줄: `theme`·`terminalfont`·`interfacefont`·`shell`·`onquit`·`restore`·`permissions`)·
//! `quick step zoom|fontsize 증감`·`quick sound`(소리 켬/끔)·`quick quitstop`(Quit and Stop All Terminals),
//! 파일 탭 `openfile N`(파일 목록 N번째 행을 눌러 연다)·`openpath 경로`(목록에 없는 파일도 연다)·`closefile`(선택한 파일 탭 닫기)·`filetab N`(N번째 파일 탭 선택)·
//! `resetgap`(아래 창 스레드 지연 값 `ui-max-gap-ms=`·`ui-max-gap-cpu-ms=`·`ui-max-busy-ms=`을 0으로 되돌린다), `notice reveal|astext|default`(안내 탭의 단추)·`setfileratio 비율`(파일 칸 경계를 끌어 비율을 바꾼 결과),
//! Source control `view work|scm`(사이드바 보기)·`stage N`·`unstage N`·`discard N`(확인 창을 띄움 — 승인·취소는 `confirm`·`cancel`)·`scmrow N`(행을 눌러 비교 탭이나 파일 탭을 연다)·
//! `stageall`·`unstageall`·`scmtoggle merge|staged|changes`·`gitrefresh`·`commit 메시지`(N은 구획 머리를 뺀 변경 행의 번호: stage는 병합·변경 행, unstage는 스테이징 행, discard는 변경 행, scmrow는 모든 행),
//! 파일 목록 `files N`(N번째 행 누름: 폴더는 접고 펼침), `filesmenu N [항목]`(N번째 행의 우클릭 메뉴를 열고 항목 이름이 있으면 고름),
//! `newfile 이름`·`newfolder 이름`(머리줄의 새 파일·새 폴더 단추를 누르고 이름을 넣어 확인), `filestoggle`(머리줄 접기 표지), `filesrefresh`(새로 고침).
//!
//! 창 스레드 지연: 창 상태 글의 `ui-max-busy-ms=`가 「창 스레드가 일하느라 막힌 시간」의 값이고, 막힘 판정(예: 100 ms 초과)은 이 값으로 한다.
//! 값은 갱신 루프(16 ms 타이머) 한 번과 다음 사이에 창 스레드가 CPU를 쓴 시간의 최댓값이다(운영체제 시계 눈금 약 15.6 ms 단위).
//! `ui-max-gap-ms=`는 그 루프 사이 벽시계 간격의 최댓값이라 타이머가 늦게 깨운 시간도 들어간다. 그 간격 동안의 CPU 시간이 `ui-max-gap-cpu-ms=`이고,
//! 간격은 긴데 이 값이 0이면 창 스레드는 막힌 것이 아니라 잠들어 있었다. 막힘 판정에 `ui-max-gap-ms=`를 쓰지 않는다.
//! 80 ms 넘게 루프가 멈추면 감시 스레드가 `events.log`에 `ui-stall silent= bg-gap= ping=`을 적는다(`ping`은 창 스레드에 빈 메시지를 동기로 보내 답이 온 시간 —
//! 0~3 ms면 창 스레드는 놀고 있었다). `pool-timer-late gap=`은 창 스레드와 무관하게 돌리는 같은 종류의 운영체제 타이머가 80 ms 넘게 늦은 때다.
//!
//! 폴더 검색: 창 상태 글에 `search open=y|n scope= query= n_results= n_files= capped=y|n state=idle|running|done|error runs= searched= options=case,word,regex|-` 줄,
//! 오류가 있으면 `search-error <글>`, 검색할 폴더가 없어 안내만 보이면 `search-notice <글>`, 일치 줄마다 `search-row <검색 폴더 기준 파일 경로> <줄>`(앞의 100개)이 붙는다. 텍스트 파일 탭의 `file-view`에는 커서가 있는 줄 `line=`이 붙는다.
//! `act search 질의`(검색 보기를 열고 질의를 넣어 실행)·`act searchrow N`(N번째 일치 줄을 눌러 파일 탭을 열고 그 줄로 감)·`act searchopt case|word|regex`(옵션 단추)·`act searchfile N`(N번째 파일 행 접기·펼침).
//! 키는 `key ctrl-shift-f`(검색 보기 열고 닫기)·`key escape`(입력칸에서 닫기)로 사람과 같은 경로를 탄다.
//!
//! 터미널 경로 링크: 창 상태 글에 선택한 터미널 화면에서 실제 파일을 가리키는 경로 글자마다 `term-link <터미널 번호> <경로> <줄|->`(화면 위에서 아래, 왼쪽에서 오른쪽 순서)가 붙고,
//! `act termlink N`이 N번째 링크를 Ctrl+클릭한 것과 같은 열기(파일 탭 + 줄 이동)를 한다. 마우스 올림·클릭 자체는 흉내 내지 않는다(야간 묶음에서 사람 손으로 확인).
//!
//! 동기화 동작: 창 상태 글에 `tab-bar-focus=yes|no`·`menu-highlight=<번호|->`·`sidebar-indent=<px>`·`copy-on-selection=on|off`, 열린 메뉴의 `menu-item N 이름`,
//! 터미널 찾기 `term-find open=y|n [terminal= query= count=3_of_12|No_results matches= offset=스크롤 줄 수 selection=]`, 지금 칸의 `file-tab-label N 이름`이 붙는다.
//! `act dblclick terminal N`(N번째 탭의 터미널 행 더블클릭)·`act dragrow N M before|after`(터미널 행 끌어 놓기)·`act showbeside N`(`Show to the Side`)·
//! `act paneadd P`(P번째 칸의 ＋)·`act tabbarfocus`(탭 줄 빈 자리 누름)·`act termfind 글`·`act termfindnext`·`act termfindprev`(터미널 찾기)·
//! `act quick step indent ±N`(Sidebar indent)·`act quick copyonselection`(Copy on selection 켬/끔). 메뉴 키(↑↓ Home End Enter)와 탭 줄 키(←→ Home End)는 `key`로 사람과 같은 경로를 탄다.
//!
//! 대본과 별개로, 창 상태(탭·선택·묻기 창)가 바뀔 때마다 `state-live.txt`를 새로 쓴다. 바깥 실행기가 지금 어느 탭이
//! 선택돼 있는지 읽는 데 쓴다.
//! | `shot NAME`        | 창 캡처를 요청한다(`shot-NAME.req`; 바깥 실행기가 PNG로 만든다)       |
//! | `mem NAME`         | 메모리 표본을 요청한다(`mem-NAME.req`)                                |
//! | `gpumem NAME`      | 같은 순간의 Private 바이트와 그래픽 메모리(DXGI local·non-local)·창이 그린 횟수(`Draws`)·틱이 다시 그리게 한 횟수(`Changed*`)·살아 있는 1 MiB 이상 블록의 크기 칸별 개수(`Big=2x7`은 2~3 MiB 블록 7개)를 `gpumem.log`에 한 줄로, 2 MiB 이상 블록의 할당 호출 스택(실행 파일 기준 오프셋)을 `bigsites.log`에 남긴다(Windows) |
//! | `net NAME`         | 앱 프로세스의 TCP 연결 목록 표본을 요청한다(`net-NAME.req` → `net.log`) |
//! | `mark TEXT`        | 이벤트 기록에 표지를 남긴다                                           |
//! | `quit`             | 앱을 끝낸다                                                           |
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

/// `waitnostate`가 글이 계속 보이지 않아야 하는 시간.
const ABSENT_FOR: Duration = Duration::from_millis(600);

/// `state`가 비싼 항목까지 채운 조립을 기다리는 최대 시간.
const STATE_WAIT: Duration = Duration::from_secs(1);

/// `key`·`text`로 쌓은 키 누름이 처리되기를 기다리는 최대 시간.
const KEY_WAIT: Duration = Duration::from_secs(5);

/// `shot` 뒤에 바깥 실행기가 캡처를 마칠 때까지 기다리는 시간.
const SHOT_SETTLE: Duration = Duration::from_millis(2500);

/// `mem` 뒤에 바깥 실행기가 표본을 뽑을 때까지 기다리는 시간.
const MEM_SETTLE: Duration = Duration::from_millis(1500);

/// `--offscreen`으로 실행했는지. 분리 창도 화면 밖에 활성화 없이 열고, 창 활성화는 요청만 센다.
static OFFSCREEN: AtomicBool = AtomicBool::new(false);

/// 자동 점검이 분리 창을 열 때 쓸 크기(논리 px, 폭×높이). 0이면 기본 크기다. 메모리를 창 크기별로 재는 데 쓴다.
static DETACH_SIZE: Mutex<(u32, u32)> = Mutex::new((0, 0));

/// 분리 창을 열 크기를 정한다(0, 0이면 기본 크기). 크기를 정하면 창을 화면 안(왼쪽 위 근처)에 연다 — 화면 밖 좌표는 시스템이 기본 크기로 되돌린다.
pub fn set_detach_size(size: (u32, u32)) {
    *DETACH_SIZE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = size;
}

/// `set_detach_size`로 정한 크기. 정하지 않았으면 None이다.
pub fn detach_size() -> Option<(u32, u32)> {
    let size = *DETACH_SIZE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// 화면 밖 실행인지 기록한다. 앱 시작 때 한 번 부른다.
pub fn set_offscreen(offscreen: bool) {
    OFFSCREEN.store(offscreen, Ordering::Relaxed);
}

/// 화면 밖 실행(`--offscreen`)인지.
pub fn is_offscreen() -> bool {
    OFFSCREEN.load(Ordering::Relaxed)
}

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

/// 호출한 스레드가 지금까지 CPU를 쓴 시간(커널+사용자). 창 스레드가 바빴는지 잠들어 있었는지 가르는 데 쓴다.
///
/// 운영체제가 시계 눈금(약 15.6 ms) 단위로 세므로 그보다 짧은 차이는 0으로 나올 수 있다.
/// Windows 밖에서는 읽을 수 없어 항상 0이다.
pub fn thread_cpu_time() -> Duration {
    #[cfg(windows)]
    let cpu = {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: 현재 스레드의 가짜 핸들과 이 함수 안 지역 변수 네 개만 쓴다.
        let ok = unsafe { GetThreadTimes(GetCurrentThread(), &mut created, &mut exited, &mut kernel, &mut user) };
        let ticks = |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        // FILETIME 한 눈금은 100 ns다.
        if ok != 0 { Duration::from_nanos((ticks(kernel) + ticks(user)) * 100) } else { Duration::ZERO }
    };
    #[cfg(not(windows))]
    let cpu = Duration::ZERO;
    cpu
}

/// 창 스레드가 마지막으로 갱신 루프를 돈 시각(프로세스 시작 뒤 ms). 감시 스레드가 읽는다.
static UI_TICK_MS: AtomicU64 = AtomicU64::new(0);

/// 창 스레드가 갱신 루프를 한 번 돌았음을 감시 스레드에 알린다.
pub fn mark_ui_tick() {
    let ms = START.get().map(|start| start.elapsed().as_millis() as u64).unwrap_or(0);
    UI_TICK_MS.store(ms, Ordering::Relaxed);
}

/// 갱신 루프가 80 ms 넘게 멈추면 그 사이 창 스레드가 막혔는지 잠들어 있었는지를 `events.log`에 남기는 감시 스레드를 띄운다.
///
/// 감시 스레드는 자기 잠(10 ms)이 제때 깨는지(`bg-gap`)와, 창 스레드에 빈 메시지를 동기로 보내 답이 오기까지 걸린 시간(`ping`)을 잰다.
/// 답이 바로 오면 창 스레드는 메시지를 기다리며 놀고 있던 것이고(깨우는 쪽이 늦은 것), 답이 늦으면 창 스레드가 다른 일에 매여 있던 것이다.
/// 창 스레드에서 한 번 호출한다. Windows 밖에서는 아무것도 하지 않는다.
pub fn watch_ui_thread() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;

        let ui_thread = unsafe { GetCurrentThreadId() };
        std::thread::spawn(move || ui_watchdog(ui_thread));
        start_pool_timer_probe();
    }
}

/// 창 스레드가 소유한 첫 최상위 창. 없으면 0.
#[cfg(windows)]
fn first_window_of(thread: u32) -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumThreadWindows;

    unsafe extern "system" fn first(
        hwnd: HWND,
        found: LPARAM,
    ) -> BOOL {
        // SAFETY: `found`는 아래에서 넘기는 지역 변수 `HWND`의 주소다.
        unsafe { *(found as *mut HWND) = hwnd };
        0
    }
    let mut hwnd: HWND = std::ptr::null_mut();
    // SAFETY: 콜백은 `hwnd` 지역 변수에만 쓰고, 열거는 이 호출 안에서 끝난다.
    unsafe { EnumThreadWindows(thread, Some(first), &mut hwnd as *mut HWND as LPARAM) };
    hwnd
}

/// 풀 타이머 탐침이 마지막으로 불린 시각(프로세스 시작 뒤 μs).
#[cfg(windows)]
static POOL_FIRED_US: AtomicU64 = AtomicU64::new(0);

/// 창 갱신 루프가 쓰는 것과 같은 운영체제 스레드 풀 타이머(16 ms)를 창 스레드와 무관하게 돌려, 타이머 자체가 늦게 불리는지 `events.log`에 남긴다.
///
/// 80 ms 넘게 늦으면 `pool-timer-late`를 적는다. 이 줄이 창 스레드의 `ui-stall`과 같은 때 나오면 지연은 창 스레드가 아니라 운영체제 타이머에 있다.
#[cfg(windows)]
fn start_pool_timer_probe() {
    use windows_sys::Win32::System::Threading::CreateThreadpoolTimer;

    // SAFETY: 콜백은 프로세스가 끝날 때까지 유효한 함수이고 컨텍스트는 쓰지 않는다.
    unsafe {
        let timer = CreateThreadpoolTimer(Some(pool_timer_fired), std::ptr::null_mut(), std::ptr::null());
        arm_pool_timer(timer);
    }
}

#[cfg(windows)]
unsafe fn arm_pool_timer(timer: windows_sys::Win32::System::Threading::PTP_TIMER) {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::SetThreadpoolTimer;

    // 음수 FILETIME은 100 ns 단위 상대 지연이다.
    let due = -(16 * 10_000i64) as u64;
    let due_time = FILETIME { dwLowDateTime: due as u32, dwHighDateTime: (due >> 32) as u32 };
    // SAFETY: `timer`는 살아 있는 풀 타이머이고 `due_time`은 이 호출 동안만 읽힌다.
    unsafe { SetThreadpoolTimer(timer, &due_time, 0, 0) };
}

#[cfg(windows)]
unsafe extern "system" fn pool_timer_fired(
    _instance: windows_sys::Win32::System::Threading::PTP_CALLBACK_INSTANCE,
    _context: *mut std::ffi::c_void,
    timer: windows_sys::Win32::System::Threading::PTP_TIMER,
) {
    let now_us = START.get().map(|start| start.elapsed().as_micros() as u64).unwrap_or(0);
    let last_us = POOL_FIRED_US.swap(now_us, Ordering::Relaxed);
    let gap_ms = now_us.saturating_sub(last_us) / 1000;
    if last_us != 0 && gap_ms > 80 {
        event(&format!("pool-timer-late gap={gap_ms}ms"));
    }
    // SAFETY: 이 콜백이 받은 타이머를 다시 건다.
    unsafe { arm_pool_timer(timer) };
}

#[cfg(windows)]
fn ui_watchdog(ui_thread: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_NULL};

    const SLEEP: Duration = Duration::from_millis(10);
    const SILENT: u64 = 80;
    let now_ms = || START.get().map(|start| start.elapsed().as_millis() as u64).unwrap_or(0);
    let mut last_wake = Instant::now();
    let mut pinged_for = 0;
    loop {
        std::thread::sleep(SLEEP);
        let bg_gap = last_wake.elapsed();
        last_wake = Instant::now();
        let tick = UI_TICK_MS.load(Ordering::Relaxed);
        let silent = now_ms().saturating_sub(tick);
        if tick != 0 && silent > SILENT && pinged_for != tick {
            pinged_for = tick;
            let hwnd = first_window_of(ui_thread);
            let started = Instant::now();
            let mut result = 0;
            // SAFETY: `hwnd`는 창 스레드의 창이고, 빈 메시지(WM_NULL)는 아무 일도 하지 않으며 결과 칸은 지역 변수다.
            let sent = unsafe { SendMessageTimeoutW(hwnd, WM_NULL, 0, 0, SMTO_ABORTIFHUNG, 3000, &mut result) };
            let ping = started.elapsed();
            event(&format!(
                "ui-stall silent={silent}ms bg-gap={}ms ping={}ms replied={}",
                bg_gap.as_millis(),
                ping.as_millis(),
                sent != 0
            ));
        }
    }
}

#[cfg(windows)]
mod gpu;

mod alloc;

/// 자동 점검 빌드는 Rust 쪽 살아 있는 바이트를 세는 할당기를 쓴다(`gpumem` 줄의 `RustLiveMB`).
#[global_allocator]
static ALLOCATOR: alloc::CountingAlloc = alloc::CountingAlloc;

/// 창이 그린 횟수와, 틱이 화면을 다시 그리게 한 횟수(원인별). `gpumem` 줄에 실려 구간별 차이로 읽는다.
static DRAWS: AtomicU64 = AtomicU64::new(0);
static CHANGED_USAGE: AtomicU64 = AtomicU64::new(0);
static CHANGED_MEMORY: AtomicU64 = AtomicU64::new(0);
static CHANGED_SETUP: AtomicU64 = AtomicU64::new(0);

/// 틱에서 화면을 다시 그리게 한 원인.
#[derive(Clone, Copy)]
pub enum Cause {
    Usage,
    Memory,
    Setup,
}

/// 창이 한 프레임을 그렸다.
pub fn count_draw() {
    DRAWS.fetch_add(1, Ordering::Relaxed);
}

/// 틱의 `pump_*`가 화면을 다시 그리게 했다.
pub fn count_changed(cause: Cause) {
    let counter = match cause {
        Cause::Usage => &CHANGED_USAGE,
        Cause::Memory => &CHANGED_MEMORY,
        Cause::Setup => &CHANGED_SETUP,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// 그린 횟수와 원인별 다시 그리기 횟수 한 줄(`gpumem` 줄 끝에 붙는다).
#[cfg(windows)]
fn draw_counts() -> String {
    format!(
        "Draws={} ChangedUsage={} ChangedMemory={} ChangedSetup={}",
        DRAWS.load(Ordering::Relaxed),
        CHANGED_USAGE.load(Ordering::Relaxed),
        CHANGED_MEMORY.load(Ordering::Relaxed),
        CHANGED_SETUP.load(Ordering::Relaxed)
    )
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

/// 창의 위치·크기를 GPUI가 말하는 값과 운영체제가 말하는 값으로 나란히 `events.log`에 남긴다. `bounds()` 원점이 창 바깥 모서리인지 그리는 영역
/// 모서리인지(제목줄을 제외하는지) 읽는 데 쓴다. `bounds`는 (x, y, 폭, 높이), `viewport`는 그리는 영역 크기이고 모두 논리 px이다.
#[cfg(windows)]
pub fn log_window_geometry(
    label: &str,
    hwnd: isize,
    bounds: (f32, f32, f32, f32),
    viewport: (f32, f32),
    scale: f32,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowInfo, WINDOWINFO};

    // SAFETY: 구조체는 0으로 채우고 크기만 적은 뒤 운영체제가 채운다. `hwnd`는 방금 GPUI 창에서 얻은 것이다.
    let info = unsafe {
        let mut info: WINDOWINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<WINDOWINFO>() as u32;
        GetWindowInfo(hwnd as *mut std::ffi::c_void, &mut info);
        info
    };
    event(&format!(
        "window-geometry {label} hwnd={hwnd} scale={scale} gpui-bounds=({:.0},{:.0}) {:.0}x{:.0} viewport={:.0}x{:.0} \
         os-window=({},{}) {}x{} os-client=({},{}) {}x{}",
        bounds.0,
        bounds.1,
        bounds.2,
        bounds.3,
        viewport.0,
        viewport.1,
        info.rcWindow.left,
        info.rcWindow.top,
        info.rcWindow.right - info.rcWindow.left,
        info.rcWindow.bottom - info.rcWindow.top,
        info.rcClient.left,
        info.rcClient.top,
        info.rcClient.right - info.rcClient.left,
        info.rcClient.bottom - info.rcClient.top,
    ));
}

/// 창의 운영체제 핸들(Windows HWND 숫자)을 `hwnd-NAME.txt`에 남기거나(`Some`) 지운다(`None`). 바깥 실행기가 분리 창을 캡처하는 데 쓴다.
/// 본 창은 `NAME`이 `main`이고 분리 창은 터미널 번호다.
pub fn note_window_handle(
    name: &str,
    hwnd: Option<isize>,
) {
    if let Some(out) = OUT.get() {
        let path = out.join(format!("hwnd-{name}.txt"));
        match hwnd {
            Some(hwnd) => {
                let _ = std::fs::write(path, hwnd.to_string());
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
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
    /// 이 터미널이 있는 창: 본 창이면 `main`, 분리 창이면 `detached`.
    pub place: &'static str,
}

/// 대본이 보는 분리 창 하나.
pub struct ProbeWindow {
    /// 이 창이 보이는 터미널 번호.
    pub id: u32,
    pub title: String,
    /// 마지막으로 그린 창 크기(논리 px).
    pub size: (f32, f32),
    pub active: bool,
    /// 본 창이 이 창을 앞으로 가져오라고 요청한 횟수.
    pub n_activation_requests: u32,
    /// 탭에 읽지 않은 완료 점이 있다.
    pub unseen: bool,
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
    /// 사이드바 행 요약(위에서 아래로). 저장소 묶음(`repo`)·폴더·터미널·`more`·Unassigned 묶음 행과 첫 실행 안내 표시다.
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
    /// 데몬 연결 상태: `same`(앱과 같은 데몬)·`outdated`(실행 파일만 다른 데몬을 계속 씀)·`-`(연결 중이거나 연결하지 못함).
    pub daemon: &'static str,
    /// 터미널 없이 사용자에게 알리는 안내 이름: `none`·`protocol-mismatch`·`protocol-mismatch-v1`(인사를 거절한 옛 데몬)·`stalled`·`failed`.
    pub notice: &'static str,
    /// 파일 목록 요약(위에서 아래로): 목록 폴더·접힘·읽기 상태 줄과 행마다 한 줄.
    pub files: Vec<String>,
    /// 동기화 동작의 줄(탭 줄 초점·메뉴 강조·메뉴 항목·터미널 찾기·사이드바 들여쓰기·선택 자동 복사·파일 탭 이름).
    pub sync: Vec<String>,
    /// 상태 줄 사용량 줄(`usage …`와 게이지마다 `usage-tip …`).
    pub usage: Vec<String>,
    /// 값이 비싼 항목(편집기 전체 글자의 해시 `content-hash`)까지 채운 조립이다. 대본의 `state`·`waitstate`·`waitnostate`가 읽는 동안에만 채우고,
    /// 아닐 때는 그 항목이 `-`다.
    pub detailed: bool,
    /// 열려 있는 분리 창(터미널 번호 순).
    pub windows: Vec<ProbeWindow>,
    /// 본 창이 활성인지.
    pub main_active: bool,
    /// 본 창을 앞으로 가져오라고 요청한 횟수.
    pub n_main_activation_requests: u32,
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
    /// 지금 상태 글을 읽는 중인 대본 명령의 수(`StateReader`). 0보다 크면 창 보기가 비싼 항목까지 채워 `Probe::detailed`로 표시한다.
    pub state_readers: Arc<AtomicUsize>,
    /// 분리 창에 넣을 키 누름(`터미널번호 키`). 창 보기가 갱신 루프 밖에서 그 창에 넣는다.
    pub window_keys: KeyQueue,
}

/// 대본 명령이 상태 글을 읽는 동안 쥐는 표지. 쥐고 있는 동안 창 보기가 비싼 항목까지 채운다.
struct StateReader<'a> {
    hooks: &'a Hooks,
}

impl<'a> StateReader<'a> {
    fn new(hooks: &'a Hooks) -> Self {
        hooks.state_readers.fetch_add(1, Ordering::Relaxed);
        Self { hooks }
    }
}

impl Drop for StateReader<'_> {
    fn drop(&mut self) {
        self.hooks.state_readers.fetch_sub(1, Ordering::Relaxed);
    }
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
            state_readers: Arc::default(),
            window_keys: Arc::default(),
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

/// `창이름 나머지` 인자를 분리 창의 이름(터미널 번호이거나 파일 분리 창의 `file_<이름>`)과 나머지 글자로 나눈다. 이름이 비면 None이다.
///
/// Examples
/// --------
/// | 인자                  | 결과                              |
/// |-----------------------|-----------------------------------|
/// | `2 ctrl-s`            | `Some(("2", "ctrl-s"))`           |
/// | `file_b.txt ctrl-end` | `Some(("file_b.txt", "ctrl-end"))` |
/// | ``                    | `None`                            |
fn split_window_name(rest: &str) -> Option<(&str, &str)> {
    let (name, text) = rest.split_once(' ').unwrap_or((rest, ""));
    (!name.is_empty()).then_some((name, text))
}

/// 줄이 앞 공백을 뗀 뒤 `prefix`로 시작하는지. `prefix`의 `#`는 숫자 한 글자와 맞는다.
fn line_starts_with(
    line: &str,
    prefix: &str,
) -> bool {
    let mut chars = line.trim_start().chars();
    prefix
        .chars()
        .all(|want| chars.next().is_some_and(|got| if want == '#' { got.is_ascii_digit() } else { got == want }))
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
        // 에이전트가 직접 알린 상태: `<상태>[:<이유>]`, 없으면 `-`.
        let signal = tab.state.as_ref().and_then(|state| state.signal.as_ref()).map_or("-".to_string(), |signal| {
            let state = format!("{:?}", signal.state).to_lowercase();
            signal.kind.as_ref().map_or(state.clone(), |kind| format!("{state}:{kind}"))
        });
        text.push_str(&format!(
            "tab {} id={} name={} pid={} program={program} activity={activity} signal={signal} unseen={} folder={} group={} cwd={} account={} place={}\n",
            index + 1,
            tab.id,
            tab.name,
            tab.shell_pid,
            if tab.unseen { "yes" } else { "no" },
            tab.folder.as_deref().unwrap_or("-"),
            tab.group.map_or("-".to_string(), |root| root.to_string()),
            tab.state.as_ref().and_then(|state| state.cwd.as_deref()).unwrap_or("-"),
            Some(crate::agent_view::account_label(tab.state.as_ref())).filter(|label| !label.is_empty()).unwrap_or("-".to_string()),
            tab.place
        ));
    }
    let detached_ids: Vec<String> = probe.windows.iter().map(|window| window.id.to_string()).collect();
    text.push_str(&format!(
        "windows main={} main-activations={} detached={}\n",
        if probe.main_active { "active" } else { "inactive" },
        probe.n_main_activation_requests,
        if detached_ids.is_empty() { "-".to_string() } else { detached_ids.join(",") }
    ));
    for window in &probe.windows {
        text.push_str(&format!(
            "dwin {} title={} size={}x{} active={} activations={} unseen={}\n",
            window.id,
            window.title,
            window.size.0.round(),
            window.size.1.round(),
            if window.active { "yes" } else { "no" },
            window.n_activation_requests,
            if window.unseen { "yes" } else { "no" }
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
    text.push_str(&format!("daemon={}\nnotice={}\n", probe.daemon, probe.notice));
    for (key, hidden_left, hidden_right) in &probe.overflow {
        text.push_str(&format!("overflow {key} left={hidden_left} right={hidden_right}\n"));
    }
    for row in &probe.rows {
        text.push_str(&format!("row {row}\n"));
    }
    for line in &probe.files {
        text.push_str(&format!("{line}\n"));
    }
    for line in &probe.sync {
        text.push_str(&format!("{line}\n"));
    }
    for line in &probe.usage {
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

/// `act clipboard`가 마지막으로 읽은 클립보드 종류(`text`·`files:N`·`image:<형식>`·`none`). 아직 읽지 않았으면 `-`다.
static LAST_CLIPBOARD: Mutex<String> = Mutex::new(String::new());

/// 클립보드를 읽은 결과를 기록한다. 클립보드를 바꾸지는 않는다(읽기만).
pub fn note_clipboard(kind: String) {
    *LAST_CLIPBOARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = kind;
}

/// `note_clipboard`가 마지막으로 기록한 값.
pub fn last_clipboard() -> String {
    let kind = LAST_CLIPBOARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    if kind.is_empty() { "-".to_string() } else { kind }
}

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
        "state" => {
            let _reader = StateReader::new(hooks);
            // 비싼 항목까지 채운 조립이 도착할 때까지 기다린 뒤 읽는다(못 받으면 마지막 조립을 그대로 쓴다).
            wait_until(STATE_WAIT.as_millis() as u64, || hooks.probe.lock().unwrap().detailed);
            write_file(&format!("state-{}.txt", rest.trim()), state_text(&hooks.probe.lock().unwrap()));
        }
        "daemonlist" => write_file(&format!("daemon-list-{}.txt", rest.trim()), daemon_list_text(hooks)),
        "waitstate" => {
            let (needle, timeout_ms) = split_timeout(rest);
            let started = Instant::now();
            let _reader = StateReader::new(hooks);
            let found = wait_until(timeout_ms, || {
                let probe = hooks.probe.lock().unwrap();
                probe.detailed && state_text(&probe).contains(needle)
            });
            event(&format!(
                "waitstate {needle:?}: {} after {} ms",
                if found { "found" } else { "TIMEOUT" },
                started.elapsed().as_millis()
            ));
        }
        "waitnostate" => {
            // 상태 글에 TEXT가 `ABSENT_FOR` 동안 계속 보이지 않아야 통과한다. 시간 안에 그렇게 되지 않으면 TIMEOUT이다(「없음」을 고정하는 점검).
            let (needle, timeout_ms) = split_timeout(rest);
            let started = Instant::now();
            let mut absent_since: Option<Instant> = None;
            let _reader = StateReader::new(hooks);
            let absent = wait_until(timeout_ms, || {
                let probe = hooks.probe.lock().unwrap();
                if !probe.detailed {
                    // 비싼 항목이 빠진 조립은 「없음」의 근거가 되지 못한다.
                    false
                } else if state_text(&probe).contains(needle) {
                    absent_since = None;
                    false
                } else {
                    absent_since.get_or_insert_with(Instant::now).elapsed() >= ABSENT_FOR
                }
            });
            event(&format!(
                "waitnostate {needle:?}: {} after {} ms",
                if absent { "absent" } else { "TIMEOUT" },
                started.elapsed().as_millis()
            ));
        }
        "nolinestart" => {
            // POS번째 탭의 화면·스크롤백에 PREFIX로 시작하는 줄이 하나도 없어야 통과한다(`#`는 숫자 한 글자). 키가 PTY로 새어 읽기 반복이 찍은 줄을 가려 낸다.
            if let Some((position, prefix)) = split_position(rest) {
                let grid = hooks.probe.lock().unwrap().grid_at(position);
                let leaked = grid.map(|grid| {
                    grid.visible_text()
                        .into_iter()
                        .chain(grid.history_text())
                        .filter(|line| line_starts_with(line, prefix.trim()))
                        .collect::<Vec<String>>()
                });
                match leaked {
                    Some(lines) if lines.is_empty() => {
                        event(&format!("nolinestart {position} {:?}: ok", prefix.trim()))
                    }
                    Some(lines) => {
                        event(&format!("nolinestart {position} {:?}: TIMEOUT found {lines:?}", prefix.trim()))
                    }
                    None => event(&format!("nolinestart {position}: TIMEOUT no such tab")),
                }
            }
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
        "shotwin" => match split_window_name(rest) {
            Some((id, name)) => {
                write_request("shotwin", &format!("{id}-{}", name.trim()));
                event(&format!("shotwin requested: {id} {}", name.trim()));
                std::thread::sleep(SHOT_SETTLE);
            }
            None => event(&format!("shotwin: bad arguments {rest:?}")),
        },
        "textwin" => match split_window_name(rest) {
            Some((id, text)) => {
                let mut queue = hooks.window_keys.lock().unwrap();
                queue.extend(text.chars().map(|c| format!("{id} {}", key_name_of(c))));
                drop(queue);
                wait_until(KEY_WAIT.as_millis() as u64, || hooks.window_keys.lock().unwrap().is_empty());
            }
            None => event(&format!("textwin: bad arguments {rest:?}")),
        },
        "keywin" => match split_window_name(rest) {
            Some((id, stroke)) => {
                hooks.window_keys.lock().unwrap().push_back(format!("{id} {}", stroke.trim()));
                wait_until(KEY_WAIT.as_millis() as u64, || hooks.window_keys.lock().unwrap().is_empty());
            }
            None => event(&format!("keywin: bad arguments {rest:?}")),
        },
        "mem" => {
            write_request("mem", rest.trim());
            std::thread::sleep(MEM_SETTLE);
        }
        "gpumem" => {
            #[cfg(windows)]
            {
                let line = gpu::sample_line(rest.trim());
                if let Some(out) = OUT.get()
                    && let Ok(mut file) =
                        std::fs::OpenOptions::new().create(true).append(true).open(out.join("gpumem.log"))
                {
                    let _ = writeln!(file, "{line}");
                }
            }
            #[cfg(not(windows))]
            event("gpumem: only available on Windows");
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
    fn test_line_starts_with_matches_a_prefix_where_hash_is_one_digit() {
        assert!(line_starts_with("  GOT-70", "GOT-#"));
        assert!(line_starts_with("GOT-1", "GOT-#"));
        assert!(!line_starts_with("(\"GOT-\" + [int]$k.Key) }", "GOT-#"));
        assert!(!line_starts_with("GOT-x", "GOT-#"));
        assert!(!line_starts_with("GOT", "GOT-#"));
    }

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
                place: "main",
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
            daemon: "same",
            notice: "none",
            files: vec!["files-folder=/repo".to_string(), "file-row 0 dir src".to_string()],
            sync: vec!["tab-bar-focus=no".to_string()],
            usage: Vec::new(),
            detailed: true,
            windows: Vec::new(),
            main_active: false,
            n_main_activation_requests: 0,
        };
        assert_eq!(
            state_text(&probe),
            "selected: 4\noverlay: none\nmenu: none\nview: 4\n\
             tab 1 id=4 name=terminal 1 pid=77 program=- activity=- signal=- unseen=no folder=/repo group=- cwd=- account=- place=main\n\
             windows main=inactive main-activations=0 detached=-\n\
             sidebar: collapsed\nbody: 1000x500\nlayout: {\"type\":\"pane\"}\n\
             look: theme=Mermaid Dark\ndaemon=same\nnotice=none\n\
             overflow 0 left=false right=true\n\
             row folder key=/repo name=repo n=1\n\
             files-folder=/repo\nfile-row 0 dir src\ntab-bar-focus=no\n"
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
            conversation: None,
            signal: Some(paddock_protocol::message::Signal {
                state: paddock_core::agent_model::StatusState::Blocked,
                kind: Some("permission".to_string()),
            }),
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
                place: "main",
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
            daemon: "outdated",
            notice: "protocol-mismatch",
            files: Vec::new(),
            sync: Vec::new(),
            usage: Vec::new(),
            detailed: false,
            windows: vec![ProbeWindow {
                id: 4,
                title: "terminal 1".to_string(),
                size: (900.4, 600.0),
                active: true,
                n_activation_requests: 2,
                unseen: true,
            }],
            main_active: true,
            n_main_activation_requests: 1,
        };
        let text = state_text(&probe);
        assert!(text.contains("\ndaemon=outdated\nnotice=protocol-mismatch\n"), "{text}");
        assert!(text.contains("windows main=active main-activations=1 detached=4\n"), "{text}");
        assert!(text.contains("dwin 4 title=terminal 1 size=900x600 active=yes activations=2 unseen=yes\n"), "{text}");
        assert!(
            text.contains(
                "program=claude activity=working signal=blocked:permission unseen=yes folder=- group=4 cwd=- account=- place=main\n"
            ),
            "{text}"
        );
        assert!(text.ends_with("sound-requested 4\nsound-requested 4\n"), "{text}");
    }

    #[test]
    fn test_split_window_name_takes_a_terminal_number_or_a_file_window_name() {
        assert_eq!(split_window_name("2 ctrl-s"), Some(("2", "ctrl-s")));
        assert_eq!(split_window_name("file_b.txt ctrl-end"), Some(("file_b.txt", "ctrl-end")));
        assert_eq!(split_window_name("file_b.txt"), Some(("file_b.txt", "")));
        assert_eq!(split_window_name(""), None);
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
