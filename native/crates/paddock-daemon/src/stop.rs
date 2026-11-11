//! `STOP_AGENT`: 터미널에서 도는 에이전트 CLI 하나를 검증한 뒤 멈춘다.
//!
//! 계정을 바꾸려고 같은 대화를 다른 계정으로 이어 열 때, 옛 CLI가 먼저 끝나야 같은 대화 파일을 두 CLI가 동시에 쓰지 않는다.
//! 그래서 요청이 가리키는 프로세스(`pid`·시작 시각)가 지금 이 터미널의 앞쪽 에이전트와 같을 때만 SIGTERM을 보내고, 셸이나 다른 프로그램은 건드리지 않는다.
//! 끝나기를 150 ms마다 다시 읽어 확인하고, 10초 안에 안 끝나면 강제로 죽이지 않고 문장으로 알린다.
//! Windows 네이티브 터미널은 CLI를 안전하게 멈출 방법이 없어 거부한다(사용자가 Ctrl+C로 끝낸다).
use std::time::{Duration, Instant};

use paddock_core::account_session::SessionIdentity;
use paddock_core::agent_model::{is_shell, program_name};
use paddock_protocol::message::{StopAgent, StopAgentReply};

use crate::probe::Reading;

/// 끝나기를 기다리는 최대 시간.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// 끝났는지 다시 읽는 간격.
pub const STOP_POLL: Duration = Duration::from_millis(150);

pub const CHANGED: &str = "The conversation changed. Open the account menu again.";
pub const NOT_IN_TIME: &str = "Claude did not stop in time. Exit it in the terminal before switching accounts.";
#[cfg(windows)]
pub const WINDOWS_NATIVE: &str =
    "Exit Claude with Ctrl+C first, then switch accounts. Windows cannot safely stop this CLI automatically.";
pub const NOT_STOPPED: &str = "Claude could not be stopped. Exit it in the terminal before switching accounts.";
pub const UNREADABLE: &str = "Claude could not be checked. Try again.";

/// 터미널 앞쪽 에이전트 프로세스의 정체: 번호와 시작 표지(`Reading.start_mark`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub pid: u32,
    pub start_mark: Option<u64>,
}

/// 지금 읽은 터미널 앞쪽의 모양. "에이전트가 없다"를 세 가지로 나눠 둔다: 끝났다고 믿어도 되는 것은 `Gone`뿐이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Front {
    /// 에이전트(claude)가 앞쪽에 있다.
    Agent(Identity),
    /// 앞쪽이 셸 자신이다. 에이전트가 끝났다.
    Gone,
    /// 셸도 claude도 아닌 다른 명령이 앞쪽에 있다.
    Other,
    /// 앞쪽을 읽지 못했다(조사 실패·시간 초과).
    Unreadable,
}

/// 조사 결과(없으면 읽지 못한 것)를 앞쪽의 모양으로 가른다.
///
/// Examples
/// --------
/// | 조사 결과                       | 결과          |
/// |---------------------------------|---------------|
/// | 앞쪽 `claude`, pid 있음         | `Agent`       |
/// | 앞쪽 `bash`                     | `Gone`        |
/// | 앞쪽 `sleep`                    | `Other`       |
/// | 앞쪽 `claude`인데 pid를 모름    | `Unreadable`  |
/// | 읽지 못함                       | `Unreadable`  |
pub fn front_of(reading: Option<&Reading>) -> Front {
    let mut front = Front::Unreadable;
    if let Some(reading) = reading {
        let program = program_name(&reading.argv);
        if program.as_deref() == Some("claude") {
            if let Some(pid) = reading.pid {
                front = Front::Agent(Identity { pid, start_mark: reading.start_mark });
            }
        } else if is_shell(program.as_deref()) {
            front = Front::Gone;
        } else if program.is_some() {
            front = Front::Other;
        }
    }
    front
}

impl Identity {
    /// 요청이 가리키는 프로세스와 같은지. 시작 표지를 둘 다 알 때만 비교한다.
    fn matches(
        &self,
        request: &StopAgent,
    ) -> bool {
        self.pid == request.pid
            && match (self.start_mark, request.start_mark) {
                (Some(now), Some(asked)) => now == asked,
                _ => true,
            }
    }
}

/// 요청을 검증하고 에이전트에 종료 신호를 보낸 뒤 끝나기를 기다린다.
///
/// `read`는 이 터미널 앞쪽을 지금 다시 읽는다. `signal`은 종료 신호를 보내고 실패하면 이유 문장을 돌려준다.
/// 앞쪽을 읽지 못한 것(`Unreadable`)을 "끝났다"로 믿으면 안 된다: 호출자가 옛 터미널을 끝내고 새 터미널을 여는 근거가 되기 때문이다.
///
/// Examples
/// --------
/// | 지금 읽은 앞쪽                | 결과                                              |
/// |-------------------------------|---------------------------------------------------|
/// | 요청과 같은 프로세스          | 신호를 보내고 끝나면 `stopped: true`              |
/// | 다른 pid·다른 시작 표지       | 신호 없이 `The conversation changed…`             |
/// | 셸 자신(`Gone`)               | 신호 없이 `stopped: true`(이미 끝났다)            |
/// | 다른 명령(`Other`)            | 신호 없이 `Finish the other command…`             |
/// | 읽지 못함(`Unreadable`)       | 신호 없이 `Claude could not be checked…`          |
/// | 신호 뒤 10초 넘게 남음        | `Claude did not stop in time…`                    |
pub fn stop_agent(
    request: &StopAgent,
    read: &dyn Fn() -> Front,
    signal: &dyn Fn(u32) -> Result<(), String>,
    timeout: Duration,
    poll: Duration,
) -> StopAgentReply {
    let reply = |stopped: bool, message: &str| StopAgentReply { id: request.id, stopped, message: message.to_string() };
    match read() {
        // 이미 끝났다. 할 일이 없다.
        Front::Gone => reply(true, ""),
        Front::Other => reply(false, SessionIdentity::BUSY),
        Front::Unreadable => reply(false, UNREADABLE),
        Front::Agent(current) if !current.matches(request) => reply(false, CHANGED),
        Front::Agent(_) => match signal(request.pid) {
            Err(message) => reply(false, &message),
            Ok(()) => {
                let end = Instant::now() + timeout;
                let mut running = true;
                while running && Instant::now() < end {
                    std::thread::sleep(poll);
                    // 읽지 못한 동안은 끝났는지 모르므로 계속 기다린다.
                    running = match read() {
                        Front::Agent(current) => current.matches(request),
                        Front::Unreadable => true,
                        Front::Gone | Front::Other => false,
                    };
                }
                if running { reply(false, NOT_IN_TIME) } else { reply(true, "") }
            }
        },
    }
}

/// 이 컴퓨터(Linux·macOS)의 프로세스에 SIGTERM을 보낸다.
#[cfg(unix)]
pub fn terminate(pid: u32) -> Result<(), String> {
    let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if result == 0 { Ok(()) } else { Err(NOT_STOPPED.to_string()) }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn request() -> StopAgent {
        StopAgent { id: 3, pid: 40, start_mark: Some(900) }
    }

    fn fast(
        read: &dyn Fn() -> Front,
        signal: &dyn Fn(u32) -> Result<(), String>,
    ) -> StopAgentReply {
        stop_agent(&request(), read, signal, Duration::from_millis(300), Duration::from_millis(10))
    }

    #[test]
    fn test_c_d77_stop_agent_signals_only_the_verified_pid_and_refuses_a_changed_process() {
        let signalled = Cell::new(None);
        let same = || Front::Agent(Identity { pid: 40, start_mark: Some(900) });
        let gone_after_signal = || if signalled.get().is_some() { Front::Gone } else { same() };
        let reply = fast(&gone_after_signal, &|pid| {
            signalled.set(Some(pid));
            Ok(())
        });
        assert_eq!((reply.stopped, reply.message.as_str(), signalled.get()), (true, "", Some(40)));
        for changed in [
            Front::Agent(Identity { pid: 41, start_mark: Some(900) }),
            Front::Agent(Identity { pid: 40, start_mark: Some(901) }),
        ] {
            let sent = Cell::new(false);
            let reply = fast(&|| changed, &|_| {
                sent.set(true);
                Ok(())
            });
            assert_eq!((reply.stopped, reply.message.as_str(), sent.get()), (false, CHANGED, false), "{changed:?}");
        }
    }

    #[test]
    fn test_c_d77_an_agent_that_is_already_gone_is_stopped_without_a_signal_and_a_stubborn_one_is_reported_not_killed()
    {
        let sent = Cell::new(false);
        let reply = fast(&|| Front::Gone, &|_| {
            sent.set(true);
            Ok(())
        });
        assert!(reply.stopped && !sent.get());
        let stubborn = fast(&|| Front::Agent(Identity { pid: 40, start_mark: Some(900) }), &|_| Ok(()));
        assert_eq!((stubborn.stopped, stubborn.message.as_str()), (false, NOT_IN_TIME));
        let refused = fast(&|| Front::Agent(Identity { pid: 40, start_mark: None }), &|_| Err("no".to_string()));
        assert_eq!((refused.stopped, refused.message.as_str()), (false, "no"));
    }

    #[test]
    fn test_c_d77_a_probe_that_failed_is_not_taken_as_the_agent_having_ended() {
        // WSL 조사가 시간 초과로 실패한 흉내: 읽지 못한 터미널은 "끝났다"가 아니다.
        let sent = Cell::new(false);
        let reply = fast(&|| front_of(None), &|_| {
            sent.set(true);
            Ok(())
        });
        assert_eq!((reply.stopped, reply.message.as_str(), sent.get()), (false, UNREADABLE, false));
        // 신호를 보낸 뒤 읽기가 계속 실패하면 끝났는지 모르므로 시간 안에 끝났다고 하지 않는다.
        let signalled = Cell::new(false);
        let gone_then_blind = || {
            if signalled.get() { Front::Unreadable } else { Front::Agent(Identity { pid: 40, start_mark: Some(900) }) }
        };
        let reply = fast(&gone_then_blind, &|_| {
            signalled.set(true);
            Ok(())
        });
        assert_eq!((reply.stopped, reply.message.as_str()), (false, NOT_IN_TIME));
    }

    #[test]
    fn test_c_d77_another_command_that_took_the_front_after_claude_ended_is_refused_without_a_signal() {
        let sent = Cell::new(false);
        let program = Reading {
            cwd: None,
            argv: vec!["sleep".to_string(), "30".to_string()],
            env: None,
            runtime: paddock_core::agent_account::Runtime::Native,
            distribution: String::new(),
            pid: Some(77),
            started_at: None,
            start_mark: Some(1),
            session_record: None,
        };
        let front = front_of(Some(&program));
        assert_eq!(front, Front::Other);
        let reply = fast(&|| front, &|_| {
            sent.set(true);
            Ok(())
        });
        assert_eq!((reply.stopped, reply.message.as_str(), sent.get()), (false, SessionIdentity::BUSY, false));
    }

    #[test]
    fn test_c_d77_the_front_is_sorted_into_agent_shell_other_and_unreadable() {
        let reading = |argv: &[&str], pid: Option<u32>| Reading {
            cwd: None,
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
            env: None,
            runtime: paddock_core::agent_account::Runtime::Native,
            distribution: String::new(),
            pid,
            started_at: None,
            start_mark: Some(5),
            session_record: None,
        };
        assert_eq!(
            front_of(Some(&reading(&["claude"], Some(9)))),
            Front::Agent(Identity { pid: 9, start_mark: Some(5) })
        );
        assert_eq!(front_of(Some(&reading(&["-bash"], Some(9)))), Front::Gone);
        assert_eq!(front_of(Some(&reading(&["sleep", "30"], Some(9)))), Front::Other);
        assert_eq!(front_of(Some(&reading(&["claude"], None))), Front::Unreadable);
        assert_eq!(front_of(Some(&reading(&[], Some(9)))), Front::Unreadable);
        assert_eq!(front_of(None), Front::Unreadable);
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d77_terminate_sends_sigterm_to_a_fake_claude_that_traps_it_and_leaves_its_parent_shell_alone() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        // 가짜 claude: TERM을 받으면 BYE를 찍고 끝나는 셸 스크립트.
        let mut fake = Command::new("sh")
            .args(["-c", "trap 'echo BYE; exit 0' TERM; echo READY; while :; do sleep 0.1; done"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(fake.stdout.take().unwrap()).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "READY");
        terminate(fake.id()).unwrap();
        assert_eq!(lines.next().unwrap().unwrap(), "BYE");
        assert!(fake.wait().unwrap().success());
    }
}
