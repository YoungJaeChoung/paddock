//! 앱 시작 때 창에 보일 터미널을 얻는 흐름: 데몬에 연결하고, 저장된 터미널에 다시 붙고, 없으면 새로 만든다.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::Path;
use std::time::Instant;

use paddock_core::file_pane::FilePanes;
use paddock_core::layout_model::Layout;
use paddock_core::work_model::WorkState;

use crate::app_log;
use crate::daemon_client::{DaemonConnection, Ensure, MismatchPolicy, ensure};
use crate::restore;
use crate::shell_choice::default_shell;
use crate::state_store;
use crate::terminal::session::{TerminalSession, Want, list_terminals, start_session};

/// 붙은 터미널. 이름과 소속은 `Restored::work`가 쥔다.
pub struct OpenedTerminal {
    pub session: TerminalSession,
}

/// 시작해서 얻은 터미널들. 탭 순서대로다.
pub struct Restored {
    pub terminals: Vec<OpenedTerminal>,
    /// 붙은 터미널의 작업 폴더·Unassigned 묶음 소속과 이름. 붙은 터미널이 모두 들어 있다.
    pub work: WorkState,
    /// 마지막에 선택했던 터미널의 id. 되살리지 못했으면 None이다.
    pub selected: Option<u32>,
    /// 저장된 칸 배치(분할). 되살린 터미널만 든다.
    pub layouts: Vec<Layout>,
    /// 저장된 사이드바 접힘.
    pub sidebar_collapsed: bool,
    /// 저장된 묶음별 파일 칸. 되살린 폴더·묶음의 것만 든다.
    pub files: FilePanes,
    /// 저장된 파일 목록 구획 접힘.
    pub files_collapsed: bool,
    /// 데몬이 앱의 작업 개체에서 분리되지 못했다. 앱을 닫으면 터미널도 끝난다.
    pub breakaway_denied: bool,
}

/// 시작 결과.
pub enum Startup {
    Ready(Restored),
    /// 실행 중인 데몬이 이 앱과 지문이 달라 쓰지 않았다. 데몬은 그대로 있다. 사용자가 교체할지 정한다.
    MismatchKept,
    /// 실행 중인 데몬이 인사에 답하지 않는다. 사용자가 데몬을 다시 시작할지 정한다.
    DaemonStalled,
}

/// 데몬에 연결해 창에 보일 터미널들을 얻는다.
///
/// 저장된 상태(state.json)의 터미널 중 같은 데몬에 아직 있는 것에 저장된 소속·이름으로 다시 붙는다(앱을 닫았다 다시 연 경우).
/// 데몬이 바뀌었으면(터미널 번호가 새로 매겨졌으므로) 저장된 소속을 잇지 않는다.
/// 상태에 없지만 데몬에 살아 있는 터미널도 버리지 않고 기본 이름으로 한 Unassigned 묶음에 모아 뒤에 붙인다.
/// 하나도 얻지 못하면 기본 셸로 새 묶음을 만든다. 터미널 하나에 붙지 못해도 나머지로 계속하고,
/// 데몬에 연결하지 못하면 서버가 준 문장 그대로의 오류다.
pub fn open_workspace(
    daemon_exe: &Path,
    policy: MismatchPolicy,
    started: Instant,
) -> io::Result<Startup> {
    match ensure(daemon_exe, policy)? {
        Ensure::MismatchKept { .. } => Ok(Startup::MismatchKept),
        Ensure::Stalled { .. } => Ok(Startup::DaemonStalled),
        Ensure::Connected(connection) => restore_terminals(daemon_exe, connection, started).map(Startup::Ready),
    }
}

/// 이미 연결된 데몬에서 터미널들을 되살린다.
pub fn restore_terminals(
    daemon_exe: &Path,
    connection: DaemonConnection,
    started: Instant,
) -> io::Result<Restored> {
    let breakaway_denied = connection.breakaway_denied;
    let daemon_id = connection.daemon_id.clone();
    let listed = list_terminals(&connection)?.terminals;
    let live_ids: HashSet<u32> = listed.iter().map(|terminal| terminal.id).collect();
    let mut state = state_store::load(&live_ids, &daemon_id);

    // 처음 연결은 첫 터미널에 쓰고, 나머지는 터미널마다 새 연결을 연다.
    let mut first_connection = Some(connection);
    // 데몬이 바뀌어 터미널이 사라졌으면, 옛 데몬의 기록으로 같은 자리에 터미널을 다시 연다.
    let mut revived_sessions: HashMap<u32, TerminalSession> = HashMap::new();
    let mut revived_unknown: Vec<(TerminalSession, Option<String>)> = Vec::new();
    if let Some(revival) = restore::collect(&daemon_id) {
        let mut revived = restore::start_all(&revival, || next_connection(daemon_exe, &mut first_connection), started);
        let new_ids: BTreeMap<u32, u32> = revived.known.iter().map(|(old, session)| (*old, session.id)).collect();
        state = revival.saved.renumbered(&new_ids, &daemon_id);
        revived_sessions = revived.known.into_values().map(|session| (session.id, session)).collect();
        revived_unknown = std::mem::take(&mut revived.unknown);
        app_log::event(&format!(
            "restored {} terminals from the history of the previous daemon",
            revived_sessions.len() + revived_unknown.len()
        ));
        restore::finish(revival, &revived.restored);
    }

    // 저장된 순서대로, 그다음 상태에 없는 살아 있는 터미널.
    let saved_ids = state.terminal_ids();
    let kept: HashSet<u32> = saved_ids.iter().copied().collect();
    let orphan_ids: Vec<u32> = listed
        .iter()
        .filter(|terminal| terminal.alive && !kept.contains(&terminal.id))
        .map(|terminal| terminal.id)
        .collect();

    let mut work = state.work.clone();
    let mut terminals = Vec::new();
    for id in saved_ids.iter().chain(orphan_ids.iter()).copied() {
        let session = match revived_sessions.remove(&id) {
            Some(session) => Ok(session),
            None => next_connection(daemon_exe, &mut first_connection)
                .and_then(|connection| start_session(connection, Want::Attach { id }, started)),
        };
        match session {
            Ok(session) => {
                if !kept.contains(&id) {
                    let name = work.next_terminal_name();
                    match terminals.iter().find_map(|opened: &OpenedTerminal| {
                        (!kept.contains(&opened.session.id)).then_some(opened.session.id)
                    }) {
                        Some(root) => work.join_group(root, id, &name),
                        None => work.add_group(id, &name),
                    }
                }
                terminals.push(OpenedTerminal { session });
            }
            Err(error) => {
                work.forget_terminal(id);
                app_log::event(&format!("could not reattach terminal {id}: {error}"));
            }
        }
    }
    // 상태에 없던 기록에서 되살린 터미널도 기본 이름의 한 묶음으로 모은다.
    for (session, title) in revived_unknown {
        let name = restore::revived_name(title.as_deref(), || work.next_terminal_name());
        match terminals
            .iter()
            .find_map(|opened: &OpenedTerminal| (!kept.contains(&opened.session.id)).then_some(opened.session.id))
        {
            Some(root) => work.join_group(root, session.id, &name),
            None => work.add_group(session.id, &name),
        }
        terminals.push(OpenedTerminal { session });
    }
    if terminals.is_empty() {
        let connection = next_connection(daemon_exe, &mut first_connection)?;
        let session = start_session(connection, new_terminal_want(), started)?;
        let name = work.next_terminal_name();
        work.add_group(session.id, &name);
        terminals.push(OpenedTerminal { session });
    }
    let selected = state.selected.filter(|id| terminals.iter().any(|terminal| terminal.session.id == *id));
    // 다시 붙지 못한 터미널이 빠지며 묶음의 시작 터미널이 바뀌었을 수 있다. 파일 칸은 묶음을 따라간다.
    let mut files = state.files.clone();
    files.follow_groups(&state.work, &work);
    Ok(Restored {
        terminals,
        work,
        selected,
        layouts: state.layouts,
        sidebar_collapsed: state.sidebar_collapsed,
        files,
        files_collapsed: state.files_collapsed,
        breakaway_denied,
    })
}

/// 쓸 연결 하나: 남겨 둔 처음 연결이 있으면 그것, 없으면 데몬에 새로 연결한다.
fn next_connection(
    daemon_exe: &Path,
    spare: &mut Option<DaemonConnection>,
) -> io::Result<DaemonConnection> {
    match spare.take() {
        Some(connection) => Ok(connection),
        None => connect_daemon(daemon_exe),
    }
}

/// 이미 떠 있는 데몬에 새 연결을 연다. 데몬을 띄우거나 교체하지는 않는다.
pub fn connect_daemon(daemon_exe: &Path) -> io::Result<DaemonConnection> {
    match ensure(daemon_exe, MismatchPolicy::Quit)? {
        Ensure::Connected(connection) => Ok(connection),
        Ensure::MismatchKept { .. } => Err(io::Error::other("A different version of the terminal daemon is running")),
        Ensure::Stalled { .. } => Err(io::Error::new(io::ErrorKind::TimedOut, "Daemon did not answer")),
    }
}

/// 기본 셸로 새 터미널을 여는 요청. 시작 폴더는 데몬의 기본 폴더(홈)다.
pub fn new_terminal_want() -> Want {
    let choice = default_shell();
    Want::New { shell: choice.shell.to_string(), cwd: None, wsl_distribution: choice.wsl_distribution.clone() }
}

/// 요청대로 새 터미널을 만들어 붙는다(Work 목록의 ＋·환경 고르기·폴더 행 ＋·탭 줄 ＋).
pub fn open_new_terminal(
    daemon_exe: &Path,
    want: Want,
) -> io::Result<TerminalSession> {
    start_session(connect_daemon(daemon_exe)?, want, Instant::now())
}

#[cfg(test)]
mod tests {
    use paddock_core::file_kind::FileKind;
    use paddock_core::file_pane::FilePanes;
    use paddock_core::session_state::SessionState;
    use paddock_core::work_model::GroupKey;

    use super::*;
    use crate::test_support::{DaemonCleanup, TempHome, daemon_exe};

    #[cfg(unix)]
    const SHELL: &str = "bash";
    #[cfg(windows)]
    const SHELL: &str = "cmd";

    fn open_shell(exe: &Path) -> TerminalSession {
        let want = Want::New { shell: SHELL.to_string(), cwd: None, wsl_distribution: None };
        start_session(connect_daemon(exe).unwrap(), want, Instant::now()).unwrap()
    }

    fn ready(startup: Startup) -> Restored {
        match startup {
            Startup::Ready(restored) => restored,
            Startup::MismatchKept | Startup::DaemonStalled => panic!("expected terminals"),
        }
    }

    /// 시험 데몬의 식별 글자.
    fn daemon_id() -> String {
        paddock_protocol::daemon_info::read_info().expect("daemon is running").identity()
    }

    fn open(exe: &Path) -> Restored {
        ready(open_workspace(exe, MismatchPolicy::Quit, Instant::now()).unwrap())
    }

    #[test]
    fn test_fresh_start_opens_one_terminal_named_terminal_1() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let restored = open(&daemon_exe());
        assert_eq!(restored.terminals.len(), 1);
        let id = restored.terminals[0].session.id;
        assert_eq!(restored.work.name_of(id), Some("terminal 1"));
        assert_eq!(restored.work.group_of(id), Some(id), "the first terminal starts an Unassigned group");
        assert_eq!(restored.selected, None);
        assert!(!restored.breakaway_denied);
    }

    #[test]
    fn test_saved_state_brings_back_the_same_terminals_in_order_with_membership_and_selection() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = open_shell(&exe);
        let second = open_shell(&exe);
        let mut work = WorkState::default();
        work.assign_terminal(second.id, "/tmp/project", "build");
        work.add_group(first.id, "terminal 1");
        let layouts = vec![Layout::pane(vec![second.id], 0)];
        let mut files = FilePanes::default();
        files.open(&GroupKey::Folder("/tmp/project".into()), "/tmp/project/a.rs", FileKind::Text);
        files.open(&GroupKey::Group(first.id), "/tmp/loose.txt", FileKind::Text);
        let state = SessionState {
            daemon: daemon_id(),
            work: work.clone(),
            selected: Some(first.id),
            layouts: layouts.clone(),
            sidebar_collapsed: true,
            files: files.clone(),
            files_collapsed: true,
        };
        state_store::save(&state).unwrap();
        // 앱이 닫혔다 다시 열린 것처럼 새 연결로 되살린다(셸은 그대로 살아 있다).
        let restored = open(&exe);
        let ids: Vec<u32> = restored.terminals.iter().map(|terminal| terminal.session.id).collect();
        assert_eq!(ids, vec![second.id, first.id], "folder terminals come first, then the Unassigned groups");
        assert_eq!(restored.work, work);
        assert_eq!(restored.terminals[0].session.shell_pid, second.shell_pid);
        assert_eq!(restored.terminals[1].session.shell_pid, first.shell_pid);
        assert_eq!(restored.selected, Some(first.id));
        assert_eq!(restored.layouts, layouts);
        assert!(restored.sidebar_collapsed);
        assert_eq!((restored.files, restored.files_collapsed), (files, true));
    }

    #[test]
    fn test_live_terminals_missing_from_state_are_adopted_into_one_group_and_unknown_saved_ids_dropped() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = open_shell(&exe);
        let second = open_shell(&exe);
        let third = open_shell(&exe);
        let mut work = WorkState::default();
        work.add_group(999, "gone");
        work.add_group(second.id, "terminal 1");
        let state = SessionState { daemon: daemon_id(), work, selected: Some(999), ..SessionState::default() };
        state_store::save(&state).unwrap();
        let restored = open(&exe);
        let ids: Vec<u32> = restored.terminals.iter().map(|terminal| terminal.session.id).collect();
        // 저장된 터미널이 먼저이고, 상태에 없던 터미널이 번호가 겹치지 않는 기본 이름으로 한 묶음이 되어 뒤에 붙는다.
        assert_eq!(ids, vec![second.id, first.id, third.id]);
        assert_eq!(restored.work.name_of(first.id), Some("terminal 2"));
        assert_eq!(restored.work.name_of(third.id), Some("terminal 3"));
        assert_eq!(restored.work.group_members(first.id), vec![first.id, third.id]);
        assert_eq!(restored.work.group_of(second.id), Some(second.id));
        assert_eq!(restored.selected, None);
    }

    #[test]
    fn test_state_saved_for_another_daemon_is_not_matched_to_the_new_daemons_terminals() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = open_shell(&exe);
        // 죽은 데몬이 남긴 상태: 번호가 우연히 지금 터미널과 같아도 이름·선택을 잇지 않는다.
        let mut work = WorkState::default();
        work.assign_terminal(first.id, "/tmp/project", "build");
        let state = SessionState {
            daemon: "1@an-older-daemon".into(),
            work,
            selected: Some(first.id),
            layouts: vec![Layout::pane(vec![first.id], 0)],
            sidebar_collapsed: true,
            ..SessionState::default()
        };
        state_store::save(&state).unwrap();
        let restored = open(&exe);
        assert_eq!(restored.terminals.len(), 1);
        assert_eq!(restored.work.name_of(first.id), Some("terminal 1"));
        // 작업 폴더 목록은 데몬과 무관해 남지만 터미널 소속은 잇지 않는다.
        assert_eq!(restored.work.folders.len(), 1);
        assert_eq!(restored.work.folder_of(first.id), None);
        assert_eq!(restored.terminals[0].session.shell_pid, first.shell_pid);
        assert_eq!(restored.selected, None);
        assert!(restored.layouts.is_empty());
        assert!(restored.sidebar_collapsed, "the sidebar collapse does not depend on the daemon");
    }

    /// `grid`의 화면에 `predicate`를 만족하는 줄이 나타날 때까지 기다린다.
    fn wait_for_screen(
        session: &TerminalSession,
        what: &str,
        predicate: impl Fn(&str) -> bool,
    ) {
        let end = Instant::now() + std::time::Duration::from_secs(20);
        while !session.grid.visible_text().iter().any(|line| predicate(line)) {
            assert!(Instant::now() < end, "{what} never appeared: {:#?}", session.grid.visible_text());
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    /// 데몬을 종료 요청으로 끝내고(`daemon.json`이 사라질 때까지) 기다린다. 터미널 기록은 남는다.
    fn shut_down_daemon(session: &TerminalSession) {
        crate::daemon_client::request_shutdown(&session.outbox, std::time::Duration::from_secs(2));
        let end = Instant::now() + std::time::Duration::from_secs(10);
        while paddock_protocol::daemon_info::read_info().is_some_and(|info| info.is_running()) {
            assert!(Instant::now() < end, "the daemon did not exit");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn folder_state(
        daemon: String,
        sessions: &[(&TerminalSession, &str, &str)],
    ) -> SessionState {
        let mut work = WorkState::default();
        for (session, folder, name) in sessions {
            work.assign_terminal(session.id, folder, name);
        }
        SessionState {
            daemon,
            work,
            selected: sessions.first().map(|(session, ..)| session.id),
            ..SessionState::default()
        }
    }

    /// 입력한 명령줄에는 나타나지 않고 실행 결과로만 나타나는 `BACKMARK` 줄을 찍는 명령.
    #[cfg(unix)]
    const ECHO_BACK: &str = "echo BACK''MARK\n";
    #[cfg(windows)]
    const ECHO_BACK: &str = "echo BACK^MARK\r\n";

    /// 셸의 현재 폴더를 줄로 찍는 명령.
    #[cfg(unix)]
    const PRINT_FOLDER: &str = "pwd\n";
    #[cfg(windows)]
    const PRINT_FOLDER: &str = "cd\r\n";

    #[test]
    fn test_c_d40_after_the_daemon_is_replaced_terminals_come_back_in_their_folder_with_name_screen_and_notice() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let folder = home.dir.path().join("proj dir");
        std::fs::create_dir_all(&folder).unwrap();
        let folder = folder.to_string_lossy().into_owned();
        let first = start_session(
            connect_daemon(&exe).unwrap(),
            Want::New { shell: SHELL.to_string(), cwd: Some(folder.clone()), wsl_distribution: None },
            Instant::now(),
        )
        .unwrap();
        first.grid.input(ECHO_BACK.as_bytes().to_vec());
        wait_for_screen(&first, "the marker", |line| line == "BACKMARK");
        state_store::save(&folder_state(daemon_id(), &[(&first, &folder, "build")])).unwrap();
        shut_down_daemon(&first);
        // 앱을 다시 연다: 새 데몬이 뜨고, 같은 폴더·이름의 터미널이 이전 화면과 안내 줄을 달고 돌아온다.
        let restored = open(&exe);
        assert_eq!(restored.terminals.len(), 1);
        let back = &restored.terminals[0].session;
        assert_ne!(back.daemon, "", "attached to the new daemon");
        assert_eq!(restored.work.name_of(back.id), Some("build"));
        assert_eq!(
            restored.work.folder_of(back.id).map(str::to_string),
            Some(paddock_core::work_model::folder_key(&folder))
        );
        assert_eq!(restored.selected, Some(back.id));
        wait_for_screen(back, "the old screen", |line| line == "BACKMARK");
        wait_for_screen(back, "the notice", |line| line.contains("Previous shell ended. Screen restored"));
        back.grid.input(PRINT_FOLDER.as_bytes().to_vec());
        wait_for_screen(back, "the restored folder", |line| paddock_core::work_model::same_folder(line, &folder));
        // 옛 데몬의 기록 폴더는 되살린 뒤 지워진다.
        let old = paddock_protocol::shared_config::history_dir(&daemon_id_of_state());
        assert!(!old.exists(), "{old:?}");
    }

    /// state.json에 적힌(되살리기 전의) 데몬 식별 글자.
    fn daemon_id_of_state() -> String {
        SessionState::restore_saved(
            &std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("state.json")).unwrap(),
        )
        .daemon
    }

    #[test]
    fn test_c_d40_a_terminal_closed_with_x_does_not_come_back() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let kept = open_shell(&exe);
        let closed = open_shell(&exe);
        state_store::save(&folder_state(daemon_id(), &[(&kept, "/tmp", "kept"), (&closed, "/tmp", "closed")])).unwrap();
        closed
            .outbox
            .send(&paddock_protocol::frame::Frame::json(
                paddock_protocol::message::kind::KILL,
                &paddock_protocol::message::Kill { id: closed.id },
            ))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        shut_down_daemon(&kept);
        let restored = open(&exe);
        let names: Vec<&str> =
            restored.terminals.iter().map(|opened| restored.work.name_of(opened.session.id).unwrap_or("")).collect();
        assert_eq!(names, vec!["kept"]);
    }

    #[test]
    fn test_c_d40_with_restore_turned_off_nothing_comes_back() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = open_shell(&exe);
        state_store::save(&folder_state(daemon_id(), &[(&first, "/tmp", "build")])).unwrap();
        let settings = home.dir.path().join(".paddock").join("native").join("settings.json");
        std::fs::write(&settings, r#"{ "paddock.terminal.restoreOnRestart": "never" }"#).unwrap();
        shut_down_daemon(&first);
        let restored = open(&exe);
        assert_eq!(restored.terminals.len(), 1);
        assert_eq!(
            restored.work.name_of(restored.terminals[0].session.id),
            Some("terminal 1"),
            "a fresh terminal, not the old one"
        );
    }
}
