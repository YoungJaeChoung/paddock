//! 터미널 한 개의 데몬 세션: 터미널을 새로 만들거나 이미 있는 터미널에 붙고, 데몬이 보내는 출력·재생을 격자에 먹인다.
use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::Processor;
use paddock_core::shared_environment::shared_environment;
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Attach, Attached, Launch, List, New, Resize, TerminalState, kind};

use crate::app_log;
use crate::daemon_client::{DaemonConnection, Inbox, Outbox};
use crate::terminal::grid::Grid;

/// 새 터미널의 처음 크기. 첫 그리기에서 창 크기에 맞춰 바뀐다.
mod initial {
    pub const COLS: u16 = 120;
    pub const ROWS: u16 = 30;
}

/// 붙기·목록 요청의 응답을 기다리는 최대 시간.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// 터미널을 어떻게 얻을 것인가.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Want {
    /// 셸을 새로 띄운다. `cwd`가 없으면 데몬의 기본 폴더에서 시작한다. `wsl_distribution`은 `shell`이 `wsl`일 때 쓸 배포판이다.
    New { shell: String, cwd: Option<String>, wsl_distribution: Option<String> },
    /// 셸 대신 `launch`의 프로그램을 띄운다(계정 터미널). `shell`·`wsl_distribution`은 데몬이 목록과 WSL 조사에 쓰는 이름이다.
    Launch { shell: String, wsl_distribution: Option<String>, launch: Box<Launch> },
    /// 데몬에 이미 있는 터미널에 다시 붙는다(데몬이 가진 크기를 그대로 쓴다).
    Attach { id: u32 },
}

impl Want {
    /// 새 탭에 붙일 이름. 계정 터미널은 `Claude · Main`처럼 자기 이름을 갖고, 나머지는 None이라 `terminal N`이 붙는다.
    pub fn title(&self) -> Option<&str> {
        match self {
            Want::Launch { launch, .. } => launch.title.as_deref(),
            Want::New { .. } | Want::Attach { .. } => None,
        }
    }
}

/// 붙은 터미널 하나.
pub struct TerminalSession {
    pub id: u32,
    /// 이 터미널을 쥔 데몬의 식별 글자. `id`는 데몬마다 1부터 매겨지므로 이 글자와 함께 터미널을 가리킨다.
    pub daemon: String,
    pub shell_pid: u32,
    pub grid: Arc<Grid>,
    /// 이 터미널에 묶인 연결로 데몬에 보내는 통로. 터미널을 끝내는 요청(`KILL`)도 이 연결로 보낸다.
    pub outbox: Outbox,
}

/// 데몬이 쥔 터미널 전체의 목록(살아 있는 것과 셸이 끝난 것). 서버가 거부하면 그 문장이 그대로 오류다.
pub fn list_terminals(connection: &DaemonConnection) -> io::Result<List> {
    connection.send(&Frame::new(kind::LIST, vec![]))?;
    let reply = connection
        .wait_for_any(&[kind::LIST_REPLY], REPLY_TIMEOUT)
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Daemon did not answer"))?;
    reply.parse_json()
}

/// 이 앱 프로세스의 환경 변수 중 새 터미널에 넘길 것. 유니코드가 아닌 이름·값은 건너뛴다.
///
/// 데몬은 앱보다 오래 살아 PATH가 낡을 수 있어, 새 터미널을 열 때마다 앱의 현재 값을 실어 보낸다.
fn app_environment() -> Vec<(String, String)> {
    shared_environment(
        std::env::vars_os().filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?))),
    )
}

/// 연결을 터미널에 묶고, 데몬이 보내는 프레임을 격자에 먹이는 스레드를 시작한다.
///
/// 데몬이 거부하면(없는 터미널, 셸 실행 실패 등) 그 문장이 그대로 오류다.
/// `connected_at`은 재접속 시간(연결부터 재생 끝까지)을 기록 파일에 남기는 기준이다.
pub fn start_session(
    connection: DaemonConnection,
    want: Want,
    connected_at: Instant,
) -> io::Result<TerminalSession> {
    start_session_with_screen(connection, want, None, connected_at)
}

/// `start_session`과 같되, `screen`이 있으면 새 터미널의 첫 화면으로 데몬 기록에 먼저 넣는다(되살린 터미널의 이전 화면과 안내 줄).
pub fn start_session_with_screen(
    connection: DaemonConnection,
    want: Want,
    screen: Option<String>,
    connected_at: Instant,
) -> io::Result<TerminalSession> {
    let request = match want {
        Want::New { shell, cwd, wsl_distribution } => Frame::json(
            kind::NEW,
            &New {
                shell,
                cwd,
                cols: initial::COLS,
                rows: initial::ROWS,
                wsl_distribution,
                launch: None,
                preload: screen,
                env: app_environment(),
            },
        ),
        Want::Launch { shell, wsl_distribution, launch } => Frame::json(
            kind::NEW,
            &New {
                shell,
                cwd: None,
                cols: initial::COLS,
                rows: initial::ROWS,
                wsl_distribution,
                launch: Some(*launch),
                preload: screen,
                env: app_environment(),
            },
        ),
        Want::Attach { id } => Frame::json(kind::ATTACH, &Attach { id, cols: 0, rows: 0 }),
    };
    connection.send(&request)?;
    let reply = connection
        .wait_for_any(&[kind::ATTACHED, kind::ERR], REPLY_TIMEOUT)
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Daemon did not answer"))?;
    if reply.kind == kind::ERR {
        Err(io::Error::other(reply.as_text()))
    } else {
        let attached: Attached = reply.parse_json()?;
        app_log::event(&format!(
            "attached terminal {} (shell pid {}, {}x{}) after {} ms",
            attached.id,
            attached.pid,
            attached.cols,
            attached.rows,
            connected_at.elapsed().as_millis()
        ));
        let daemon = connection.daemon_id.clone();
        let input_outbox = connection.outbox.clone();
        let resize_outbox = connection.outbox.clone();
        let grid = Arc::new(Grid::new(
            attached.cols,
            attached.rows,
            move |bytes| {
                let _ = input_outbox.send(&Frame::new(kind::INPUT, bytes));
            },
            move |cols, rows| {
                let _ = resize_outbox.send(&Frame::json(kind::RESIZE, &Resize { cols, rows }));
            },
        ));
        // 재생이 끝나기 전에는 격자가 만드는 응답을 셸에 쓰지 않는다.
        grid.shared.replaying.store(true, Ordering::Relaxed);
        let loop_grid = grid.clone();
        let frames = connection.frames;
        // 보내는 통로(`outbox`)는 세션과 격자의 입력·크기 함수가 쥐고 있어 연결이 유지된다.
        thread::spawn(move || run_receive_loop(frames, &loop_grid, connected_at));
        Ok(TerminalSession { id: attached.id, daemon, shell_pid: attached.pid, grid, outbox: connection.outbox })
    }
}

/// 데몬이 보낸 재생·출력·종료 프레임을 격자에 먹인다. 연결이 끊겨 프레임이 더 오지 않으면 끝난다.
///
/// `RESYNC`는 읽는 속도가 느려 출력 일부가 건너뛰어졌다는 뜻이다. 그때까지 쌓은 화면과 파서 상태를 버리고
/// 이어지는 재생으로 화면을 처음부터 다시 만든다.
pub fn run_receive_loop(
    frames: Inbox,
    grid: &Grid,
    connected_at: Instant,
) {
    let mut parser: Processor = Processor::new();
    let mut n_replay_bytes = 0usize;
    while let Ok(frame) = frames.recv() {
        match frame.kind {
            kind::REPLAY => {
                *grid.shared.last_output_at.lock().unwrap() = Some(Instant::now());
                n_replay_bytes += frame.payload.len();
                parser.advance(&mut *grid.term.lock(), &frame.payload);
            }
            kind::REPLAY_END => {
                grid.shared.replaying.store(false, Ordering::Relaxed);
                let on_alt_screen = grid.term.lock().mode().contains(TermMode::ALT_SCREEN);
                grid.shared.replay_notice.store(on_alt_screen, Ordering::Relaxed);
                grid.shared.dirty.store(true, Ordering::Relaxed);
                app_log::event(&format!(
                    "replay applied: {n_replay_bytes} bytes, connect to replay end {} ms",
                    connected_at.elapsed().as_millis()
                ));
            }
            kind::RESYNC => {
                app_log::event("resync: rebuilding the screen from a full replay");
                parser = Processor::new();
                n_replay_bytes = 0;
                grid.shared.replaying.store(true, Ordering::Relaxed);
                grid.reset_screen();
            }
            kind::OUTPUT => {
                *grid.shared.last_output_at.lock().unwrap() = Some(Instant::now());
                parser.advance(&mut *grid.term.lock(), &frame.payload);
                grid.answer_color_requests();
                grid.shared.dirty.store(true, Ordering::Relaxed);
            }
            kind::DETACHED => {
                grid.shared.detached.store(true, Ordering::Relaxed);
                grid.shared.dirty.store(true, Ordering::Relaxed);
                app_log::event("terminal taken over by another connection");
            }
            kind::TERMINAL_STATE => {
                if let Ok(state) = frame.parse_json::<TerminalState>() {
                    record_state(grid, state);
                }
            }
            kind::EXITED => {
                let code = frame.as_text();
                app_log::event(&format!("terminal exited: {code}"));
                *grid.shared.exit_code.lock().unwrap() = Some(code);
                grid.shared.exited.store(true, Ordering::Relaxed);
                grid.shared.dirty.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    grid.shared.disconnected.store(true, Ordering::Relaxed);
    grid.shared.dirty.store(true, Ordering::Relaxed);
    app_log::event("daemon connection closed");
}

/// 데몬이 알린 상태를 격자 옆에 보관한다. 값이 달라졌을 때만 버전을 올리고, 완료 알림은 사건으로 센다.
fn record_state(
    grid: &Grid,
    state: TerminalState,
) {
    let finished = state.finished;
    let state = TerminalState { finished: false, ..state };
    let mut stored = grid.shared.state.lock().unwrap();
    if stored.as_ref() != Some(&state) {
        *stored = Some(state);
        grid.shared.state_version.fetch_add(1, Ordering::Relaxed);
    }
    if finished {
        grid.shared.finished_pending.fetch_add(1, Ordering::Relaxed);
        grid.shared.state_version.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use paddock_protocol::frame::Frame;
    use paddock_protocol::message::kind;

    use super::*;
    use crate::daemon_client::{Ensure, MismatchPolicy, ensure, inbox_channel};
    use crate::test_support::{DaemonCleanup, TempHome, daemon_exe};

    #[cfg(unix)]
    const SHELL: &str = "bash";
    #[cfg(windows)]
    const SHELL: &str = "cmd";

    /// 입력한 명령줄에는 나타나지 않고 실행 결과로만 나타나는 줄(한글 포함).
    #[cfg(unix)]
    const ECHO_MARKER: &str = "echo 한''글-MARKER\n";
    #[cfg(windows)]
    const ECHO_MARKER: &str = "echo 한^글-MARKER\r\n";
    const MARKER_LINE: &str = "한글-MARKER";

    /// 종료 코드 3으로 셸을 끝내는 명령.
    #[cfg(unix)]
    const EXIT_COMMAND: &str = "exit 3\n";
    #[cfg(windows)]
    const EXIT_COMMAND: &str = "exit 3\r\n";

    fn grid_with_counter() -> (Arc<Grid>, Arc<AtomicUsize>) {
        let n_inputs = Arc::new(AtomicUsize::new(0));
        let counter = n_inputs.clone();
        let grid = Arc::new(Grid::new(
            80,
            24,
            move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            },
            |_, _| {},
        ));
        (grid, n_inputs)
    }

    fn run_loop_over(
        grid: &Grid,
        frames: Vec<Frame>,
    ) {
        let (sender, inbox) = inbox_channel(usize::MAX);
        for frame in frames {
            assert!(sender.push(frame));
        }
        drop(sender);
        run_receive_loop(inbox, grid, Instant::now());
    }

    fn state_frame(
        program: &str,
        activity: Option<paddock_core::agent_model::ActivityState>,
        finished: bool,
    ) -> Frame {
        let state = TerminalState {
            id: 1,
            cwd: Some("/work".to_string()),
            program: Some(program.to_string()),
            is_agent: activity.is_some(),
            activity,
            finished,
            account: None,
        };
        Frame::json(kind::TERMINAL_STATE, &state)
    }

    #[test]
    fn test_c_d37_unknown_frame_kind_is_ignored() {
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        // 앞으로 더해질 알림 종류가 와도 세션은 계속 일한다.
        run_loop_over(
            &grid,
            vec![Frame::new(250, b"future".to_vec()), Frame::new(kind::OUTPUT, b"still alive".to_vec())],
        );
        assert_eq!(grid.visible_text()[0], "still alive");
    }

    #[test]
    fn test_c_d37_state_frames_update_the_stored_state_and_only_changes_bump_the_version() {
        use paddock_core::agent_model::ActivityState;
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        assert!(grid.shared.state.lock().unwrap().is_none());
        run_loop_over(
            &grid,
            vec![
                state_frame("claude", Some(ActivityState::Working), false),
                state_frame("claude", Some(ActivityState::Working), false),
            ],
        );
        let stored = grid.shared.state.lock().unwrap().clone().expect("a state");
        assert_eq!((stored.program.as_deref(), stored.activity), (Some("claude"), Some(ActivityState::Working)));
        assert_eq!(grid.shared.state_version.load(Ordering::Relaxed), 1, "the repeated value is not a change");
        assert_eq!(grid.shared.finished_pending.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_c_d37_a_finished_frame_counts_one_event_and_is_not_kept_as_state() {
        use paddock_core::agent_model::ActivityState;
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        run_loop_over(&grid, vec![state_frame("claude", Some(ActivityState::Quiet), true)]);
        assert_eq!(grid.shared.finished_pending.swap(0, Ordering::Relaxed), 1);
        assert!(!grid.shared.state.lock().unwrap().as_ref().expect("a state").finished);
    }

    #[test]
    fn test_c_d3c_alt_screen_sets_replay_notice() {
        // 받는 루프가 앱 기록 파일에 쓰므로 임시 홈 폴더 안에서 돌린다.
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        grid.shared.replaying.store(true, Ordering::Relaxed);
        run_loop_over(
            &grid,
            vec![Frame::new(kind::REPLAY, b"\x1b[?1049h".to_vec()), Frame::new(kind::REPLAY_END, vec![])],
        );
        assert!(grid.shared.replay_notice.load(Ordering::Relaxed));
        assert!(!grid.shared.replaying.load(Ordering::Relaxed));
        // 사용자가 입력하면 안내가 사라진다.
        grid.input(b"x".to_vec());
        assert!(!grid.shared.replay_notice.load(Ordering::Relaxed));
    }

    #[test]
    fn test_replay_without_alt_screen_sets_no_notice() {
        // 받는 루프가 앱 기록 파일에 쓰므로 임시 홈 폴더 안에서 돌린다.
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        grid.shared.replaying.store(true, Ordering::Relaxed);
        run_loop_over(&grid, vec![Frame::new(kind::REPLAY, b"hello".to_vec()), Frame::new(kind::REPLAY_END, vec![])]);
        assert!(!grid.shared.replay_notice.load(Ordering::Relaxed));
        assert_eq!(grid.visible_text()[0], "hello");
    }

    #[test]
    fn test_replay_never_answers_terminal_queries() {
        // 받는 루프가 앱 기록 파일에 쓰므로 임시 홈 폴더 안에서 돌린다.
        let _home = TempHome::new();
        // 커서 위치 질의(ESC [ 6 n)에 격자가 답을 만든다. 재생 중의 질의는 지난 일이라 셸에 답하면 입력이 오염된다.
        let (grid, n_inputs) = grid_with_counter();
        grid.shared.replaying.store(true, Ordering::Relaxed);
        run_loop_over(
            &grid,
            vec![
                Frame::new(kind::REPLAY, b"\x1b[6n".to_vec()),
                Frame::new(kind::REPLAY_END, vec![]),
                Frame::new(kind::OUTPUT, b"\x1b[6n".to_vec()),
            ],
        );
        assert_eq!(n_inputs.load(Ordering::Relaxed), 1, "only the live query is answered");
    }

    #[test]
    fn test_exit_and_disconnect_are_recorded() {
        // 받는 루프가 앱 기록 파일에 쓰므로 임시 홈 폴더 안에서 돌린다.
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        run_loop_over(&grid, vec![Frame::text(kind::EXITED, "0")]);
        assert!(grid.shared.exited.load(Ordering::Relaxed));
        assert!(grid.shared.disconnected.load(Ordering::Relaxed));
    }

    fn wait_for_line(
        grid: &Grid,
        line: &str,
    ) {
        let end = Instant::now() + Duration::from_secs(15);
        while !grid.visible_text().iter().any(|text| text == line) {
            assert!(Instant::now() < end, "line {line:?} never appeared: {:#?}", grid.visible_text());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn connect(exe: &std::path::Path) -> crate::daemon_client::DaemonConnection {
        match ensure(exe, MismatchPolicy::Quit).unwrap() {
            Ensure::Connected(connection) => connection,
            Ensure::MismatchKept { .. } | Ensure::Stalled { .. } => panic!("expected a connection"),
        }
    }

    #[test]
    fn test_new_terminal_then_reattach_restores_the_same_screen() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();

        let first = start_session(
            connect(&exe),
            Want::New { shell: SHELL.to_string(), cwd: None, wsl_distribution: None },
            Instant::now(),
        )
        .unwrap();
        first.grid.input(ECHO_MARKER.as_bytes().to_vec());
        wait_for_line(&first.grid, MARKER_LINE);
        // 셸이 프롬프트를 다시 그릴 시간을 준다.
        std::thread::sleep(Duration::from_millis(500));
        let before = first.grid.visible_text();

        // 앱이 다시 실행된 것처럼 새 연결로 같은 터미널에 붙는다.
        let again = start_session(connect(&exe), Want::Attach { id: first.id }, Instant::now()).unwrap();
        wait_for_line(&again.grid, MARKER_LINE);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(again.id, first.id);
        assert_eq!(again.shell_pid, first.shell_pid);
        assert_eq!(again.grid.visible_text(), before);
    }

    #[test]
    fn test_closed_tab_leaves_no_connection_or_grid_behind() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let session = start_session(
            connect(&daemon_exe()),
            Want::New { shell: SHELL.to_string(), cwd: None, wsl_distribution: None },
            Instant::now(),
        )
        .unwrap();
        let grid = Arc::downgrade(&session.grid);
        // 탭을 닫을 때와 같이 터미널을 끝내는 요청을 보낸다. 데몬이 연결을 닫으면 받는 스레드가 끊김으로 끝난다.
        session.outbox.send(&Frame::json(kind::KILL, &paddock_protocol::message::Kill { id: session.id })).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while !session.grid.shared.disconnected.load(Ordering::Relaxed) {
            assert!(Instant::now() < end, "the daemon never closed the connection of the killed terminal");
            std::thread::sleep(Duration::from_millis(20));
        }
        // 탭이 사라지면(세션을 버리면) 받는 스레드가 쥐던 격자와 쓰기 스레드까지 모두 풀려야 한다.
        drop(session);
        while grid.upgrade().is_some() {
            assert!(Instant::now() < end, "the grid of a closed tab is still alive");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn test_attach_to_missing_terminal_reports_server_message() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let error = start_session(connect(&daemon_exe()), Want::Attach { id: 99 }, Instant::now())
            .err()
            .expect("no such terminal");
        assert_eq!(error.to_string(), "No such terminal");
    }

    #[test]
    fn test_list_terminals_lists_existing_terminals() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        assert!(list_terminals(&connect(&exe)).unwrap().terminals.is_empty());
        let session = start_session(
            connect(&exe),
            Want::New { shell: SHELL.to_string(), cwd: None, wsl_distribution: None },
            Instant::now(),
        )
        .unwrap();
        let ids: Vec<u32> =
            list_terminals(&connect(&exe)).unwrap().terminals.iter().map(|terminal| terminal.id).collect();
        assert_eq!(ids, vec![session.id]);
    }

    #[test]
    fn test_resync_discards_the_screen_and_rebuilds_it_from_the_replay() {
        let _home = TempHome::new();
        let (grid, n_inputs) = grid_with_counter();
        grid.shared.replaying.store(true, Ordering::Relaxed);
        run_loop_over(
            &grid,
            vec![
                Frame::new(kind::REPLAY, b"old screen".to_vec()),
                Frame::new(kind::REPLAY_END, vec![]),
                Frame::new(kind::OUTPUT, b" more".to_vec()),
                // 읽는 속도가 느려 출력을 건너뛴 연결은 화면 전체를 다시 받는다. 질의 응답도 재생 중이라 막힌다.
                Frame::new(kind::RESYNC, vec![]),
                Frame::new(kind::REPLAY, b"new\x1b[6n".to_vec()),
                Frame::new(kind::REPLAY_END, vec![]),
            ],
        );
        assert_eq!(grid.visible_text()[0], "new");
        assert!(grid.visible_text()[1..].iter().all(String::is_empty));
        assert!(!grid.shared.replaying.load(Ordering::Relaxed));
        assert_eq!(n_inputs.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_detached_marks_the_terminal_as_taken_by_another_window() {
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        run_loop_over(&grid, vec![Frame::new(kind::DETACHED, vec![])]);
        assert!(grid.shared.detached.load(Ordering::Relaxed));
    }

    #[test]
    fn test_exited_keeps_the_exit_code_text() {
        let _home = TempHome::new();
        let (grid, _) = grid_with_counter();
        run_loop_over(&grid, vec![Frame::text(kind::EXITED, "7")]);
        assert_eq!(grid.shared.exit_code.lock().unwrap().as_deref(), Some("7"));
        let (grid, _) = grid_with_counter();
        run_loop_over(&grid, vec![Frame::text(kind::EXITED, "unknown")]);
        assert_eq!(grid.shared.exit_code.lock().unwrap().as_deref(), Some("unknown"));
    }

    #[test]
    fn test_list_terminals_reports_exit_code_of_ended_shell() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let session = start_session(
            connect(&exe),
            Want::New { shell: SHELL.to_string(), cwd: None, wsl_distribution: None },
            Instant::now(),
        )
        .unwrap();
        session.grid.input(EXIT_COMMAND.as_bytes().to_vec());
        let end = Instant::now() + Duration::from_secs(15);
        while !session.grid.shared.exited.load(Ordering::Relaxed) {
            assert!(Instant::now() < end, "shell never exited");
            std::thread::sleep(Duration::from_millis(20));
        }
        let listed = list_terminals(&connect(&exe)).unwrap();
        let info =
            listed.terminals.iter().find(|terminal| terminal.id == session.id).expect("exited terminal is listed");
        assert!(!info.alive);
        assert_eq!(info.exit_code, Some(3));
        assert_eq!(session.grid.shared.exit_code.lock().unwrap().as_deref(), Some("3"));
    }
}
