//! 데몬이 쥔 터미널과 그 목록. 터미널마다 PTY와 원시 출력 링 버퍼를 갖고, 앱이 붙으면 링을 재생한 뒤 실시간 출력을 보낸다.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

use paddock_core::agent_account::AccountProfile;
use paddock_core::claude_wrapper_path::with_wrapper_path;
use paddock_core::replay_ring::ReplayRing;
use paddock_core::shell_catalog::{ShellSpec, shell_spec};
use paddock_core::terminal_history::{HistoryInput, HistoryLimits, HistoryRecord, TrailingDebounce, build_record};
use paddock_core::wsl_probe::{MARKER, forwarded_wslenv};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Attached, Launch, New, TerminalInfo, TerminalState, kind};
use paddock_protocol::shared_config::config_dir;

use crate::activity::{Observed, QUERY_REPLY, Tracker, now_ms, refresh_terminals};
use crate::history::{self, AgentContext, History};
use crate::log;
use crate::outbox::Outbox;
use crate::probe::{Probe, ProbeTarget, Reading, WslTarget};
use crate::pty::{self, PtyHandle, PtyMsg};

/// 터미널마다 보관하는 원시 출력 바이트 수(1 MiB).
pub const RING_BYTES: usize = 1 << 20;

/// 재생 프레임 하나에 담는 바이트 수.
const REPLAY_CHUNK_BYTES: usize = 64 * 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// PTY 읽기 스레드와 연결 스레드가 함께 만지는 터미널 상태.
struct Live {
    cols: u16,
    rows: u16,
    ring: ReplayRing,
    /// 지금 이 터미널의 출력을 받는 연결(번호와 보내기 통로). 한 번에 하나다.
    client: Option<(u64, Outbox)>,
    alive: bool,
    /// 셸이 끝난 뒤 그 종료 코드. 살아 있거나 코드를 알 수 없으면 None.
    exit_code: Option<i32>,
    /// 폴더·앞쪽 프로그램·에이전트 활동의 기록.
    tracker: Tracker,
    /// 터미널 기록을 언제 저장할지. 출력이 올 때마다, 그리고 조사로 폴더·프로그램·대화가 바뀔 때마다 미룬다.
    debounce: TrailingDebounce,
    /// 셸이 낸 출력의 누적 바이트 수. 링의 맨 앞 `ring.len() - shell_output_len` 바이트는 되살린 이전 화면이라 기록의 꼬리가 아니다.
    shell_output_len: usize,
    /// 지금까지 만든 기록의 차례. 파일에는 더 큰 차례가 이미 쓰였다면 쓰지 않는다.
    history_generation: u64,
}

impl Live {
    /// 이 연결에 링 전체를 다시 보낸다: `RESYNC` → `REPLAY`(0개 이상) → `REPLAY_END` → (셸이 이미 끝났다면) `EXITED`.
    ///
    /// 보내기 통로가 넘쳐 버린 출력이 있을 때 연결을 따라잡게 하는 용도다. 링 스냅샷 전송과 수신자 등록이 같은 잠금 안에서
    /// 이뤄지므로, 재생 뒤 실시간 출력 사이에 빠짐·중복이 없다.
    fn resync_if_overflowed(
        &self,
        conn_id: u64,
    ) {
        if let Some((id, outbox)) = &self.client
            && *id == conn_id
            && outbox.take_overflow()
        {
            outbox.send(Frame::new(kind::RESYNC, Vec::new()));
            self.send_replay(outbox);
            log::event(&format!("connection {conn_id}: output backlog dropped, resynced from the ring"));
        }
    }

    /// 링 스냅샷을 `REPLAY`로 보내고 `REPLAY_END`로 맺는다. 셸이 이미 끝났다면 종료 코드를 `EXITED`로 이어 보낸다.
    fn send_replay(
        &self,
        outbox: &Outbox,
    ) {
        for chunk in self.ring.snapshot().chunks(REPLAY_CHUNK_BYTES) {
            outbox.send(Frame::new(kind::REPLAY, chunk.to_vec()));
        }
        outbox.send(Frame::new(kind::REPLAY_END, Vec::new()));
        if !self.alive {
            outbox.send(Frame::text(kind::EXITED, &exit_text(self.exit_code)));
        }
    }

    /// 앞쪽 연결에 `state`를 알린다. 붙은 연결이 없으면 알릴 곳이 없다.
    fn send_state(
        &self,
        state: &TerminalState,
    ) {
        if let Some((_, outbox)) = &self.client {
            outbox.send(Frame::json(kind::TERMINAL_STATE, state));
        }
    }
}

/// `EXITED` 프레임의 글자. 종료 코드를 모르면 `unknown`.
fn exit_text(exit_code: Option<i32>) -> String {
    exit_code.map_or_else(|| "unknown".to_string(), |code| code.to_string())
}

/// 셸 대신 직접 띄우는 프로그램(`launch`)의 실행 명세. 경로·인자는 셸 문자열을 거치지 않고 프로세스 인자로 넘어간다.
///
/// 지울 환경 변수(`env_remove`)는 값을 읽지 않고 이름만으로 지운다. Unix는 `env -u 이름 … 프로그램`으로 감싸 첫 프로세스부터
/// 변수가 없게 하고, Windows는 시작 명령이 지운다(PTY 계층이 환경 변수 삭제를 받지 않는다).
fn launch_spec(launch: &Launch) -> ShellSpec {
    let mut env =
        vec![("TERM".to_string(), "xterm-256color".to_string()), ("COLORTERM".to_string(), "truecolor".to_string())];
    env.extend(launch.env.iter().cloned());
    let (program, args) = if cfg!(unix) && !launch.env_remove.is_empty() {
        let mut args: Vec<String> =
            launch.env_remove.iter().flat_map(|name| ["-u".to_string(), name.clone()]).collect();
        args.push(launch.program.clone());
        args.extend(launch.args.iter().cloned());
        ("env".to_string(), args)
    } else {
        (launch.program.clone(), launch.args.clone())
    };
    ShellSpec { program, args, working_directory: launch.cwd.clone(), env }
}

/// 설정 폴더의 Claude 래퍼 폴더(`~/.paddock/config/bin`). 폴더가 있을 때만 돌려준다.
fn wrapper_directory() -> Option<String> {
    let directory = config_dir().join("bin");
    directory.is_dir().then(|| directory.to_string_lossy().into_owned())
}

/// 셸 프로세스 하나와 그 출력 기록.
pub struct Terminal {
    pub id: u32,
    shell: String,
    /// `wsl` 터미널이 띄운 배포판. 앞쪽 프로그램을 WSL 안에서 읽을 때 같은 배포판을 가리키려고 남긴다.
    wsl_distribution: Option<String>,
    /// 계정 버튼으로 연 터미널을 연 계정. 일반 터미널이면 None이다.
    account: Option<AccountProfile>,
    /// 요청한 실행 명세. 터미널 기록에 남겨 같은 방식으로 다시 연다.
    launch: Option<Launch>,
    /// 터미널을 처음 연 폴더.
    cwd_start: Option<String>,
    pty: PtyHandle,
    live: Arc<Mutex<Live>>,
}

impl Terminal {
    /// 계정 버튼으로 연 터미널을 연 계정.
    pub fn account(&self) -> Option<&AccountProfile> {
        self.account.as_ref()
    }

    /// 요청대로 셸을 띄워 목록에 넣는다. 셸을 띄우지 못하면 오류이고 목록은 그대로다.
    ///
    /// 셸이 이미 끝났다면 `alive`가 거짓인 채 목록에 남는다 — 붙은 연결이 끊길 때 정리된다.
    pub fn spawn(
        registry: &Arc<Registry>,
        request: &New,
    ) -> std::io::Result<Arc<Terminal>> {
        // 목록에 들어가기 전까지 유휴 판정에서 빠지도록 자리를 예약해 둔다. 함수가 끝날 때(삽입 뒤 또는 실패) 풀린다.
        let reservation = registry.reserve_terminal();
        let id = reservation.id;
        let account = request.launch.as_ref().and_then(|launch| launch.account.clone());
        let live = Arc::new(Mutex::new(Live {
            cols: request.cols,
            rows: request.rows,
            ring: ReplayRing::new(RING_BYTES),
            client: None,
            alive: true,
            exit_code: None,
            tracker: Tracker::opened_by(account.as_ref()),
            debounce: TrailingDebounce::new(HistoryLimits::QUIET_MS, HistoryLimits::MAX_WAIT_MS),
            shell_output_len: 0,
            history_generation: 0,
        }));
        // 되살리는 터미널은 이전 화면이 셸 출력보다 먼저 링에 들어가, 앱이 다시 붙어도 같은 화면이 재생된다.
        // 이 화면은 재생용일 뿐 기록의 꼬리가 아니다(`shell_output_len`) — 다시 끝나도 이전 화면이 겹겹이 쌓이지 않는다.
        if let Some(screen) = &request.preload {
            lock(&live).ring.push(screen.as_bytes());
        }
        // 경로·인자는 셸 문자열을 거치지 않고 프로세스 인자로 넘어간다.
        let mut spec = match &request.launch {
            Some(launch) => launch_spec(launch),
            None => shell_spec(&request.shell, request.cwd.as_deref(), request.wsl_distribution.as_deref()),
        };
        // 앱이 보낸 최신 환경(PATH·계정 폴더 등)을 앞에 둔다 — 데몬은 앱보다 오래 살아 자기 환경이 낡았을 수 있다.
        // `launch.env`처럼 명세가 직접 정한 값은 뒤에 있어 앱 값보다 앞선다.
        spec.env.splice(0..0, request.env.iter().cloned());
        // 래퍼 폴더가 있으면 모든 터미널의 PATH 맨 앞에 둔다. WSL 터미널은 자기 안의 PATH를 쓰므로 제외한다.
        if request.shell != "wsl"
            && let Some(directory) = wrapper_directory()
        {
            let base_env: Vec<(String, String)> = std::env::vars().collect();
            spec.env = with_wrapper_path(&spec.env, &directory, &base_env, cfg!(windows));
        }
        if request.shell == "wsl" {
            // WSL 안에서 이 터미널의 셸을 찾을 표지. 데몬 번호와 터미널 번호로 다른 데몬·터미널과 섞이지 않는다.
            spec.env.push((MARKER.to_string(), marker_of(id)));
            let own_wslenv = request
                .env
                .iter()
                .find(|(name, _)| name == "WSLENV")
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| std::env::var("WSLENV").unwrap_or_default());
            spec.env.retain(|(name, _)| name != "WSLENV");
            spec.env.push(("WSLENV".to_string(), forwarded_wslenv(&own_wslenv)));
        }
        let reply_pty: Arc<OnceLock<PtyHandle>> = Arc::new(OnceLock::new());
        let on_data_reply = reply_pty.clone();
        let on_data_live = live.clone();
        let on_exit_live = live.clone();
        let on_exit_registry = registry.clone();
        let on_exit_history = registry.history().clone();
        let pty = pty::spawn(
            &spec,
            request.cols,
            request.rows,
            move |data| {
                let mut live = lock(&on_data_live);
                live.ring.push(data);
                live.shell_output_len = live.shell_output_len.saturating_add(data.len());
                let now = now_ms();
                live.debounce.touch(now);
                let query_seen = live.tracker.note_output(data, now);
                if let Some(state) = live.tracker.evaluate(id, now) {
                    live.send_state(&state);
                }
                // 에이전트가 OSC 7501 지원을 물으면 같은 시퀀스를 터미널 입력으로 돌려준다. 재생(링)에는 섞이지 않는다.
                if query_seen && let Some(pty) = on_data_reply.get() {
                    pty.send(PtyMsg::Input(QUERY_REPLY.to_vec()));
                }
                if let Some((conn_id, outbox)) = &live.client {
                    // 클라이언트가 읽지 못해 상한을 넘으면 이 출력은 버린다(링에 있다). 그 순간 밀린 프레임이 이미 비었다면
                    // 훅이 불릴 기회가 없으므로 여기서 바로 다시 맞춘다.
                    if !outbox.send_output(Frame::new(kind::OUTPUT, data.to_vec())) && outbox.is_drained() {
                        let conn_id = *conn_id;
                        live.resync_if_overflowed(conn_id);
                    }
                }
            },
            move |exit_code| {
                let mut live = lock(&on_exit_live);
                live.alive = false;
                live.exit_code = exit_code;
                let text = exit_text(exit_code);
                log::event(&format!("terminal {id} stopped (exit code {text})"));
                // 사용자가 셸을 끝냈으면 되살릴 것이 없다. 데몬이 끝나는 중이면 기록을 남겨 다시 열 때 되살린다.
                if !on_exit_history.is_stopping() {
                    on_exit_history.remove(id);
                }
                let has_client = live.client.is_some();
                if let Some((_, outbox)) = &live.client {
                    outbox.send(Frame::text(kind::EXITED, &text));
                }
                drop(live);
                // 붙은 연결이 없으면 알릴 곳이 없으므로 바로 목록에서 뺀다. 붙은 연결이 있으면 그 연결이 끊길 때 뺀다.
                if !has_client {
                    on_exit_registry.remove(id);
                }
            },
        )?;
        log::event(&format!("terminal {id} started (shell {}, pid {})", request.shell, pty.pid));
        let _ = reply_pty.set(pty.clone());
        let terminal = Arc::new(Terminal {
            id,
            shell: request.shell.clone(),
            wsl_distribution: request.wsl_distribution.clone(),
            account,
            launch: request.launch.clone(),
            cwd_start: request.cwd.clone().or_else(|| request.launch.as_ref().and_then(|launch| launch.cwd.clone())),
            pty,
            live,
        });
        registry.insert(terminal.clone());
        drop(reservation);
        Ok(terminal)
    }

    /// 이 연결이 출력을 받도록 등록하고, 링에 담긴 출력을 처음부터 재생한다.
    ///
    /// 보내는 순서는 `ATTACHED` → `REPLAY`(0개 이상) → `REPLAY_END` → (셸이 이미 끝났다면) `EXITED`이고, 그 뒤에 실시간 `OUTPUT`이 이어진다.
    /// `cols`가 0이면 "지금 크기를 유지"라는 뜻이다. 재접속하는 앱은 데몬의 크기로 격자를 만든다.
    ///
    /// 출력을 받는 연결은 하나라서, 다른 연결이 받고 있었다면 그 연결은 `DETACHED`를 받고 출력에서 밀려난다.
    pub fn attach(
        &self,
        conn_id: u64,
        outbox: &Outbox,
        cols: u16,
        rows: u16,
    ) {
        let mut live = lock(&self.live);
        if let Some((previous_id, previous)) = live.client.take()
            && previous_id != conn_id
        {
            previous.send(Frame::new(kind::DETACHED, Vec::new()));
            log::event(&format!("terminal {} taken over: connection {previous_id} replaced by {conn_id}", self.id));
        }
        // 링 스냅샷 전송과 수신자 등록을 같은 잠금 안에서 해서 재생 뒤 실시간 출력 사이에 빠짐·중복이 없다.
        live.client = Some((conn_id, outbox.clone()));
        let weak_live: Weak<Mutex<Live>> = Arc::downgrade(&self.live);
        // 이 연결이 밀린 출력을 모두 쓰고 나면, 버려진 출력이 있었을 때 링을 다시 보낸다.
        outbox.set_drained_hook(move || {
            if let Some(live) = weak_live.upgrade() {
                lock(&live).resync_if_overflowed(conn_id);
            }
        });
        if cols != 0 && rows != 0 && (live.cols != cols || live.rows != rows) {
            live.cols = cols;
            live.rows = rows;
            self.pty.send(PtyMsg::Resize(cols, rows));
        }
        log::event(&format!("terminal {} attached by connection {conn_id}", self.id));
        let attached = Attached { id: self.id, pid: self.pty.pid, cols: live.cols, rows: live.rows, alive: live.alive };
        outbox.send(Frame::json(kind::ATTACHED, &attached));
        live.send_replay(outbox);
        // 붙는 순간 지금의 폴더·프로그램·활동을 한 번 알린다. 그 뒤로는 바뀔 때만 간다.
        let state = live.tracker.snapshot(self.id, now_ms());
        outbox.send(Frame::json(kind::TERMINAL_STATE, &state));
    }

    /// 입력 바이트를 셸에 넣는다. 출력에서 밀려난 연결의 입력도 그대로 셸에 들어간다.
    pub fn input(
        &self,
        bytes: Vec<u8>,
    ) {
        {
            let mut live = lock(&self.live);
            let now = now_ms();
            live.tracker.note_input(&bytes, now);
            if let Some(state) = live.tracker.evaluate(self.id, now) {
                live.send_state(&state);
            }
        }
        self.pty.send(PtyMsg::Input(bytes));
    }

    /// 틱이 이 터미널의 프로세스를 읽어야 하면 읽을 대상을 돌려주고 요청을 지운다.
    /// 지난 조사 뒤 출력이나 Enter가 없었거나 셸이 이미 끝났으면 None이다.
    pub fn take_probe_request(&self) -> Option<ProbeTarget> {
        let wanted = lock(&self.live).tracker.take_probe_request();
        wanted.then(|| self.probe_target()).flatten()
    }

    /// 출력이나 Enter와 무관하게 이 터미널을 지금 읽을 대상. 셸이 이미 끝났으면 None이다.
    pub fn probe_target(&self) -> Option<ProbeTarget> {
        let alive = lock(&self.live).alive;
        alive.then(|| ProbeTarget {
            id: self.id,
            shell_pid: self.pty.pid,
            wsl: (self.shell == "wsl")
                .then(|| WslTarget { distribution: self.wsl_distribution.clone(), marker: marker_of(self.id) }),
        })
    }

    /// 앞쪽 프로그램이 이어 열 수 있는 에이전트이면 그 대화를 찾는다(같은 프로세스에서 이미 찾았으면 다시 읽지 않는다).
    pub fn find_agent(
        &self,
        reading: &Reading,
        program: Option<&str>,
    ) -> Option<history::AgentSeen> {
        let (known, reported) = {
            let live = lock(&self.live);
            (live.tracker.agent().cloned(), live.tracker.cwd().map(str::to_string))
        };
        let context =
            AgentContext { opened_by: self.account.as_ref(), reported_cwd: reported.as_deref(), known: known.as_ref() };
        history::resolve_agent(reading, program, &context)
    }

    /// 기록을 저장할 때가 됐거나(`force`) 강제로 저장해야 하는가. 셸이 이미 끝났으면 아니다.
    fn is_history_due(
        &self,
        now_ms: u64,
        force: bool,
    ) -> bool {
        let live = lock(&self.live);
        live.alive && (force || live.debounce.is_due(now_ms))
    }

    /// 지금의 기록을 만들고 그 차례를 돌려준다. 셸이 이미 끝났으면 None이다.
    ///
    /// 꼬리는 셸이 낸 출력만 쓴다 — 링 앞쪽의 되살린 이전 화면은 뺀다. 파일에 쓰는 일은 호출자가 잠금 밖에서 한다.
    fn take_history_record(&self) -> Option<(u64, HistoryRecord)> {
        let mut live = lock(&self.live);
        live.alive.then(|| {
            live.debounce.finish();
            live.history_generation += 1;
            let n_tail = (HistoryLimits::N_TAIL_BYTES + 1).min(live.shell_output_len.min(live.ring.len()));
            // 링의 마지막 부분을 그대로 쓰므로 화면 꼬리를 위한 별도 보관 메모리가 없다.
            let output = live.ring.last_bytes(n_tail);
            let saved_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0));
            let record = build_record(&HistoryInput {
                saved_at,
                shell: &self.shell,
                wsl_distribution: self.wsl_distribution.as_deref(),
                launch: self.launch.as_ref(),
                cwd_start: self.cwd_start.as_deref(),
                cwd_now: live.tracker.cwd(),
                program: live.tracker.program(),
                agent: live.tracker.agent().map(|seen| &seen.conversation),
                output: &output,
            });
            (live.history_generation, record)
        })
    }

    /// 조사 결과를 반영한다. 폴더·앞쪽 프로그램·대화가 바뀌었으면 기록 저장을 예약한다 — 출력이 없어도 바뀐 내용이 파일에 실린다.
    pub fn apply_observed(
        &self,
        observed: Observed,
        now_ms: u64,
    ) {
        let mut live = lock(&self.live);
        if live.tracker.apply_probe(observed) {
            live.debounce.touch(now_ms);
        }
    }

    /// 시간을 반영해 판정하고 바뀐 상태가 있으면 앞쪽 연결에 알린다.
    pub fn evaluate_and_send(
        &self,
        now: u64,
    ) {
        let mut live = lock(&self.live);
        if let Some(state) = live.tracker.evaluate(self.id, now) {
            live.send_state(&state);
        }
    }

    /// 크기를 바꾼다. 0이 든 크기는 무시한다.
    pub fn resize(
        &self,
        cols: u16,
        rows: u16,
    ) {
        if cols != 0 && rows != 0 {
            let mut live = lock(&self.live);
            live.cols = cols;
            live.rows = rows;
            self.pty.send(PtyMsg::Resize(cols, rows));
        }
    }

    /// 셸 프로세스를 끝낸다. 목록에서 빼는 일은 호출자가 한다.
    pub fn kill(&self) {
        log::event(&format!("terminal {} killed on request", self.id));
        self.pty.send(PtyMsg::Shutdown);
    }

    /// 연결이 끊겼을 때 이 연결이 붙어 있었다면 수신자를 비운다. 셸이 이미 끝난 터미널이면 `true`를 돌려 호출자가 목록에서 빼게 한다.
    pub fn detach(
        &self,
        conn_id: u64,
    ) -> bool {
        let mut live = lock(&self.live);
        if matches!(&live.client, Some((id, _)) if *id == conn_id) {
            live.client = None;
        }
        !live.alive && live.client.is_none()
    }

    fn info(&self) -> TerminalInfo {
        let live = lock(&self.live);
        TerminalInfo {
            id: self.id,
            pid: self.pty.pid,
            cols: live.cols,
            rows: live.rows,
            alive: live.alive,
            shell: self.shell.clone(),
            exit_code: live.exit_code,
            state: Some(live.tracker.state_now(self.id, now_ms())),
            account: self.account.clone(),
        }
    }
}

/// WSL 안의 셸에 붙이는 터미널 표지: 데몬 프로세스 번호와 터미널 번호.
fn marker_of(id: u32) -> String {
    format!("{}-{id}", std::process::id())
}

/// 기록을 쓰다 실패했음을 데몬 기록 파일에 남긴다. 사유만 적고 화면 출력·폴더는 적지 않는다.
fn note_history_failure(
    id: u32,
    error: &std::io::Error,
) {
    log::event(&format!("terminal history: could not write terminal {id} ({:?})", error.kind()));
}

struct RegistryState {
    terminals: BTreeMap<u32, Arc<Terminal>>,
    next_id: u32,
    /// 인사를 마치고 열려 있는 연결 수.
    n_connections: usize,
    /// 셸을 띄우는 중이라 아직 목록에 들어가지 않은 터미널 수.
    n_spawning: usize,
    /// 터미널·연결·생성 중인 터미널이 모두 0이 된 시각. 하나라도 있으면 None.
    idle_since: Option<Instant>,
}

impl RegistryState {
    /// 데몬이 할 일이 없는 상태가 시작됐으면 그 시각을 적고, 할 일이 생겼으면 지운다.
    fn refresh_idle(&mut self) {
        let busy = !self.terminals.is_empty() || self.n_connections > 0 || self.n_spawning > 0;
        if busy {
            self.idle_since = None;
        } else if self.idle_since.is_none() {
            self.idle_since = Some(Instant::now());
            log::event("no terminals or connections: idle timer started");
        }
    }
}

/// 데몬이 쥔 터미널 목록과 유휴 시각. 연결 스레드와 PTY 스레드가 함께 쓴다.
pub struct Registry {
    state: Mutex<RegistryState>,
    history: Arc<History>,
}

/// 터미널 번호 예약. 셸을 띄우는 동안 데몬이 유휴로 판정돼 끝나지 않게 붙들고, 버리면 풀린다.
pub struct Reservation<'a> {
    registry: &'a Registry,
    pub id: u32,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut state = lock(&self.registry.state);
        state.n_spawning -= 1;
        state.refresh_idle();
    }
}

/// 열려 있는 연결 하나의 표시. 버리면(연결이 끝나면) 연결 수가 줄고, 할 일이 없어졌으면 유휴 시간을 다시 센다.
pub struct ConnectionGuard<'a> {
    registry: &'a Registry,
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        let mut state = lock(&self.registry.state);
        state.n_connections -= 1;
        state.refresh_idle();
    }
}

impl Registry {
    /// 비어 있는 목록. 만든 순간부터 유휴 시간을 센다. 터미널 기록은 `history` 폴더에 남긴다.
    pub fn new(history: Arc<History>) -> Self {
        Self {
            history,
            state: Mutex::new(RegistryState {
                terminals: BTreeMap::new(),
                next_id: 1,
                n_connections: 0,
                n_spawning: 0,
                idle_since: Some(Instant::now()),
            }),
        }
    }

    /// 이 데몬의 터미널 기록.
    pub fn history(&self) -> &Arc<History> {
        &self.history
    }

    /// 저장할 때가 된 터미널의 기록을 쓴다. 되살리기가 꺼져 있으면 쓰지 않고 있던 기록을 지운다.
    pub fn save_due_histories(
        &self,
        now_ms: u64,
        probe: &dyn Probe,
    ) {
        if !self.history.is_stopping() {
            self.save_histories(now_ms, false, probe);
        }
    }

    /// 모든 터미널의 기록을 지금 바로 쓰고, 이후 끝나는 터미널의 기록은 지우지 않게 한다. 데몬을 끝내기 직전에 부른다.
    pub fn flush_histories(
        &self,
        probe: &dyn Probe,
    ) {
        self.history.begin_stop();
        self.save_histories(now_ms(), true, probe);
    }

    /// 쓸 때가 된(또는 `force`) 터미널마다 지금 그 터미널을 조사해 최신 폴더·프로그램·대화를 얻은 뒤 기록을 만들어 쓴다.
    fn save_histories(
        &self,
        now_ms: u64,
        force: bool,
        probe: &dyn Probe,
    ) {
        let due: Vec<Arc<Terminal>> =
            self.all().into_iter().filter(|terminal| terminal.is_history_due(now_ms, force)).collect();
        refresh_terminals(&due, probe, now_ms);
        for terminal in &due {
            if let Some((generation, record)) = terminal.take_history_record() {
                self.write_history(terminal.id, generation, &record);
            }
        }
    }

    fn write_history(
        &self,
        id: u32,
        generation: u64,
        record: &HistoryRecord,
    ) {
        if history::is_enabled() {
            if let Err(error) = self.history.save(id, generation, record) {
                note_history_failure(id, &error);
            }
        } else {
            self.history.clear_all();
        }
    }

    /// 새 터미널의 번호를 예약한다. 예약을 쥔 동안은 유휴 판정에서 제외된다.
    fn reserve_terminal(&self) -> Reservation<'_> {
        let mut state = lock(&self.state);
        let id = state.next_id;
        state.next_id += 1;
        state.n_spawning += 1;
        state.idle_since = None;
        Reservation { registry: self, id }
    }

    /// 인사를 마친 연결 하나를 센다. 돌려준 표시를 쥐고 있는 동안 데몬은 유휴로 판정되지 않는다.
    pub fn connection_opened(&self) -> ConnectionGuard<'_> {
        let mut state = lock(&self.state);
        state.n_connections += 1;
        state.idle_since = None;
        ConnectionGuard { registry: self }
    }

    fn insert(
        &self,
        terminal: Arc<Terminal>,
    ) {
        let mut state = lock(&self.state);
        state.terminals.insert(terminal.id, terminal);
        state.refresh_idle();
    }

    /// 쥐고 있는 모든 터미널(번호 순). 잠금을 풀고 각 터미널을 만질 때 쓴다.
    pub fn all(&self) -> Vec<Arc<Terminal>> {
        lock(&self.state).terminals.values().cloned().collect()
    }

    /// 번호로 터미널을 찾는다.
    pub fn get(
        &self,
        id: u32,
    ) -> Option<Arc<Terminal>> {
        lock(&self.state).terminals.get(&id).cloned()
    }

    /// 목록에서 터미널을 빼고 돌려준다. 셸은 끝내지 않는다. 없는 번호면 None.
    pub fn remove(
        &self,
        id: u32,
    ) -> Option<Arc<Terminal>> {
        let mut state = lock(&self.state);
        let removed = state.terminals.remove(&id);
        state.refresh_idle();
        removed
    }

    /// 쥐고 있는 모든 터미널의 요약(번호 순).
    pub fn list(&self) -> Vec<TerminalInfo> {
        lock(&self.state).terminals.values().map(|terminal| terminal.info()).collect()
    }

    /// 쥐고 있는 터미널 수.
    pub fn len(&self) -> usize {
        lock(&self.state).terminals.len()
    }

    /// 모든 터미널의 셸을 끝내고 목록을 비운다.
    pub fn kill_all(&self) {
        let mut state = lock(&self.state);
        for terminal in state.terminals.values() {
            terminal.kill();
        }
        state.terminals.clear();
    }

    /// 데몬이 할 일 없이 `limit` 이상 지났으면 `action`을 실행한다.
    ///
    /// 할 일이 없다는 것은 터미널이 0개이고, 인사를 마친 연결이 0개이고, 셸을 띄우는 중인 터미널도 0개라는 뜻이다.
    /// 연결이 하나라도 남아 있으면 앱이 아직 붙어 있는 것이므로 터미널이 없어도 끝나지 않는다.
    /// `action`이 끝날 때까지 목록 잠금을 쥐므로, 판정 뒤 새 터미널이나 연결이 끼어들 수 없다.
    pub fn when_idle_for(
        &self,
        limit: Duration,
        action: impl FnOnce(),
    ) {
        let state = lock(&self.state);
        if matches!(state.idle_since, Some(since) if since.elapsed() >= limit) {
            action();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 유휴 시간 0으로 판정해 지금 데몬이 할 일이 없는 상태인지 본다.
    fn is_idle(registry: &Registry) -> bool {
        let mut idle = false;
        registry.when_idle_for(Duration::ZERO, || idle = true);
        idle
    }

    fn registry() -> Registry {
        Registry::new(Arc::new(History::new("unit")))
    }

    #[test]
    fn test_c_d4a_new_registry_is_idle() {
        assert!(is_idle(&registry()));
    }

    #[test]
    fn test_c_d4a_open_connection_blocks_idle_until_it_closes() {
        let _home = crate::test_home::TempHome::new();
        let registry = registry();
        let first = registry.connection_opened();
        let second = registry.connection_opened();
        drop(first);
        assert!(!is_idle(&registry), "one connection is still open");
        drop(second);
        assert!(is_idle(&registry));
    }

    #[test]
    fn test_c_d4a_terminal_being_spawned_blocks_idle() {
        let _home = crate::test_home::TempHome::new();
        let registry = registry();
        let reservation = registry.reserve_terminal();
        assert!(!is_idle(&registry), "the shell is still starting");
        drop(reservation);
        assert!(is_idle(&registry), "a failed spawn leaves nothing behind");
    }

    #[test]
    fn test_c_d4a_idle_timer_restarts_when_the_last_connection_closes() {
        let _home = crate::test_home::TempHome::new();
        let registry = registry();
        let connection = registry.connection_opened();
        std::thread::sleep(Duration::from_millis(60));
        drop(connection);
        let mut ran = false;
        registry.when_idle_for(Duration::from_millis(50), || ran = true);
        assert!(!ran, "idle time counts from the moment nothing was left to serve");
    }

    /// 조사 호출마다 넘겨받은 대상 수를 기록하는 조사기.
    #[cfg(unix)]
    struct CountingProbe {
        batch_sizes: Mutex<Vec<usize>>,
    }

    #[cfg(unix)]
    impl Probe for CountingProbe {
        fn read(
            &self,
            targets: &[ProbeTarget],
        ) -> BTreeMap<u32, Reading> {
            lock(&self.batch_sizes).push(targets.len());
            BTreeMap::new()
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_c_d46_saving_histories_reads_all_due_terminals_in_one_probe_call() {
        let _home = crate::test_home::TempHome::new();
        let registry = Arc::new(registry());
        for _ in 0..3 {
            let request = New {
                shell: "bash".to_string(),
                cwd: None,
                cols: 80,
                rows: 24,
                wsl_distribution: None,
                launch: None,
                preload: None,
                env: Vec::new(),
            };
            Terminal::spawn(&registry, &request).expect("spawn a shell");
        }
        let probe = CountingProbe { batch_sizes: Mutex::new(Vec::new()) };
        registry.flush_histories(&probe);
        assert_eq!(*lock(&probe.batch_sizes), vec![3], "one probe call covers every terminal that is due");
        registry.kill_all();
    }
}
