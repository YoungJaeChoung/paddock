//! 계정 바꾸기의 작업 스레드 몫: 대화 파일 찾기, 대상 계정의 터미널 요청, 옛 CLI 멈추기, 새 터미널 열기. 창을 모르는 순서만 둔다.
//!
//! 이어 열기는 `locate_transcript` → `account_want(--resume <경로>)` → `STOP_AGENT` → `open_new_terminal`이다. 앞 단계가 실패하면 뒤 단계는 하지 않으므로
//! 대화 파일이 없거나 계정이 거부하면 터미널은 그대로다(옛 CLI는 멈추지 않는다).
use std::path::PathBuf;
use std::time::Duration;

use paddock_core::account_launch::Permissions;
use paddock_core::account_session::{Resume, SessionIdentity, SwitchSource};
use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{
    CheckIdle, CheckIdleReply, Conversation, Kill, StopAgent, StopAgentReply, TerminalState, kind,
};
use paddock_protocol::shared_config::native_profile_directory;

use super::launch::account_want;
use super::transcript::{ConfigFolder, Found, locate_transcript, still_there};
use crate::daemon_client::Outbox;
use crate::startup::{connect_daemon, open_new_terminal};
use crate::terminal::session::{TerminalSession, Want};
use crate::usage::files::native_default_config_dir;
use crate::usage::wsl;

/// 데몬이 `STOP_AGENT`에 답하기를 기다리는 최대 시간(데몬은 10초까지 기다려 답한다).
const STOP_REPLY_WAIT: Duration = Duration::from_secs(13);

/// 데몬이 `CHECK_IDLE`에 답하기를 기다리는 최대 시간(WSL 조사는 4초까지 걸린다).
const CHECK_REPLY_WAIT: Duration = Duration::from_secs(6);

/// 터미널 하나의 이어 열기에 필요한 값. 창이 모아 작업 스레드로 넘긴다.
pub struct ContinueJob {
    pub daemon_exe: PathBuf,
    pub terminal: u32,
    pub conversation: Conversation,
    pub target: AccountProfile,
    pub origin: ConfigFolder,
    pub current: ConfigFolder,
    /// 새 계정 터미널의 시작 폴더(WSL이면 대화의 Linux 폴더).
    pub folder: Option<String>,
    pub home: String,
    pub permissions: Permissions,
}

/// 이어 열기·계정 시작이 끝났을 때의 결과: 새 터미널과 탭 이름 후보.
pub type Outcome = Result<(TerminalSession, Option<String>), String>;

/// 터미널이 실행 중인 Claude의 출처: 도구·환경·계정. Claude가 아니거나 대화를 모르면 None이다.
pub fn source_of(
    state: &TerminalState,
    shell: &str,
    distribution: Option<&str>,
) -> Option<SwitchSource> {
    let conversation = state.conversation.as_ref()?;
    let is_wsl = shell == "wsl";
    let distribution = is_wsl
        .then(|| {
            distribution.map(str::to_string).or_else(|| crate::shell_choice::machine_default_distribution().cloned())
        })
        .flatten();
    Some(SwitchSource {
        provider: conversation.provider.clone(),
        runtime: if is_wsl { Runtime::Wsl } else { Runtime::Native },
        wsl_distribution: distribution,
        account_id: state.account.as_ref().and_then(|scope| scope.profile.as_ref()).map(|profile| profile.id.clone()),
    })
}

/// 대화 파일을 찾을 두 설정 폴더(출처 계정 폴더, CLI가 지금 쓰는 폴더). WSL 폴더는 배포판의 홈으로 정한다.
pub fn config_folders(
    state: &TerminalState,
    source: &SwitchSource,
    conversation: &Conversation,
) -> Result<(ConfigFolder, ConfigFolder), String> {
    let profile = state.account.as_ref().and_then(|scope| scope.profile.as_ref());
    match source.runtime {
        Runtime::Native => {
            let origin = profile.map_or_else(
                || native_default_config_dir("claude").to_string_lossy().into_owned(),
                native_profile_directory,
            );
            let current =
                conversation.config_dir.clone().filter(|dir| !dir.is_empty()).unwrap_or_else(|| origin.clone());
            Ok((ConfigFolder::native(&origin), ConfigFolder::native(&current)))
        }
        Runtime::Wsl => {
            let (_, mapping) = wsl::mapping_of(source.wsl_distribution.as_deref())
                .ok_or_else(|| "Open this WSL terminal on its Windows device.".to_string())?;
            let home = mapping.linux_home.clone();
            let origin = match profile {
                Some(profile) => format!("{home}/.paddock/agent-profiles/{}/{}", profile.id, profile.provider),
                None => format!("{home}/.claude"),
            };
            let current =
                conversation.config_dir.clone().filter(|dir| dir.starts_with('/')).unwrap_or_else(|| origin.clone());
            Ok((ConfigFolder::wsl(&origin, mapping.clone()), ConfigFolder::wsl(&current, mapping)))
        }
    }
}

/// 옛 CLI를 멈추게 하고(`STOP_AGENT`) 답을 기다린다. 멈추지 못하면 그 문장이 오류다.
fn stop_old_cli(
    daemon_exe: &std::path::Path,
    terminal: u32,
    conversation: &Conversation,
) -> Result<(), String> {
    let connection =
        connect_daemon(daemon_exe).map_err(|error| format!("The terminal daemon is unavailable: {error}"))?;
    let request = StopAgent { id: terminal, pid: conversation.pid, start_mark: conversation.start_mark };
    connection.send(&Frame::json(kind::STOP_AGENT, &request)).map_err(|error| error.to_string())?;
    let reply = connection
        .wait_for_any(&[kind::STOP_AGENT_REPLY], STOP_REPLY_WAIT)
        .ok_or_else(|| "Claude did not stop in time. Exit it in the terminal before switching accounts.".to_string())?;
    let reply: StopAgentReply = reply.parse_json().map_err(|error| error.to_string())?;
    if reply.stopped { Ok(()) } else { Err(reply.message) }
}

/// 이어 열기의 작업 스레드 몫: 대화 파일 찾기 → 새 계정 터미널 요청 → 옛 CLI 멈추기 → 새 터미널 열기.
pub fn run_continue(job: ContinueJob) -> Outcome {
    let found: Found =
        locate_transcript(&job.origin, &job.current, &job.conversation.session_id, &job.conversation.cwd)?
            .ok_or_else(|| SessionIdentity::MISSING_FILE.to_string())?;
    // CLI가 읽을 경로: WSL 계정은 Linux 경로, 이 컴퓨터 계정은 이 컴퓨터의 경로다.
    let cli_path = if job.target.runtime == Runtime::Wsl {
        found.tool_path.clone()
    } else {
        found.local.to_string_lossy().into_owned()
    };
    let resume = Resume::TranscriptPath(cli_path.clone());
    let want =
        account_want(&job.target, job.folder.as_deref(), &job.home, job.permissions, cfg!(windows), Some(&resume))?;
    // 멈추기 직전에 파일이 아직 있는지 다시 본다. 멈춘 뒤에 잃으면 되돌릴 수 없다.
    still_there(&found)?.then_some(()).ok_or_else(|| SessionIdentity::MISSING_FILE.to_string())?;
    stop_old_cli(&job.daemon_exe, job.terminal, &job.conversation)?;
    let title = want.title().map(str::to_string);
    // 옛 CLI는 이미 멈췄다. 새 터미널을 열지 못하면 사용자가 옛 터미널에서 같은 대화를 직접 이어 열 수 있도록 명령을 알린다.
    let session = open_new_terminal(&job.daemon_exe, want)
        .map_err(|error| format!("Could not open a terminal: {error} {}", SessionIdentity::resume_hint(&cli_path)))?;
    Ok((session, title))
}

/// 터미널 앞쪽이 지금 셸 자신인지 데몬에 다시 읽게 한다. 셸이 아니거나 읽지 못하면 그 문장이 오류다.
///
/// 창이 쥔 상태는 Enter 직후 몇 초 낡을 수 있어 쓰지 않는다(`sleep 30⏎` 직후에 골라도 일하는 터미널을 끝내지 않는다).
fn check_idle(
    connection: &crate::daemon_client::DaemonConnection,
    terminal: u32,
) -> Result<(), String> {
    connection.send(&Frame::json(kind::CHECK_IDLE, &CheckIdle { id: terminal })).map_err(|error| error.to_string())?;
    let reply = connection
        .wait_for_any(&[kind::CHECK_IDLE_REPLY], CHECK_REPLY_WAIT)
        .ok_or_else(|| SessionIdentity::NOT_CHECKED.to_string())?;
    let reply: CheckIdleReply = reply.parse_json().map_err(|error| error.to_string())?;
    if reply.idle { Ok(()) } else { Err(reply.message) }
}

/// 새 터미널을 놓을 탭이 그 사이 닫혀 자리가 없을 때, 데몬에 열어 둔 그 터미널을 끝낸다(남기면 아무도 못 보는 터미널이 데몬에 쌓인다).
pub fn kill_unplaced(
    outbox: &Outbox,
    id: u32,
) {
    let _ = outbox.send(&Frame::json(kind::KILL, &Kill { id }));
}

/// 계정 시작의 작업 스레드 몫: 데몬에서 앞쪽을 지금 다시 읽어 셸일 때만 새 계정 터미널을 연다.
pub fn run_start(
    daemon_exe: PathBuf,
    terminal: u32,
    want: Want,
) -> Outcome {
    let connection =
        connect_daemon(&daemon_exe).map_err(|error| format!("The terminal daemon is unavailable: {error}"))?;
    check_idle(&connection, terminal)?;
    let title = want.title().map(str::to_string);
    let session =
        open_new_terminal(&daemon_exe, want).map_err(|error| format!("Could not open a terminal: {error}"))?;
    Ok((session, title))
}

#[cfg(test)]
mod tests {
    use paddock_core::account_launch::Permissions;

    use super::*;
    #[cfg(unix)]
    use crate::test_support::{DaemonCleanup, TempHome, daemon_exe};

    fn conversation() -> Conversation {
        Conversation {
            provider: "claude".into(),
            session_id: "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10".into(),
            cwd: "/w".into(),
            config_dir: None,
            pid: 1,
            start_mark: None,
        }
    }

    fn job(
        origin: ConfigFolder,
        daemon_exe: PathBuf,
    ) -> ContinueJob {
        ContinueJob {
            daemon_exe,
            terminal: 1,
            conversation: conversation(),
            target: AccountProfile {
                id: "00000000-0000-4000-8000-000000000002".into(),
                provider: "claude".into(),
                label: "B".into(),
                runtime: Runtime::Native,
                wsl_distribution: None,
            },
            current: origin.clone(),
            origin,
            folder: None,
            home: "/home/x".into(),
            permissions: Permissions::Ask,
        }
    }

    #[test]
    fn test_c_d77_a_missing_transcript_keeps_the_terminal_unchanged() {
        // 대화 파일이 없으면 데몬에 연결하기 전에 끝난다(없는 데몬 경로를 줘도 `MISSING_FILE`이 먼저다).
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude");
        std::fs::create_dir_all(config.join("projects").join("-w")).unwrap();
        let origin = ConfigFolder::native(&config.to_string_lossy());
        let error = run_continue(job(origin, PathBuf::from("/nonexistent/paddock-daemon"))).err().unwrap();
        assert_eq!(error, SessionIdentity::MISSING_FILE);
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d77_start_here_refuses_a_busy_shell_and_opens_the_account_in_an_idle_one() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first =
            open_new_terminal(&exe, Want::New { shell: "bash".into(), cwd: None, wsl_distribution: None }).unwrap();
        let terminal = first.id;
        let want = || Want::New { shell: "bash".into(), cwd: None, wsl_distribution: None };
        // 빈 셸: 데몬이 앞쪽을 지금 읽어 셸이면 시작한다.
        let (opened, _) = run_start(exe.clone(), terminal, want()).expect("an idle shell may start an account");
        assert_ne!(opened.id, terminal);
        // 바쁜 셸: Enter 직후(데몬이 저장해 둔 상태가 아직 셸이라고 말할 시각)에 골라도 앞쪽을 지금 읽어 거부한다.
        first.outbox.send(&Frame::new(kind::INPUT, b"sleep 30\n".to_vec())).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let error = run_start(exe.clone(), terminal, want()).err().expect("a running sleep refuses the account");
        assert_eq!(error, SessionIdentity::BUSY_SHELL);
    }

    #[test]
    fn test_c_d78_a_terminal_without_a_tab_to_take_it_is_killed_in_the_daemon() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(
                &mut self,
                bytes: &[u8],
            ) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let written = Arc::new(Mutex::new(Vec::new()));
        let outbox = Outbox::spawn(Sink(written.clone()));
        kill_unplaced(&outbox, 41);
        assert!(outbox.flush(Duration::from_secs(5)));
        let bytes = written.lock().unwrap().clone();
        let frame = Frame::read_from(&mut bytes.as_slice()).unwrap();
        assert_eq!(frame.kind, kind::KILL);
        assert_eq!(frame.parse_json::<Kill>().unwrap(), Kill { id: 41 });
    }
}
