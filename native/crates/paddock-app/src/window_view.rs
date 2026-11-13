//! 앱 창의 뿌리 보기: Work 목록, 탭 줄, 선택한 터미널, 묻기 창, 연결 안내를 한 창에 모은다.
//!
//! 창은 데몬 연결을 기다리지 않고 먼저 뜬다. 연결·되살리기는 다른 스레드에서 하고, 끝나면 터미널 탭이 채워진다.
//! 터미널마다 보기(`TerminalView`) 하나를 만들되 선택한 탭의 보기만 그린다. 숨은 탭의 출력은 계속 격자에 쌓인다.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, DragMoveEvent, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render,
    ScrollHandle, Styled, Window, div, px,
};
use paddock_core::agent_account::Runtime;
use paddock_core::app_shortcut::{AppShortcut, app_shortcut, file_shortcut, preview_shortcut};
use paddock_core::entry_name::{EntryNameCheck, validate_entry_name};
use paddock_core::file_tree::{files_folder, initial_selection};
#[cfg(feature = "automation")]
use paddock_core::git_status::Change;
use paddock_core::key_encoding::{KeyInput, ShortcutScheme};
use paddock_core::layout_model::{Orientation, drag_boundary};
#[cfg(feature = "automation")]
use paddock_core::markdown_view::View;
use paddock_core::markdown_view::ViewChoices;
use paddock_core::menu_nav::NavKey;
use paddock_core::pane_columns::{split_body_width, strip_body_size};
use paddock_core::paste_paths::place_of;
use paddock_core::paths::file_name_of;
use paddock_core::settings_text::{step_font_size, step_memory_notice_mb, step_sidebar_indent, step_zoom_level};
use paddock_core::theme::{dialog as dialog_size, sidebar, split, status_bar, tab, text};
use paddock_core::unsaved::{UnsavedSet, needs_close_prompt};
use paddock_core::usage_scopes::{BarMode, bar_groups, bar_mode, search_visible};
use paddock_core::work_model::{Candidate, GroupKey, NewPlace, TabTarget, folder_name, tab_target};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Kill, PROTOCOL_VERSION, kind};
use serde_json::Value;

use crate::accounts::window::AccountsPanel;
use crate::app_log;
use crate::chrome::{ButtonKind, button, ui_family, zpx};
use crate::daemon_client::{Policy, request_shutdown, restart_stalled_daemon};
use crate::daemon_launch::sibling_daemon_exe;
use crate::done_notice::DoneNotice;
use crate::done_sound;
use crate::files::active::{SelectionChange, file_stays_active};
use crate::files::images::Graveyard;
use crate::files::menu_model::reveal_command;
use crate::files::open::FileStamp;
#[cfg(feature = "automation")]
use crate::files::open::NoticeAction;
use crate::files::pane::{FileContent, file_divider, file_pane};
use crate::files::state::FilesState;
use crate::git::state::{GitState, Repo};
use crate::look::colors;
pub use crate::overlay::Overlay;
use crate::windows::DetachedHandle;

/// 상태 줄 묶음 캐시의 열쇠: book 변경 번호, 분 단위 현재 시각, 켜진 도구(Claude·Codex).
type BarKey = (u64, i64, bool, bool);
use crate::panes::pane_area;
use crate::quick_settings::model::{Field, QuickState, Row, setting_of};
use crate::quick_settings::panel::quick_panel;
use crate::quit_policy::{
    OnQuit, QuitStep, may_save_state_on_close, quit_and_stop_all_step, quit_step, read_on_quit, write_on_quit,
};
use crate::scm::ScmState;
#[cfg(feature = "automation")]
use crate::scm::SidebarView;
use crate::search::SearchState;
use crate::settings::{Settings, key, write_native};
use crate::shell_choice::{Environment, available_environments, default_shell, folder_want};
use crate::startup::{
    OpenedTerminal, Startup, new_terminal_want, open_new_terminal, open_workspace, restore_terminals,
};
use crate::state_store;
use crate::tab_drag::{DragFileTab, DragTab};
use crate::tabs::{StripGeometry, overflow_look, scroll_by_step, tab_bar};
use crate::terminal::ime::SentComposition;
use crate::terminal::paint::{GlyphCache, hsla};
use crate::terminal::session::{TerminalSession, Want};
use crate::terminal::view::{OpenLink, TerminalView};
use crate::titlebar::titlebar_view;
use crate::usage::data::{StatusItems, UsageData};
use crate::work::list_view::work_list;
use crate::work::menu::menu_panel;
use crate::work::menu_model::{ContextMenu, MenuAction, MenuKind, clamped_position, panel_height};
use crate::work::repo_read::RepoReader;
use crate::work::terminal_row::RowTarget;
use crate::workspace::{TabRef, Workspace};

gpui_kit::actions!(paddock_window, [StepPreviousTab, StepNextTab, SearchInFolderFromEditor]);

/// 텍스트 보기 편집기가 쥔 `Ctrl+←/→`(단어 이동)를 앱의 탭 이동으로 되돌리는 키 연결을 등록한다. 앱 시작 때 한 번 부른다.
///
/// 키 연결은 키 누름 처리기보다 먼저 실행되므로, 편집기에 포커스가 있으면 `capture_key_down`의 앱 단축키가 이 두 키를 받지 못한다.
/// 읽기 전용 보기에서는 글자 단위 이동이 필요 없고 탭 이동은 터미널과 같은 키여야 하므로 편집기 문맥에서만 앱 동작으로 덮는다.
///
/// macOS는 탭 이동(Cmd+Shift+[ / ])을 편집기 부품이 쓰지 않아 덮을 키 연결이 없고 Ctrl+←/→는 앱이 가로채지 않는다.
/// 대신 편집기 부품이 쥔 Cmd+Shift+F(찾아 바꾸기)를 앱의 폴더 검색으로 덮고, 찾아 바꾸기는 Cmd+Option+F로 옮긴다(VS Code 맥 관례).
pub fn bind_keys(cx: &mut gpui_kit::App) {
    match ShortcutScheme::NATIVE {
        ShortcutScheme::Ctrl => cx.bind_keys([
            gpui_kit::KeyBinding::new("ctrl-left", StepPreviousTab, Some("Input && mode == editor")),
            gpui_kit::KeyBinding::new("ctrl-right", StepNextTab, Some("Input && mode == editor")),
        ]),
        ShortcutScheme::Command => cx.bind_keys([
            gpui_kit::KeyBinding::new("cmd-shift-f", SearchInFolderFromEditor, Some("Input && mode == editor")),
            gpui_kit::KeyBinding::new(
                "cmd-alt-f",
                gpui_kit::component::input::Replace,
                Some("Input && mode == editor"),
            ),
        ]),
    }
}

/// 터미널 상태(에이전트 활동·완료 알림)가 바뀌었는지 확인하는 간격. 바뀐 것이 있을 때만 목록을 다시 그린다.
const AGENT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 자동 점검이 상태 글 재료를 다시 조립하는 간격. 대본 동작·키·창 크기 변경을 처리한 틱은 간격과 무관하게 바로 조립한다.
/// 매 틱(16 ms)마다 조립하면 점검 빌드의 CPU가 사용자 빌드보다 훨씬 높게 잰다.
#[cfg(feature = "automation")]
const PROBE_INTERVAL: Duration = Duration::from_millis(50);

/// 키보드로 탭을 옮긴 뒤 선택한 탭을 보이게 하는 스크롤을 다시 적용하는 시간.
const REVEAL_SETTLE: Duration = Duration::from_millis(400);

/// 종료 때 `KILL` 요청이 데몬 쪽으로 쓰이기를 기다리는 최대 시간.
const KILL_FLUSH_WAIT: Duration = Duration::from_secs(2);

/// 탭 하나가 쥔 것: 데몬 세션과 그 터미널의 보기.
pub struct TabContent {
    pub session: TerminalSession,
    pub view: Entity<TerminalView>,
}

/// 터미널 없이 사용자에게 상황을 알리고 다음 행동을 고르게 하는 안내.
pub enum Notice {
    /// 실행 중인 터미널 데몬의 프로토콜 버전이 이 앱과 다르다. 교체하거나 앱을 끝낸다.
    /// `daemon_version`이 None이면 인사를 거절한 옛 데몬(버전 1)이라 교체가 프로세스를 강제로 끝낸다.
    ProtocolMismatch { daemon_version: Option<u32> },
    /// 실행 중인 터미널 데몬이 응답하지 않는다. 다시 시작하거나 앱을 끝낸다.
    DaemonStalled,
    /// 데몬에 연결하지 못했다. 서버나 시스템이 준 문장을 그대로 보인다.
    Failed(String),
}

/// 프로토콜 불일치 안내의 제목과 설명.
///
/// 인사를 거절한 옛 데몬(`daemon_version`이 None)은 종료 요청을 받지 못해 강제로 끝내야 하므로, 마지막 몇 초의 터미널 기록이
/// 사라질 수 있음을 설명에 더한다.
fn protocol_mismatch_text(daemon_version: Option<u32>) -> (String, String) {
    let headline = format!(
        "The running terminal daemon speaks protocol v{}; this Paddock needs v{PROTOCOL_VERSION}",
        daemon_version.unwrap_or(1)
    );
    let mut detail = "Replacing it ends the terminals it holds".to_string();
    if daemon_version.is_none() {
        detail.push_str(" (forced stop — the last few seconds of terminal history may be lost)");
    }
    detail.push_str(". Quit leaves it running untouched.");
    (headline, detail)
}

/// 창이 지금 보여 주는 단계.
enum Phase {
    /// 데몬에 연결해 터미널을 얻는 중이다. 문장은 무엇을 하는 중인지 알린다.
    Connecting(&'static str),
    Ready,
    Notice(Notice),
}

/// 이름 바꾸기 창이 바꿀 대상.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Terminal(u32),
    /// 작업 폴더의 목록 표시 이름(디스크의 폴더 이름은 그대로).
    Folder(String),
    /// 파일 목록의 파일·폴더 이름(디스크의 이름이 바뀐다).
    Entry {
        path: String,
        is_dir: bool,
    },
}

/// 터미널 선택이 바뀔 수 있는 연산 앞의 활성 상태.
pub(crate) struct ActiveSnapshot {
    was_file_active: bool,
    /// 그때 보이던 묶음의 터미널.
    group_ids: Vec<u32>,
}

/// 키를 받는 탭이 터미널인지 파일 탭인지. `Workspace::selected`는 "마지막으로 고른 터미널"이라 파일 탭이 활성이어도 그대로다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Active {
    Terminal,
    /// 이 묶음 파일 칸의 선택한 탭이 활성이다. 보는 묶음이 다른 곳으로 바뀌면(이름표가 다르면) 터미널이 활성인 것으로 본다.
    File(GroupKey),
}

/// 칸 경계 끌기를 시작할 때 알리는 값.
pub struct DividerStart {
    /// 끄는 경계가 속한 분할의 자리(자식 번호의 열).
    pub path: Vec<usize>,
    /// 경계 앞쪽 칸의 번호(`index`와 `index + 1`번째 칸 사이의 경계).
    pub index: usize,
    pub orientation: Orientation,
    /// 누른 자리의 화면 좌표(가로 분할은 x, 세로 분할은 y).
    pub position: f32,
    /// 터미널 배치와 파일 칸 사이의 경계다(이때 `path`·`index`는 쓰지 않는다).
    pub file: bool,
}

/// 끌고 있는 칸 경계. 끌기를 시작할 때의 크기 비율을 기준으로 끈 거리만큼 옮긴다.
struct DividerDrag {
    start: DividerStart,
    sizes: Vec<f64>,
    /// 끌기 시작 때 그 분할이 끌 방향으로 차지한 길이(화면 크기 단위).
    extent: f32,
}

/// 데몬에 연결하는 방식.
#[derive(Clone, Copy)]
enum Connect {
    /// 데몬에 연결한다. 데몬이 앱과 다르면 `policy`를 따른다.
    Open(Policy),
    /// 멈춘 데몬을 끝내고 새로 띄워 연결한다.
    RestartStalled,
}

/// 앱 창의 뿌리 보기.
pub struct WorkWindow {
    daemon_exe: Option<PathBuf>,
    phase: Phase,
    pub(crate) workspace: Workspace<TabContent>,
    pub(crate) cache: Rc<RefCell<GlyphCache>>,
    /// 키·포커스 이동으로 이미 내보낸 조합 글자의 기억. 이 창의 모든 터미널 보기가 함께 쥔다.
    pub(crate) already_sent: SentComposition,
    /// 터미널이 없을 때 키를 받는 자리.
    focus: FocusHandle,
    /// 묻기 창이 떠 있는 동안 키를 받는 자리. 터미널이 글자를 받지 못하게 포커스를 옮겨 둔다.
    pub(crate) overlay_focus: FocusHandle,
    pub(crate) overlay: Overlay,
    /// 열려 있는 우클릭·환경 고르기 메뉴.
    pub(crate) menu: Option<ContextMenu>,
    /// 이름 바꾸기 창의 입력칸.
    pub(crate) rename_input: Option<Entity<InputState>>,
    /// 사이드바 파일 목록의 상태(보이는 폴더·펼친 폴더·읽기 결과).
    pub(crate) files: FilesState,
    /// 디스크에 쓰는 모든 일(붙여넣기 복사·그림 저장·이름 바꾸기, 이어서 파일 저장·바꾸기)이 지나는 한 길.
    pub(crate) writes: crate::files::write_service::WriteService,
    /// 보이는 폴더의 Git 상태(변경 목록·표시 조회표·실행 시점 판정).
    pub(crate) git: GitState,
    /// Source control 보기 상태(사이드바 보기·커밋 입력칸·접은 구획).
    pub(crate) scm: ScmState,
    /// 폴더 검색 보기 상태(열림·질의 입력칸·결과).
    pub(crate) search: SearchState,
    /// 파일을 연 뒤 가야 할 줄(경로별, 1부터). 편집기가 처음 그려지면 그 줄로 스크롤하고 지운다.
    pub(crate) pending_lines: HashMap<String, usize>,
    /// 새 파일·새 폴더 창의 입력칸.
    pub(crate) entry_input: Option<Entity<InputState>>,
    /// 탭 줄에 초점이 있다(빈 자리를 눌렀을 때). ←·→·Home·End가 탭을 고르고, Enter·Esc·다른 누름·다른 키가 거둔다.
    pub(crate) tab_bar_focused: bool,
    /// 터미널 안 찾기 띠(`Ctrl+Shift+H`). 열려 있지 않으면 None.
    pub(crate) find: Option<crate::terminal::find_bar::FindBar>,
    /// 터미널 행을 끌어 오는 중 놓을 자리 표식: (대상 터미널 행, 행 앞/뒤).
    pub(crate) row_drop: Option<(u32, paddock_core::work_model::DropPosition)>,
    /// 키를 받는 탭: 터미널 또는 파일 탭.
    active: Active,
    /// 묶음마다 마지막으로 활성이었던 쪽이 파일 탭이었는지. 보이는 묶음이 저절로 바뀔 때(닫기) 그 묶음의 마지막 상태로 돌아가는 데 쓴다.
    file_active_memory: HashMap<GroupKey, bool>,
    /// `Open as text anyway`로 연 파일. 다시 읽을 때도 이진 판정을 건너뛴다(저장하지 않아 재실행하면 사라진다).
    pub(crate) forced_text: HashSet<String>,
    /// 열린 파일 탭의 내용(경로별). 같은 파일이 여러 묶음에 열려 있어도 한 벌을 나눠 쓴다.
    pub(crate) file_views: HashMap<String, FileContent>,
    /// 열린 파일이 마지막으로 읽힐 때의 쓴 시각·크기. 달라지면 다시 읽는다.
    pub(crate) file_stamps: HashMap<String, Option<FileStamp>>,
    /// 열린 파일의 변경 확인을 마지막으로 시작한 시각과 확인이 도는 중인지.
    pub(crate) open_files_checked_at: Option<Instant>,
    pub(crate) open_files_checking: bool,
    /// 지금 도는 열린 파일 변경 확인을 시작한 시각과 그 확인의 번호. 오래 돌아오지 않으면 새 확인을 허용하고 옛 결과는 버린다.
    pub(crate) open_files_check_started: Option<Instant>,
    pub(crate) open_files_check_generation: u64,
    /// 고치고 아직 저장하지 않은 파일(편집기의 글자 변경이 넣고, 저장하거나 디스크에서 다시 읽으면 뺀다).
    pub(crate) unsaved: UnsavedSet,
    /// 저장하지 않은 편집이 있는데 디스크의 파일이 바뀐 파일. 띠가 `Reload`·`Keep mine`을 묻는다.
    pub(crate) disk_changed: HashSet<String>,
    /// 저장 명령이 가 있는 파일(결과가 오기 전에 다시 저장하지 않는다).
    pub(crate) saving: HashSet<String>,
    /// 방금 저장한 파일과 그 시각. 상태 줄이 2초 동안 `Saved`를 보인다.
    pub(crate) saved_flash: Option<(String, Instant)>,
    /// 종료 묻기에서 `Save all`을 골라 저장 결과를 기다리는 파일. 모두 저장되면 창을 닫는다.
    pub(crate) quit_saving: HashSet<String>,
    /// 줄로 가기 창(Ctrl+G)의 입력칸.
    pub(crate) goto_input: Option<Entity<InputState>>,
    /// 모은 검토 메모(창 수명, 파일로 남기지 않는다)와 메모 입력창의 입력칸.
    pub(crate) notes: Vec<paddock_core::review_notes::Note>,
    pub(crate) note_input: Option<Entity<InputState>>,
    /// 열려 있는 파일 빠른 열기(닫으면 색인과 함께 버린다).
    pub(crate) quick_open: Option<crate::quick_open::QuickOpenState>,
    /// 바꾸기를 보낸 뒤 결과를 기다리는 동안, 저장하지 않아 건너뛴 파일의 이름(결과 문장에 덧붙인다).
    pub(crate) replace_skipped: Vec<String>,
    /// 이름 바꾸기 명령을 보내고 결과를 기다리는 항목의 옛 경로(그 안의 파일은 이 동안 저장하지 않는다 — 옛 경로에 파일이 다시 생기지 않게).
    pub(crate) renaming: Vec<String>,
    /// `Don't Save`로 탭을 닫았는데 저장 명령이 이미 가 있던 파일(결과가 오면 저장됐음을 상태 줄에 알린다).
    pub(crate) closed_while_saving: HashSet<String>,
    /// `Save`를 골라 닫으려는 파일 탭의 경로(저장이 끝나면 닫는다).
    pub(crate) close_after_save: HashMap<String, GroupKey>,
    /// 덮어쓰기 묻기를 기다리는 파일(묻기 창이 하나 떠 있는 동안 도착한 충돌). 앞에서부터 차례로 묻는다.
    pub(crate) overwrite_queue: std::collections::VecDeque<String>,
    /// 파일마다 자기 쓰기가 끝났을 때의 변경 확인 세대. 그 세대 이하 확인의 결과는 자기 쓰기를 본 것일 수 있어 그 파일에 한해 버린다.
    pub(crate) own_write_generation: HashMap<String, u64>,
    /// 저장하지 않은 파일에 대한 종료 묻기를 마쳤다(저장했거나 버리기로 했다). 창 닫기가 다시 묻지 않는다.
    pub(crate) quit_unsaved_resolved: bool,
    /// 열린 파일을 마지막으로 확인했을 때 본 파일 감시의 사건 횟수(`FilesState::n_events`). 달라지면 감시하는 폴더 안 열린 파일을 바로 확인한다.
    pub(crate) open_files_events_seen: usize,
    /// 파일마다 마지막으로 고른 Markdown 보기 방식.
    pub(crate) markdown_views: ViewChoices,
    /// 닫힌 이미지·PDF 쪽의 그래픽 메모리 반납 대기소. 다음 그리기에서 비운다.
    pub(crate) graveyard: Graveyard,
    /// 파일 칸이 있을 때 터미널 배치와 파일 칸을 합친 본문 폭(화면 크기 단위). 경계를 끌 때 비율의 기준이다.
    file_body_width: Cell<f32>,
    /// Accounts 창의 상태.
    pub(crate) accounts: AccountsPanel,
    /// 빠른 설정 패널의 열림·펼친 선택상자.
    pub(crate) quick: QuickState,
    /// 패널을 열 때 읽은, 이 컴퓨터에 설치된 글꼴 이름들.
    installed_fonts: Vec<String>,
    /// 패널을 열 때(그리고 값을 바꿀 때) 읽은 설정. 패널을 그릴 때마다 설정 파일을 다시 읽지 않으려고 둔다.
    quick_settings: Settings,
    /// 상태 줄 사용량: 도구별 선택과 읽기 결과. 읽기는 작업 스레드가 하고 창은 결과만 받는다.
    pub(crate) usage: UsageData,
    /// 상태 줄 Memory 게이지와 90% 경고.
    pub(crate) memory: crate::usage::memory::MemoryState,
    /// Paddock 전용 Claude 설정(설정 파일·스크립트 사본·실행 래퍼·WSL `.bashrc` 줄)을 쓰는 작업 줄.
    setup: crate::usage::setup::Setup,
    /// 같은 문장을 반복해 띄우지 않으려고 마지막으로 알린 설정 쓰기 결과 문장을 기억한다.
    setup_notices: crate::usage::setup::NoticeFilter,
    /// 마지막 `~/.bashrc` 줄 작업의 결과 상태(`added`·`present`·`removed`·`absent`·`blocked`). 한 적이 없으면 None.
    wsl_launcher_state: Option<String>,
    /// WSL 실행 래퍼 묻기 창을 띄울 차례를 기다린다(다른 묻기 창이 열려 있을 수 있어서다).
    wsl_launcher_question_pending: bool,
    /// 열려 있는 사용량 목록(팝업).
    pub(crate) usage_popup: Option<crate::usage::popup::PopupState>,
    /// 계정을 바꾸는 중인 터미널(같은 터미널의 두 번째 요청을 막는다).
    pub(crate) switching: HashSet<u32>,
    /// 마지막으로 그린 상태 줄 사용량 모양(펼침·접힘). 자동 점검이 읽는다.
    usage_mode: Cell<BarMode>,
    /// 상태 줄 묶음 계산 결과와 그 열쇠(book 변경 번호·분 단위 현재 시각·켜진 도구). 열쇠가 같으면 매 그리기마다 다시 계산하지 않는다.
    bar_cache: RefCell<Option<(BarKey, Rc<Vec<paddock_core::usage_scopes::GroupView>>)>>,
    /// 마지막으로 `pump_usage`가 반영한 관찰 대상(터미널·그 상태 번호·파일 탭 여부). 같으면 200 ms마다 상태를 복제·잠그지 않는다.
    usage_observed: Cell<Option<(Option<u32>, u32, bool)>>,
    /// `Show N more`를 눌러 한도 없이 보이는 작업 폴더.
    pub(crate) show_all: HashSet<String>,
    /// 작업 폴더의 `.git`을 읽어 저장소 묶음의 입력(`Folder::repo`)을 채우는 도구.
    repo_reader: RepoReader,
    /// 종료 묻기의 "Don't ask again" 선택.
    pub(crate) remember_choice: bool,
    /// 데몬이 앱의 작업 개체에서 분리되지 못해 앱을 닫으면 터미널도 끝난다.
    pub(crate) breakaway_denied: bool,
    /// 쓰는 데몬의 실행 파일이 이 앱의 것과 다르다(프로토콜은 같아 계속 쓴다). 데몬 쪽 수정은 터미널을 모두 닫고 다시 시작해야 반영된다.
    /// 시작할 때 한 번만 정한다. 쓰는 중에 앱 옆 데몬 파일이 바뀌어도(개발 중 재빌드) 이 값은 그대로다 — 세션 중에는
    /// 데몬을 바꾸지 않으므로, 다음 시작 때 다시 판정하면 된다.
    pub(crate) daemon_outdated: bool,
    /// 사용자가 종료를 정했다. 창 닫기 요청이 다시 묻지 않고 닫는다.
    quit_confirmed: bool,
    /// 마지막으로 실패한 동작을 알리는 문장.
    pub(crate) status: Option<String>,
    /// 에이전트 완료 알림의 미확인 표시와 소리 쿨다운.
    pub(crate) done: DoneNotice,
    /// 탭마다 마지막으로 반영한 상태 번호. 번호가 달라진 탭이 있을 때만 다시 그린다.
    seen_state_versions: BTreeMap<u32, u32>,
    /// 완료 알림의 시각 기준(밀리초).
    started: Instant,
    /// 탭 묶음(칸 열)마다의 가로 스크롤 상태. 번호는 묶음 번호이고, 칸 위 탭 줄은 1000 이상이다.
    tab_scrolls: RefCell<BTreeMap<usize, ScrollHandle>>,
    /// 다음 그리기에서 선택한 탭을 보이는 곳으로 스크롤한다. 키보드로 탭을 옮긴 직후에만 켠다.
    pub(crate) reveal_selected: Cell<bool>,
    /// 키보드로 탭을 옮긴 뒤 이 시각까지의 그리기에서도 선택한 탭을 보이게 다시 스크롤한다. 넘김 단추가 새로 생기며 탭 상자가
    /// 좁아진 뒤의 그리기에서 위치를 바로잡는 데 쓴다.
    reveal_until: Cell<Option<Instant>>,
    /// 마지막으로 그린 탭 줄 넘침 모양. 스크롤 상자가 배치된 뒤 모양이 달라지면 다시 그린다.
    last_overflow: RefCell<Vec<(usize, bool, bool)>>,
    /// 그리는 동안 잰 본문 크기(화면 크기 단위). 아직 못 쟀으면 (0, 0).
    pub(crate) body_size: Rc<Cell<(f32, f32)>>,
    /// 탭 줄이 마지막으로 쓴 본문 크기. 잰 크기와 달라지면 다시 그린다.
    body_size_used: Cell<(f32, f32)>,
    /// 마지막으로 그렸을 때의 창 크기. `body_size`는 그 그리기에서 잰 값이라, 창 크기가 달라졌으면 낡았다.
    rendered_viewport: Cell<(f32, f32)>,
    /// 끌고 있는 칸 경계.
    divider_drag: Option<DividerDrag>,
    /// 끌고 있는 탭(본 창에서 끌기 시작한 것). 창 밖에서 뗀 MouseUp이 이 상태로 새 창 열기를 정한다.
    pub(crate) tab_drag: Option<crate::tab_drag_model::TabDrag>,
    /// 끌어 온 탭이 놓일 자리 미리보기(이 창에서 끌거나 분리 창에서 끌어 왔다).
    pub(crate) dock_preview: Option<crate::tab_drag_model::DockPreview>,
    /// 칸 배치가 그려진 본문의 왼쪽 위(창 안 좌표). 놓을 자리를 찾는 칸 상자의 원점이다.
    pub(crate) body_origin: Rc<Cell<(f32, f32)>>,
    /// 이 창(본 창)의 손잡이. 분리 창이 본 창의 메서드를 부를 때 이 손잡이로 부른다 — 분리 창의 `window`로 묻기·메뉴·포커스를 열면 본 창이 그리지 못한다.
    pub(crate) main_window: AnyWindowHandle,
    /// 열려 있는 분리 창(터미널 번호별). 분리 창은 탭 하나를 보이고, 탭·세션·Work 목록의 소속은 본 창이 쥔다.
    pub(crate) detached_windows: BTreeMap<u32, DetachedHandle>,
    /// 열려 있는 파일 분리 창(파일 탭 열쇠별). 파일 내용·저장하지 않은 표시·편집기는 본 창이 쥐고 분리 창은 빌려 그린다.
    pub(crate) detached_file_windows: BTreeMap<String, DetachedHandle>,
    /// 분리 창의 파일 단축키(줄로 가기·검토 메모)가 묻기 창을 연 동안 그 파일 탭의 열쇠. 이 동안 "지금 파일 탭"은 이 파일이다.
    pub(crate) detached_file_context: Option<String>,
    /// 끌고 있는 파일 탭(본 창에서 끌기 시작한 것). 파일 탭은 칸에 놓을 수 없고 창 밖에서 뗄 때만 새 창으로 뗀다.
    pub(crate) file_drag: Option<crate::tab_drag_model::FileDrag>,
    /// 본 창을 앞으로 가져오라고 요청한 횟수. 자동 점검이 화면 밖 창을 실제로 활성화하지 않고 요청만 세는 데 쓴다.
    pub(crate) n_main_activation_requests: u32,
    #[cfg(feature = "automation")]
    hooks: Option<crate::automation::Hooks>,
    /// 자동 점검: 마지막 갱신 루프 시각과 그 사이 가장 길었던 간격. 타이머가 늦게 깨운 시간도 들어가므로 창 스레드가 막힌 시간이 아니라 그 상한이다.
    #[cfg(feature = "automation")]
    tick_gap: (Option<Instant>, Duration),
    /// 자동 점검: 직전 갱신 루프 때 창 스레드의 CPU 사용 누계, 가장 긴 간격 동안 쓴 CPU 시간, 어느 간격에서든 쓴 CPU 시간의 최댓값.
    /// 간격이 길어도 CPU를 쓰지 않았으면 창 스레드는 막힌 것이 아니라 잠들어 있었다.
    #[cfg(feature = "automation")]
    tick_cpu: (Duration, Duration, Duration),
    /// 자동 점검: 상태 글 재료(`Probe`)를 마지막으로 조립한 시각. 조립은 화면 전체의 요약을 만들어 비싸므로 `PROBE_INTERVAL`마다만 한다.
    #[cfg(feature = "automation")]
    probe_at: Option<Instant>,
    /// 자동 점검: 선택한 터미널 화면에서 실제 파일을 가리키는 경로 링크(터미널 번호와 함께). 갱신 루프가 터미널 보기의 확인 기억에서 채운다.
    #[cfg(feature = "automation")]
    term_links: Vec<(u32, crate::terminal_link::Target)>,
}

impl WorkWindow {
    /// 창을 만들고 데몬 연결을 다른 스레드에서 시작한다. 창 닫기 요청은 종료 묻기를 거치게 한다.
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let daemon_exe = sibling_daemon_exe();
        let phase = match &daemon_exe {
            Ok(_) => Phase::Connecting("Connecting to the terminal daemon..."),
            Err(error) => Phase::Notice(Notice::Failed(error.to_string())),
        };
        let mut this = Self {
            daemon_exe: daemon_exe.ok(),
            phase,
            workspace: Workspace::default(),
            cache: Rc::new(RefCell::new(GlyphCache::default())),
            already_sent: SentComposition::default(),
            focus: cx.focus_handle(),
            overlay_focus: cx.focus_handle(),
            overlay: Overlay::None,
            menu: None,
            rename_input: None,
            files: FilesState::default(),
            writes: crate::files::write_service::WriteService::new(),
            git: GitState::default(),
            scm: ScmState::default(),
            search: SearchState::default(),
            pending_lines: HashMap::new(),
            entry_input: None,
            tab_bar_focused: false,
            find: None,
            row_drop: None,
            active: Active::Terminal,
            file_active_memory: HashMap::new(),
            forced_text: HashSet::new(),
            file_views: HashMap::new(),
            file_stamps: HashMap::new(),
            open_files_checked_at: None,
            open_files_checking: false,
            open_files_check_started: None,
            open_files_check_generation: 0,
            unsaved: UnsavedSet::default(),
            disk_changed: HashSet::new(),
            saving: HashSet::new(),
            saved_flash: None,
            quit_saving: HashSet::new(),
            goto_input: None,
            notes: Vec::new(),
            note_input: None,
            quick_open: None,
            replace_skipped: Vec::new(),
            renaming: Vec::new(),
            closed_while_saving: HashSet::new(),
            close_after_save: HashMap::new(),
            overwrite_queue: std::collections::VecDeque::new(),
            own_write_generation: HashMap::new(),
            quit_unsaved_resolved: false,
            open_files_events_seen: 0,
            markdown_views: ViewChoices::default(),
            graveyard: Graveyard::default(),
            file_body_width: Cell::new(0.0),
            accounts: AccountsPanel::default(),
            quick: QuickState::default(),
            installed_fonts: Vec::new(),
            quick_settings: Settings::default(),
            usage: {
                let settings = Settings::load();
                UsageData::new(
                    &state_store::load_saved().usage_selection,
                    StatusItems::from_settings(settings.values()),
                )
            },
            usage_popup: None,
            memory: Default::default(),
            setup: crate::usage::setup::Setup::new(),
            setup_notices: Default::default(),
            wsl_launcher_state: None,
            wsl_launcher_question_pending: false,
            usage_mode: Cell::new(BarMode::Full),
            switching: HashSet::new(),
            bar_cache: RefCell::new(None),
            usage_observed: Cell::new(None),
            show_all: HashSet::new(),
            repo_reader: RepoReader::new(),
            remember_choice: false,
            breakaway_denied: false,
            daemon_outdated: false,
            quit_confirmed: false,
            status: None,
            done: DoneNotice::default(),
            seen_state_versions: BTreeMap::new(),
            started: Instant::now(),
            tab_scrolls: RefCell::new(BTreeMap::new()),
            reveal_selected: Cell::new(false),
            reveal_until: Cell::new(None),
            last_overflow: RefCell::new(Vec::new()),
            body_size: Rc::new(Cell::new((0.0, 0.0))),
            body_size_used: Cell::new((0.0, 0.0)),
            rendered_viewport: Cell::new((0.0, 0.0)),
            divider_drag: None,
            tab_drag: None,
            dock_preview: None,
            body_origin: Rc::new(Cell::new((0.0, 0.0))),
            main_window: window.window_handle(),
            detached_windows: BTreeMap::new(),
            detached_file_windows: BTreeMap::new(),
            detached_file_context: None,
            file_drag: None,
            n_main_activation_requests: 0,
            #[cfg(feature = "automation")]
            hooks: None,
            #[cfg(feature = "automation")]
            tick_gap: (None, Duration::ZERO),
            #[cfg(feature = "automation")]
            tick_cpu: (Duration::ZERO, Duration::ZERO, Duration::ZERO),
            #[cfg(feature = "automation")]
            probe_at: None,
            #[cfg(feature = "automation")]
            term_links: Vec::new(),
        };
        window.focus(&this.focus, cx);
        let entity = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            entity.update(cx, |this, cx| this.on_close_requested(window, cx)).unwrap_or(true)
        });
        if this.daemon_exe.is_some() {
            this.connect(Connect::Open(Policy::ReplaceIfIdle), window, cx);
        }
        this.watch_agents(window, cx);
        // 창 크기가 바뀌는 사건에서 바로 다시 그린다. 200ms 주기 확인을 기다리면 탭 열 구간이 그만큼 낡은 폭으로 남는다.
        cx.observe_window_bounds(window, |_this, _window, cx| cx.notify()).detach();
        // 창이 비활성이 되면 끝나지 않은 탭 끌기를 버린다(창 밖에서 뗀 MouseUp을 받지 못한 경우의 잔존 방지).
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.reset_tab_drag(window, cx);
            }
        })
        .detach();
        this.start_setup();
        this
    }

    /// 앱을 시작할 때 한 번, 지난번 설정(`usage/state.json`)대로 Paddock 전용 Claude 설정 파일과 `claude` 실행 래퍼를 다시 만든다.
    /// 앱을 새 판으로 올리거나 설정 폴더를 옮겨도 경로가 맞게 되고, 실패해도 앱은 그대로 시작한다(실패 문장은 상태 줄에 보인다).
    fn start_setup(&self) {
        let settings = Settings::load();
        let values = settings.values();
        self.setup.submit(crate::usage::setup::Job::Start {
            claude: values.status_item(key::STATUS_BAR_CLAUDE),
            memory: (values.memory_notice(), values.memory_notice_minimum_mb()),
            launcher: values.wsl_claude_launcher(),
        });
    }

    /// Paddock 전용 Claude 설정 쓰기의 요청을 넘기고 끝난 결과를 반영한다. 화면을 다시 그려야 하면 true다.
    ///
    /// 결과 문장은 상태 줄에 한 번 보인다(같은 문장이 이어지면 다시 띄우지 않고, 성공하면 기억을 비운다).
    fn pump_setup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        while let Some(done) = self.setup.try_next() {
            for notice in &done.notices {
                app_log::event(&format!("setup: {notice}"));
            }
            if let Some(notice) = self.setup_notices.fresh(&done.notices) {
                self.status = Some(notice);
            }
            if done.usage_changed {
                self.usage.reread();
            }
            if done.launcher.is_some() {
                self.wsl_launcher_state = done.launcher;
            }
            self.wsl_launcher_question_pending |= done.ask_launcher;
            changed = true;
        }
        if self.wsl_launcher_question_pending && self.overlay == Overlay::None {
            self.wsl_launcher_question_pending = false;
            // 터미널에서 한글을 조합하던 중이면 먼저 확정하고, 묻기 창이 키 초점을 가져가 조합 글자가 뒤의 터미널로 들어가지 않게 한다.
            self.settle_composition(window, cx);
            self.open_overlay(Overlay::WslLauncher, window, cx);
            changed = true;
        }
        changed
    }

    /// WSL 묻기 창의 답. 설정에 남기고(`on`·`off`) 창을 닫는다. 설정이 바뀌면 `.bashrc` 줄을 넣거나 뺀다.
    pub(crate) fn wsl_launcher_answer(
        &mut self,
        turn_on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_overlay(window, cx);
        self.apply_setting(key::WSL_CLAUDE_LAUNCHER, Value::from(if turn_on { "on" } else { "off" }), cx);
    }

    /// 설정을 바꾼 뒤 그 설정이 Paddock 전용 Claude 설정 파일에 닿는 것이면 쓰기 작업을 요청한다.
    fn setup_after_setting(
        &mut self,
        setting_key: &str,
    ) {
        use crate::usage::setup::Job;
        use paddock_core::settings_text::WslClaudeLauncherSetting;
        let values = self.quick_settings.values();
        match setting_key {
            // 상태 줄의 Claude usage 항목이 사용량 표시 설정의 기준이다: 켜면 상태 줄을 넣고 끄면 뺀다.
            key::STATUS_BAR_CLAUDE => {
                self.setup.submit(Job::Claude { enabled: values.status_item(key::STATUS_BAR_CLAUDE), announce: false })
            }
            key::MEMORY_NOTICE | key::MEMORY_NOTICE_MINIMUM_MB => self.setup.submit(Job::MemoryNotice {
                enabled: values.memory_notice(),
                minimum_mb: values.memory_notice_minimum_mb(),
            }),
            key::WSL_CLAUDE_LAUNCHER => match values.wsl_claude_launcher() {
                WslClaudeLauncherSetting::On => self.setup.submit(Job::WslLauncher { enable: true, quiet: false }),
                WslClaudeLauncherSetting::Off => self.setup.submit(Job::WslLauncher { enable: false, quiet: false }),
                // `ask`로 되돌려도 이미 넣은 줄은 그대로 둔다.
                WslClaudeLauncherSetting::Ask => {}
            },
            _ => {}
        }
    }

    /// 터미널 상태가 바뀌었는지 주기적으로 확인하는 일을 시작한다. 창이 사라지면 끝난다.
    fn watch_agents(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_POLL_INTERVAL).await;
                if this.update_in(cx, |this, window, cx| this.refresh_agents(window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// 데몬이 알려 온 상태 변화를 반영한다: 바뀐 탭이 있으면 다시 그리고, 완료 알림은 미확인 표시와 소리로 바꾼다.
    ///
    /// 지금 보는 터미널(선택됨·창 활성)은 확인한 것으로 보아 소리도 표시도 없다.
    fn refresh_agents(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 지금 사람이 보고 있는 터미널: 활성인 본 창의 선택한 터미널(다른 창에 있는 터미널은 본 창에서 보이지 않는다)과 활성인 분리 창의 터미널.
        let mut watching: Vec<u32> = window
            .is_window_active()
            .then(|| self.workspace.selected())
            .flatten()
            .filter(|id| !self.workspace.is_detached(*id))
            .into_iter()
            .collect();
        watching.extend(
            self.detached_windows.iter().filter(|(_, detached)| detached.shared.active.get()).map(|(id, _)| *id),
        );
        let now = self.started.elapsed().as_millis() as u64;
        let mut changed = false;
        let mut finished_ids = Vec::new();
        for tab in self.workspace.tabs() {
            let shared = &tab.content.session.grid.shared;
            let version = shared.state_version.load(Ordering::Relaxed);
            if self.seen_state_versions.insert(tab.id, version) != Some(version) {
                changed = true;
            }
            for _ in 0..shared.finished_pending.swap(0, Ordering::Relaxed) {
                finished_ids.push(tab.id);
            }
        }
        if !finished_ids.is_empty() {
            let sound_enabled = Settings::load().agent_done_sound();
            for id in finished_ids {
                if self.done.on_finished(id, watching.contains(&id), sound_enabled, now).sound {
                    done_sound::request(id);
                }
            }
            changed = true;
        }
        for id in &watching {
            if self.done.acknowledge(*id) {
                changed = true;
            }
        }
        if self.layout_measures_changed() {
            changed = true;
        }
        let listings_before = self.files.n_listings();
        if self.sync_files(window) {
            changed = true;
        }
        if self.sync_search(window, cx) {
            changed = true;
        }
        if self.pump_writes(window, cx) {
            changed = true;
        }
        self.apply_pending_lines(window, cx);
        self.watch_open_files(self.files.n_listings() != listings_before, window, cx);
        if self.load_visible_file(window, cx) {
            changed = true;
        }
        if self.pump_git(window, cx) {
            changed = true;
        }
        if self.pump_usage(window) {
            #[cfg(feature = "automation")]
            crate::automation::count_changed(crate::automation::Cause::Usage);
            changed = true;
        }
        if self.pump_memory(window) {
            #[cfg(feature = "automation")]
            crate::automation::count_changed(crate::automation::Cause::Memory);
            changed = true;
        }
        if self.pump_setup(window, cx) {
            #[cfg(feature = "automation")]
            crate::automation::count_changed(crate::automation::Cause::Setup);
            changed = true;
        }
        if self.pump_repos() {
            changed = true;
        }
        if changed && self.promote_agents() {
            self.persist();
        }
        if changed {
            cx.notify();
        }
    }

    /// 작업 폴더의 `.git` 읽기 결과를 폴더의 저장소 key로 기록하고, 아직 읽지 못한 폴더의 읽기를 시작한다.
    /// 같은 저장소의 폴더가 한 묶음으로 보이게 하는 입력이다. 기록이 바뀌어 다시 그려야 하면 true다.
    fn pump_repos(&mut self) -> bool {
        let mut changed = false;
        for (folder, repo) in self.repo_reader.finished(self.workspace.work()) {
            if self.workspace.work().folder_repo(&folder) != repo.as_deref() {
                self.workspace.work_mut().set_folder_repo(&folder, repo.as_deref());
                changed = true;
            }
        }
        self.repo_reader.request(self.workspace.work());
        if changed {
            self.persist();
        }
        changed
    }

    /// 상태 줄 사용량을 갱신한다: 보고 있는 터미널의 범위로 선택을 바꾸고, 끝난 읽기를 반영하고, 주기가 되면 새로 읽는다.
    /// 선택이 바뀌어 저장할 값이 생겼으면 state.json에 쓴다. 화면을 다시 그려야 하면 true다.
    fn pump_usage(
        &mut self,
        window: &Window,
    ) -> bool {
        // 보는 터미널과 그 상태 번호가 지난 틱과 같으면 상태를 복제·잠그지 않고 읽기 진행만 이어 간다.
        let is_file = self.file_active();
        let selected = (!is_file).then(|| self.workspace.selected().and_then(|id| self.workspace.tab(id))).flatten();
        let key = (
            selected.map(|tab| tab.id),
            selected.map_or(0, |tab| tab.content.session.grid.shared.state_version.load(Ordering::Relaxed)),
            is_file,
        );
        let mut redraw = false;
        if self.usage_observed.get() != Some(key) {
            self.usage_observed.set(Some(key));
            let observed = selected.map(|tab| {
                (
                    tab.id,
                    tab.content.session.grid.shared.state.lock().ok().and_then(|state| state.clone()),
                    (tab.content.session.shell == "wsl").then(|| tab.content.session.wsl_distribution.clone()),
                )
            });
            let (terminal, state, wsl) = match observed {
                Some((id, state, wsl)) => (Some(id), state, wsl),
                None => (None, None, None),
            };
            let account = state.as_ref().and_then(|state| state.account.clone());
            let program = state.as_ref().and_then(|state| state.program.clone());
            let environment = match &wsl {
                Some(distribution) => (Runtime::Wsl, distribution.as_deref()),
                None => (Runtime::Native, None),
            };
            redraw = self.usage.observe(terminal, account.as_ref(), program.as_deref(), environment);
        }
        redraw |= self.usage.tick(window.is_window_active(), self.usage_popup.is_some());
        if let Some(popup) = self.usage_popup.as_mut()
            && popup.retrying
            && !self.usage.is_reading()
        {
            popup.retrying = false;
            redraw = true;
        }
        if self.usage.unsaved_selection.take().is_some() {
            self.persist();
        }
        redraw
    }

    /// Memory 게이지를 갱신한다: 항목이 켜져 있고 창이 포커스를 가진 동안(자동 점검은 늘) 10초마다 데몬에 WSL 메모리를 묻고(Windows),
    /// 이 컴퓨터 몫은 직접 읽는다. 새 답이 오거나 물을 때가 되면 게이지와 경고를 다시 계산한다. 화면을 다시 그려야 하면 true다.
    fn pump_memory(
        &mut self,
        window: &Window,
    ) -> bool {
        let mut changed = false;
        if self.usage.items.memory {
            let now = Instant::now();
            let may_ask = window.is_window_active() || cfg!(feature = "automation");
            let mut recompute = self.memory.take_new_report();
            if may_ask && self.memory.due(now) {
                self.memory.mark_requested(now);
                if cfg!(windows)
                    && let Some(tab) = self.workspace.tabs().first()
                {
                    let _ = tab.content.session.outbox.send(&Frame::new(kind::MEMORY_REQUEST, Vec::new()));
                }
                recompute = true;
            }
            if recompute {
                let workspace = &self.workspace;
                changed = self.memory.refresh(&|id| workspace.tab(id).map(|tab| workspace.name(tab.id).to_string()));
            }
        } else {
            changed = self.memory.clear();
        }
        changed
    }

    /// 경고의 `Go to <이름>`: 그 터미널을 선택하고 경고를 닫는다.
    pub(crate) fn memory_go_to(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.memory.dismiss();
        if self.workspace.tab(id).is_some() {
            self.select_tab(id, window, cx);
        }
        cx.notify();
    }

    /// 상태 줄 사용량 항목의 켬/끔.
    pub(crate) fn usage_items(&self) -> StatusItems {
        self.usage.items
    }

    /// 본문 크기나 탭 줄 넘침 모양이 마지막으로 그린 것과 달라졌는지. 둘 다 그린 뒤에야 알 수 있어, 달라졌으면 한 번 더 그린다.
    ///
    /// 스크롤 위치는 건드리지 않는다 — 사용자가 옮긴 탭 줄이 주기 갱신 때 되돌아가지 않는다.
    fn layout_measures_changed(&self) -> bool {
        let overflow: Vec<(usize, bool, bool)> = self
            .tab_scrolls
            .borrow()
            .iter()
            .map(|(key, handle)| {
                let look = overflow_look(handle);
                (*key, look.hidden_left, look.hidden_right)
            })
            .collect();
        let overflow_changed = *self.last_overflow.borrow() != overflow;
        *self.last_overflow.borrow_mut() = overflow;
        overflow_changed || self.body_size.get() != self.body_size_used.get()
    }

    /// 키보드로 탭을 옮긴 직후라 이번 그리기에서 선택한 탭을 보이게 스크롤해야 하는지.
    pub(crate) fn reveal_pending(&self) -> bool {
        self.reveal_selected.get() || self.reveal_until.get().is_some_and(|until| Instant::now() < until)
    }

    /// `key`번 탭 묶음의 스크롤 상태. 없으면 만든다.
    pub(crate) fn tab_scroll(
        &self,
        key: usize,
    ) -> ScrollHandle {
        self.tab_scrolls.borrow_mut().entry(key).or_default().clone()
    }

    /// 넘김 단추: 탭 묶음을 한 걸음 옮긴다.
    pub(crate) fn scroll_tabs(
        &self,
        handle: &ScrollHandle,
        toward_right: bool,
    ) {
        self.cancel_reveal();
        scroll_by_step(handle, toward_right);
    }

    /// 사용자가 넘김 단추나 휠로 탭 줄을 직접 움직이면 키보드 이동 뒤의 재스크롤을 바로 멈춘다.
    pub(crate) fn cancel_reveal(&self) {
        self.reveal_selected.set(false);
        self.reveal_until.set(None);
    }

    /// 에이전트가 실행 중인 터미널을 그 터미널의 현재 폴더의 작업 폴더로 올린다. 목록이 바뀌었으면 true다.
    ///
    /// 폴더를 아직 모르는 터미널은 올리지 않고 다음 상태 변화 때 다시 본다. 에이전트가 끝나도 터미널은 폴더에 남는다.
    fn promote_agents(&mut self) -> bool {
        let candidates: Vec<Candidate> = self
            .workspace
            .tabs()
            .iter()
            .filter_map(|tab| {
                let state = tab.content.session.grid.shared.state.lock().unwrap().clone()?;
                Some(Candidate {
                    id: tab.id,
                    cwd: state.cwd,
                    program: state.program,
                    is_agent: state.is_agent,
                    name: self.workspace.name(tab.id).to_string(),
                })
            })
            .collect();
        let before = self.snapshot_active();
        let promotion = self.workspace.promote(&candidates);
        // 묶음이 폴더로 올라가며 이름표가 바뀌어도 파일 탭이 활성인 채로 이어진다.
        self.settle_active(before, false);
        if !promotion.promoted.is_empty() {
            app_log::event(&format!(
                "work list: {} terminals moved to folders, {} folders added",
                promotion.promoted.len(),
                promotion.added.len()
            ));
        }
        !promotion.promoted.is_empty()
    }

    pub fn n_terminals(&self) -> usize {
        self.workspace.tabs().len()
    }

    /// 데몬 연결과 터미널 되살리기를 다른 스레드에서 하고, 끝나면 결과를 창에 반영한다.
    fn connect(
        &mut self,
        how: Connect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(daemon_exe) = self.daemon_exe.clone() {
            self.phase = Phase::Connecting(match how {
                Connect::Open(Policy::Keep | Policy::ReplaceIfIdle) => "Connecting to the terminal daemon...",
                Connect::Open(Policy::Replace) => "Replacing the terminal daemon...",
                Connect::RestartStalled => "Restarting the terminal daemon...",
            });
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let started = Instant::now();
                        match how {
                            Connect::Open(policy) => open_workspace(&daemon_exe, policy, started),
                            Connect::RestartStalled => restart_stalled_daemon(&daemon_exe)
                                .and_then(|connection| restore_terminals(&daemon_exe, connection, started))
                                .map(Startup::Ready),
                        }
                    })
                    .await;
                let _ = this.update_in(cx, |this, window, cx| this.finish_connect(result, window, cx));
            })
            .detach();
        }
    }

    fn finish_connect(
        &mut self,
        result: io::Result<Startup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Startup::Ready(restored)) => {
                self.breakaway_denied = restored.breakaway_denied;
                self.daemon_outdated = restored.daemon_outdated;
                self.workspace.set_work(restored.work);
                for OpenedTerminal { session } in restored.terminals {
                    self.attach_tab(session, window, cx);
                }
                self.workspace.set_layouts(restored.layouts);
                self.workspace.set_sidebar_collapsed(restored.sidebar_collapsed);
                self.workspace.select_or_first(restored.selected);
                self.workspace.set_files(restored.files);
                self.files.files_expanded = !restored.files_collapsed;
                self.phase = Phase::Ready;
                self.load_visible_file(window, cx);
                self.focus_selected(window, cx);
                self.persist();
                app_log::event(&format!(
                    "window ready with {} terminals{}",
                    self.n_terminals(),
                    if self.daemon_outdated { " (using the previous terminal daemon)" } else { "" }
                ));
            }
            Ok(Startup::ProtocolMismatch { daemon_version }) => {
                self.phase = Phase::Notice(Notice::ProtocolMismatch { daemon_version })
            }
            Ok(Startup::DaemonStalled) => self.phase = Phase::Notice(Notice::DaemonStalled),
            Err(error) => self.phase = Phase::Notice(Notice::Failed(error.to_string())),
        }
        cx.notify();
    }

    /// 세션에 터미널 보기를 만든다.
    pub(crate) fn view_of(
        &self,
        session: &TerminalSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalView> {
        let grid = session.grid.clone();
        let cache = self.cache.clone();
        let already_sent = self.already_sent.clone();
        let paste_place = place_of(&session.shell, std::env::consts::OS);
        let view = cx.new(|cx| TerminalView::new(grid, cache, already_sent, paste_place, window, cx));
        // 터미널 글자 위의 파일 경로를 Ctrl+클릭하면 그 파일을 파일 탭으로 연다(줄 번호가 있으면 그 줄로 간다).
        cx.subscribe_in(&view, window, |this, _view, event: &OpenLink, window, cx| {
            this.open_terminal_link(&event.target, window, cx)
        })
        .detach();
        // 경로 없는 클립보드 그림: 쓰기 길로 `pasted-images`에 저장하고, 끝나면 그 경로를 이 터미널에 붙여 넣는다.
        let terminal = session.id;
        cx.subscribe_in(&view, window, move |this, _view, event: &crate::terminal::view::PasteImage, _window, _cx| {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            this.writes.submit(crate::files::write_service::WriteCommand::PastedImage {
                terminal,
                folder: paddock_protocol::daemon_info::native_dir().join("pasted-images"),
                kind: event.kind,
                bytes: event.bytes.clone(),
                unix_seconds: seconds,
                random: format!("{:04x}", rand::random::<u16>()),
            });
        })
        .detach();
        cx.subscribe_in(&view, window, |this, _view, _event: &crate::terminal::view::PasteRefused, _window, cx| {
            this.status = Some(paddock_core::paste_paths::CONTROL_CHARACTER_NOTICE.to_string());
            cx.notify();
        })
        .detach();
        // 터미널로 글자가 나가면(입력 방식 경로 포함) 탭 줄 초점은 풀린다.
        cx.subscribe_in(&view, window, |this, _view, _event: &crate::terminal::view::Typed, _window, cx| {
            this.files.focus_event(paddock_core::file_tree::ListFocusEvent::TextTyped);
            this.leave_tab_bar(cx)
        })
        .detach();
        view
    }

    /// 터미널 경로 링크가 가리키는 파일을 연다. 줄 번호가 있으면 그 줄로 간다.
    pub(crate) fn open_terminal_link(
        &mut self,
        target: &crate::terminal_link::Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target.line {
            Some(line) => self.open_file_at(&target.path, usize::try_from(line).unwrap_or(1), window, cx),
            None => self.open_file(&target.path, window, cx),
        }
    }

    /// 저장된 소속을 되살린 뒤, 이미 소속이 정해진 터미널을 탭으로 붙인다(선택은 호출자가 정한다).
    fn attach_tab(
        &mut self,
        session: TerminalSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.view_of(&session, window, cx);
        let (daemon, id) = (session.daemon.clone(), session.id);
        self.workspace.attach(&daemon, id, TabContent { session, view });
    }

    /// 데몬이 준비되었을 때만 그 실행 파일(새 터미널·계정 바꾸기가 데몬에 연결할 때 쓴다).
    pub(crate) fn daemon_exe_if_ready(&self) -> Option<PathBuf> {
        match (&self.daemon_exe, &self.phase) {
            (Some(exe), Phase::Ready) => Some(exe.clone()),
            _ => None,
        }
    }

    /// 터미널이 탭에서 빠졌을 때(닫힘·다른 터미널로 교체) 창이 그 번호로 쥔 상태를 비운다.
    pub(crate) fn forget_terminal_state(
        &mut self,
        id: u32,
    ) {
        if paddock_core::terminal_find::bar_after_close(self.find.as_ref().map(|bar| bar.terminal), id).is_none() {
            self.find = None;
        }
        if self.row_drop.is_some_and(|(over, _)| over == id) {
            self.row_drop = None;
        }
        self.done.forget(id);
        self.seen_state_versions.remove(&id);
    }

    /// 새 터미널을 `place`에 소속시켜 탭으로 더하고 선택한다.
    fn add_new_tab(
        &mut self,
        place: &NewPlace,
        split: Option<Orientation>,
        session: TerminalSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.view_of(&session, window, cx);
        let (daemon, id) = (session.daemon.clone(), session.id);
        // 죽은 데몬의 옛 번호와 같은 번호를 받은 새 터미널에는 옛 찾기 띠를 붙이지 않는다.
        self.find = self.find.take().filter(|bar| bar.terminal != id);
        let name = self.workspace.next_name();
        let content = TabContent { session, view };
        self.reveal_selected.set(true);
        let before = self.snapshot_active();
        // 다른 데몬의 탭(죽은 데몬의 것)은 번호가 겹칠 수 있어 함께 정리된다. 그 세션은 이미 끊겨 있다.
        match split {
            Some(orientation) => drop(self.workspace.add_split(&daemon, id, &name, place, content, orientation)),
            None => drop(self.workspace.add(&daemon, id, &name, place, content)),
        }
        // 사용자가 새로 연 터미널이 선택되므로 터미널이 활성이고 포커스도 그쪽으로 간다.
        self.settle_active(before, true);
        // 데몬이 바뀌어 쫓겨난 터미널 중 분리 창이 열려 있던 것은 그 창도 닫는다(번호가 새 터미널에 다시 쓰여도 옛 창이 남지 않게).
        self.prune_detached_windows(cx);
    }

    /// 활성 탭에 키 입력을 준다: 파일 탭이 활성이면 그 내용(텍스트 보기), 아니면 선택한 터미널. 받을 곳이 없으면 창 자신이 포커스를 쥔다.
    pub(crate) fn focus_selected(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 키 입력을 터미널·편집기로 돌려주면 파일 목록은 키를 받지 않는다(F2·Ctrl+V는 다시 터미널의 것이다).
        self.files.focus_event(paddock_core::file_tree::ListFocusEvent::KeysReturned);
        if self.file_active() {
            let handle = self
                .selected_file_key()
                .and_then(|key| self.file_views.get(&key))
                .and_then(|content| content.focus_handle(cx));
            window.focus(handle.as_ref().unwrap_or(&self.focus), cx);
        } else {
            // 선택이 다른 창에 있는 터미널이면(묶음의 탭이 모두 분리됨) 그 보기는 이 창에 없으므로 이 창이 자기 포커스를 쥔다.
            let selected = self
                .workspace
                .selected()
                .filter(|id| !self.workspace.is_detached(*id))
                .and_then(|id| self.workspace.tab(id));
            match selected {
                Some(tab) => window.focus(&tab.content.view.focus_handle(cx), cx),
                None => window.focus(&self.focus, cx),
            }
        }
    }

    /// 탭 순서·이름·선택을 state.json에 남긴다. 저장하지 못해도 앱은 계속 일한다.
    ///
    /// 터미널을 되살리기 전(연결 중·안내 단계)에는 탭이 비어 있어 저장하지 않는다. 저장하면 이전 상태를 빈 목록으로 잃는다.
    pub(crate) fn persist(&self) {
        if may_save_state_on_close(matches!(self.phase, Phase::Ready))
            && let Err(error) = state_store::save(&self.state_to_save())
        {
            app_log::event(&format!("could not save state.json: {error}"));
        }
    }

    /// state.json에 저장할 상태: 작업 공간의 상태에 창이 쥔 파일 목록 구획 접힘을 더한 것.
    fn state_to_save(&self) -> paddock_core::session_state::SessionState {
        let mut state = self.workspace.to_state();
        state.files_collapsed = !self.files.files_expanded;
        state.usage_selection = self.usage.selection_to_store();
        state
    }

    pub(crate) fn select_tab(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.select(id) {
            self.tab_bar_focused = false;
            self.set_active(Active::Terminal);
            // 사용자가 직접 고른 터미널은 확인한 것이다.
            self.done.acknowledge(id);
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    pub(crate) fn step_tab(
        &mut self,
        target: TabTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let order = self.workspace.tab_order();
        let next = self.active_tab().and_then(|current| tab_target(&order, &current, target).copied());
        if let Some(next) = next {
            self.activate_tab(next);
            // 키보드로 옮긴 탭은 가려져 있으면 보이는 곳으로 스크롤한다(마우스로 고른 탭은 그 자리에 있다).
            self.reveal_selected.set(true);
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 지금 키를 받는 탭. 보이는 터미널이 없으면 None이다.
    fn active_tab(&self) -> Option<TabRef> {
        match (self.file_active(), self.workspace.file_pane()) {
            (true, Some(pane)) => Some(TabRef::File(pane.selected)),
            _ => self.workspace.selected().map(TabRef::Terminal),
        }
    }

    /// `tab`을 활성으로 만든다(선택 기록과 읽음 표시까지). 포커스와 저장은 호출자가 한다.
    pub(crate) fn activate_tab(
        &mut self,
        tab: TabRef,
    ) {
        match tab {
            TabRef::Terminal(id) => {
                if self.workspace.select(id) {
                    self.done.acknowledge(id);
                    self.set_active(Active::Terminal);
                }
            }
            TabRef::File(index) => {
                if let Some(key) = self.workspace.view_key()
                    && self.workspace.select_file(&key, index)
                {
                    self.set_active(Active::File(key));
                }
            }
        }
    }

    /// 파일 탭이 키를 받고 있는지. 보는 묶음이 바뀌었거나 파일 칸이 없어졌으면 거짓이다.
    pub(crate) fn file_active(&self) -> bool {
        match &self.active {
            Active::File(key) => {
                self.workspace.view_key().as_ref() == Some(key) && self.workspace.files().get(key).is_some()
            }
            Active::Terminal => false,
        }
    }

    /// 포커스를 쥔 터미널(탭 줄이 `Focused`로 강조하는 탭). 파일 탭이 활성이면 없다.
    pub(crate) fn focused_terminal(&self) -> Option<u32> {
        if self.file_active() { None } else { self.workspace.selected() }
    }

    /// `tab`이 지금 키를 받는 탭인지.
    pub(crate) fn is_active_tab(
        &self,
        tab: TabRef,
    ) -> bool {
        self.active_tab() == Some(tab)
    }

    /// 키를 받는 탭을 정하고, 보이는 묶음에서 마지막으로 활성이었던 쪽으로 기억한다.
    pub(crate) fn set_active(
        &mut self,
        active: Active,
    ) {
        if let Some(key) = self.workspace.view_key() {
            self.file_active_memory.insert(key, matches!(active, Active::File(_)));
        }
        self.active = active;
    }

    /// 터미널 선택이 바뀔 수 있는 연산 앞의 상태.
    pub(crate) fn snapshot_active(&self) -> ActiveSnapshot {
        ActiveSnapshot { was_file_active: self.file_active(), group_ids: self.workspace.view_ids() }
    }

    /// 터미널 선택이 바뀔 수 있는 연산(새 터미널·닫기·폴더로 올리기) 뒤 어느 탭이 활성인지 `file_stays_active` 규칙 하나로 정한다.
    /// 묶음 이름표가 바뀌어도(`follow_groups`가 파일 칸을 옮긴 뒤) 같은 묶음이면 그 묶음의 파일 칸을 가리킨다.
    pub(crate) fn settle_active(
        &mut self,
        before: ActiveSnapshot,
        user_chose_terminal: bool,
    ) {
        let key = self.workspace.view_key();
        let selected = self.workspace.selected();
        let has_pane = key.as_ref().is_some_and(|key| self.workspace.files().get(key).is_some());
        let change = SelectionChange {
            was_file_active: before.was_file_active,
            user_chose_terminal,
            same_group: selected.is_some_and(|id| before.group_ids.contains(&id)),
            remembered_file_active: key.as_ref().is_some_and(|key| self.file_active_memory.get(key) == Some(&true)),
            has_pane,
        };
        let next = match (file_stays_active(&change), key) {
            (true, Some(key)) => Active::File(key),
            _ => Active::Terminal,
        };
        self.set_active(next);
    }

    /// `Ctrl+Shift+W`: 활성 탭을 닫는다. 파일 탭이면 바로 닫고, 터미널이면 닫기 확인 규칙을 따른다.
    fn close_active_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.active_tab() {
            Some(TabRef::File(index)) => self.close_file_tab(index, window, cx),
            Some(TabRef::Terminal(id)) => self.request_close(id, window, cx),
            None => {}
        }
    }

    /// 요청대로 새 터미널을 열어 `place`의 새 탭으로 선택한다. 연결은 다른 스레드에서 한다.
    pub(crate) fn open_terminal(
        &mut self,
        want: Want,
        place: NewPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_terminal_arranged(want, place, None, window, cx);
    }

    /// `open_terminal`과 같되 `split`이 있으면 선택한 터미널의 옆(가로)·아래(세로) 새 칸에 둔다.
    fn open_terminal_arranged(
        &mut self,
        want: Want,
        place: NewPlace,
        split: Option<Orientation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(daemon_exe), Phase::Ready) = (self.daemon_exe.clone(), &self.phase) {
            self.status = None;
            // 계정 터미널은 `Claude · Main`처럼 자기 이름을 갖고, 일반 터미널은 `terminal N`이 붙는다.
            let title = want.title().map(str::to_string);
            cx.spawn_in(window, async move |this, cx| {
                let result = cx.background_executor().spawn(async move { open_new_terminal(&daemon_exe, want) }).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(session) => {
                            this.add_new_tab(&place, split, session, window, cx);
                            if let (Some(title), Some(id)) = (title, this.workspace.selected()) {
                                this.workspace.rename(id, &title);
                            }
                            this.focus_selected(window, cx);
                            this.persist();
                        }
                        Err(error) => this.status = Some(format!("Could not open a terminal: {error}")),
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Work 머리의 ＋·▾: 새 Unassigned 묶음을 `environment`(없으면 기본 환경)로 홈에서 연다.
    pub(crate) fn new_terminal_in_new_group(
        &mut self,
        environment: Option<Environment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let want = match environment {
            Some(environment) => Want::New {
                shell: environment.shell.to_string(),
                cwd: None,
                wsl_distribution: environment.wsl_distribution,
            },
            None => new_terminal_want(),
        };
        self.open_terminal(want, NewPlace::NewGroup, window, cx);
    }

    /// 폴더 행의 ＋: 그 작업 폴더에서 새 터미널을 연다(WSL 폴더는 그 배포판으로).
    pub(crate) fn new_terminal_in_folder(
        &mut self,
        key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let want = folder_want(&key, &default_shell());
        self.open_terminal(want, NewPlace::Folder(key), window, cx);
    }

    /// 탭 줄의 ＋·`Ctrl+Shift+T`·``Ctrl+` ``: 지금 보는 화면의 자리에 새 터미널을 연다.
    /// 작업 폴더가 보이면 그 폴더, Unassigned 묶음이 보이면 그 묶음, 아무것도 없으면 새 묶음이다.
    pub(crate) fn new_terminal_here(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.workspace.new_place() {
            NewPlace::Folder(key) => self.new_terminal_in_folder(key, window, cx),
            NewPlace::Group(root) => self.open_terminal(new_terminal_want(), NewPlace::Group(root), window, cx),
            NewPlace::NewGroup => self.new_terminal_in_new_group(None, window, cx),
        }
    }

    /// `Ctrl+Shift+C`·`Ctrl+\`·`Ctrl+Shift+5`(옆 칸)·``Ctrl+Shift+` ``(아래 칸)와 탭 줄 오른쪽 단추:
    /// 지금 보는 자리에 새 터미널을 선택한 터미널의 옆·아래 새 칸으로 연다. 보이는 터미널이 없으면 그냥 새 터미널이다.
    pub(crate) fn split_selected(
        &mut self,
        orientation: Orientation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.selected().is_none() {
            self.new_terminal_here(window, cx);
        } else {
            let place = self.workspace.new_place();
            let want = match &place {
                NewPlace::Folder(key) => folder_want(key, &default_shell()),
                NewPlace::Group(_) | NewPlace::NewGroup => new_terminal_want(),
            };
            self.open_terminal_arranged(want, place, Some(orientation), window, cx);
        }
    }

    /// `Ctrl+B`와 탭 줄 왼쪽 단추: 왼쪽 사이드바(Work 목록)를 접거나 펼친다. 터미널과 선택은 그대로다.
    pub(crate) fn toggle_sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collapsed = !self.workspace.sidebar_collapsed();
        if collapsed {
            // 빠른 설정은 사이드바의 보기 줄에서 열렸으므로 사이드바와 함께 닫는다.
            self.quick = QuickState::default();
        }
        self.workspace.set_sidebar_collapsed(collapsed);
        self.persist();
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 파일 목록이 가리킬 폴더: 보이는 작업 폴더, 없으면 선택한 터미널의 현재 폴더, 그것도 없으면 묶음 시작 터미널의 현재 폴더.
    /// 홈 폴더와 파일 시스템 루트는 목록을 보이지 않는다.
    pub(crate) fn files_folder_now(&self) -> Option<String> {
        let selected = self.workspace.selected()?;
        let work = self.workspace.work();
        let cwd_of = |id: u32| {
            self.workspace
                .tab(id)
                .and_then(|tab| tab.content.session.grid.shared.state.lock().ok().and_then(|state| state.clone()))
                .and_then(|state| state.cwd)
        };
        let selected_cwd = cwd_of(selected);
        let group_root_cwd = work.group_of(selected).and_then(cwd_of);
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
        files_folder(work.folder_of(selected), selected_cwd.as_deref(), group_root_cwd.as_deref(), home.as_deref())
    }

    /// 파일 목록이 읽고 감시할 폴더를 지금 상태에 맞추고, 도착한 읽기 결과를 반영한다. 화면에 보이는 것이 바뀌었으면 true다.
    ///
    /// 목록은 사이드바가 보이고 구획이 펼쳐져 있을 때만 읽는다 — 가려진 동안은 폴더를 읽지도 감시하지도 않는다.
    /// 사이드바가 접혀 있는 동안에도 목록이 가리키는 폴더와 펼친 폴더는 기억한다.
    fn sync_files(
        &mut self,
        window: &Window,
    ) -> bool {
        self.files.set_window_visible(window.is_visible());
        let ready = matches!(self.phase, Phase::Ready);
        let sidebar_shown = ready && !self.workspace.sidebar_collapsed();
        let folder = if ready { self.files_folder_now() } else { None };
        let visible = sidebar_shown && self.files.files_expanded;
        let now = Instant::now();
        self.git.show(folder.as_deref(), now);
        let changed = self.files.show(folder, visible);
        let polled = self.files.poll();
        // WSL 폴더의 9P 수정 시각 비교가 폴더 안 변경을 봤다: Git 표시를 다시 읽을 사건이다.
        if self.files.take_change_seen() {
            self.git.note(crate::git::gate::Trigger::Change, now);
        }
        polled || changed
    }

    /// 파일 목록 머리줄의 접기 표지: 구획을 접으면 폴더를 읽지 않는다.
    pub(crate) fn toggle_files_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files.files_expanded = !self.files.files_expanded;
        self.sync_files(window);
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 폴더 행을 눌렀을 때: 접거나 펼친다.
    pub(crate) fn toggle_files_dir(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files.toggle_dir(path);
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 머리줄의 새로 고침: 읽어 둔 목록을 버리고 디스크에서 다시 읽는다.
    pub(crate) fn refresh_files(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files.refresh();
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 파일 목록 행의 우클릭 메뉴를 `position`(창 안 좌표)에 연다.
    pub(crate) fn open_entry_menu(
        &mut self,
        path: &str,
        is_dir: bool,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu(MenuKind::Entry { path: path.to_string(), is_dir }, position, window, cx);
    }

    /// 파일 관리자에서 항목을 보여 준다. 셸을 거치지 않고 인자 배열로 띄우며 콘솔 창을 만들지 않는다.
    pub(crate) fn reveal_in_system(
        &mut self,
        path: &str,
    ) {
        let (program, args) = reveal_command(std::env::consts::OS, path);
        self.spawn_external(&program, &args, "open the file manager");
    }

    /// 바깥 프로그램을 띄운다. 인자 배열로 직접 실행하고(셸 미경유) Windows에서는 콘솔 창을 만들지 않으며,
    /// 띄우지 못하면 상태 줄에 `Could not <action>: <이유>`를 알린다.
    pub(crate) fn spawn_external(
        &mut self,
        program: &str,
        args: &[String],
        action: &str,
    ) {
        let mut command = std::process::Command::new(program);
        command.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        match command.spawn() {
            // 띄운 프로그램이 끝난 뒤 좀비로 남지 않도록 다른 스레드가 거둔다.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => self.status = Some(format!("Could not {action}: {error}")),
        }
    }

    /// 새 파일·새 폴더 창을 연다. `parent`가 없으면 머리줄 단추가 만드는 자리(마지막으로 펼친 폴더 또는 목록 폴더 맨 위)다.
    pub(crate) fn begin_new_entry(
        &mut self,
        parent: Option<String>,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(parent) = parent.or_else(|| self.files.creation_folder()) {
            self.settle_composition(window, cx);
            let input = cx.new(|cx| InputState::new(window, cx));
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } if this.entry_input.as_ref() == Some(input) => {
                    this.commit_new_entry(window, cx)
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            })
            .detach();
            self.entry_input = Some(input.clone());
            self.overlay = Overlay::NewEntry { parent, is_dir };
            self.menu = None;
            self.quick = QuickState::default();
            self.remember_choice = false;
            input.update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    /// 새 파일·새 폴더 창의 이름 검사 결과.
    pub(crate) fn entry_name_check(
        &self,
        cx: &gpui_kit::App,
    ) -> EntryNameCheck {
        self.entry_input.as_ref().map_or(EntryNameCheck::Empty, |input| validate_entry_name(&input.read(cx).value()))
    }

    /// 입력한 이름으로 항목을 만들고 창을 닫는다. 이름이 규칙에 맞지 않으면 아무것도 하지 않는다.
    /// 만들기는 다른 스레드에서 하고, 같은 이름이 있으면 상태 줄에 알린다.
    pub(crate) fn commit_new_entry(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Overlay::NewEntry { parent, is_dir }, Some(input)) = (self.overlay.clone(), self.entry_input.clone())
            && self.entry_name_check(cx) == EntryNameCheck::Valid
        {
            let typed = input.read(cx).value().to_string();
            // 입력 방식이 마지막 글자의 확정을 창이 닫힌 뒤 터미널에 뒤늦게 알려도 PTY로 가지 않게 기억한다.
            self.already_sent.note_commit_elsewhere(&typed);
            self.entry_input = None;
            self.overlay = Overlay::None;
            self.status = None;
            self.focus_selected(window, cx);
            cx.spawn_in(window, async move |this, cx| {
                let created = {
                    let (parent, name) = (parent.clone(), typed.clone());
                    cx.background_executor()
                        .spawn(async move { crate::files::create_entry(&parent, &name, is_dir) })
                        .await
                };
                let _ = this.update_in(cx, |this, _window, cx| {
                    match created {
                        Ok(path) => this.files.show_added(&parent, is_dir.then_some(path.as_str())),
                        Err(message) => this.status = Some(message),
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
        }
    }

    /// 칸 경계 끌기를 시작한다. 끌기 시작 때의 비율과 그 분할의 길이를 기억한다.
    pub(crate) fn begin_divider_drag(
        &mut self,
        start: DividerStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (width, height) = self.body_size.get();
        let layout = self.workspace.view_layout();
        let found = layout.as_ref().and_then(|layout| {
            let extent = layout.extent_at(&start.path, width, height)?;
            let sizes = layout.sizes_at(&start.path)?;
            Some((extent, sizes))
        });
        if let Some(((extent_width, extent_height), sizes)) = found {
            let extent = match start.orientation {
                Orientation::Horizontal => extent_width,
                Orientation::Vertical => extent_height,
            };
            self.divider_drag = Some(DividerDrag { start, sizes, extent });
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 터미널 배치와 파일 칸 사이의 경계 끌기를 시작한다. 기준은 두 칸을 합친 본문 폭과 그때의 폭 비율이다.
    pub(crate) fn begin_file_divider_drag(
        &mut self,
        position: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ratio = self.workspace.file_pane().map(|pane| f64::from(pane.ratio));
        let extent = self.file_body_width.get();
        if let Some(ratio) = ratio
            && extent > 0.0
        {
            let start =
                DividerStart { path: Vec::new(), index: 0, orientation: Orientation::Horizontal, position, file: true };
            self.divider_drag = Some(DividerDrag { start, sizes: vec![1.0 - ratio, ratio], extent });
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 파일 칸 경계를 끌고 있는지(강조 표시용).
    pub(crate) fn file_divider_is_dragged(&self) -> bool {
        self.divider_drag.as_ref().is_some_and(|drag| drag.start.file)
    }

    /// 끌고 있는 칸 경계가 `path`의 `index`번째 경계인지(강조 표시용).
    pub(crate) fn divider_is_dragged(
        &self,
        path: &[usize],
        index: usize,
    ) -> bool {
        self.divider_drag
            .as_ref()
            .is_some_and(|drag| !drag.start.file && drag.start.path == path && drag.start.index == index)
    }

    /// 마우스가 움직이면 끌고 있는 경계를 옮긴다. 이웃 두 칸이 크기를 주고받고 어느 칸도 최소 크기 밑으로 줄지 않는다.
    fn drag_divider(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.divider_drag = None;
        } else if let Some(drag) = &self.divider_drag
            && drag.extent > 0.0
        {
            let position = match drag.start.orientation {
                Orientation::Horizontal => f32::from(event.position.x),
                Orientation::Vertical => f32::from(event.position.y),
            };
            let delta = f64::from((position - drag.start.position) / drag.extent);
            let min_fraction = f64::from(crate::look::scaled(split::MIN_PANE_SIZE) / drag.extent);
            let sizes = drag_boundary(&drag.sizes, drag.start.index, delta, min_fraction);
            let (path, is_file) = (drag.start.path.clone(), drag.start.file);
            if is_file {
                // 파일 칸의 폭 비율은 두 칸 중 뒤쪽(파일 칸)의 몫이다.
                if let (Some(key), Some(ratio)) = (self.workspace.view_key(), sizes.get(1)) {
                    self.workspace.set_file_ratio(&key, *ratio as f32);
                }
            } else {
                self.workspace.resize_split(&path, sizes);
            }
            cx.notify();
        }
    }

    /// 마우스를 놓으면 끌기를 끝내고 바뀐 비율을 저장한다.
    fn end_divider_drag(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.divider_drag.take().is_some() {
            self.persist();
            cx.notify();
        }
    }

    /// 사이드바 행을 눌렀을 때: 터미널 행은 그 터미널을, 묶음 행은 묶음에서 마지막으로 쓴 터미널을 선택한다.
    /// 그 터미널이 다른 창에 있으면 선택하지 않고 그 창을 앞으로 가져온다.
    pub(crate) fn select_row(
        &mut self,
        target: RowTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = match target {
            RowTarget::Terminal(id) => Some(id),
            RowTarget::Group(root) => {
                self.workspace.work().groups.iter().find(|group| group.root == root).map(|group| group.last_used)
            }
        };
        match id {
            // 다른 창에 있는 터미널의 행·탭을 누르면 이 창의 선택을 바꾸지 않고 그 창을 앞으로 가져온다.
            Some(id) if self.workspace.is_detached(id) => self.activate_detached(id, cx),
            Some(id) => self.select_tab(id, window, cx),
            None => {}
        }
    }

    /// 작업 폴더 행을 접거나 펼친다.
    pub(crate) fn toggle_folder(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let expanded =
            self.workspace.work().folders.iter().find(|folder| folder.key == key).map(|folder| folder.expanded);
        if let Some(expanded) = expanded {
            self.workspace.work_mut().set_expanded(key, !expanded);
            self.persist();
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 저장소 묶음 행을 접고 펼친다. 저장된 펼침을 뒤집는다 — 현재 터미널 때문에 억지로 펼쳐 보이는 상태가 아니라 저장된 값 기준이다.
    pub(crate) fn toggle_repo(
        &mut self,
        repo: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let expanded = self.workspace.work().repo_expanded(repo);
        self.workspace.work_mut().set_repo_expanded(repo, !expanded);
        self.persist();
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// `Show N more`: 그 폴더의 터미널을 한도 없이 모두 보인다.
    pub(crate) fn show_more(
        &mut self,
        folder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_all.insert(folder.to_string());
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 메뉴를 `position`(창 안 좌표)에 연다. 창 오른쪽·아래 끝을 넘으면 안쪽으로 당긴다.
    pub(crate) fn open_menu(
        &mut self,
        kind: MenuKind,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = self.menu_entries(&kind);
        let has_note = matches!(kind, MenuKind::Folder(_));
        let viewport = window.viewport_size();
        let panel = (
            crate::look::scaled(paddock_core::theme::menu::MIN_WIDTH),
            crate::look::scaled(panel_height(items.len(), has_note)),
        );
        let (x, y) = clamped_position(
            (f32::from(viewport.width), f32::from(viewport.height)),
            (f32::from(position.x), f32::from(position.y)),
            panel,
        );
        self.quick = QuickState::default();
        self.tab_bar_focused = false;
        self.menu = Some(ContextMenu { x, y, kind, nav: paddock_core::menu_nav::MenuNav::default() });
        cx.notify();
    }

    pub(crate) fn close_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.take().is_some() {
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 메뉴에서 고른 항목을 실행한다.
    pub(crate) fn run_menu_action(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (menu_x, menu_y) = self.menu.as_ref().map_or((0.0, 0.0), |menu| (menu.x, menu.y));
        self.menu = None;
        match action {
            MenuAction::RenameTerminal(id) => self.begin_rename(RenameTarget::Terminal(id), window, cx),
            MenuAction::CloseTerminal(id) => self.request_close(id, window, cx),
            MenuAction::CloseGroup(root) => self.request_close_group(root, window, cx),
            MenuAction::NewInFolder(key) => self.new_terminal_in_folder(key, window, cx),
            MenuAction::RenameFolder(key) => self.begin_rename(RenameTarget::Folder(key), window, cx),
            MenuAction::RemoveFolder(key) => self.request_remove_folder(key, window, cx),
            MenuAction::NewInEnvironment(index) => {
                let environment = available_environments().get(index).cloned();
                self.new_terminal_in_new_group(environment, window, cx);
            }
            MenuAction::NewPlain(place) => self.open_plain_terminal(place, window, cx),
            MenuAction::NewAccount { id, place } => self.open_account_terminal(&id, place, window, cx),
            MenuAction::ManageAccounts => self.open_accounts(window, cx),
            MenuAction::ContinueWith { terminal, id } => self.switch_terminal_account(terminal, &id, window, cx),
            MenuAction::StartHere { terminal, id } => self.start_account_here(terminal, &id, window, cx),
            MenuAction::OpenNewTerminalMenu(place) => {
                let at = gpui_kit::point(gpui_kit::px(menu_x), gpui_kit::px(menu_y));
                self.open_new_terminal_menu(place, at, true, window, cx)
            }
            MenuAction::Noop => {}
            MenuAction::NewEntryIn { parent, is_dir } => self.begin_new_entry(Some(parent), is_dir, window, cx),
            MenuAction::Reveal(path) => self.reveal_in_system(&path),
            MenuAction::OpenFile(path) => self.open_file(&path, window, cx),
            // 다른 창에 있는 터미널의 `Show to the Side`는 이 창의 옆 칸으로 되돌리는 길이다.
            MenuAction::ShowBeside(id) if self.workspace.is_detached(id) => self.move_back_beside(id, window, cx),
            MenuAction::ShowBeside(id) => self.show_beside_terminal(id, window, cx),
            MenuAction::PasteInto { path, is_dir } => {
                self.files.select_row(&path);
                self.paste_into_files(Some((path, is_dir)), window, cx)
            }
            MenuAction::RenameEntry { path, is_dir } => {
                self.begin_rename(RenameTarget::Entry { path, is_dir }, window, cx)
            }
            MenuAction::EditorCut => {
                self.editor_clipboard_action(Box::new(gpui_kit::component::input::Cut), window, cx)
            }
            MenuAction::EditorCopy => {
                self.editor_clipboard_action(Box::new(gpui_kit::component::input::Copy), window, cx)
            }
            MenuAction::EditorPaste => {
                self.editor_clipboard_action(Box::new(gpui_kit::component::input::Paste), window, cx)
            }
            MenuAction::AddReviewNote => {
                self.begin_review_note(window, cx);
            }
            MenuAction::SendReviewNotes => {
                let at = gpui_kit::point(gpui_kit::px(menu_x), gpui_kit::px(menu_y));
                self.send_review_notes(at, window, cx)
            }
            MenuAction::ClearReviewNotes => self.clear_review_notes(cx),
            MenuAction::SendNotesTo(id) => self.send_notes_to(id, window, cx),
            MenuAction::MoveToNewWindow(id) => self.move_to_new_window(id, None, window, cx),
            MenuAction::MoveBack(id) => self.move_back(id, None, window, cx),
            MenuAction::MoveFileToNewWindow(index) => {
                if let Some(group) = self.workspace.view_key() {
                    self.move_file_to_new_window(group, index, None, window, cx);
                }
            }
            MenuAction::CloseFileTab(index) => self.close_file_tab(index, window, cx),
        }
        cx.notify();
    }

    /// 이름 바꾸기 창을 연다. 입력칸에는 지금 이름이 전체 선택된 채 들어 있다.
    /// 터미널에서 한글을 조합하던 중이면, 입력칸이 열리기 전에 그 글자를 터미널로 확정한다. 입력칸을 여는 모든 경로가 이 함수 하나를 지난다.
    ///
    /// 운영체제 입력기의 조합을 먼저 끝내고(그 글자는 입력기가 터미널의 입력 처리기로 보낸다), 앱이 쥔 조합 기억도 비운다.
    /// 그러지 않으면 입력기가 조합을 새 입력칸에 확정해 이름 앞에 끼어든다.
    pub(crate) fn settle_composition(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::platform_keys::complete_composition(window);
        if let Some(tab) = self.workspace.selected().and_then(|id| self.workspace.tab(id)) {
            tab.content.view.update(cx, |view, cx| view.flush_composition(cx));
        }
    }

    pub(crate) fn begin_rename(
        &mut self,
        target: RenameTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = match &target {
            RenameTarget::Terminal(id) => self.workspace.name(*id).to_string(),
            RenameTarget::Folder(key) => {
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
                folder_name(key, self.workspace.work().folder_label(key), home.as_deref())
            }
            RenameTarget::Entry { path, .. } => file_name_of(path),
        };
        // 파일 이름은 확장자 앞까지만 골라 두고(확장자는 그대로 두는 일이 많다), 그 밖의 이름은 전체를 고른다.
        let selection = match &target {
            RenameTarget::Entry { is_dir, .. } => Some(initial_selection(&initial, *is_dir)),
            _ => None,
        };
        self.settle_composition(window, cx);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { .. } if this.rename_input.as_ref() == Some(input) => {
                this.commit_rename(window, cx)
            }
            // 이름 바꾸기 창의 이유 줄·OK 단추가 입력을 따라가도록 다시 그린다.
            InputEvent::Change => cx.notify(),
            _ => {}
        })
        .detach();
        self.rename_input = Some(input.clone());
        self.overlay = Overlay::Rename(target);
        self.remember_choice = false;
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            match selection {
                Some(range) => state.set_selected_range(range, cx),
                None => state.select_all(window, cx),
            }
        });
        cx.notify();
    }

    /// 입력칸의 이름을 적용하고 창을 닫는다. 터미널 이름은 비우면 바꾸지 않고, 폴더 이름은 비우면 폴더 이름으로 돌아간다.
    pub(crate) fn commit_rename(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Overlay::Rename(target), Some(input)) = (self.overlay.clone(), self.rename_input.clone()) {
            let typed = input.read(cx).value().to_string();
            // 파일·폴더 이름이 규칙에 어긋나면 창을 닫지 않는다(이유 줄이 보이고 OK는 흐리다).
            let blocked =
                matches!(target, RenameTarget::Entry { .. }) && validate_entry_name(&typed) != EntryNameCheck::Valid;
            if !blocked {
                // 입력 방식이 마지막 글자의 확정을 창이 닫힌 뒤 터미널에 뒤늦게 알려도 PTY로 가지 않게 기억한다.
                self.already_sent.note_commit_elsewhere(&typed);
                let name = typed.trim().to_string();
                match target {
                    RenameTarget::Terminal(id) if !name.is_empty() => self.workspace.rename(id, &name),
                    RenameTarget::Terminal(_) => {}
                    RenameTarget::Folder(key) => self.workspace.work_mut().set_folder_label(&key, &name),
                    // 디스크의 이름 바꾸기는 입력한 그대로 한다(앞뒤 빈칸도 이름의 일부다).
                    RenameTarget::Entry { path, .. } => self.rename_entry_to(path, typed, window, cx),
                }
                self.rename_input = None;
                self.overlay = Overlay::None;
                self.persist();
                self.focus_selected(window, cx);
                cx.notify();
            }
        }
    }

    /// Unassigned 묶음을 닫으려 한다. 살아 있는 셸이 있으면 닫을 터미널 수를 보이며 확인을 묻는다.
    pub(crate) fn request_close_group(
        &mut self,
        root: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self.workspace.work().group_members(root);
        let any_alive = ids.iter().any(|id| !self.is_exited(*id));
        let n_unsaved = self.n_unsaved_in(&GroupKey::Group(root));
        if needs_close_prompt(any_alive, n_unsaved) {
            if !ids.is_empty() {
                self.open_overlay(Overlay::CloseGroup(root), window, cx);
            }
        } else {
            self.close_group(root, window, cx);
        }
    }

    pub(crate) fn close_group(
        &mut self,
        root: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for id in self.workspace.work().group_members(root) {
            self.close_terminal(id, window, cx);
        }
    }

    /// 작업 폴더를 목록에서 빼려 한다. 디스크의 폴더는 그대로이고 그 폴더의 터미널은 닫힌다.
    fn request_remove_folder(
        &mut self,
        key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self.workspace.work().terminals_of(&key);
        let any_alive = ids.iter().any(|id| !self.is_exited(*id));
        let n_unsaved = self.n_unsaved_in(&GroupKey::Folder(key.clone()));
        if needs_close_prompt(any_alive, n_unsaved) {
            self.open_overlay(Overlay::RemoveFolder(key), window, cx);
        } else {
            self.remove_folder(&key, window, cx);
        }
    }

    pub(crate) fn remove_folder(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 터미널을 하나씩 닫으면 선택이 곧 닫힐 터미널로 옮겨 다니므로, 뺀 뒤 갈 터미널을 먼저 정해 둔다.
        let next = self.workspace.work().clone().remove_folder(key, self.workspace.selected()).next;
        for id in self.workspace.work().terminals_of(key) {
            self.close_terminal(id, window, cx);
        }
        self.workspace.remove_folder(key);
        self.prune_file_views();
        self.show_all.remove(key);
        if let Some(next) = next.filter(|id| self.workspace.tab(*id).is_some()) {
            self.select_tab(next, window, cx);
        }
        self.persist();
        cx.notify();
    }

    /// 터미널 `id`를 닫으면 함께 닫히는 파일 탭 수. Unassigned 묶음의 마지막 터미널이면 그 묶음의 파일 탭이고, 아니면 0이다.
    pub(crate) fn files_closed_with(
        &self,
        id: u32,
    ) -> usize {
        let work = self.workspace.work();
        match work.group_of(id) {
            Some(root) if work.group_members(root).len() == 1 => {
                self.workspace.files().n_open_in(&GroupKey::Group(root))
            }
            _ => 0,
        }
    }

    fn is_exited(
        &self,
        id: u32,
    ) -> bool {
        self.workspace.tab(id).is_none_or(|tab| tab.content.session.grid.shared.exited.load(Ordering::Relaxed))
    }

    /// 터미널을 닫으려 한다. 셸이 살아 있으면 확인을 묻고, 이미 끝났으면 바로 닫는다.
    pub(crate) fn request_close(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let exited = self
            .workspace
            .tab(id)
            .is_none_or(|tab| tab.content.session.grid.shared.exited.load(std::sync::atomic::Ordering::Relaxed));
        if self.workspace.tab(id).is_some() {
            // 끝난 터미널이라도 함께 닫히는 파일 탭에 저장하지 않은 변경이 있으면 묻는다.
            if !needs_close_prompt(!exited, self.n_unsaved_closed_with(id)) {
                self.close_terminal(id, window, cx);
            } else {
                self.open_overlay(Overlay::CloseTerminal(id), window, cx);
            }
        }
    }

    /// 데몬에 종료를 요청하고 탭을 닫는다. 선택한 탭이었으면 가까운 탭으로 옮긴다.
    pub(crate) fn close_terminal(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.snapshot_active();
        if paddock_core::terminal_find::bar_after_close(self.find.as_ref().map(|bar| bar.terminal), id).is_none() {
            // 닫는 터미널의 찾기 띠는 같이 닫는다(번호가 다시 쓰여도 새 터미널에 옛 띠가 뜨지 않게).
            self.find = None;
        }
        if self.row_drop.is_some_and(|(over, _)| over == id) {
            self.row_drop = None;
        }
        if let Some(tab) = self.workspace.close(id) {
            // 다른 창에 있던 터미널이면 그 창도 닫는다(빈 창을 남기지 않는다).
            self.close_detached_window(id, cx);
            self.done.forget(id);
            self.seen_state_versions.remove(&id);
            // 같은 묶음에 머물면(닫은 것이 선택한 터미널이든 시작 터미널이든) 파일 탭이 활성인 채로 둔다.
            self.settle_active(before, false);
            self.prune_file_views();
            let _ = tab.content.session.outbox.send(&Frame::json(kind::KILL, &Kill { id }));
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 종료 묻기에서 고른 선택을 따라 창을 닫는다. "Don't ask again"이 켜져 있으면 선택을 설정에 저장한다.
    pub(crate) fn choose_quit(
        &mut self,
        choice: OnQuit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.remember_choice
            && let Err(error) = write_on_quit(choice)
        {
            app_log::event(&format!("could not save the quit setting: {error}"));
        }
        self.overlay = Overlay::None;
        if choice == OnQuit::Stop {
            self.stop_all();
        } else {
            self.persist();
        }
        self.quit_confirmed = true;
        self.close_detached_windows(cx);
        window.remove_window();
        cx.notify();
    }

    /// 모든 터미널을 끝내도록 데몬에 종료를 요청한다. 앱이 곧 끝나므로 요청이 데몬에 쓰이기를 기다린다.
    ///
    /// 터미널을 하나씩 닫는 요청(`KILL`)이 아니라 데몬 종료를 쓴다: 사용자가 닫은 터미널은 기록이 지워지지만 종료로 끝난 터미널은
    /// 기록이 남아, 다음에 앱을 열면 같은 폴더·같은 대화로 되살아난다. 되살릴 때 쓰도록 탭 상태를 먼저 저장하고, 이후 탭을 비워 저장하지 않는다.
    fn stop_all(&mut self) {
        self.persist();
        if let Some(tab) = self.workspace.tabs().first() {
            request_shutdown(&tab.content.session.outbox, KILL_FLUSH_WAIT);
        }
    }

    /// 보기 줄 톱니: 빠른 설정 패널을 열거나 닫는다.
    pub(crate) fn toggle_quick_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quick.open {
            self.close_quick_settings(window, cx);
        } else {
            self.menu = None;
            self.installed_fonts = cx.text_system().all_font_names();
            self.quick_settings = Settings::load();
            self.quick = QuickState { open: true, expanded: None };
            cx.notify();
        }
    }

    pub(crate) fn close_quick_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quick.open {
            self.quick = QuickState::default();
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 이 컴퓨터에 설치된 글꼴 이름들(패널을 연 때 읽은 것).
    pub(crate) fn installed_fonts(&self) -> &[String] {
        &self.installed_fonts
    }

    /// 패널이 보여 주는 설정(패널을 열 때, 그리고 값을 바꿀 때 다시 읽는다).
    pub(crate) fn quick_settings(&self) -> &Settings {
        &self.quick_settings
    }

    /// 선택상자의 선택지를 펼치거나 접는다. 하나만 펼쳐 둔다.
    pub(crate) fn quick_expand(
        &mut self,
        field: Field,
        cx: &mut Context<Self>,
    ) {
        self.quick.expanded = if self.quick.expanded == Some(field) { None } else { Some(field) };
        cx.notify();
    }

    /// 선택상자에서 고른 값을 네이티브 설정 파일에 쓰고 바로 적용한다.
    pub(crate) fn quick_choose(
        &mut self,
        field: Field,
        value: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (key, value) = setting_of(field, &value);
        self.quick.expanded = None;
        self.apply_setting(key, value, cx);
    }

    /// ± 단추: 배율 단계나 터미널 글꼴 크기를 한 칸 바꾼다.
    pub(crate) fn quick_step(
        &mut self,
        row: Row,
        delta: i32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = &self.quick_settings;
        match row {
            Row::Zoom => {
                let level = step_zoom_level(settings.zoom_level(), delta);
                self.apply_setting(key::ZOOM_LEVEL, Value::from(level as i64), cx);
            }
            Row::FontSize => {
                let size = step_font_size(settings.values().terminal_font_size(), delta);
                self.apply_setting(key::TERMINAL_FONT_SIZE, Value::from(size), cx);
            }
            Row::SidebarIndent => {
                let indent = step_sidebar_indent(settings.values().sidebar_indent(), delta);
                self.apply_setting(key::SIDEBAR_INDENT, Value::from(indent), cx);
            }
            Row::MemoryNoticeMinimumMb => {
                let minimum = step_memory_notice_mb(settings.values().memory_notice_minimum_mb(), delta);
                self.apply_setting(key::MEMORY_NOTICE_MINIMUM_MB, Value::from(minimum), cx);
            }
            _ => {}
        }
    }

    /// 에이전트 완료 소리를 켜고 끈다. 다음 완료부터 적용된다.
    pub(crate) fn quick_toggle_sound(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = self.quick_settings.agent_done_sound();
        self.apply_setting(key::AGENT_DONE_SOUND, Value::Bool(!on), cx);
    }

    /// 상태 줄 항목(Claude usage·Codex usage·Memory) 하나를 켜고 끈다. 끄면 그 묶음만 상태 줄에서 빠지고 원래 설정은 그대로다.
    pub(crate) fn quick_toggle_status_item(
        &mut self,
        row: Row,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(item_key) = row.status_item_key() {
            let on = self.quick_settings.values().status_item(item_key);
            self.apply_setting(item_key, Value::Bool(!on), cx);
        }
    }

    /// 메모리 안내(메모리가 부족할 때 Claude Code 에이전트에게 알리는 훅)를 켜고 끈다. 바꾼 뒤 시작하는 Claude부터 적용된다.
    pub(crate) fn quick_toggle_memory_notice(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = self.quick_settings.values().memory_notice();
        self.apply_setting(key::MEMORY_NOTICE, Value::Bool(!on), cx);
    }

    /// 터미널에서 고른 글자를 바로 복사하는 설정을 켜고 끈다. 다음 선택부터 적용된다.
    pub(crate) fn quick_toggle_copy_on_selection(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = self.quick_settings.values().copy_on_selection();
        self.apply_setting(key::COPY_ON_SELECTION, Value::Bool(!on), cx);
    }

    /// 설정 하나를 네이티브 설정 파일에 쓰고, 화면 모양(테마·배율·글꼴)을 다시 읽어 모든 보기에 바로 적용한다.
    /// 터미널 칸 수는 다음 그리기에서 새 글꼴 크기로 다시 계산되고 데몬에 알려진다. 쓰지 못하면 상태 줄에 알린다.
    fn apply_setting(
        &mut self,
        key: &str,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        match write_native(key, value) {
            Ok(()) => {
                self.quick_settings = Settings::load();
                self.usage.set_items(StatusItems::from_settings(self.quick_settings.values()));
                self.setup_after_setting(key);
                crate::look::reload();
                crate::chrome::sync_component_theme(cx);
            }
            Err(error) => {
                app_log::event(&format!("could not save the setting {key}: {:?}", error.kind()));
                self.status = Some(format!("Could not save the setting: {error}"));
            }
        }
        cx.notify();
    }

    /// 빠른 설정의 `Quit and Stop All Terminals`: 설정(`onQuit`)과 무관하게 이번만 모든 터미널을 끝내고 앱을 닫는다. 설정은 그대로다.
    pub(crate) fn quit_and_stop_all(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.quick = QuickState::default();
        match quit_and_stop_all_step(self.n_terminals()) {
            QuitStep::StopAll => self.stop_all(),
            QuitStep::Close | QuitStep::Keep | QuitStep::Prompt => self.persist(),
        }
        self.quit_confirmed = true;
        self.close_detached_windows(cx);
        window.remove_window();
        cx.notify();
    }

    /// 창 닫기 요청을 처리한다. 닫아도 되면 true이고, 사용자에게 물어야 하면 묻기 창을 띄우고 false다.
    pub(crate) fn on_close_requested(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // 저장하지 않은 파일이 있으면 터미널 종료 묻기보다 먼저 저장 여부를 묻는다(저장하거나 버리기로 한 뒤에는 건너뛴다).
        let asks_unsaved = !self.quit_confirmed && !self.quit_unsaved_resolved && !self.unsaved.is_empty();
        if asks_unsaved {
            self.open_overlay(Overlay::QuitUnsaved, window, cx);
        }
        let close_now = !asks_unsaved
            && (self.quit_confirmed
                || match quit_step(read_on_quit(), self.n_terminals()) {
                    QuitStep::Close | QuitStep::Keep => {
                        self.persist();
                        true
                    }
                    QuitStep::StopAll => {
                        self.stop_all();
                        true
                    }
                    QuitStep::Prompt => {
                        self.open_overlay(Overlay::Quit, window, cx);
                        false
                    }
                });
        if close_now {
            self.quit_confirmed = true;
            // 분리 창이 하나라도 남으면 앱이 끝나지 않는다. 탭은 이미 저장했고 다음 시작에는 첫 칸 끝에 놓인다.
            self.close_detached_windows(cx);
        }
        close_now
    }

    /// 앱 종료 요청(macOS의 Cmd+Q·메뉴 Quit): 창 닫기 버튼과 같은 흐름(저장 안 한 파일·종료 묻기)을 타고, 닫아도 되면 창을 닫는다.
    /// 묻기 창이 떠야 하면 창은 그대로 두고 사용자의 선택을 기다린다.
    #[cfg(target_os = "macos")]
    pub(crate) fn request_quit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.on_close_requested(window, cx) {
            window.remove_window();
        } else {
            // 묻기 창은 본 창에 뜨므로, 분리 창이나 다른 앱에서 눌렀어도 본 창을 앞으로 가져와 보이게 한다.
            self.activate_main(window);
        }
    }

    /// 창 전체가 받는 키: 앱 단축키(탭 이동·분할·새 터미널·닫기·사이드바 접기)와 묻기 창의 Enter·Esc. 터미널보다 먼저 받는다.
    fn on_capture_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // 비라틴 배열의 글자 키는 같은 자리의 영문 이름으로 바꿔 단축키를 맞춘다.
        let key_name = crate::platform_keys::shortcut_key(&keystroke.key);
        let key = key_name.as_str();
        if key == "escape" && (self.tab_drag.is_some() || self.file_drag.is_some()) {
            // 탭을 끌던 중의 Esc: 끌기를 취소한다(놓아도 아무 일 없다).
            self.cancel_tab_drag(window, cx);
            cx.stop_propagation();
        } else if self.menu.is_some() {
            self.menu_key(key, window, cx);
            cx.stop_propagation();
        } else if self.usage_popup.is_some() {
            // 사용량 목록: Esc는 닫고, 검색칸이 있으면 나머지 키는 입력칸이 받는다. 검색칸이 없으면 키가 터미널로 가지 않는다.
            let search_shown = search_visible(self.usage.book.profiles().len(), &self.usage_popup_query(cx));
            if key == "escape" {
                self.close_usage_popup(window, cx);
                cx.stop_propagation();
            } else if !search_shown {
                cx.stop_propagation();
            }
        } else if self.quick.open {
            // 빠른 설정이 열려 있는 동안 키는 터미널로 가지 않는다. Esc는 펼친 선택지를 먼저 접고, 그다음 패널을 닫는다.
            if key == "escape" {
                if self.quick.expanded.is_some() {
                    self.quick.expanded = None;
                    cx.notify();
                } else {
                    self.close_quick_settings(window, cx);
                }
            }
            cx.stop_propagation();
        } else if self.overlay == Overlay::Accounts {
            // Accounts 창은 입력칸이 키를 받는다. 여기서는 Esc만 다룬다: 열린 수정이 있으면 그것을, 없으면 창을 닫는다.
            if key == "escape" {
                if self
                    .accounts
                    .model
                    .as_ref()
                    .is_some_and(|model| model.editing != crate::accounts::model::Editing::None)
                {
                    self.accounts_cancel_edit(window, cx);
                } else {
                    self.cancel_overlay(window, cx);
                }
                cx.stop_propagation();
            }
        } else if self.overlay == Overlay::QuickOpen {
            // 빠른 열기: ↑·↓·Home·End는 목록 강조, Esc는 닫기, Enter는 입력칸이 알리는 사건으로 받는다. 나머지 키는 입력칸이 받는다.
            if let Some(nav) =
                paddock_core::menu_nav::NavKey::from_name(key).filter(|nav| matches!(nav, NavKey::Up | NavKey::Down))
            {
                self.quick_open_step(nav, cx);
                cx.stop_propagation();
            } else if key == "escape" {
                self.cancel_overlay(window, cx);
                cx.stop_propagation();
            }
        } else if matches!(
            self.overlay,
            Overlay::Rename(_) | Overlay::NewEntry { .. } | Overlay::GoToLine { .. } | Overlay::ReviewNote { .. }
        ) {
            // 이름 바꾸기·새 항목 창의 Enter는 입력칸이 한글 조합을 확정한 뒤 알리는 사건으로 받는다. 여기서는 Esc만 다루고
            // 나머지 키는 입력칸이 받도록 흘려보낸다.
            if key == "escape" {
                self.cancel_overlay(window, cx);
                cx.stop_propagation();
            }
        } else if self.overlay != Overlay::None {
            match key {
                "enter" => self.confirm_overlay(window, cx),
                "escape" => self.cancel_overlay(window, cx),
                _ => {}
            }
            cx.stop_propagation();
        } else if key == "escape" && self.find_input_focused(window, cx) {
            // 찾기 입력칸에서 Esc: 띠를 닫고 키 입력을 선택한 터미널로 돌린다.
            self.close_terminal_find(window, cx);
            cx.stop_propagation();
        } else if key == "escape" && self.search.open && self.search_input_focused(window, cx) {
            // 검색 입력칸에서 Esc: 검색 보기를 닫고 키 입력을 선택한 탭으로 돌린다.
            self.close_search(window, cx);
            cx.stop_propagation();
        } else if ShortcutScheme::NATIVE.accepts_command_key(modifiers.platform) {
            let scheme = ShortcutScheme::NATIVE;
            let input = KeyInput {
                key,
                key_char: keystroke.key_char.as_deref(),
                control: modifiers.control,
                shift: modifiers.shift,
                alt: modifiers.alt,
                command: modifiers.platform,
            };
            // 목록이 키를 받는 동안의 F2(이름 바꾸기)·Ctrl+V(붙여넣기, macOS는 Cmd+V). 아니면 이 키들은 터미널로 가고, 다른 키는 목록이 키를 거두게 한다.
            let list_modifiers = (scheme.primary(&input), modifiers.shift, modifiers.alt);
            if self.list_key_pressed(key, list_modifiers, window, cx)
                || self.tab_bar_key_pressed(key, modifiers.control || modifiers.platform, modifiers.alt, window, cx)
                || self.run_file_shortcut(file_shortcut(&input, scheme), window, cx)
                // PDF 탭이 활성이면 Cmd·Ctrl + `=`·`-`·`0`은 터미널 글꼴 크기 키보다 먼저 PDF 배율로 간다.
                || self.run_preview_shortcut(preview_shortcut(&input, scheme), window, cx)
            {
                cx.stop_propagation();
            } else if let Some(shortcut) = app_shortcut(&input, scheme) {
                self.run_shortcut(shortcut, window, cx);
                cx.stop_propagation();
            }
        }
    }

    /// 앱 단축키 하나를 실행한다. 터미널보다 먼저 받으므로 이 키들은 PTY로 가지 않는다.
    pub(crate) fn run_shortcut(
        &mut self,
        shortcut: AppShortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match shortcut {
            AppShortcut::PreviousTab => self.step_tab(TabTarget::Previous, window, cx),
            AppShortcut::NextTab => self.step_tab(TabTarget::Next, window, cx),
            AppShortcut::FirstTab => self.step_tab(TabTarget::First, window, cx),
            AppShortcut::LastTab => self.step_tab(TabTarget::Last, window, cx),
            AppShortcut::TabAt(index) => self.step_tab(TabTarget::Index(index), window, cx),
            AppShortcut::FontLarger => self.quick_step(Row::FontSize, 1, window, cx),
            AppShortcut::FontSmaller => self.quick_step(Row::FontSize, -1, window, cx),
            AppShortcut::NewTerminalHere => self.new_terminal_here(window, cx),
            AppShortcut::SplitRight => self.split_selected(Orientation::Horizontal, window, cx),
            AppShortcut::SplitDown => self.split_selected(Orientation::Vertical, window, cx),
            AppShortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
            AppShortcut::CloseActiveTab => self.close_active_tab(window, cx),
            AppShortcut::SearchInFolder => self.toggle_search(window, cx),
            AppShortcut::FindInTerminal => self.open_terminal_find(window, cx),
            AppShortcut::QuickOpen => self.toggle_quick_open(window, cx),
        }
    }

    /// 자동 점검 대본을 창에 연결한다. 갱신 루프가 대본이 부탁한 일(키·크기·닫기)을 처리하고 창 상태를 대본에 보인다.
    #[cfg(feature = "automation")]
    pub fn start_automation(
        &mut self,
        hooks: crate::automation::Hooks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hooks = Some(hooks.clone());
        crate::automation::watch_ui_thread();
        crate::platform_keys::note_window_handle("main", window);
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let keys: Vec<String> = hooks.pending_keys.lock().unwrap().iter().cloned().collect();
                // 키는 창 보기를 빌려 쓰지 않는 갱신으로 넣는다 — 키 처리기가 창 보기를 다시 빌리기 때문이다.
                let _ = cx.update(|window, cx| {
                    for key in &keys {
                        if let Ok(keystroke) = gpui_kit::Keystroke::parse(key) {
                            window.dispatch_keystroke(keystroke, cx);
                        }
                    }
                });
                // 분리 창에 넣을 키도 같은 이유로 창 보기를 빌리지 않는 갱신으로 그 창에 넣는다.
                let window_keys: Vec<String> = hooks.window_keys.lock().unwrap().iter().cloned().collect();
                for entry in &window_keys {
                    let target = entry.split_once(' ').and_then(|(id, key)| {
                        let handle = this.read_with(cx, |this, _| this.detached_window_handle_named(id)).ok().flatten();
                        handle.zip(gpui_kit::Keystroke::parse(key).ok())
                    });
                    if let Some((handle, keystroke)) = target {
                        // 본 창을 빌리지 않는 앱 수준 갱신으로 넣는다 — 분리 창의 키 처리기가 본 창을 다시 빌리기 때문이다.
                        let _ = cx.update_window(handle, |_, detached_window, app| {
                            detached_window.dispatch_keystroke(keystroke, app)
                        });
                    }
                }
                hooks.window_keys.lock().unwrap().drain(..window_keys.len());
                let updated =
                    this.update_in(cx, |this, window, cx| this.automation_tick(&hooks, !keys.is_empty(), window, cx));
                hooks.pending_keys.lock().unwrap().drain(..keys.len());
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    #[cfg(feature = "automation")]
    fn automation_tick(
        &mut self,
        hooks: &crate::automation::Hooks,
        keys_sent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use std::sync::atomic::Ordering;

        use crate::automation::{Probe, ProbeTab};

        let now = Instant::now();
        crate::automation::mark_ui_tick();
        let cpu = crate::automation::thread_cpu_time();
        if let Some(last) = self.tick_gap.0 {
            let gap = now - last;
            let busy = cpu.saturating_sub(self.tick_cpu.0);
            if gap > self.tick_gap.1 {
                self.tick_gap.1 = gap;
                self.tick_cpu.1 = busy;
            }
            self.tick_cpu.2 = self.tick_cpu.2.max(busy);
        }
        self.tick_gap.0 = Some(now);
        self.tick_cpu.0 = cpu;
        let actions: Vec<String> = hooks.pending_actions.lock().unwrap().iter().cloned().collect();
        for action in &actions {
            self.automation_action(action, window, cx);
        }
        hooks.pending_actions.lock().unwrap().drain(..actions.len());
        let mut forced = keys_sent || !actions.is_empty();
        if let Some((width, height)) = hooks.pending_resize.lock().unwrap().take() {
            window.resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
            cx.notify();
            forced = true;
        }
        let due = self.probe_at.is_none_or(|at| now.duration_since(at) >= PROBE_INTERVAL);
        if forced || due {
            self.probe_at = Some(now);
            // 경로 링크 목록은 요약을 조립할 때만 새로 채운다(갱신 루프마다 화면을 훑지 않는다).
            self.refresh_term_links(cx);
            let detailed = hooks.state_readers.load(Ordering::Relaxed) > 0;
            let mut probe = hooks.probe.lock().unwrap();
            *probe = Probe {
                tabs: self
                    .workspace
                    .tabs()
                    .iter()
                    .map(|tab| ProbeTab {
                        id: tab.id,
                        name: self.workspace.name(tab.id).to_string(),
                        shell_pid: tab.content.session.shell_pid,
                        grid: tab.content.session.grid.clone(),
                        state: tab.content.session.grid.shared.state.lock().unwrap().clone(),
                        unseen: self.done.is_unseen(tab.id),
                        folder: self.workspace.work().folder_of(tab.id).map(str::to_string),
                        group: self.workspace.work().group_of(tab.id),
                        place: if self.workspace.is_detached(tab.id) { "detached" } else { "main" },
                    })
                    .collect(),
                selected: self.workspace.selected(),
                overlay: self.overlay.name(),
                sound_requests: crate::automation::sound_requests(),
                view: self.workspace.view_ids(),
                menu: match self.menu.as_ref().map(|menu| &menu.kind) {
                    None => "none",
                    Some(MenuKind::Terminal(_)) => "terminal",
                    Some(MenuKind::Group(_)) => "group",
                    Some(MenuKind::Folder(_)) => "folder",
                    Some(MenuKind::Environments) => "environments",
                    Some(MenuKind::NewTerminal(_)) => "new-terminal",
                    Some(MenuKind::AccountSwitch(_)) => "account-switch",
                    Some(MenuKind::Entry { .. }) => "entry",
                    Some(MenuKind::Editor { .. }) => "editor",
                    Some(MenuKind::PickAgent(_)) => "pick-agent",
                    Some(MenuKind::FileTab(_)) => "file-tab",
                },
                rows: self.sidebar_rows(),
                accounts: self.accounts_rows(),
                layout: self
                    .workspace
                    .view_layout()
                    .map(|layout| paddock_core::layout_model::Layout::serialize(Some(&layout)).to_string())
                    .unwrap_or_default(),
                sidebar_collapsed: self.workspace.sidebar_collapsed(),
                look: self.look_summary(),
                daemon: self.daemon_summary().0,
                notice: self.daemon_summary().1,
                files: self.files_summary(detailed, cx),
                sync: self.sync_summary(cx),
                usage: self.usage_summary(window, cx),
                detailed,
                windows: self
                    .detached_windows
                    .iter()
                    .map(|(id, detached)| crate::automation::ProbeWindow {
                        id: *id,
                        title: detached.shared.title.borrow().clone(),
                        size: detached.shared.size.get(),
                        active: detached.shared.active.get(),
                        n_activation_requests: detached.shared.n_activation_requests.get(),
                        unseen: self.done.is_unseen(*id),
                    })
                    .collect(),
                main_active: window.is_window_active(),
                n_main_activation_requests: self.n_main_activation_requests,
                body: self.body_size.get(),
                overflow: self
                    .tab_scrolls
                    .borrow()
                    .iter()
                    .map(|(key, handle)| {
                        let look = overflow_look(handle);
                        (*key, look.hidden_left, look.hidden_right)
                    })
                    .collect(),
            };
            crate::automation::publish_state(&probe);
        }
        if hooks.close_window.swap(false, Ordering::Relaxed) && self.on_close_requested(window, cx) {
            window.remove_window();
        }
    }

    /// 상태 줄 묶음. book이 바뀌지 않고 같은 분 안이면 앞서 계산한 것을 다시 쓴다(경과 시간·초기화까지 남은 시간이 분 단위라서다).
    fn usage_bar_groups(&self) -> Rc<Vec<paddock_core::usage_scopes::GroupView>> {
        let now = chrono::Utc::now().timestamp();
        let items = self.usage.items;
        let key: BarKey = (self.usage.book.n_changes(), now / 60, items.claude, items.codex);
        let cached = self
            .bar_cache
            .borrow()
            .as_ref()
            .filter(|(cached_key, _)| *cached_key == key)
            .map(|(_, groups)| groups.clone());
        cached.unwrap_or_else(|| {
            let groups = Rc::new(bar_groups(&self.usage.book, &|provider| items.provider(provider), now));
            *self.bar_cache.borrow_mut() = Some((key, groups.clone()));
            groups
        })
    }

    /// 상태 줄 사용량 요약(대본이 읽는다): `usage mode= button= items= reading= <도구>=<이름>:<창>=<사용량>…`와 게이지마다 말풍선 `usage-tip`.
    /// 값이 없으면 그 이유 글이 괄호로 들어간다. 계정 폴더 경로나 로그인 값은 싣지 않는다.
    #[cfg(feature = "automation")]
    fn usage_summary(
        &self,
        window: &Window,
        cx: &gpui_kit::App,
    ) -> Vec<String> {
        use paddock_core::usage_scopes::{ValueView, compact_button_visible, group_summary};
        let items = self.usage.items;
        let groups = bar_groups(&self.usage.book, &|provider| items.provider(provider), chrono::Utc::now().timestamp());
        let mode = self.usage_mode.get();
        let on = |value: bool| if value { "on" } else { "off" };
        let mut lines = vec![format!(
            "usage mode={} button={} items=claude:{},codex:{},memory:{} reading={} {}",
            if mode == BarMode::Compact { "compact" } else { "full" },
            if compact_button_visible(mode, &groups, items.claude || items.codex) { "yes" } else { "no" },
            on(items.claude),
            on(items.codex),
            on(items.memory),
            if self.usage.is_reading() { "yes" } else { "no" },
            if groups.is_empty() {
                "groups=none".to_string()
            } else {
                groups.iter().map(group_summary).collect::<Vec<_>>().join(" ")
            }
        )];
        lines.push(format!(
            "memory percent={} source={} warned={} notice={} target={}",
            self.memory
                .gauge
                .as_ref()
                .and_then(|gauge| gauge.percent)
                .map_or("-".to_string(), |percent| percent.to_string()),
            match self.memory.gauge.as_ref().map(|gauge| gauge.source) {
                Some(paddock_core::memory_guard::Source::Wsl) => "wsl",
                Some(paddock_core::memory_guard::Source::System) => "system",
                None => "-",
            },
            if self.memory.warned() { "yes" } else { "no" },
            self.memory.notice.as_ref().map_or("-", |notice| notice.message.as_str()),
            self.memory
                .notice
                .as_ref()
                .and_then(|notice| notice.target.as_ref())
                .map_or("-", |(_, label)| label.as_str()),
        ));
        if let Some(details) = self.memory.gauge.as_ref().and_then(|gauge| gauge.details.as_ref()) {
            lines.push(format!("memory-tip {}", details.summary));
        }
        lines.extend(crate::usage::setup::summary_lines(self.wsl_launcher_state.as_deref()));
        let selected = self.workspace.selected();
        let conversation = selected
            .and_then(|id| self.workspace.tab(id))
            .and_then(|tab| tab.content.session.grid.shared.state.lock().ok().and_then(|state| state.clone()))
            .and_then(|state| state.conversation);
        let start_mark = if conversation.as_ref().is_some_and(|conversation| conversation.start_mark.is_some()) {
            "yes"
        } else {
            "no"
        };
        let conversation = conversation.map_or("-".to_string(), |conversation| conversation.session_id);
        lines.push(format!(
            "account-switch switching={} conversation={conversation} start-mark={start_mark}",
            if selected.is_some_and(|id| self.switching.contains(&id)) { "yes" } else { "no" }
        ));
        lines.extend(self.account_menu_summary());
        let loaded = Settings::load();
        let settings = loaded.values();
        lines.push(format!(
            "usage-setup-settings memory-notice={} limit={} wsl-launcher={} focus={} notice={}",
            on(settings.memory_notice()),
            settings.memory_notice_minimum_mb(),
            settings.wsl_claude_launcher().as_str(),
            if self.overlay_focus.is_focused(window) { "overlay" } else { "other" },
            self.status.as_deref().unwrap_or("-")
        ));
        for group in &groups {
            match &group.value {
                ValueView::Meters { meters, .. } => {
                    lines.extend(
                        meters
                            .iter()
                            .map(|meter| format!("usage-tip {} {}: {}", group.provider, meter.label, meter.title)),
                    );
                }
                ValueView::State { text, title } => {
                    lines.push(format!("usage-tip {} ({text}): {title}", group.provider))
                }
            }
        }
        if let Some(popup) = self.usage_popup.as_ref() {
            let sections = self.usage_popup_sections(cx);
            let query = self.usage_popup_query(cx);
            let rows: Vec<&paddock_core::usage_scopes::PopupRow> =
                sections.iter().flat_map(|section| section.rows.iter()).collect();
            let state_of = |row: &paddock_core::usage_scopes::PopupRow| match &row.value {
                ValueView::Meters { .. } => "ready",
                ValueView::State { text, .. } => match *text {
                    "Loading\u{2026}" => "loading",
                    "Unavailable" => "unavailable",
                    "Hidden in Quick settings" => "hidden",
                    _ => "empty",
                },
            };
            lines.push(format!(
                "usage-popup open=yes search={} search-shown={} retry={} opening={} error={} rows={}",
                if query.is_empty() { "-" } else { query.as_str() },
                if search_visible(self.usage.book.profiles().len(), &query) { "yes" } else { "no" },
                if self.usage.book.retry_visible(&|provider| items.provider(provider)) { "yes" } else { "no" },
                popup.opening.as_deref().unwrap_or("-"),
                if popup.error.is_empty() { "-" } else { popup.error.as_str() },
                if rows.is_empty() {
                    "-".to_string()
                } else {
                    rows.iter()
                        .map(|row| format!("{}:{}:{}", row.scope.provider, row.name.replace(' ', "_"), state_of(row)))
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            ));
            for (index, row) in rows.iter().enumerate() {
                let values = match &row.value {
                    ValueView::Meters { meters, .. } => meters
                        .iter()
                        .map(|meter| format!("{}={}", meter.label, meter.used))
                        .collect::<Vec<_>>()
                        .join(","),
                    ValueView::State { text, .. } => format!("({text})"),
                };
                lines.push(format!(
                    "usage-row {} {} name={} in-bar={} default={} env={} open={} values={}",
                    index + 1,
                    row.scope.provider,
                    row.name.replace(' ', "_"),
                    if row.in_bar { "yes" } else { "no" },
                    if row.is_default_cli { "yes" } else { "no" },
                    row.environment.replace(' ', "_"),
                    if row.can_open { "yes" } else { "no" },
                    values
                ));
            }
        } else {
            lines.push("usage-popup open=no".to_string());
        }
        lines
    }

    /// 데몬 연결 상태와 터미널 없이 보이는 안내 이름(대본이 읽는다). 연결 중이거나 연결하지 못했으면 데몬은 `-`다.
    #[cfg(feature = "automation")]
    fn daemon_summary(&self) -> (&'static str, &'static str) {
        match &self.phase {
            Phase::Ready => (if self.daemon_outdated { "outdated" } else { "same" }, "none"),
            Phase::Connecting(_) => ("-", "none"),
            Phase::Notice(Notice::ProtocolMismatch { daemon_version: Some(_) }) => ("-", "protocol-mismatch"),
            Phase::Notice(Notice::ProtocolMismatch { daemon_version: None }) => ("-", "protocol-mismatch-v1"),
            Phase::Notice(Notice::DaemonStalled) => ("-", "stalled"),
            Phase::Notice(Notice::Failed(_)) => ("-", "failed"),
        }
    }

    /// 지금 적용된 화면 모양과 빠른 설정 패널 상태(대본이 읽는다). 배경색은 지금 색 묶음의 본문 바탕이다.
    #[cfg(feature = "automation")]
    fn look_summary(&self) -> String {
        format!(
            "theme={} zoom={} font={}px terminal_family={} interface_family={} app_bg=#{:06x} quick={} expanded={}",
            crate::look::mode().theme_name(),
            crate::look::zoom(),
            crate::look::terminal_font_size(),
            crate::look::terminal_family().replace(' ', "_"),
            crate::look::interface_family().replace(' ', "_"),
            colors().app_bg,
            if self.quick.open { "open" } else { "closed" },
            self.quick.expanded.map_or("-".to_string(), |field| format!("{field:?}"))
        )
    }

    /// 사이드바 행 요약(대본이 읽는다): 사이드바가 그리는 것과 같은 행 목록에서 만든다.
    #[cfg(feature = "automation")]
    fn sidebar_rows(&self) -> Vec<String> {
        use paddock_core::work_model::Row;

        let work = self.workspace.work();
        let mut rows = Vec::new();
        if work.folders.is_empty() {
            rows.push("empty-guide".to_string());
        }
        for row in work.visible_rows(self.workspace.selected(), &self.show_all) {
            rows.push(match row {
                Row::Repo { key, name, expanded, n_folders, current } => {
                    let tip = crate::work::list_view::repo_tip(self, &key).replace(' ', "_");
                    format!("repo key={key} name={name} expanded={expanded} n={n_folders} current={current} tip={tip}")
                }
                Row::Folder { key, expanded, n_terminals, current, in_repo } => {
                    let name = folder_name(&key, work.folder_label(&key), None);
                    let tip = crate::work::list_view::folder_tip(self, &key).replace(' ', "_");
                    format!(
                        "folder key={key} name={name} expanded={expanded} n={n_terminals} current={current} in_repo={in_repo} tip={tip}"
                    )
                }
                Row::Terminal(terminal) => {
                    format!(
                        "terminal id={} name={}{}{} status={}",
                        terminal.id,
                        terminal.name,
                        terminal.suffix,
                        if self.workspace.is_detached(terminal.id) { " meta=other_window" } else { "" },
                        self.status_label(terminal.id)
                    )
                }
                Row::More { folder, n_hidden } => format!("more folder={folder} n={n_hidden}"),
            });
        }
        for group in &work.groups {
            let name = work.name_of(group.root).unwrap_or("");
            let tip = crate::work::list_view::group_tip(self, group).replace(' ', "_");
            rows.push(format!(
                "group root={} name={name} n={} last_used={}{} status={} tip={tip}",
                group.root,
                group.members.len(),
                group.last_used,
                if self.workspace.is_detached(group.last_used) { " meta=other_window" } else { "" },
                self.status_label(group.root)
            ));
        }
        rows
    }

    /// 행에 보이는 에이전트 상태 글자(띄어쓰기는 `_`). 그리지 않으면 `-`다(대본이 읽는다).
    #[cfg(feature = "automation")]
    fn status_label(
        &self,
        id: u32,
    ) -> String {
        let state = crate::work::list_view::terminal_state(self, id);
        crate::agent_view::status_of(state.as_ref()).map_or("-".to_string(), |status| status.label.replace(' ', "_"))
    }

    /// 파일 목록 요약(대본이 읽는다): 목록이 그리는 것과 같은 행에서 만든다.
    #[cfg(feature = "automation")]
    fn files_summary(
        &self,
        detailed: bool,
        cx: &gpui_kit::App,
    ) -> Vec<String> {
        let watch = match (self.files.is_loaded(), self.files.is_polled()) {
            (false, _) => "-",
            (true, true) => "polling",
            (true, false) => "events",
        };
        let mut lines = vec![
            format!("files-folder={}", self.files.folder().unwrap_or("-")),
            format!("files-expanded={}", self.files.files_expanded),
            format!("files-loaded={}", self.files.is_loaded()),
            format!("files-watch={watch}"),
            format!("files-read-ms={}", self.files.last_read_ms()),
            format!("files-reads={}", self.files.n_listings()),
            format!("files-read-off-window={}", self.files.read_off_window()),
        ];
        let shown_rows = if self.files.files_expanded { self.files.rows() } else { &[] };
        for row in shown_rows {
            lines.push(match row.notice {
                Some(notice) => format!("file-notice {} {notice}", row.depth),
                None => format!(
                    "file-row {} {} {}{}{}",
                    row.depth,
                    if row.is_dir { "dir" } else { "file" },
                    row.name,
                    if row.expanded { " expanded" } else { "" },
                    self.mark_summary(&row.path, row.is_dir)
                ),
            });
        }
        lines.extend(self.file_tab_lines(detailed, cx));
        lines.extend(self.search_lines());
        lines.extend(self.terminal_link_lines());
        lines.extend(self.scm_lines());
        lines.push(format!("ui-max-gap-ms={}", self.tick_gap.1.as_millis()));
        lines.push(format!("ui-max-gap-cpu-ms={}", self.tick_cpu.1.as_millis()));
        lines.push(format!("ui-max-busy-ms={}", self.tick_cpu.2.as_millis()));
        lines
    }

    /// 폴더 검색 요약(대본이 읽는다): `search open=y|n scope= query= n_results= n_files= capped=y|n state=idle|running|done|error runs= searched= options=`,
    /// 오류가 있으면 `search-error <글>`, 일치 줄마다 `search-row <검색 폴더 기준 파일 경로> <줄>`(앞의 100개).
    #[cfg(feature = "automation")]
    fn search_lines(&self) -> Vec<String> {
        const ROWS_SHOWN: usize = 100;
        let state = &self.search;
        let results = state.results();
        let yes_no = |on: bool| if on { "y" } else { "n" };
        let status = match (state.failure().is_some(), state.is_running(), state.is_finished()) {
            (true, _, _) => "error",
            (_, true, _) => "running",
            (_, _, true) => "done",
            _ => "idle",
        };
        let options: Vec<&str> = [
            (state.options.case_sensitive, "case"),
            (state.options.whole_word, "word"),
            (state.options.regex, "regex"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name)
        .collect();
        let mut lines = vec![format!(
            "search open={} scope={} query={} n_results={} n_files={} capped={} state={status} runs={} searched={} options={}",
            yes_no(state.open),
            state.scope().unwrap_or("-"),
            state.query(),
            results.n_matches(),
            results.files().len(),
            yes_no(results.capped()),
            state.n_started(),
            state.n_files_searched(),
            if options.is_empty() { "-".to_string() } else { options.join(",") }
        )];
        lines.extend(state.failure().map(|message| format!("search-error {message}")));
        if state.open && state.scope().is_none() {
            lines.push(format!("search-notice {}", paddock_core::search_model::NO_FOLDER_NOTICE));
        }
        for file in results.files() {
            for found in &file.matches {
                if lines.len() <= ROWS_SHOWN {
                    lines.push(format!("search-row {} {}", file.relative, found.line));
                }
            }
        }
        lines
    }

    /// 선택한 터미널 화면의 경로 링크 목록을 터미널 보기의 확인 기억에서 새로 채운다(파일 시스템은 읽지 않는다).
    #[cfg(feature = "automation")]
    fn refresh_term_links(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let selected =
            self.workspace.selected().and_then(|id| self.workspace.tab(id).map(|tab| (id, tab.content.view.clone())));
        self.term_links = match selected {
            Some((id, view)) => {
                view.update(cx, |view, cx| view.checked_targets(cx)).into_iter().map(|target| (id, target)).collect()
            }
            None => Vec::new(),
        };
    }

    /// 선택한 터미널 화면에서 실제 파일을 가리키는 경로 링크 요약(대본이 읽는다): `term-link <터미널 번호> <경로> <줄|->`, 화면 위에서 아래 순서.
    #[cfg(feature = "automation")]
    fn terminal_link_lines(&self) -> Vec<String> {
        self.term_links
            .iter()
            .map(|(id, target)| {
                format!(
                    "term-link {id} {} {}",
                    target.path,
                    target.line.map_or("-".to_string(), |line| line.to_string())
                )
            })
            .collect()
    }

    /// 파일 행의 Git 표시 요약: ` mark=M color=modified`(파일) 또는 ` mark=- color=modified`(폴더). 표시가 없으면 빈 글자다.
    #[cfg(feature = "automation")]
    fn mark_summary(
        &self,
        path: &str,
        is_dir: bool,
    ) -> String {
        let color_name = |color: paddock_core::file_marks::MarkColor| format!("{color:?}").to_lowercase();
        if is_dir {
            self.git.marks().folder(path).map_or(String::new(), |color| format!(" mark=- color={}", color_name(color)))
        } else {
            self.git
                .marks()
                .file(path)
                .map_or(String::new(), |mark| format!(" mark={} color={}", mark.letter.glyph(), color_name(mark.color)))
        }
    }

    /// Source control 요약(대본이 읽는다): 사이드바 보기, 저장소 읽기 상태·개수, 변경 행, 입력칸 아래 한 줄, git 실행 횟수.
    ///
    /// `view=work|scm`, `scm state=ready|reading|none|unavailable repo= branch= merge= staged= changes= untracked= runs= generation=`,
    /// `scm-row <구획> <글자> <경로>`, `scm-note <글>`, `git-calls local= wsl=`(앱이 띄운 git·`wsl.exe` 프로세스 수).
    #[cfg(feature = "automation")]
    fn scm_lines(&self) -> Vec<String> {
        use paddock_core::git_status::{Section, mark_of};

        let mut lines = vec![format!("view={}", self.scm.view.name())];
        let (state, repo, branch, counts) = match self.git.repo() {
            Repo::Ready(repo) => (
                "ready",
                repo.root.clone(),
                repo.status.head_label().unwrap_or_else(|| "-".to_string()),
                [Section::Merge, Section::Staged, Section::Changes, Section::Untracked].map(|s| repo.status.n_in(s)),
            ),
            Repo::Reading => ("reading", "-".to_string(), "-".to_string(), [0; 4]),
            Repo::NoRepository => ("none", "-".to_string(), "-".to_string(), [0; 4]),
            Repo::Unavailable(_) => ("unavailable", "-".to_string(), "-".to_string(), [0; 4]),
        };
        lines.push(format!(
            "scm state={state} repo={repo} branch={branch} merge={} staged={} changes={} untracked={} runs={} generation={} badge={}",
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            self.git.n_status_runs(),
            self.git.generation(),
            crate::scm::model::badge_text(self.git.n_changes()).unwrap_or_else(|| "-".to_string())
        ));
        if let Overlay::Discard(change) = &self.overlay {
            let prompt = crate::scm::model::discard_prompt(change);
            lines.push(format!("discard-prompt {} | {} | {}", prompt.headline, prompt.detail, prompt.confirm));
        }
        if let Some(repo) = self.git.ready() {
            for group in crate::scm::model::GROUPS {
                for change in crate::scm::model::rows_of(&repo.status, group) {
                    lines.push(format!("scm-row {} {} {}", group.name(), mark_of(change).glyph(), change.path));
                }
            }
        }
        if let Some(note) = &self.scm.note {
            lines.push(format!("scm-note {note}"));
        }
        let (local, wsl) = crate::git::runner::n_runs();
        lines.push(format!("git-calls local={local} wsl={wsl}"));
        lines
    }

    /// 파일 탭 요약(대본이 읽는다): 지금 보는 묶음의 칸 상태, 모든 묶음의 파일 탭, 선택한 탭의 내용 요약.
    ///
    /// `file-pane ratio= tabs= active=`, `file-tab <묶음> <종류> <경로> [selected] [shown]`,
    /// `file-view kind= state= … external-button=yes|no`. 묶음은 `folder:<key>` 또는 `group:<시작 터미널>`이다.
    #[cfg(feature = "automation")]
    fn file_tab_lines(
        &self,
        detailed: bool,
        cx: &gpui_kit::App,
    ) -> Vec<String> {
        let label = |key: &GroupKey| match key {
            GroupKey::Folder(folder) => format!("folder:{folder}"),
            GroupKey::Group(root) => format!("group:{root}"),
        };
        let shown = self.workspace.view_key();
        let mut lines = Vec::new();
        if let Some(pane) = self.workspace.file_pane() {
            lines.push(format!(
                "file-pane ratio={:.2} tabs={} active={}",
                pane.ratio,
                pane.tabs.len(),
                if self.file_active() { "yes" } else { "no" }
            ));
        }
        for (key, pane) in self.workspace.files().iter() {
            for (index, tab) in pane.tabs.iter().enumerate() {
                lines.push(format!(
                    "file-tab {} {} {}{}{} place=main",
                    label(key),
                    tab.kind.name(),
                    tab.path,
                    if index == pane.selected { " selected" } else { "" },
                    if Some(key) == shown.as_ref() { " shown" } else { "" }
                ));
            }
        }
        // 다른 창(파일 분리 창)에 있는 파일 탭: 칸에는 없고, 창 이름·크기·활성·저장하지 않은 표시·내용 요약이 따로 보인다.
        for detached in self.workspace.detached_files() {
            let tab = &detached.tab;
            lines.push(format!("file-tab {} {} {} place=detached", label(&detached.group), tab.kind.name(), tab.path));
            let name = crate::detached_file::file_window_name(&tab.key());
            let window_state = self.detached_file_windows.get(&tab.key()).map_or_else(
                || "window=none".to_string(),
                |handle| {
                    format!(
                        "title={} size={}x{} active={} activations={}",
                        handle.shared.title.borrow(),
                        handle.shared.size.get().0.round(),
                        handle.shared.size.get().1.round(),
                        if handle.shared.active.get() { "yes" } else { "no" },
                        handle.shared.n_activation_requests.get()
                    )
                },
            );
            lines.push(format!(
                "dfile {name} unsaved={} {window_state}",
                if self.unsaved.contains(&tab.key()) { "yes" } else { "no" }
            ));
            let summary = self.file_views.get(&tab.key()).map_or_else(
                || format!("kind={} state=none", tab.kind.name()),
                |content| content.summary(tab.kind, detailed, cx),
            );
            lines.push(format!("dfile-view {name} {summary}{}", self.editing_view_suffix(&tab.path, cx)));
        }
        if let Some(tab) = self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)) {
            let summary = self.file_views.get(&tab.key()).map_or_else(
                || format!("kind={} state=none", tab.kind.name()),
                |content| content.summary(tab.kind, detailed, cx),
            );
            let untracked = if self.git.is_untracked(&tab.path) { " untracked=yes" } else { "" };
            lines.push(format!(
                "file-view {summary}{untracked}{} external-button={}",
                self.editing_view_suffix(&tab.path, cx),
                if self.file_active() && tab.kind.opens_in_text_editor() { "yes" } else { "no" }
            ));
        }
        lines
    }

    /// Accounts 창이 보이는 내용 요약(대본이 읽는다). 경로나 로그인 값은 싣지 않는다.
    #[cfg(feature = "automation")]
    fn accounts_rows(&self) -> Vec<String> {
        self.accounts.model.as_ref().map_or_else(Vec::new, |model| {
            let mut rows = vec![format!(
                "dialog form={} load_failed={} editing={:?} provider={} error={}",
                model.form_visible,
                model.load_failed,
                model.editing,
                model.provider,
                model.error.as_deref().unwrap_or("-")
            )];
            rows.extend(model.profiles.iter().map(|profile| {
                format!(
                    "profile id={} provider={} label={} runtime={}",
                    profile.id,
                    profile.provider,
                    profile.label,
                    crate::accounts::model::runtime_text(profile)
                )
            }));
            rows
        })
    }

    /// 대본의 N번째(1부터) 등록 계정의 식별자.
    #[cfg(feature = "automation")]
    fn automation_account_id(
        &self,
        n: Option<usize>,
    ) -> Option<String> {
        let model = self.accounts.model.as_ref()?;
        n.and_then(|n| n.checked_sub(1)).and_then(|index| model.profiles.get(index)).map(|profile| profile.id.clone())
    }

    /// 대본이 부르는 빠른 설정 선택상자 이름.
    #[cfg(feature = "automation")]
    fn automation_quick_field(name: &str) -> Option<Field> {
        match name {
            "theme" => Some(Field::Theme),
            "terminalfont" => Some(Field::TerminalFont),
            "interfacefont" => Some(Field::InterfaceFont),
            "shell" => Some(Field::DefaultShell),
            "onquit" => Some(Field::OnQuit),
            "restore" => Some(Field::RestoreOnRestart),
            "permissions" => Some(Field::AgentPermissions),
            "wsllauncher" => Some(Field::WslClaudeLauncher),
            _ => None,
        }
    }

    /// 대본의 `act` 한 줄을 사람이 누른 것과 같은 창 동작으로 실행한다.
    #[cfg(feature = "automation")]
    fn automation_action(
        &mut self,
        line: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let number = |index: usize| parts.get(index).and_then(|part| part.parse::<usize>().ok());
        let position = gpui_kit::point(
            gpui_kit::px(crate::automation::menu_anchor::X),
            gpui_kit::px(crate::automation::menu_anchor::Y),
        );
        // N번째 폴더는 화면에 보이는 순서(저장소 묶음 아래로 모인 순서)의 N번째 폴더 행이다.
        let folder_key = |n: Option<usize>| {
            n.and_then(|n| n.checked_sub(1)).and_then(|index| {
                self.workspace
                    .work()
                    .visible_rows(self.workspace.selected(), &self.show_all)
                    .into_iter()
                    .filter_map(|row| {
                        if let paddock_core::work_model::Row::Folder { key, .. } = row { Some(key) } else { None }
                    })
                    .nth(index)
            })
        };
        let group_root = |n: Option<usize>| {
            n.and_then(|n| n.checked_sub(1))
                .and_then(|index| self.workspace.work().groups.get(index))
                .map(|group| group.root)
        };
        match parts.as_slice() {
            ["workadd"] => self.new_terminal_in_new_group(None, window, cx),
            ["envmenu"] => self.open_menu(MenuKind::Environments, position, window, cx),
            ["tabadd"] => self.new_terminal_here(window, cx),
            ["plusmenu"] => {
                let place = self.workspace.new_place();
                self.open_new_terminal_menu(place, position, false, window, cx);
            }
            ["pickermenu"] => self.open_account_picker(position, window, cx),
            ["continuewith", name @ ..] | ["starthere", name @ ..] => {
                let name = name.join(" ");
                let id = self
                    .registered_accounts()
                    .into_iter()
                    .find(|profile| profile.label.eq_ignore_ascii_case(&name))
                    .map(|profile| profile.id);
                let terminal = self.workspace.selected();
                if let (Some(id), Some(terminal)) = (id, terminal) {
                    if parts.first() == Some(&"continuewith") {
                        self.switch_terminal_account(terminal, &id, window, cx);
                    } else {
                        self.start_account_here(terminal, &id, window, cx);
                    }
                }
            }
            // 선택한 탭이 아니라 번호로 고른 터미널에서 계정으로 시작한다(다른 창에 있는 터미널의 계정 시작).
            ["startwin", _, name @ ..] => {
                let name = name.join(" ");
                let id = self
                    .registered_accounts()
                    .into_iter()
                    .find(|profile| profile.label.eq_ignore_ascii_case(&name))
                    .map(|profile| profile.id);
                if let (Some(id), Some(terminal)) = (id, number(1)) {
                    self.start_account_here(terminal as u32, &id, window, cx);
                }
            }
            ["accounts"] => self.open_accounts(window, cx),
            ["acct", "provider", name] => {
                let provider = crate::accounts::model::PROVIDERS.iter().find(|provider| *provider == name).copied();
                if let Some(provider) = provider {
                    self.accounts_set_provider(provider, window, cx);
                }
            }
            ["acct", "runtime", _] => {
                if let Some(index) = number(2) {
                    self.accounts_set_runtime(index, window, cx);
                }
            }
            ["acct", "name", ..] => {
                let text = parts[2..].join(" ");
                if let Some(input) = self.accounts.name_input.clone() {
                    input.update(cx, |state, cx| state.set_value(text, window, cx));
                }
            }
            ["acct", "add"] => self.accounts_add(window, cx),
            ["acct", "showform"] => self.accounts_show_form(window, cx),
            ["acct", "rename", _, ..] => {
                if let Some(id) = self.automation_account_id(number(2)) {
                    self.accounts_begin_rename(&id, window, cx);
                    let text = parts[3..].join(" ");
                    if !text.is_empty()
                        && let Some(input) = self.accounts.rename_input.clone()
                    {
                        input.update(cx, |state, cx| state.set_value(text, window, cx));
                    }
                    self.accounts_save_rename(&id, window, cx);
                }
            }
            ["acct", "remove", _] => {
                if let Some(id) = self.automation_account_id(number(2)) {
                    self.accounts_begin_remove(&id, window, cx);
                    self.accounts_confirm_remove(&id, window, cx);
                }
            }
            ["acct", "open", _] => {
                if let Some(id) = self.automation_account_id(number(2)) {
                    self.accounts_open(&id, window, cx);
                }
            }
            ["acct", "close"] => self.cancel_overlay(window, cx),
            ["menu", "terminal", _] => {
                let id = number(2)
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.workspace.tabs().get(index))
                    .map(|tab| tab.id);
                if let Some(id) = id {
                    self.open_menu(MenuKind::Terminal(id), position, window, cx);
                }
            }
            ["menu", "filetab", _] => {
                if let Some(index) = number(2).and_then(|n| n.checked_sub(1)) {
                    self.open_menu(MenuKind::FileTab(index), position, window, cx);
                }
            }
            ["menu", "group", _] => {
                if let Some(root) = group_root(number(2)) {
                    self.open_menu(MenuKind::Group(root), position, window, cx);
                }
            }
            ["menu", "folder", _] => {
                if let Some(key) = folder_key(number(2)) {
                    self.open_menu(MenuKind::Folder(key), position, window, cx);
                }
            }
            ["menuitem", ..] => {
                let wanted = parts[1..].join(" ");
                let action = self.menu.as_ref().and_then(|menu| {
                    self.menu_entries(&menu.kind).into_iter().find(|item| item.label == wanted).map(|item| item.action)
                });
                if let Some(action) = action {
                    self.run_menu_action(action, window, cx);
                }
            }
            ["repotoggle", _] => {
                let repo = number(1).and_then(|n| n.checked_sub(1)).and_then(|index| {
                    self.workspace
                        .work()
                        .visible_rows(self.workspace.selected(), &self.show_all)
                        .into_iter()
                        .filter_map(|row| {
                            if let paddock_core::work_model::Row::Repo { key, .. } = row { Some(key) } else { None }
                        })
                        .nth(index)
                });
                if let Some(key) = repo {
                    self.toggle_repo(&key, window, cx);
                }
            }
            ["foldertoggle", _] => {
                if let Some(key) = folder_key(number(1)) {
                    self.toggle_folder(&key, window, cx);
                }
            }
            ["folderadd", _] => {
                if let Some(key) = folder_key(number(1)) {
                    self.new_terminal_in_folder(key, window, cx);
                }
            }
            ["showmore", _] => {
                if let Some(key) = folder_key(number(1)) {
                    self.show_more(&key, window, cx);
                }
            }
            ["selectgroup", _] => {
                if let Some(root) = group_root(number(1)) {
                    self.select_row(RowTarget::Group(root), window, cx);
                }
            }
            ["closegroup", _] => {
                if let Some(root) = group_root(number(1)) {
                    self.request_close_group(root, window, cx);
                }
            }
            ["selecttab", _] => {
                let id = number(1)
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.workspace.tabs().get(index))
                    .map(|tab| tab.id);
                if let Some(id) = id {
                    self.select_row(RowTarget::Terminal(id), window, cx);
                }
            }
            ["files", _] => {
                let row = number(1).and_then(|n| n.checked_sub(1)).and_then(|index| self.files.rows().get(index));
                if let Some(path) = row.filter(|row| row.is_dir && row.notice.is_none()).map(|row| row.path.clone()) {
                    self.press_file_row(&path, true, window, cx);
                }
            }
            ["filesmenu", _, item @ ..] => {
                let row = number(1).and_then(|n| n.checked_sub(1)).and_then(|index| self.files.rows().get(index));
                if let Some((path, is_dir)) =
                    row.filter(|row| row.notice.is_none()).map(|row| (row.path.clone(), row.is_dir))
                {
                    self.open_entry_menu(&path, is_dir, position, window, cx);
                    let wanted = item.join(" ");
                    let action = self.menu.as_ref().and_then(|menu| {
                        self.menu_entries(&menu.kind)
                            .into_iter()
                            .find(|candidate| candidate.label == wanted)
                            .map(|candidate| candidate.action)
                    });
                    if let Some(action) = action {
                        self.run_menu_action(action, window, cx);
                    }
                }
            }
            ["newfile", name @ ..] | ["newfolder", name @ ..] => {
                self.begin_new_entry(None, parts[0] == "newfolder", window, cx);
                if let Some(input) = self.entry_input.clone() {
                    input.update(cx, |state, cx| state.set_value(name.join(" "), window, cx));
                    self.commit_new_entry(window, cx);
                }
            }
            ["openfile", _] => {
                let row = number(1).and_then(|n| n.checked_sub(1)).and_then(|index| self.files.rows().get(index));
                if let Some(path) = row.filter(|row| !row.is_dir && row.notice.is_none()).map(|row| row.path.clone()) {
                    self.press_file_row(&path, false, window, cx);
                }
            }
            // 열린 파일 내용(편집기)을 눌러 포커스를 주는 것과 같다. 행을 눌러 연 직후에는 목록이 키를 쥐고 있다.
            ["focusfile"] => self.focus_selected(window, cx),
            ["openpath", path @ ..] => self.open_file(&path.join(" "), window, cx),
            ["closefile"] => {
                if let Some(index) = self.workspace.file_pane().map(|pane| pane.selected) {
                    self.close_file_tab(index, window, cx);
                }
            }
            ["filetab", _] => {
                if let Some(index) = number(1).and_then(|n| n.checked_sub(1)) {
                    self.select_file_tab(index, window, cx);
                }
            }
            ["notice", which] => {
                let action = match *which {
                    "reveal" => Some(NoticeAction::ShowInFileManager),
                    "astext" => Some(NoticeAction::OpenAsText),
                    "default" => Some(NoticeAction::OpenWithDefaultApp),
                    _ => None,
                };
                if let (Some(action), Some(path)) = (action, self.selected_file_path()) {
                    self.run_notice_action(action, &path, window, cx);
                }
            }
            ["mdview", name] => {
                if let Some(view) = View::from_name(name) {
                    self.set_markdown_view(view, window, cx);
                }
            }
            ["textscroll", y] => {
                if let (Some(content), Ok(y)) =
                    (self.selected_file_key().and_then(|key| self.file_views.get(&key)), y.parse::<f32>())
                {
                    let editor = match content {
                        FileContent::Text(body) => Some(body.editor.clone()),
                        FileContent::Markdown(body) => body.source.as_ref().map(|source| source.editor.clone()),
                        _ => None,
                    };
                    if let Some(editor) = editor {
                        editor.update(cx, |state, cx| {
                            state.set_scroll_offset(gpui_kit::point(Default::default(), gpui_kit::Pixels::from(-y)), cx)
                        });
                    }
                }
            }
            ["imgzoom"] => {
                if let Some(FileContent::Image(view)) =
                    self.selected_file_key().and_then(|key| self.file_views.get(&key))
                {
                    view.update(cx, |view, cx| view.toggle_zoom(cx));
                }
            }
            ["imgscroll", x, y] => {
                if let (Some(FileContent::Image(view)), Ok(x), Ok(y)) = (
                    self.selected_file_key().and_then(|key| self.file_views.get(&key)),
                    x.parse::<f32>(),
                    y.parse::<f32>(),
                ) {
                    view.update(cx, |view, cx| view.scroll_to(x, y, cx));
                }
            }
            ["pdfzoom", which] => {
                let step = match *which {
                    "in" => Some(Some(true)),
                    "out" => Some(Some(false)),
                    "fit" => Some(None),
                    _ => None,
                };
                if let Some(step) = step {
                    self.zoom_pdf(step, window, cx);
                }
            }
            ["pdfscroll", page] => {
                if let (Some(FileContent::Pdf(view)), Some(index)) = (
                    self.selected_file_key().and_then(|key| self.file_views.get(&key)),
                    page.parse::<usize>().ok().and_then(|n| n.checked_sub(1)),
                ) {
                    view.update(cx, |view, cx| view.scroll_to_page(index, cx));
                }
            }
            ["setfileratio", ratio] => {
                if let (Some(key), Ok(ratio)) = (self.workspace.view_key(), ratio.parse::<f32>()) {
                    self.workspace.set_file_ratio(&key, ratio);
                    self.persist();
                    cx.notify();
                }
            }
            ["resetgap"] => {
                self.tick_gap.1 = Duration::ZERO;
                self.tick_cpu.1 = Duration::ZERO;
                self.tick_cpu.2 = Duration::ZERO;
            }
            ["view", which] => {
                let view = if *which == "scm" { SidebarView::SourceControl } else { SidebarView::Work };
                self.show_sidebar_view(view, window, cx);
            }
            ["gitrefresh"] => self.scm_refresh(window, cx),
            ["stageall"] => self.scm_stage_all(window, cx),
            ["unstageall"] => self.scm_unstage_all(window, cx),
            ["scmtoggle", which] => {
                let group = crate::scm::model::GROUPS.into_iter().find(|group| group.name() == *which);
                if let Some(group) = group {
                    self.scm_toggle_group(group, window, cx);
                }
            }
            ["stage", _] | ["unstage", _] | ["discard", _] | ["scmrow", _] => {
                // N은 화면에 그려진 변경 행(구획 머리 제외)의 번호다. stage는 작업 쪽(병합·변경), unstage는 스테이징된 행, discard는 변경 행만 센다.
                let n = number(1).and_then(|n| n.checked_sub(1));
                let rows: Vec<(crate::scm::model::Group, Change)> = self
                    .git
                    .ready()
                    .map(|repo| {
                        crate::scm::model::GROUPS
                            .into_iter()
                            .flat_map(|group| {
                                crate::scm::model::rows_of(&repo.status, group)
                                    .into_iter()
                                    .map(move |change| (group, change.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let pick = |wanted: &dyn Fn(crate::scm::model::Group) -> bool| {
                    n.and_then(|n| rows.iter().filter(|(group, _)| wanted(*group)).nth(n))
                        .map(|(_, change)| change.clone())
                };
                match parts[0] {
                    "stage" => {
                        if let Some(change) = pick(&|group| group != crate::scm::model::Group::Staged) {
                            self.scm_stage(&change.path, window, cx);
                        }
                    }
                    "unstage" => {
                        if let Some(change) = pick(&|group| group == crate::scm::model::Group::Staged) {
                            self.scm_unstage(&change.path, window, cx);
                        }
                    }
                    "discard" => {
                        if let Some(change) = pick(&|group| group == crate::scm::model::Group::Changes) {
                            self.scm_ask_discard(change, window, cx);
                        }
                    }
                    _ => {
                        if let Some(change) = pick(&|_| true) {
                            self.scm_open_row(&change, window, cx);
                        }
                    }
                }
            }
            ["commit", message @ ..] => {
                if let Some(input) = self.scm.commit_input.clone() {
                    input.update(cx, |state, cx| state.set_value(message.join(" "), window, cx));
                }
                self.scm_commit(window, cx);
            }
            ["search", query @ ..] => {
                if !self.search.open {
                    self.open_search(window, cx);
                }
                if let Some(input) = self.search.input.clone() {
                    input.update(cx, |state, cx| state.set_value(query.join(" "), window, cx));
                    self.run_search(window, cx);
                }
            }
            ["searchrow", _] => {
                let target = number(1)
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.search.results().match_at(index))
                    .map(|(file, found)| (file.path.clone(), found.line));
                if let Some((path, line)) = target {
                    self.open_file_at(&path, usize::try_from(line).unwrap_or(1), window, cx);
                }
            }
            ["searchopt", name] => self.toggle_search_option(name, window, cx),
            ["searchfile", _] => {
                if let Some(file) = number(1).and_then(|n| n.checked_sub(1)) {
                    self.toggle_search_file(file, cx);
                }
            }
            ["termlink", _] => {
                let target = number(1)
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.term_links.get(index).map(|(_, target)| target.clone()));
                if let Some(target) = target {
                    self.open_terminal_link(&target, window, cx);
                }
            }
            ["filestoggle"] => self.toggle_files_section(window, cx),
            ["filesrefresh"] => self.refresh_files(window, cx),
            ["splitright"] => self.split_selected(Orientation::Horizontal, window, cx),
            ["splitdown"] => self.split_selected(Orientation::Vertical, window, cx),
            ["sidebar"] => self.toggle_sidebar(window, cx),
            ["quick", "open"] => {
                if !self.quick.open {
                    self.toggle_quick_settings(window, cx);
                }
            }
            ["quick", "close"] => self.close_quick_settings(window, cx),
            ["quick", "choose", field, value @ ..] => {
                if let Some(field) = Self::automation_quick_field(field) {
                    self.quick_choose(field, value.join(" "), window, cx);
                }
            }
            ["quick", "expand", field] => {
                if let Some(field) = Self::automation_quick_field(field) {
                    self.quick_expand(field, cx);
                }
            }
            ["quick", "step", which, delta] => {
                let row = match *which {
                    "zoom" => Row::Zoom,
                    "indent" => Row::SidebarIndent,
                    "memorynoticemb" => Row::MemoryNoticeMinimumMb,
                    _ => Row::FontSize,
                };
                if let Ok(delta) = delta.parse::<i32>() {
                    self.quick_step(row, delta, window, cx);
                }
            }
            ["quick", "sound"] => self.quick_toggle_sound(window, cx),
            ["quick", "memorynotice", state @ ("on" | "off")] => {
                self.quick_settings = Settings::load();
                self.apply_setting(key::MEMORY_NOTICE, Value::Bool(*state == "on"), cx);
            }
            ["quick", "memorynoticemb", delta] => {
                if let Ok(delta) = delta.parse::<i32>() {
                    self.quick_settings = Settings::load();
                    self.quick_step(Row::MemoryNoticeMinimumMb, delta, window, cx);
                }
            }
            // WSL 묻기 창의 답: `on`·`off`는 단추를 누른 것과 같고, `later`는 닫기(`Ask me later`)다.
            ["wsllauncher", answer @ ("on" | "off" | "later")] => {
                if self.overlay == Overlay::WslLauncher {
                    match *answer {
                        "later" => self.cancel_overlay(window, cx),
                        other => self.wsl_launcher_answer(other == "on", window, cx),
                    }
                }
            }
            ["quick", "statusbar", item, state] => {
                let item_key = match *item {
                    "claude" => Some(key::STATUS_BAR_CLAUDE),
                    "codex" => Some(key::STATUS_BAR_CODEX),
                    "memory" => Some(key::STATUS_BAR_MEMORY),
                    _ => None,
                };
                if let (Some(item_key), "on" | "off") = (item_key, *state) {
                    self.quick_settings = Settings::load();
                    self.apply_setting(item_key, Value::Bool(*state == "on"), cx);
                }
            }
            ["usageopen", _] => self.open_usage_popup(window, cx),
            ["usageclose"] => self.close_usage_popup(window, cx),
            ["usagesearch", words @ ..] => {
                if let Some(popup) = self.usage_popup.as_ref() {
                    let search = popup.search.clone();
                    search.update(cx, |state, cx| state.set_value(words.join(" "), window, cx));
                    cx.notify();
                }
            }
            ["usagerow", row, "open"] => {
                let target = row.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|index| {
                    self.usage_popup_sections(cx).into_iter().flat_map(|section| section.rows).nth(index)
                });
                if let Some(row) = target {
                    self.usage_popup_open_account(&row.scope, window, cx);
                }
            }
            ["usageretry"] => {
                self.usage_popup_retry();
                cx.notify();
            }
            ["usagemanage"] => self.usage_popup_manage(window, cx),
            ["memoryfake", values @ ..] => {
                // 가짜 메모리 값 주입: `<이 컴퓨터 전체 GiB> <쓸 수 있는 GiB> [<WSL 전체> <WSL 쓸 수 있는 몫> <swap 전체> [<탭 위치> <그 터미널 GiB>]]`.
                let gib = |text: &str| text.parse::<f64>().ok().map(|value| (value * 1024.0 * 1024.0 * 1024.0) as u64);
                let numbers: Vec<Option<u64>> = values.iter().map(|text| gib(text)).collect();
                if let (Some(Some(total)), Some(Some(free))) = (numbers.first(), numbers.get(1)) {
                    let wsl = match (numbers.get(2), numbers.get(3), numbers.get(4)) {
                        (Some(Some(total)), Some(Some(available)), swap) => {
                            let mut wsl = paddock_core::memory_guard::WslMemory {
                                total: *total,
                                available: *available,
                                swap_total: swap.copied().flatten().unwrap_or(0),
                                ..Default::default()
                            };
                            if let (Some(position), Some(Some(bytes))) =
                                (values.get(5).and_then(|text| text.parse::<usize>().ok()), numbers.get(6))
                                && let Some(tab) =
                                    position.checked_sub(1).and_then(|index| self.workspace.tabs().get(index))
                            {
                                wsl.terminals.insert(tab.id, *bytes);
                            }
                            Some(wsl)
                        }
                        _ => None,
                    };
                    self.memory.injected = Some(crate::usage::memory::Injected { system: Some((*total, *free)), wsl });
                    let workspace = &self.workspace;
                    self.memory.refresh(&|id| workspace.tab(id).map(|tab| workspace.name(tab.id).to_string()));
                    cx.notify();
                }
            }
            ["memorygoto"] => {
                if let Some((id, _)) = self.memory.notice.as_ref().and_then(|notice| notice.target.clone()) {
                    self.memory_go_to(id, window, cx);
                }
            }
            ["memoryclose"] => {
                self.memory.dismiss();
                cx.notify();
            }
            ["usagerefresh"] => {
                self.usage.refresh(false);
                cx.notify();
            }
            ["quick", "quitstop"] => self.quit_and_stop_all(window, cx),
            ["setsplit", path, sizes @ ..] => {
                let path: Vec<usize> = if *path == "-" {
                    Vec::new()
                } else {
                    path.split('.').filter_map(|part| part.parse().ok()).collect()
                };
                let sizes: Vec<f64> = sizes.iter().filter_map(|size| size.parse().ok()).collect();
                self.workspace.resize_split(&path, sizes);
                self.persist();
                cx.notify();
            }
            ["detachsize", _, _] => {
                if let (Some(width), Some(height)) = (number(1), number(2)) {
                    crate::automation::set_detach_size((width as u32, height as u32));
                }
            }
            ["movetowindow", "file", key] => {
                if let Some((group, index)) = self.workspace.find_file_tab(key) {
                    self.move_file_to_new_window(group, index, None, window, cx);
                }
            }
            ["movetowindow", _] => {
                if let Some(id) = number(1) {
                    self.move_to_new_window(id as u32, None, window, cx);
                }
            }
            ["dragfile", index] => {
                // 파일 탭을 창 밖에 놓은 것과 같은 길(끌기 상태 → 창 밖 놓기)로 새 창에 뗀다. `dragfile <번호> inside`는 칸 위에 놓은 것(아무 일 없어야 한다).
                if let Ok(index) = index.parse::<usize>() {
                    let viewport = window.viewport_size();
                    self.file_drag = Some(crate::tab_drag_model::FileDrag::new(index, (40.0, 40.0)));
                    self.finish_file_release((f32::from(viewport.width) + 40.0, 120.0), window, cx);
                }
            }
            ["dragfile", index, "inside"] => {
                if let Ok(index) = index.parse::<usize>() {
                    self.file_drag = Some(crate::tab_drag_model::FileDrag::new(index, (40.0, 40.0)));
                    self.finish_file_release((600.0, 400.0), window, cx);
                }
            }
            ["resizewin", "file", key, width, height] => {
                if let (Ok(width), Ok(height)) = (width.parse::<f32>(), height.parse::<f32>()) {
                    self.resize_detached_file(key, width, height, cx);
                }
            }
            ["moveback", "file", key] => self.move_file_back(key, window, cx),
            ["windowclose", "file", key] => self.post_close_to_detached_file(key, cx),
            ["detachedclose", "file", key] => self.request_close_detached_file(key, window, cx),
            ["detachedmenu", "file", key] => self.open_detached_file_menu(key, cx),
            ["detachedmenuitem", "file", key, label @ ..] => {
                self.run_detached_file_menu_item(key, &label.join(" "), window, cx)
            }
            ["activatewindow", "file", key] => self.activate_detached_file(key, cx),
            ["dragtab", _, ..] => self.automation_drag(false, &parts[1..], window, cx),
            ["dragback", _, ..] => self.automation_drag(true, &parts[1..], window, cx),
            ["moveback", _] => {
                if let Some(id) = number(1) {
                    self.move_back(id as u32, None, window, cx);
                }
            }
            ["activatewindow", _] => {
                if let Some(id) = number(1) {
                    self.activate_detached(id as u32, cx);
                }
            }
            ["windowclose", _] => {
                if let Some(id) = number(1) {
                    self.post_close_to_detached(id as u32, cx);
                }
            }
            ["detachedclose", _] => {
                if let Some(id) = number(1) {
                    self.request_close_from_detached(id as u32, window, cx);
                }
            }
            ["resizewin", _, _, _] => {
                if let (Some(id), Some(width), Some(height)) = (number(1), number(2), number(3)) {
                    self.resize_detached(id as u32, width as f32, height as f32, cx);
                }
            }
            ["detachedmenu", _] => {
                if let Some(id) = number(1) {
                    self.open_detached_menu(id as u32, cx);
                }
            }
            ["detachedmenuitem", _, ..] => {
                if let Some(id) = number(1) {
                    self.run_detached_menu_item(id as u32, &parts[2..].join(" "), window, cx);
                }
            }
            ["confirm"] => self.confirm_overlay(window, cx),
            // 종료 묻기의 "Stop all and quit" 단추와 같은 선택.
            ["quitstop"] => self.choose_quit(OnQuit::Stop, window, cx),
            ["cancel"] => self.cancel_overlay(window, cx),
            other => {
                if !self.sync_action(other, window, cx)
                    && !self.editing_action(other, window, cx)
                    && !self.quick_open_action(other, window, cx)
                    && !self.replace_action(other, window, cx)
                {
                    crate::automation::event(&format!("act: unknown action {other:?}"));
                }
            }
        }
    }

    fn render_notice(
        &self,
        notice: &Notice,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let (headline, detail) = match notice {
            Notice::ProtocolMismatch { daemon_version } => protocol_mismatch_text(*daemon_version),
            Notice::DaemonStalled => (
                "The terminal daemon is not responding".to_string(),
                "Restarting it ends the terminals it holds. Quit leaves it running untouched.".to_string(),
            ),
            Notice::Failed(message) => ("Could not open a terminal".to_string(), message.clone()),
        };
        let palette = colors();
        let mut buttons = div().flex().flex_row().gap(zpx(dialog_size::BUTTON_GAP));
        match notice {
            Notice::ProtocolMismatch { .. } => {
                buttons = buttons.child(button("Replace", ButtonKind::Primary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        this.connect(Connect::Open(Policy::Replace), window, cx)
                    }),
                ));
            }
            Notice::DaemonStalled => {
                buttons = buttons.child(button("Restart daemon", ButtonKind::Primary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        this.connect(Connect::RestartStalled, window, cx)
                    }),
                ));
            }
            Notice::Failed(_) => {
                buttons = buttons.child(button("Retry", ButtonKind::Primary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        this.connect(Connect::Open(Policy::ReplaceIfIdle), window, cx)
                    }),
                ));
            }
        }
        buttons = buttons.child(
            button("Quit", ButtonKind::Secondary)
                .on_mouse_down(MouseButton::Left, cx.listener(|_this, _event: &MouseDownEvent, _window, cx| cx.quit())),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(zpx(dialog_size::CONTENT_PADDING))
            .child(div().text_size(zpx(text::DIALOG_TITLE)).text_color(hsla(palette.warning)).child(headline))
            .child(div().child(detail))
            .child(buttons)
    }

    /// 사이드바·탭 줄·본문·상태 줄을 담은 창 본체.
    fn render_workspace(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let palette = colors();
        let collapsed = self.workspace.sidebar_collapsed();
        let viewport = window.viewport_size();
        // 탭 줄과 칸 위치는 본문이 실제로 차지한 크기를 쓴다. 처음 그릴 때는 창 크기에서 사이드바를 뺀 값으로 어림한다.
        let measured = self.body_size.get();
        let sidebar_width = if collapsed { 0.0 } else { crate::look::scaled(sidebar::WIDTH + sidebar::SPLIT_WIDTH) };
        let estimate_height = f32::from(viewport.height)
            - crate::look::scaled(paddock_core::theme::titlebar::HEIGHT + tab::BAR_HEIGHT + status_bar::HEIGHT);
        let viewport_now = (f32::from(viewport.width), f32::from(viewport.height));
        // 파일 칸이 있으면 본문 폭을 터미널 배치와 파일 칸이 나눠 쓴다.
        let file_pane_now = self.workspace.file_pane().cloned();
        let full_width = f32::from(viewport.width) - sidebar_width;
        let (terminal_width, file_width) = match &file_pane_now {
            Some(pane) => split_body_width(full_width, pane.ratio, crate::look::scaled(split::MIN_PANE_SIZE)),
            None => (full_width, 0.0),
        };
        self.file_body_width.set(full_width);
        let estimate = (terminal_width, estimate_height.max(0.0));
        let (width, height) =
            strip_body_size(measured, self.rendered_viewport.replace(viewport_now), viewport_now, estimate);
        let geometry = StripGeometry { width, height, file_width };
        self.body_size_used.set(self.body_size.get());
        let layout = self.workspace.view_layout();
        let terminal_area = match &layout {
            Some(layout) => pane_area(self, layout, geometry, self.body_size.clone(), cx),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(hsla(palette.fg_muted))
                .child("No terminal open. Click + in the Work list to open one."),
        };
        let body = match &file_pane_now {
            Some(pane) => {
                div().size_full().flex().flex_row().child(div().flex_1().min_w_0().h_full().child(terminal_area)).child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px(file_width))
                        .h_full()
                        .child(file_pane(self, pane, cx))
                        .child(file_divider(self, cx)),
                )
            }
            None => terminal_area,
        };
        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(tab_bar(self, geometry, layout.as_ref(), cx))
            .child(div().flex_1().min_h_0().child(body));
        // 알릴 문제가 없으면 선택한 터미널이 쓰는 계정(없으면 `Local shell`)을 보인다.
        let git_problem = match self.git.repo() {
            Repo::Unavailable(message) => Some(message.clone()),
            _ => None,
        };
        let problem = if self.breakaway_denied {
            Some("Terminals will stop when Paddock closes".to_string())
        } else {
            self.status.clone().or(git_problem)
        };
        let is_problem = problem.is_some();
        let status_text = problem.or_else(|| {
            let tab = self.workspace.selected().and_then(|id| self.workspace.tab(id))?;
            let state = tab.content.session.grid.shared.state.lock().ok().and_then(|state| state.clone());
            Some(crate::agent_view::status_scope_text(state.as_ref()))
        });
        // 오른쪽 묶음의 순서는 사용량 → 검토 메모 수 → 저장 표시다. 검토 메모 수와 저장 표시가 먼저 자리를 쥐고,
        // 사용량 묶음이 남은 폭에 안 들어가면 `Usage` 단추 하나로 접힌다.
        let usage_groups = self.usage_bar_groups();
        let review_label = self.review_notes_label();
        let memory_shown = self.usage.items.memory;
        let other_right = crate::usage::bar::other_right_width(review_label.as_deref(), self.save_status())
            + if memory_shown { crate::usage::bar::memory_group_width() } else { 0.0 };
        let usage_mode = bar_mode(
            f32::from(viewport.width) / crate::look::zoom(),
            status_text.as_ref().map_or(0, |text| text.chars().count()),
            other_right,
            &usage_groups,
            status_bar::PADDING_X,
        );
        self.usage_mode.set(usage_mode);
        let status = div()
            .flex_none()
            .h(zpx(status_bar::HEIGHT))
            .px(zpx(status_bar::PADDING_X))
            .flex()
            .items_center()
            .border_t_1()
            .border_color(hsla(palette.status_bar_border))
            .bg(hsla(palette.status_bar_bg))
            .text_size(zpx(text::SMALL))
            .text_color(hsla(if is_problem { palette.warning } else { palette.status_bar_fg }))
            .justify_between()
            .child(div().flex_1().min_w_0().truncate().children(status_text))
            .child(
                div()
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(zpx(paddock_core::theme::editor::STATUS_GAP))
                    .text_color(hsla(palette.status_bar_fg))
                    .children(crate::usage::bar::usage_elements(self, &usage_groups, usage_mode, cx))
                    .children(memory_shown.then(|| crate::usage::bar::memory_group(self.memory.gauge.as_ref())))
                    .children(review_label)
                    .children(crate::files::editing_view::save_status_view(self, cx)),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .children((!collapsed).then(|| work_list(self, cx)))
                    .children(
                        (!collapsed).then(|| {
                            div().flex_none().w(zpx(sidebar::SPLIT_WIDTH)).h_full().bg(hsla(palette.split_line))
                        }),
                    )
                    .child(main),
            )
            .child(status)
    }
}

impl Focusable for WorkWindow {
    fn focus_handle(
        &self,
        _cx: &gpui_kit::App,
    ) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for WorkWindow {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        #[cfg(feature = "automation")]
        crate::automation::count_draw();
        let palette = colors();
        // 행 끌기가 끝났으면(행 밖에서 놓은 경우 포함) 놓을 자리 표식을 비운다.
        if self.row_drop.is_some() && !cx.has_active_drag() {
            self.row_drop = None;
        }
        // 탭 끌기도 같다: 끌기가 끝났으면 끌기 상태와 놓을 자리 미리보기를 비운다.
        self.settle_tab_drag(cx);
        // 닫힌 이미지·PDF 쪽의 그래픽 메모리를 돌려준다.
        let buried = self.graveyard.drain();
        let has_buried = !buried.is_empty();
        for image in buried {
            let _ = window.drop_image(image);
        }
        // 그래픽 드라이버가 텍스처를 한 프레임 뒤에 풀기도 하므로, 반납한 뒤 한 번 더 그려 놀 때도 풀리게 한다.
        // 그리는 도중의 `refresh`는 무시되므로, 다음 프레임이 시작될 때 다시 그리도록 맡긴다.
        if has_buried {
            // 분리된 파일 창도 자기 텍스처 보관소의 몫을 비우도록 같이 다시 그리게 한다(`ImageMirror`).
            let file_windows: Vec<AnyWindowHandle> =
                self.detached_file_windows.values().map(|handle| handle.window).collect();
            window.on_next_frame(move |window, cx| {
                window.refresh();
                for handle in file_windows {
                    let _ = handle.update(cx, |_, file_window, _| file_window.refresh());
                }
            });
        }
        let content = match &self.phase {
            Phase::Connecting(message) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(hsla(palette.fg_muted))
                .child(*message),
            Phase::Ready => {
                let workspace = self.render_workspace(window, cx);
                if self.reveal_selected.replace(false) {
                    self.reveal_until.set(Some(Instant::now() + REVEAL_SETTLE));
                }
                workspace
            }
            Phase::Notice(notice) => self.render_notice(notice, cx),
        };
        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(hsla(palette.app_bg))
            .text_color(hsla(palette.fg))
            .font_family(ui_family())
            .text_size(zpx(text::UI))
            .track_focus(&self.focus)
            .key_context("WorkWindow")
            .on_action(
                cx.listener(|this, _: &StepPreviousTab, window, cx| this.step_tab(TabTarget::Previous, window, cx)),
            )
            .on_action(cx.listener(|this, _: &StepNextTab, window, cx| this.step_tab(TabTarget::Next, window, cx)))
            .on_action(cx.listener(|this, _: &SearchInFolderFromEditor, window, cx| this.toggle_search(window, cx)))
            .capture_key_down(cx.listener(Self::on_capture_key))
            .capture_any_mouse_down(
                cx.listener(|this, _event: &MouseDownEvent, window, cx| this.reset_tab_drag(window, cx)),
            )
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<DragTab>, window, cx| this.tab_drag_moved(event, window, cx)),
            )
            .on_drop(cx.listener(|this, dropped: &DragTab, window, cx| this.tab_dropped(dropped, window, cx)))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<DragFileTab>, _window, cx| this.file_drag_moved(event, cx)),
            )
            .on_drop(cx.listener(|this, _dropped: &DragFileTab, _window, cx| this.file_dropped(cx)))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.tab_released_outside(event.position, window, cx)
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| this.drag_divider(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| this.end_divider_drag(cx)),
            )
            .children(titlebar_view(window, "Paddock"))
            .child(div().flex_1().min_h_0().child(content))
            .children(self.tab_drag_overlay());
        root = self.render_overlay(root, window, cx);
        if let Some(menu) = self.menu.clone() {
            root = root.child(menu_panel(self, &menu, cx));
        }
        if let Some(popup) = crate::usage::popup::usage_popup_view(
            self,
            (f32::from(window.viewport_size().width), f32::from(window.viewport_size().height)),
            cx,
        ) {
            root = root.child(popup);
        }
        if let Some(notice) = self.memory.notice.as_ref() {
            root = root.child(crate::usage::bar::memory_notice_view(notice, cx));
        }
        if self.quick.open {
            let viewport = window.viewport_size();
            root = root.child(quick_panel(self, (f32::from(viewport.width), f32::from(viewport.height)), cx));
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c7_protocol_mismatch_notice_names_both_versions_and_warns_of_a_forced_stop_only_for_a_rejecting_daemon() {
        let (headline, detail) = protocol_mismatch_text(Some(1));
        assert_eq!(
            headline,
            format!("The running terminal daemon speaks protocol v1; this Paddock needs v{PROTOCOL_VERSION}")
        );
        assert_eq!(detail, "Replacing it ends the terminals it holds. Quit leaves it running untouched.");
        // 인사를 거절한 옛 데몬은 종료 요청을 받지 못해 강제로 끝내므로 기록 손실을 알린다.
        let (headline_forced, detail_forced) = protocol_mismatch_text(None);
        assert_eq!(headline_forced, headline);
        assert!(detail_forced.contains("forced stop"), "{detail_forced}");
        assert!(detail_forced.contains("history may be lost"), "{detail_forced}");
        assert_eq!(
            detail_forced,
            "Replacing it ends the terminals it holds (forced stop — the last few seconds of terminal history may be lost). Quit leaves it running untouched."
        );
    }
}
