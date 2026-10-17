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
    AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render, ScrollHandle, Styled, Window, div, px,
};
use paddock_core::app_shortcut::{AppShortcut, app_shortcut};
use paddock_core::entry_name::{EntryNameCheck, validate_entry_name};
use paddock_core::file_kind::{FileKind, kind_of};
use paddock_core::file_tree::{files_folder, folder_label};
use paddock_core::git_status::Change;
use paddock_core::key_encoding::KeyInput;
use paddock_core::layout_model::{Orientation, drag_boundary};
use paddock_core::markdown_view::{View, ViewChoices};
use paddock_core::pane_columns::{split_body_width, strip_body_size};
use paddock_core::settings_text::{step_font_size, step_zoom_level};
use paddock_core::theme::{dialog as dialog_size, sidebar, split, status_bar, tab, text};
use paddock_core::work_model::{Candidate, GroupKey, NewPlace, TabTarget, folder_name, tab_target};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Kill, kind};
use serde_json::Value;

use crate::accounts::window::AccountsPanel;
use crate::app_log;
use crate::chrome::{ButtonKind, button, ui_family, zpx};
use crate::daemon_client::{MismatchPolicy, request_shutdown, restart_stalled_daemon};
use crate::daemon_launch::sibling_daemon_exe;
use crate::done_notice::DoneNotice;
use crate::done_sound;
use crate::files::active::{SelectionChange, file_stays_active};
use crate::files::image_view::ImageView;
use crate::files::images::{Graveyard, render_image};
use crate::files::markdown_view::{Origin, build_markdown};
use crate::files::menu_model::{entry_title, reveal_command};
use crate::files::open::{FileStamp, Loaded, NoticeAction, NoticeReason, file_stamp, load_fit, open_command};
use crate::files::pane::{FileContent, file_divider, file_pane};
use crate::files::pdf_view::PdfView;
use crate::files::state::FilesState;
use crate::files::text_view::build_text;
use crate::git::state::{GitState, Repo};
use crate::look::colors;
use crate::panes::pane_area;
use crate::quick_settings::model::{Field, QuickState, Row, setting_of};
use crate::quick_settings::panel::quick_panel;
use crate::quit_policy::{
    OnQuit, QuitStep, default_quit_choice, may_save_state_on_close, quit_and_stop_all_step, quit_step, read_on_quit,
    write_on_quit,
};
use crate::quit_prompt::{
    close_dialog, close_group_message, close_many_dialog, close_terminal_message, quit_dialog, remove_folder_detail,
};
use crate::scm::{ScmState, SidebarView};
use crate::settings::{Settings, key, write_native};
use crate::shell_choice::{Environment, available_environments, default_shell, folder_want};
use crate::startup::{
    OpenedTerminal, Startup, new_terminal_want, open_new_terminal, open_workspace, restore_terminals,
};
use crate::state_store;
use crate::tabs::{StripGeometry, overflow_look, scroll_by_step, tab_bar};
use crate::terminal::ime::SentComposition;
use crate::terminal::paint::{GlyphCache, hsla};
use crate::terminal::session::{TerminalSession, Want};
use crate::terminal::view::TerminalView;
use crate::titlebar::titlebar_view;
use crate::work::list_view::work_list;
use crate::work::menu::menu_panel;
use crate::work::menu_model::{ContextMenu, MenuAction, MenuKind, clamped_position, menu_items, panel_height};
use crate::work::rename_dialog::{name_dialog, rename_dialog};
use crate::work::terminal_row::RowTarget;
use crate::workspace::{TabRef, Workspace};

gpui_kit::actions!(paddock_window, [StepPreviousTab, StepNextTab]);

/// 텍스트 보기 편집기가 쥔 `Ctrl+←/→`(단어 이동)를 앱의 탭 이동으로 되돌리는 키 연결을 등록한다. 앱 시작 때 한 번 부른다.
///
/// 키 연결은 키 누름 처리기보다 먼저 실행되므로, 편집기에 포커스가 있으면 `capture_key_down`의 앱 단축키가 이 두 키를 받지 못한다.
/// 읽기 전용 보기에서는 글자 단위 이동이 필요 없고 탭 이동은 터미널과 같은 키여야 하므로 편집기 문맥에서만 앱 동작으로 덮는다.
pub fn bind_keys(cx: &mut gpui_kit::App) {
    cx.bind_keys([
        gpui_kit::KeyBinding::new("ctrl-left", StepPreviousTab, Some("Input && mode == editor")),
        gpui_kit::KeyBinding::new("ctrl-right", StepNextTab, Some("Input && mode == editor")),
    ]);
}

/// 터미널 상태(에이전트 활동·완료 알림)가 바뀌었는지 확인하는 간격. 바뀐 것이 있을 때만 목록을 다시 그린다.
const AGENT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 자동 점검이 상태 글 재료를 다시 조립하는 간격. 대본 동작·키·창 크기 변경을 처리한 틱은 간격과 무관하게 바로 조립한다.
/// 매 틱(16 ms)마다 조립하면 점검 빌드의 CPU가 사용자 빌드보다 훨씬 높게 잰다.
#[cfg(feature = "automation")]
const PROBE_INTERVAL: Duration = Duration::from_millis(50);

/// 열린 파일 탭의 파일이 디스크에서 바뀌었는지 확인하는 주기. 폴더 감시 사건이 오지 않는 폴더(WSL 공유)와 파일 내용만 바뀐 경우를 위한 것이다.
const OPEN_FILE_CHECK_INTERVAL: Duration = Duration::from_secs(3);

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
    /// 실행 중인 터미널 데몬이 이 앱과 다른 버전이다. 교체하거나 앱을 끝낸다.
    DaemonMismatch,
    /// 실행 중인 터미널 데몬이 응답하지 않는다. 다시 시작하거나 앱을 끝낸다.
    DaemonStalled,
    /// 데몬에 연결하지 못했다. 서버나 시스템이 준 문장을 그대로 보인다.
    Failed(String),
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
}

/// 창 위에 떠 있는 묻기 창.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// 터미널 하나를 닫을지 확인한다.
    CloseTerminal(u32),
    /// Unassigned 묶음의 터미널을 모두 닫을지 확인한다. 값은 묶음의 시작 터미널.
    CloseGroup(u32),
    /// 작업 폴더를 목록에서 빼며 그 폴더의 터미널을 모두 닫을지 확인한다.
    RemoveFolder(String),
    /// 터미널이나 폴더의 이름을 바꾼다.
    Rename(RenameTarget),
    /// `parent` 폴더 안에 새 파일(`is_dir`이 거짓)이나 새 폴더를 만들 이름을 받는다.
    NewEntry {
        parent: String,
        is_dir: bool,
    },
    /// 등록 계정을 보고 더하고 이름을 바꾸고 뺀다.
    Accounts,
    /// 작업 폴더의 변경 하나를 되돌릴지(추적 안 하는 새 파일이면 지울지) 확인한다.
    Discard(Change),
    /// 창을 닫을 때 열린 터미널을 어떻게 할지 묻는다.
    Quit,
}

/// 터미널 선택이 바뀔 수 있는 연산 앞의 활성 상태.
struct ActiveSnapshot {
    was_file_active: bool,
    /// 그때 보이던 묶음의 터미널.
    group_ids: Vec<u32>,
}

/// 키를 받는 탭이 터미널인지 파일 탭인지. `Workspace::selected`는 "마지막으로 고른 터미널"이라 파일 탭이 활성이어도 그대로다.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Active {
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
    /// 데몬에 연결한다. 지문이 다르면 `policy`를 따른다.
    Open(MismatchPolicy),
    /// 멈춘 데몬을 끝내고 새로 띄워 연결한다.
    RestartStalled,
}

/// 앱 창의 뿌리 보기.
pub struct WorkWindow {
    daemon_exe: Option<PathBuf>,
    phase: Phase,
    pub(crate) workspace: Workspace<TabContent>,
    cache: Rc<RefCell<GlyphCache>>,
    /// 키·포커스 이동으로 이미 내보낸 조합 글자의 기억. 이 창의 모든 터미널 보기가 함께 쥔다.
    pub(crate) already_sent: SentComposition,
    /// 터미널이 없을 때 키를 받는 자리.
    focus: FocusHandle,
    /// 묻기 창이 떠 있는 동안 키를 받는 자리. 터미널이 글자를 받지 못하게 포커스를 옮겨 둔다.
    pub(crate) overlay_focus: FocusHandle,
    overlay: Overlay,
    /// 열려 있는 우클릭·환경 고르기 메뉴.
    menu: Option<ContextMenu>,
    /// 이름 바꾸기 창의 입력칸.
    rename_input: Option<Entity<InputState>>,
    /// 사이드바 파일 목록의 상태(보이는 폴더·펼친 폴더·읽기 결과).
    pub(crate) files: FilesState,
    /// 보이는 폴더의 Git 상태(변경 목록·표시 조회표·실행 시점 판정).
    pub(crate) git: GitState,
    /// Source control 보기 상태(사이드바 보기·커밋 입력칸·접은 구획).
    pub(crate) scm: ScmState,
    /// 새 파일·새 폴더 창의 입력칸.
    entry_input: Option<Entity<InputState>>,
    /// 키를 받는 탭: 터미널 또는 파일 탭.
    active: Active,
    /// 묶음마다 마지막으로 활성이었던 쪽이 파일 탭이었는지. 보이는 묶음이 저절로 바뀔 때(닫기) 그 묶음의 마지막 상태로 돌아가는 데 쓴다.
    file_active_memory: HashMap<GroupKey, bool>,
    /// `Open as text anyway`로 연 파일. 다시 읽을 때도 이진 판정을 건너뛴다(저장하지 않아 재실행하면 사라진다).
    forced_text: HashSet<String>,
    /// 열린 파일 탭의 내용(경로별). 같은 파일이 여러 묶음에 열려 있어도 한 벌을 나눠 쓴다.
    pub(crate) file_views: HashMap<String, FileContent>,
    /// 열린 파일이 마지막으로 읽힐 때의 쓴 시각·크기. 달라지면 다시 읽는다.
    file_stamps: HashMap<String, Option<FileStamp>>,
    /// 열린 파일의 변경 확인을 마지막으로 시작한 시각과 확인이 도는 중인지.
    open_files_checked_at: Option<Instant>,
    open_files_checking: bool,
    /// 파일마다 마지막으로 고른 Markdown 보기 방식.
    markdown_views: ViewChoices,
    /// 닫힌 이미지·PDF 쪽의 그래픽 메모리 반납 대기소. 다음 그리기에서 비운다.
    graveyard: Graveyard,
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
    /// `Show N more`를 눌러 한도 없이 보이는 작업 폴더.
    pub(crate) show_all: HashSet<String>,
    /// 종료 묻기의 "Don't ask again" 선택.
    pub(crate) remember_choice: bool,
    /// 데몬이 앱의 작업 개체에서 분리되지 못해 앱을 닫으면 터미널도 끝난다.
    pub(crate) breakaway_denied: bool,
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
    reveal_selected: Cell<bool>,
    /// 키보드로 탭을 옮긴 뒤 이 시각까지의 그리기에서도 선택한 탭을 보이게 다시 스크롤한다. 넘김 단추가 새로 생기며 탭 상자가
    /// 좁아진 뒤의 그리기에서 위치를 바로잡는 데 쓴다.
    reveal_until: Cell<Option<Instant>>,
    /// 마지막으로 그린 탭 줄 넘침 모양. 스크롤 상자가 배치된 뒤 모양이 달라지면 다시 그린다.
    last_overflow: RefCell<Vec<(usize, bool, bool)>>,
    /// 그리는 동안 잰 본문 크기(화면 크기 단위). 아직 못 쟀으면 (0, 0).
    body_size: Rc<Cell<(f32, f32)>>,
    /// 탭 줄이 마지막으로 쓴 본문 크기. 잰 크기와 달라지면 다시 그린다.
    body_size_used: Cell<(f32, f32)>,
    /// 마지막으로 그렸을 때의 창 크기. `body_size`는 그 그리기에서 잰 값이라, 창 크기가 달라졌으면 낡았다.
    rendered_viewport: Cell<(f32, f32)>,
    /// 끌고 있는 칸 경계.
    divider_drag: Option<DividerDrag>,
    #[cfg(feature = "automation")]
    hooks: Option<crate::automation::Hooks>,
    /// 자동 점검: 마지막 갱신 루프 시각과 그 사이 가장 길었던 간격(창 스레드가 막힌 시간의 상한 진단).
    #[cfg(feature = "automation")]
    tick_gap: (Option<Instant>, Duration),
    /// 자동 점검: 상태 글 재료(`Probe`)를 마지막으로 조립한 시각. 조립은 화면 전체의 요약을 만들어 비싸므로 `PROBE_INTERVAL`마다만 한다.
    #[cfg(feature = "automation")]
    probe_at: Option<Instant>,
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
            git: GitState::default(),
            scm: ScmState::default(),
            entry_input: None,
            active: Active::Terminal,
            file_active_memory: HashMap::new(),
            forced_text: HashSet::new(),
            file_views: HashMap::new(),
            file_stamps: HashMap::new(),
            open_files_checked_at: None,
            open_files_checking: false,
            markdown_views: ViewChoices::default(),
            graveyard: Graveyard::default(),
            file_body_width: Cell::new(0.0),
            accounts: AccountsPanel::default(),
            quick: QuickState::default(),
            installed_fonts: Vec::new(),
            quick_settings: Settings::default(),
            show_all: HashSet::new(),
            remember_choice: false,
            breakaway_denied: false,
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
            #[cfg(feature = "automation")]
            hooks: None,
            #[cfg(feature = "automation")]
            tick_gap: (None, Duration::ZERO),
            #[cfg(feature = "automation")]
            probe_at: None,
        };
        window.focus(&this.focus, cx);
        let entity = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            entity.update(cx, |this, cx| this.on_close_requested(window, cx)).unwrap_or(true)
        });
        if this.daemon_exe.is_some() {
            this.connect(Connect::Open(MismatchPolicy::Quit), window, cx);
        }
        this.watch_agents(window, cx);
        // 창 크기가 바뀌는 사건에서 바로 다시 그린다. 200ms 주기 확인을 기다리면 탭 열 구간이 그만큼 낡은 폭으로 남는다.
        cx.observe_window_bounds(window, |_this, _window, cx| cx.notify()).detach();
        this
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
        let watching = window.is_window_active().then(|| self.workspace.selected()).flatten();
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
                if self.done.on_finished(id, watching == Some(id), sound_enabled, now).sound {
                    done_sound::request(id);
                }
            }
            changed = true;
        }
        if let Some(id) = watching
            && self.done.acknowledge(id)
        {
            changed = true;
        }
        if self.layout_measures_changed() {
            changed = true;
        }
        let listings_before = self.files.n_listings();
        if self.sync_files(window) {
            changed = true;
        }
        self.watch_open_files(self.files.n_listings() != listings_before, window, cx);
        if self.load_visible_file(window, cx) {
            changed = true;
        }
        if self.pump_git(window, cx) {
            changed = true;
        }
        if changed && self.promote_agents() {
            self.persist();
        }
        if changed {
            cx.notify();
        }
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
                Connect::Open(MismatchPolicy::Quit) => "Connecting to the terminal daemon...",
                Connect::Open(MismatchPolicy::Replace) => "Replacing the terminal daemon...",
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
                app_log::event(&format!("window ready with {} terminals", self.n_terminals()));
            }
            Ok(Startup::MismatchKept) => self.phase = Phase::Notice(Notice::DaemonMismatch),
            Ok(Startup::DaemonStalled) => self.phase = Phase::Notice(Notice::DaemonStalled),
            Err(error) => self.phase = Phase::Notice(Notice::Failed(error.to_string())),
        }
        cx.notify();
    }

    /// 세션에 터미널 보기를 만든다.
    fn view_of(
        &self,
        session: &TerminalSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalView> {
        let grid = session.grid.clone();
        let cache = self.cache.clone();
        let already_sent = self.already_sent.clone();
        cx.new(|cx| TerminalView::new(grid, cache, already_sent, window, cx))
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
    }

    /// 활성 탭에 키 입력을 준다: 파일 탭이 활성이면 그 내용(텍스트 보기), 아니면 선택한 터미널. 받을 곳이 없으면 창 자신이 포커스를 쥔다.
    pub(crate) fn focus_selected(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_active() {
            let handle = self
                .selected_file_key()
                .and_then(|key| self.file_views.get(&key))
                .and_then(|content| content.focus_handle(cx));
            window.focus(handle.as_ref().unwrap_or(&self.focus), cx);
        } else {
            let selected = self.workspace.selected().and_then(|id| self.workspace.tab(id));
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
        state
    }

    pub(crate) fn select_tab(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.select(id) {
            self.set_active(Active::Terminal);
            // 사용자가 직접 고른 터미널은 확인한 것이다.
            self.done.acknowledge(id);
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    fn step_tab(
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
    fn activate_tab(
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

    /// 보는 묶음의 선택한 파일 탭 경로.
    pub(crate) fn selected_file_path(&self) -> Option<String> {
        self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).map(|tab| tab.path.clone())
    }

    /// 보는 묶음의 선택한 파일 탭의 열쇠(`file_views`의 열쇠).
    pub(crate) fn selected_file_key(&self) -> Option<String> {
        self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).map(|tab| tab.key())
    }

    /// 키를 받는 탭을 정하고, 보이는 묶음에서 마지막으로 활성이었던 쪽으로 기억한다.
    fn set_active(
        &mut self,
        active: Active,
    ) {
        if let Some(key) = self.workspace.view_key() {
            self.file_active_memory.insert(key, matches!(active, Active::File(_)));
        }
        self.active = active;
    }

    /// 터미널 선택이 바뀔 수 있는 연산 앞의 상태.
    fn snapshot_active(&self) -> ActiveSnapshot {
        ActiveSnapshot { was_file_active: self.file_active(), group_ids: self.workspace.view_ids() }
    }

    /// 터미널 선택이 바뀔 수 있는 연산(새 터미널·닫기·폴더로 올리기) 뒤 어느 탭이 활성인지 `file_stays_active` 규칙 하나로 정한다.
    /// 묶음 이름표가 바뀌어도(`follow_groups`가 파일 칸을 옮긴 뒤) 같은 묶음이면 그 묶음의 파일 칸을 가리킨다.
    fn settle_active(
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

    /// 파일 목록 행·메뉴의 `Open`·대본의 `openfile`: 파일을 열어 활성 탭으로 만든다.
    ///
    /// 탭은 바로 만들고 내용은 작업 스레드가 읽는 대로 채운다(읽는 동안 창이 멈추지 않는다). 이미 열려 있는 파일이면 그 탭을
    /// 고르고 디스크의 지금 내용으로 다시 읽는다.
    pub(crate) fn open_file(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file_as(path, kind_of(path), window, cx);
    }

    /// `open_file`과 같되 탭이 보여 주는 방식을 정한다(변경 비교 탭은 `FileKind::Diff`).
    pub(crate) fn open_file_as(
        &mut self,
        path: &str,
        kind: FileKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((key, index)) = self.workspace.open_file(path, kind) {
            let kind = self.workspace.files().get(&key).and_then(|pane| pane.tabs.get(index)).map(|tab| tab.kind);
            let ignore_nul = self.forced_text.contains(path);
            self.load_tab(path.to_string(), kind.unwrap_or(FileKind::Text), ignore_nul, window, cx);
            if self.workspace.view_key().as_ref() == Some(&key) {
                self.set_active(Active::File(key));
            }
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 보이는 묶음에서 선택한 파일 탭의 내용이 아직 없으면 읽기를 시작한다. 보이지 않는 묶음이나 선택하지 않은 탭은 보일 때 읽는다
    /// (앱을 다시 켜도 열어 둔 모든 파일을 한꺼번에 읽지 않는다). 읽기를 시작했으면 true다.
    fn load_visible_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let wanted = self
            .workspace
            .file_pane()
            .and_then(|pane| pane.tabs.get(pane.selected))
            .filter(|tab| !self.file_views.contains_key(&tab.key()))
            .map(|tab| (tab.path.clone(), tab.kind));
        let started = wanted.is_some();
        if let Some((path, kind)) = wanted {
            let ignore_nul = self.forced_text.contains(&path);
            self.load_tab(path, kind, ignore_nul, window, cx);
        }
        started
    }

    /// 탭 종류에 맞는 읽기를 시작한다: 변경 비교 탭은 git에게, 나머지는 파일을 읽는 작업 스레드에게 시킨다.
    fn load_tab(
        &mut self,
        path: String,
        kind: FileKind,
        ignore_nul: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            FileKind::Diff { staged } => self.start_diff_load(path, staged),
            _ => self.start_file_load(path, kind, ignore_nul, window, cx),
        }
    }

    /// 파일을 작업 스레드에서 읽어 내용을 만든다. 읽는 동안 이미 보이던 내용이 있으면 그대로 두고, 없으면 빈 자리를 보인다.
    /// `ignore_nul`은 사용자가 `Open as text anyway`로 고른 경우다. 이미지는 지금 창 크기 안으로 줄여 올린다(1:1로 바꿀 때 원본을 다시 읽는다).
    fn start_file_load(
        &mut self,
        path: String,
        kind: FileKind,
        ignore_nul: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_views.entry(path.clone()).or_insert(FileContent::Loading);
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let bound = ((f32::from(viewport.width) * scale) as u32, (f32::from(viewport.height) * scale) as u32);
        cx.spawn_in(window, async move |this, cx| {
            let (loaded, stamp) = {
                let path = path.clone();
                cx.background_executor()
                    .spawn(async move {
                        // 읽기 전에 쓴 시각을 먼저 잰다: 읽는 사이 바뀐 파일은 다음 확인에서 다시 읽힌다.
                        let stamp = file_stamp(&path);
                        (load_fit(&path, kind, ignore_nul, Some(bound)), stamp)
                    })
                    .await
            };
            let _ = this.update_in(cx, |this, window, cx| this.finish_file_load(path, kind, loaded, stamp, window, cx));
        })
        .detach();
    }

    /// 파일이 속한 작업 폴더(Markdown 속 이미지를 읽을 수 있는 범위). 폴더 묶음이 아니면 파일이 있는 폴더다.
    fn work_folder_of_file(
        &self,
        path: &str,
    ) -> String {
        let owner =
            self.workspace.files().iter().find(|(_, pane)| pane.tabs.iter().any(|tab| tab.path == path)).and_then(
                |(key, _)| match key {
                    GroupKey::Folder(folder) => Some(folder.clone()),
                    GroupKey::Group(_) => None,
                },
            );
        owner.unwrap_or_else(|| path.rsplit_once(['/', '\\']).map_or(String::new(), |(dir, _)| dir.to_string()))
    }

    /// 읽기 결과로 파일 탭의 내용을 바꾼다. 읽는 사이 탭이 닫혔으면 버린다. `kind`는 읽기를 시작할 때 탭이 보여 주기로 한 방식이다.
    /// 같은 종류의 내용이 이미 보이고 있으면 새로 만들지 않고 그 안의 내용만 바꿔 스크롤 위치·배율·보기 방식을 지킨다.
    fn finish_file_load(
        &mut self,
        path: String,
        kind: FileKind,
        loaded: Loaded,
        stamp: Option<FileStamp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.files().open_keys().any(|open| open == path) {
            self.file_stamps.insert(path.clone(), stamp);
            let root = self.work_folder_of_file(&path);
            let graveyard = self.graveyard.clone();
            let view = self.markdown_views.get(&path);
            let replaced = match (self.file_views.get_mut(&path), loaded) {
                (Some(FileContent::Text(body)), Loaded::Text { text, size }) if kind != FileKind::Markdown => {
                    body.replace(text, size, window, cx);
                    None
                }
                (Some(FileContent::Markdown(body)), Loaded::Text { text, size }) if kind == FileKind::Markdown => {
                    body.replace(text, size, window, cx);
                    None
                }
                (Some(FileContent::Image(existing)), Loaded::Image { pixels, original, size }) => {
                    match render_image(pixels) {
                        Some(image) => {
                            existing.update(cx, |existing, cx| existing.replace(image, original, size, cx));
                            None
                        }
                        None => Some(FileContent::Notice { reason: NoticeReason::ImageUnreadable, size }),
                    }
                }
                (Some(FileContent::Pdf(existing)), Loaded::Pdf { document, size }) => {
                    existing.update(cx, |existing, cx| existing.replace(document, size, cx));
                    None
                }
                (_, loaded) => Some(match loaded {
                    Loaded::Text { text, size } if kind == FileKind::Markdown => FileContent::Markdown(build_markdown(
                        text,
                        size,
                        Origin { path: &path, root_dir: &root },
                        view,
                        graveyard,
                        window,
                        cx,
                    )),
                    Loaded::Text { text, size } => FileContent::Text(build_text(text, size, &path, window, cx)),
                    Loaded::Image { pixels, original, size } => match render_image(pixels) {
                        Some(image) => FileContent::Image(
                            cx.new(|_| ImageView::new(path.clone(), image, original, size, graveyard)),
                        ),
                        None => FileContent::Notice { reason: NoticeReason::ImageUnreadable, size },
                    },
                    Loaded::Pdf { document, size } => {
                        FileContent::Pdf(cx.new(|_| PdfView::new(document, size, graveyard)))
                    }
                    Loaded::Notice { reason, size } => FileContent::Notice { reason, size },
                    Loaded::Failed(message) => FileContent::Failed(message),
                }),
            };
            if let Some(content) = replaced {
                self.file_views.insert(path, content);
            }
            // 묻기 창이 떠 있으면 늦게 도착한 내용이 포커스를 가져가지 않는다.
            if self.file_active() && self.overlay == Overlay::None {
                self.focus_selected(window, cx);
            }
            cx.notify();
        }
    }

    /// 열린 파일 탭의 파일이 디스크에서 바뀌었는지 작업 스레드에서 쓴 시각·크기로 보고, 바뀐 파일을 다시 읽는다.
    ///
    /// 파일 목록이 폴더를 다시 읽은 직후(감시 사건)와 `OPEN_FILE_CHECK_INTERVAL`마다 확인한다 — 폴더 목록은 파일 내용만 바뀔 때 달라지지 않고
    /// WSL 폴더는 감시 사건이 오지 않으므로 주기 확인이 있어야 한다. 쓴 시각을 읽지 못한 파일(지워졌거나 잠김)은 마지막 내용을 그대로 둔다.
    fn watch_open_files(
        &mut self,
        listing_changed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let due = self.open_files_checked_at.is_none_or(|at| at.elapsed() >= OPEN_FILE_CHECK_INTERVAL);
        if (listing_changed || due) && !self.open_files_checking {
            let open: Vec<(String, FileKind)> = self
                .workspace
                .files()
                .iter()
                .flat_map(|(_, pane)| pane.tabs.iter())
                .filter(|tab| !matches!(tab.kind, FileKind::Diff { .. } | FileKind::Media | FileKind::Binary))
                .filter(|tab| matches!(self.file_views.get(&tab.key()), Some(content) if !matches!(content, FileContent::Loading)))
                .map(|tab| (tab.path.clone(), tab.kind))
                .collect();
            self.open_files_checked_at = Some(Instant::now());
            if !open.is_empty() {
                self.open_files_checking = true;
                let known = self.file_stamps.clone();
                cx.spawn_in(window, async move |this, cx| {
                    let changed: Vec<(String, FileKind)> = cx
                        .background_executor()
                        .spawn(async move {
                            open.into_iter()
                                .filter(|(path, _)| {
                                    let now = file_stamp(path);
                                    now.is_some() && known.get(path).is_some_and(|before| *before != now)
                                })
                                .collect()
                        })
                        .await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.open_files_checking = false;
                        for (path, kind) in changed {
                            let ignore_nul = this.forced_text.contains(&path);
                            this.start_file_load(path, kind, ignore_nul, window, cx);
                        }
                    });
                })
                .detach();
            }
        }
    }

    /// 어느 묶음에도 열려 있지 않은 파일의 내용을 버린다.
    pub(crate) fn prune_file_views(&mut self) {
        let open: HashSet<String> = self.workspace.files().open_keys().collect();
        self.file_views.retain(|key, _| open.contains(key));
        self.forced_text.retain(|path| open.contains(path));
    }

    /// 탭 줄의 파일 탭을 누르면 그 탭을 고르고 키를 파일 쪽으로 보낸다.
    pub(crate) fn select_file_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_tab(TabRef::File(index));
        self.focus_selected(window, cx);
        self.persist();
        cx.notify();
    }

    /// 파일 칸을 누르면 파일 탭이 활성이 되어 키가 파일 쪽으로 간다.
    pub(crate) fn activate_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_active()
            && let Some(key) = self.workspace.view_key()
            && self.workspace.files().get(&key).is_some()
        {
            self.set_active(Active::File(key));
        }
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 파일 탭을 닫는다(확인 없이 — 파일은 읽기 전용이라 잃을 것이 없다). 칸의 탭이 모두 닫히면 터미널이 다시 활성이 된다.
    pub(crate) fn close_file_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = self.workspace.view_key()
            && self.workspace.close_file(&key, index)
        {
            if self.workspace.files().get(&key).is_none() {
                self.set_active(Active::Terminal);
            }
            self.prune_file_views();
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
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

    /// 안내 탭의 단추를 실행한다.
    pub(crate) fn run_notice_action(
        &mut self,
        action: NoticeAction,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            NoticeAction::ShowInFileManager => self.reveal_in_system(path),
            NoticeAction::OpenAsText => {
                if let Some(key) = self.workspace.view_key()
                    && let Some(index) = self
                        .workspace
                        .files()
                        .get(&key)
                        .and_then(|pane| pane.tabs.iter().position(|tab| tab.path == path))
                {
                    self.workspace.set_file_kind(&key, index, FileKind::Text);
                    self.forced_text.insert(path.to_string());
                    self.start_file_load(path.to_string(), FileKind::Text, true, window, cx);
                }
            }
            NoticeAction::OpenWithDefaultApp => {
                // 너무 큰 텍스트는 편집기로, 이미지·PDF·오디오·영상은 기본 앱으로 연다.
                let as_text = matches!(
                    self.file_views.get(path),
                    Some(FileContent::Notice { reason: NoticeReason::TooLarge, .. })
                );
                self.open_externally(path, as_text);
            }
        }
        cx.notify();
    }

    /// 보는 묶음의 선택한 파일 탭이 보여 주는 방식.
    pub(crate) fn selected_file_kind(&self) -> Option<FileKind> {
        self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).map(|tab| tab.kind)
    }

    /// 선택한 파일 탭이 Markdown이면 지금 보이는 방식.
    pub(crate) fn selected_markdown_view(&self) -> Option<View> {
        match self.selected_file_key().and_then(|key| self.file_views.get(&key)) {
            Some(FileContent::Markdown(body)) => Some(body.view),
            _ => None,
        }
    }

    /// 탭 줄의 `File`·`Preview`·`Both`: 선택한 Markdown 탭의 보기 방식을 바꾸고 파일별로 기억한다. 키는 계속 파일 칸이 받는다.
    pub(crate) fn set_markdown_view(
        &mut self,
        view: View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(path), Some(key)) = (self.selected_file_path(), self.selected_file_key())
            && self.selected_file_kind() == Some(FileKind::Markdown)
        {
            self.markdown_views.set(&path, view);
            if let Some(FileContent::Markdown(body)) = self.file_views.get_mut(&key) {
                body.view = view;
                if view != View::Preview {
                    body.ensure_source(window, cx);
                }
            }
            self.activate_file(window, cx);
        }
    }

    /// 탭 줄의 `−`·`+`: 선택한 PDF 탭의 배율을 한 단계 바꾼다. `None`은 칸 폭 맞춤으로 되돌린다.
    pub(crate) fn zoom_pdf(
        &mut self,
        step: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(FileContent::Pdf(view)) = self.selected_file_key().and_then(|key| self.file_views.get(&key)) {
            view.update(cx, |view, cx| match step {
                Some(true) => view.zoom_in(cx),
                Some(false) => view.zoom_out(cx),
                None => view.zoom_fit(cx),
            });
            self.activate_file(window, cx);
        }
    }

    /// 탭 줄의 `Open with default app`: 선택한 PDF 파일을 기본 앱에 맡긴다.
    pub(crate) fn open_active_file_with_default_app(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.selected_file_path() {
            self.open_externally(&path, false);
        }
        cx.notify();
    }

    /// 탭 줄 오른쪽의 `Open in external editor`: 활성 파일 탭의 파일을 바깥 편집기로 연다.
    pub(crate) fn open_active_file_externally(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.selected_file_path() {
            self.open_externally(&path, true);
        }
        cx.notify();
    }

    /// 파일을 바깥 프로그램으로 연다. 셸을 거치지 않고 인자 배열로 띄우며 콘솔 창을 만들지 않는다.
    fn open_externally(
        &mut self,
        path: &str,
        as_text: bool,
    ) {
        let (program, args) = open_command(std::env::consts::OS, path, as_text);
        // Windows의 기본 앱 열기는 명령줄을 거치지 않고 셸에 경로를 그대로 넘긴다(쉼표·공백·한글이 든 경로 때문).
        #[cfg(windows)]
        let handed_to_shell = program == "explorer.exe";
        #[cfg(not(windows))]
        let handed_to_shell = false;
        if handed_to_shell {
            #[cfg(windows)]
            {
                let path = path.to_string();
                std::thread::spawn(move || {
                    if let Err(reason) = crate::files::open::shell_open(&path) {
                        app_log::event(&format!("could not open a file with the default app: {reason}"));
                    }
                });
            }
        } else {
            self.spawn_external(&program, &args, "open the file");
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
    fn reveal_in_system(
        &mut self,
        path: &str,
    ) {
        let (program, args) = reveal_command(std::env::consts::OS, path);
        self.spawn_external(&program, &args, "open the file manager");
    }

    /// 바깥 프로그램을 띄운다. 인자 배열로 직접 실행하고(셸 미경유) Windows에서는 콘솔 창을 만들지 않으며,
    /// 띄우지 못하면 상태 줄에 `Could not <action>: <이유>`를 알린다.
    fn spawn_external(
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
    fn entry_name_check(
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
        if let Some(id) = id {
            self.select_tab(id, window, cx);
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
        let items = menu_items(&kind, &available_environments(), &self.registered_accounts());
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
        self.menu = Some(ContextMenu { x, y, kind });
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
            MenuAction::NewEntryIn { parent, is_dir } => self.begin_new_entry(Some(parent), is_dir, window, cx),
            MenuAction::Reveal(path) => self.reveal_in_system(&path),
            MenuAction::OpenFile(path) => self.open_file(&path, window, cx),
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

    fn begin_rename(
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
        };
        self.settle_composition(window, cx);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) && this.rename_input.as_ref() == Some(input) {
                this.commit_rename(window, cx);
            }
        })
        .detach();
        self.rename_input = Some(input.clone());
        self.overlay = Overlay::Rename(target);
        self.remember_choice = false;
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
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
            // 입력 방식이 마지막 글자의 확정을 창이 닫힌 뒤 터미널에 뒤늦게 알려도 PTY로 가지 않게 기억한다.
            self.already_sent.note_commit_elsewhere(&typed);
            let name = typed.trim().to_string();
            match target {
                RenameTarget::Terminal(id) if !name.is_empty() => self.workspace.rename(id, &name),
                RenameTarget::Terminal(_) => {}
                RenameTarget::Folder(key) => self.workspace.work_mut().set_folder_label(&key, &name),
            }
            self.rename_input = None;
            self.overlay = Overlay::None;
            self.persist();
            self.focus_selected(window, cx);
            cx.notify();
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
        let all_exited = ids.iter().all(|id| self.is_exited(*id));
        if all_exited {
            self.close_group(root, window, cx);
        } else if !ids.is_empty() {
            self.open_overlay(Overlay::CloseGroup(root), window, cx);
        }
    }

    fn close_group(
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
        if ids.iter().all(|id| self.is_exited(*id)) {
            self.remove_folder(&key, window, cx);
        } else {
            self.open_overlay(Overlay::RemoveFolder(key), window, cx);
        }
    }

    fn remove_folder(
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
    fn files_closed_with(
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
            if exited {
                self.close_terminal(id, window, cx);
            } else {
                self.open_overlay(Overlay::CloseTerminal(id), window, cx);
            }
        }
    }

    /// 데몬에 종료를 요청하고 탭을 닫는다. 선택한 탭이었으면 가까운 탭으로 옮긴다.
    fn close_terminal(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.snapshot_active();
        if let Some(tab) = self.workspace.close(id) {
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

    pub(crate) fn open_overlay(
        &mut self,
        overlay: Overlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = overlay;
        self.menu = None;
        self.quick = QuickState::default();
        self.remember_choice = false;
        window.focus(&self.overlay_focus, cx);
        cx.notify();
    }

    pub(crate) fn cancel_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Overlay::None;
        self.rename_input = None;
        self.entry_input = None;
        self.clear_accounts();
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 묻기 창의 기본 선택(Enter)을 따른다: 터미널 닫기는 닫기, 종료 묻기는 백그라운드 유지.
    pub(crate) fn confirm_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.overlay.clone() {
            Overlay::CloseTerminal(id) => {
                self.overlay = Overlay::None;
                self.close_terminal(id, window, cx);
            }
            Overlay::CloseGroup(root) => {
                self.overlay = Overlay::None;
                self.close_group(root, window, cx);
            }
            Overlay::RemoveFolder(key) => {
                self.overlay = Overlay::None;
                self.remove_folder(&key, window, cx);
            }
            Overlay::Rename(_) => self.commit_rename(window, cx),
            Overlay::NewEntry { .. } => self.commit_new_entry(window, cx),
            Overlay::Discard(change) => self.scm_confirm_discard(&change, window, cx),
            // Accounts 창의 Enter는 입력칸이 알리는 확정 사건과 단추가 다룬다.
            Overlay::Accounts => {}
            Overlay::Quit => {
                // 앱이 닫혀도 터미널이 남지 못하는 환경에서는 기본 선택이 없다(Enter로 끝낼 수 없다).
                if let Some(choice) = default_quit_choice(self.breakaway_denied) {
                    self.choose_quit(choice, window, cx);
                }
            }
            Overlay::None => {}
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
        window.remove_window();
        cx.notify();
    }

    /// 창 닫기 요청을 처리한다. 닫아도 되면 true이고, 사용자에게 물어야 하면 묻기 창을 띄우고 false다.
    fn on_close_requested(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let close_now = self.quit_confirmed
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
            };
        if close_now {
            self.quit_confirmed = true;
        }
        close_now
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
        if self.menu.is_some() {
            if key == "escape" {
                self.close_menu(window, cx);
            }
            cx.stop_propagation();
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
        } else if matches!(self.overlay, Overlay::Rename(_) | Overlay::NewEntry { .. }) {
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
        } else if !modifiers.platform {
            let input = KeyInput {
                key,
                key_char: keystroke.key_char.as_deref(),
                control: modifiers.control,
                shift: modifiers.shift,
                alt: modifiers.alt,
            };
            if let Some(shortcut) = app_shortcut(&input) {
                self.run_shortcut(shortcut, window, cx);
                cx.stop_propagation();
            }
        }
    }

    /// 앱 단축키 하나를 실행한다. 터미널보다 먼저 받으므로 이 키들은 PTY로 가지 않는다.
    fn run_shortcut(
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
            AppShortcut::NewTerminalHere => self.new_terminal_here(window, cx),
            AppShortcut::SplitRight => self.split_selected(Orientation::Horizontal, window, cx),
            AppShortcut::SplitDown => self.split_selected(Orientation::Vertical, window, cx),
            AppShortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
            AppShortcut::CloseActiveTab => self.close_active_tab(window, cx),
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
        if let Some(last) = self.tick_gap.0 {
            self.tick_gap.1 = self.tick_gap.1.max(now - last);
        }
        self.tick_gap.0 = Some(now);
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
                    })
                    .collect(),
                selected: self.workspace.selected(),
                overlay: match self.overlay {
                    Overlay::None => "none",
                    Overlay::CloseTerminal(_) => "close-terminal",
                    Overlay::CloseGroup(_) => "close-group",
                    Overlay::RemoveFolder(_) => "remove-folder",
                    Overlay::Rename(_) => "rename",
                    Overlay::NewEntry { .. } => "new-entry",
                    Overlay::Accounts => "accounts",
                    Overlay::Discard(_) => "discard",
                    Overlay::Quit => "quit",
                },
                sound_requests: crate::automation::sound_requests(),
                view: self.workspace.view_ids(),
                menu: match self.menu.as_ref().map(|menu| &menu.kind) {
                    None => "none",
                    Some(MenuKind::Terminal(_)) => "terminal",
                    Some(MenuKind::Group(_)) => "group",
                    Some(MenuKind::Folder(_)) => "folder",
                    Some(MenuKind::Environments) => "environments",
                    Some(MenuKind::NewTerminal(_)) => "new-terminal",
                    Some(MenuKind::Entry { .. }) => "entry",
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
                files: self.files_summary(cx),
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
                Row::Folder { key, expanded, n_terminals, current } => {
                    let name = folder_name(&key, work.folder_label(&key), None);
                    format!("folder key={key} name={name} expanded={expanded} n={n_terminals} current={current}")
                }
                Row::Terminal(terminal) => {
                    format!("terminal id={} name={}{}", terminal.id, terminal.name, terminal.suffix)
                }
                Row::More { folder, n_hidden } => format!("more folder={folder} n={n_hidden}"),
            });
        }
        for group in &work.groups {
            let name = work.name_of(group.root).unwrap_or("");
            rows.push(format!(
                "group root={} name={name} n={} last_used={}",
                group.root,
                group.members.len(),
                group.last_used
            ));
        }
        rows
    }

    /// 파일 목록 요약(대본이 읽는다): 목록이 그리는 것과 같은 행에서 만든다.
    #[cfg(feature = "automation")]
    fn files_summary(
        &self,
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
        lines.extend(self.file_tab_lines(cx));
        lines.extend(self.scm_lines());
        lines.push(format!("ui-max-gap-ms={}", self.tick_gap.1.as_millis()));
        lines
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
                    "file-tab {} {} {}{}{}",
                    label(key),
                    tab.kind.name(),
                    tab.path,
                    if index == pane.selected { " selected" } else { "" },
                    if Some(key) == shown.as_ref() { " shown" } else { "" }
                ));
            }
        }
        if let Some(tab) = self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)) {
            let summary = self.file_views.get(&tab.key()).map_or_else(
                || format!("kind={} state=none", tab.kind.name()),
                |content| content.summary(tab.kind, cx),
            );
            let untracked = if self.git.is_untracked(&tab.path) { " untracked=yes" } else { "" };
            lines.push(format!(
                "file-view {summary}{untracked} external-button={}",
                if self.file_active() { "yes" } else { "no" }
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
        let folder_key = |n: Option<usize>| {
            n.and_then(|n| n.checked_sub(1))
                .and_then(|index| self.workspace.work().folders.get(index))
                .map(|folder| folder.key.clone())
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
            ["pickermenu"] => {
                let place = self.workspace.new_place();
                self.open_new_terminal_menu(place, position, true, window, cx);
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
                    menu_items(&menu.kind, &available_environments(), &self.registered_accounts())
                        .into_iter()
                        .find(|item| item.label == wanted)
                        .map(|item| item.action)
                });
                if let Some(action) = action {
                    self.run_menu_action(action, window, cx);
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
                    self.toggle_files_dir(&path, window, cx);
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
                        menu_items(&menu.kind, &available_environments(), &self.registered_accounts())
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
                    self.open_file(&path, window, cx);
                }
            }
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
            ["resetgap"] => self.tick_gap.1 = Duration::ZERO,
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
                let row = if *which == "zoom" { Row::Zoom } else { Row::FontSize };
                if let Ok(delta) = delta.parse::<i32>() {
                    self.quick_step(row, delta, window, cx);
                }
            }
            ["quick", "sound"] => self.quick_toggle_sound(window, cx),
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
            ["confirm"] => self.confirm_overlay(window, cx),
            // 종료 묻기의 "Stop all and quit" 단추와 같은 선택.
            ["quitstop"] => self.choose_quit(OnQuit::Stop, window, cx),
            ["cancel"] => self.cancel_overlay(window, cx),
            other => crate::automation::event(&format!("act: unknown action {other:?}")),
        }
    }

    fn render_notice(
        &self,
        notice: &Notice,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let (headline, detail) = match notice {
            Notice::DaemonMismatch => (
                "A different version of the terminal daemon is running".to_string(),
                "Replacing it ends the terminals it holds. Quit leaves it running untouched.".to_string(),
            ),
            Notice::DaemonStalled => (
                "The terminal daemon is not responding".to_string(),
                "Restarting it ends the terminals it holds. Quit leaves it running untouched.".to_string(),
            ),
            Notice::Failed(message) => ("Could not open a terminal".to_string(), message.clone()),
        };
        let palette = colors();
        let mut buttons = div().flex().flex_row().gap(zpx(dialog_size::BUTTON_GAP));
        match notice {
            Notice::DaemonMismatch => {
                buttons = buttons.child(button("Replace", ButtonKind::Primary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        this.connect(Connect::Open(MismatchPolicy::Replace), window, cx)
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
                        this.connect(Connect::Open(MismatchPolicy::Quit), window, cx)
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
            .children(status_text);
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
        let palette = colors();
        // 닫힌 이미지·PDF 쪽의 그래픽 메모리를 돌려준다.
        let buried = self.graveyard.drain();
        let has_buried = !buried.is_empty();
        for image in buried {
            let _ = window.drop_image(image);
        }
        // 그래픽 드라이버가 텍스처를 한 프레임 뒤에 풀기도 하므로, 반납한 뒤 한 번 더 그려 놀 때도 풀리게 한다.
        if has_buried {
            window.refresh();
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
            .capture_key_down(cx.listener(Self::on_capture_key))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| this.drag_divider(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| this.end_divider_drag(cx)),
            )
            .children(titlebar_view(window))
            .child(div().flex_1().min_h_0().child(content));
        match self.overlay.clone() {
            Overlay::None => {}
            Overlay::CloseTerminal(id) => {
                let name = self.workspace.name(id).to_string();
                let message = close_terminal_message(&name, self.files_closed_with(id));
                root = root.child(close_dialog(&message, &self.overlay_focus, cx));
            }
            Overlay::CloseGroup(group_root) => {
                let n_terminals = self.workspace.work().group_members(group_root).len();
                let n_files = self.workspace.files().n_open_in(&GroupKey::Group(group_root));
                root = root.child(close_many_dialog(
                    "Close terminals",
                    &close_group_message(n_terminals, n_files),
                    "The shells and anything running in them will end.",
                    "Close terminals",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::RemoveFolder(key) => {
                let n_terminals = self.workspace.work().terminals_of(&key).len();
                let n_files = self.workspace.files().n_open_in(&GroupKey::Folder(key.clone()));
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
                let name = folder_name(&key, self.workspace.work().folder_label(&key), home.as_deref());
                root = root.child(close_many_dialog(
                    "Remove from list",
                    &format!("Remove \"{name}\" from the list?"),
                    &remove_folder_detail(n_terminals, n_files),
                    "Remove from list",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::Rename(target) => {
                if let Some(input) = self.rename_input.clone() {
                    root = root.child(rename_dialog(&target, &input, &self.overlay_focus, cx));
                }
            }
            Overlay::NewEntry { parent, is_dir } => {
                if let Some(input) = self.entry_input.clone() {
                    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
                    let label = folder_label(&self.workspace.work().folders, &parent, home.as_deref());
                    let check = self.entry_name_check(cx);
                    let reason = match check {
                        EntryNameCheck::Invalid(reason) => Some(reason),
                        EntryNameCheck::Valid | EntryNameCheck::Empty => None,
                    };
                    root = root.child(name_dialog(
                        entry_title(is_dir, &label),
                        &input,
                        reason,
                        check == EntryNameCheck::Valid,
                        &self.overlay_focus,
                        WorkWindow::commit_new_entry,
                        cx,
                    ));
                }
            }
            Overlay::Discard(change) => {
                let prompt = crate::scm::model::discard_prompt(&change);
                root = root.child(close_many_dialog(
                    prompt.title,
                    &prompt.headline,
                    prompt.detail,
                    prompt.confirm,
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::Accounts => {
                if let Some(model) = self.accounts.model.clone() {
                    let max_height = (f32::from(window.viewport_size().height) / crate::look::zoom()
                        - paddock_core::theme::accounts_dialog::SCREEN_MARGIN
                        - dialog_size::TITLE_HEIGHT
                        - 2.0 * dialog_size::CONTENT_PADDING
                        - 70.0)
                        .max(120.0);
                    let inputs = crate::accounts::dialog::AccountInputs {
                        name: self.accounts.name_input.as_ref(),
                        rename: self.accounts.rename_input.as_ref(),
                    };
                    root = root.child(crate::accounts::dialog::accounts_dialog(
                        &model,
                        inputs,
                        &self.overlay_focus,
                        max_height,
                        cx,
                    ));
                }
            }
            Overlay::Quit => root = root.child(quit_dialog(self, &self.overlay_focus, cx)),
        }
        if let Some(menu) = self.menu.clone() {
            root = root.child(menu_panel(self, &menu, cx));
        }
        if self.quick.open {
            let viewport = window.viewport_size();
            root = root.child(quick_panel(self, (f32::from(viewport.width), f32::from(viewport.height)), cx));
        }
        root
    }
}
