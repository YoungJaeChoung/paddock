//! alacritty_terminal의 PTY(Windows는 ConPTY)를 열어 원시 바이트만 주고받는 스레드.
//! 데몬이 쓴다. 화면 격자는 만들지 않는다.
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{self, EventedPty, EventedReadWrite, Options, Pty, Shell};
use paddock_core::shell_catalog::ShellSpec;
use polling::{Event, Events, PollMode, Poller};

/// `polling`에 등록한 대상을 구분하는 키. 자식 프로세스 종료 알림과 PTY 읽기·쓰기 두 가지다.
mod poll_key {
    #[cfg(windows)]
    pub const CHILD: usize = 1;
    #[cfg(windows)]
    pub const READ_WRITE: usize = 2;
    #[cfg(unix)]
    pub const CHILD: usize = 1;
    #[cfg(unix)]
    pub const READ_WRITE: usize = 0;
}

/// 끝내 달라고 한 자식이 종료하기를 기다리는 최대 시간(신호 한 번당).
const TERMINATE_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// 종료를 기다리며 확인하는 간격.
#[cfg(unix)]
const TERMINATE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

/// 요청으로 강제 종료한 Windows 셸이 남기는 종료 코드.
#[cfg(windows)]
const KILLED_EXIT_CODE: u32 = 1;

/// PTY 스레드가 받는 명령.
pub enum PtyMsg {
    /// 셸에 넣을 입력 바이트.
    Input(Vec<u8>),
    /// 새 크기(열, 행).
    Resize(u16, u16),
    /// 셸을 끝내고 스레드를 멈춘다. 끝난 셸의 종료 코드가 `on_exit`로 간다.
    Shutdown,
}

/// 실행 중인 PTY 스레드에 명령을 보내는 손잡이. 복제해도 같은 PTY를 가리킨다.
#[derive(Clone)]
pub struct PtyHandle {
    tx: Sender<PtyMsg>,
    poller: Arc<Poller>,
    pub pid: u32,
}

impl PtyHandle {
    /// PTY 스레드에 명령을 보내고 깨운다. 스레드가 이미 끝났으면 조용히 버린다.
    pub fn send(
        &self,
        msg: PtyMsg,
    ) {
        let _ = self.tx.send(msg);
        let _ = self.poller.notify();
    }
}

/// 격자를 만들지 않는 데몬은 글자 칸의 픽셀 크기를 알 수 없으므로 고정값을 알린다.
fn window_size(
    cols: u16,
    rows: u16,
) -> WindowSize {
    WindowSize { num_lines: rows, num_cols: cols, cell_width: 8, cell_height: 16 }
}

/// 셸 실행 명세를 PTY 열기 옵션으로 바꾼다.
fn options_of(spec: &ShellSpec) -> Options {
    Options {
        shell: Some(Shell::new(spec.program.clone(), spec.args.clone())),
        working_directory: spec.working_directory.as_ref().map(std::path::PathBuf::from),
        drain_on_exit: true,
        env: spec.env.iter().cloned().collect(),
        #[cfg(target_os = "windows")]
        escape_args: true,
    }
}

/// PTY를 열고 읽기 스레드를 띄운다. `on_data`는 읽은 청크마다, `on_exit`는 자식 종료 시 한 번 불린다.
///
/// 셸을 띄우지 못하면(없는 실행 파일·없는 작업 폴더) 스레드를 만들지 않고 그 오류를 돌려준다.
/// 작업 폴더는 운영체제 PTY 계층이 없는 폴더를 조용히 무시하므로 여기서 먼저 확인한다.
pub fn spawn(
    spec: &ShellSpec,
    cols: u16,
    rows: u16,
    on_data: impl FnMut(&[u8]) + Send + 'static,
    on_exit: impl FnOnce(Option<i32>) + Send + 'static,
) -> std::io::Result<PtyHandle> {
    let missing_dir = spec.working_directory.as_deref().filter(|dir| !std::path::Path::new(dir).is_dir());
    match missing_dir {
        Some(dir) => Err(std::io::Error::new(ErrorKind::NotFound, format!("Working directory does not exist: {dir}"))),
        None => open(spec, cols, rows, on_data, on_exit),
    }
}

/// PTY를 열고 읽기 스레드를 띄우는 실제 작업. 작업 폴더 확인은 `spawn`이 먼저 한다.
fn open(
    spec: &ShellSpec,
    cols: u16,
    rows: u16,
    on_data: impl FnMut(&[u8]) + Send + 'static,
    on_exit: impl FnOnce(Option<i32>) + Send + 'static,
) -> std::io::Result<PtyHandle> {
    let pty = tty::new(&options_of(spec), window_size(cols, rows), 0)?;
    #[cfg(windows)]
    let pid = pty.child_watcher().pid().map(|p| p.get()).unwrap_or(0);
    #[cfg(unix)]
    let pid = pty.child().id();

    let poller = Arc::new(Poller::new()?);
    let (tx, rx): (Sender<PtyMsg>, Receiver<PtyMsg>) = channel();
    let handle = PtyHandle { tx, poller: poller.clone(), pid };

    std::thread::Builder::new().name("pty-raw".into()).spawn(move || {
        // 출력 처리 중 panic이 나도 스레드가 말없이 사라지지 않게 한다. 터미널을 "종료 코드 모름"으로 끝내 앱이 알게 한다.
        let pumped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pump(pty, &poller, &rx, on_data)));
        on_exit(pumped.unwrap_or(None));
    })?;
    Ok(handle)
}

/// 끝난 자식 프로세스의 종료 코드. Unix에서 신호로 죽었으면 셸 관례대로 128 + 신호 번호이고, 상태를 모르면 None이다.
fn exit_code_of(status: Option<std::process::ExitStatus>) -> Option<i32> {
    status.and_then(|status| {
        let code = status.code();
        #[cfg(unix)]
        let code = code.or_else(|| {
            use std::os::unix::process::ExitStatusExt as _;
            status.signal().map(|signal| 128 + signal)
        });
        code
    })
}

/// 자식 셸에 종료를 요청하고 끝나기를 기다려 종료 코드를 돌려준다. 정해진 시간 안에 끝나지 않으면 None이다.
///
/// 먼저 연결 끊김 신호(SIGHUP)로 정중히 끝내게 하고, 그래도 남아 있으면 강제로 죽인다.
#[cfg(unix)]
fn terminate_child(pty: &mut Pty) -> Option<i32> {
    let pid = pty.child().id() as i32;
    let mut exit_code = None;
    'signals: for signal in [libc::SIGHUP, libc::SIGKILL] {
        unsafe { libc::kill(pid, signal) };
        let end = std::time::Instant::now() + TERMINATE_WAIT;
        while std::time::Instant::now() < end {
            if let Some(tty::ChildEvent::Exited(status)) = pty.next_child_event() {
                exit_code = exit_code_of(status);
                break 'signals;
            }
            std::thread::sleep(TERMINATE_POLL_INTERVAL);
        }
    }
    exit_code
}

/// 자식 셸 프로세스를 끝내고 기다려 종료 코드를 돌려준다. 정해진 시간 안에 끝나지 않으면 None이다.
#[cfg(windows)]
fn terminate_child(pty: &mut Pty) -> Option<i32> {
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, TerminateProcess, WaitForSingleObject};
    let handle = pty.child_watcher().raw_handle();
    let mut code = 0u32;
    let exited = unsafe {
        TerminateProcess(handle, KILLED_EXIT_CODE) != 0
            && WaitForSingleObject(handle, TERMINATE_WAIT.as_millis() as u32) == 0
            && GetExitCodeProcess(handle, &mut code) != 0
    };
    if exited { Some(code as i32) } else { None }
}

/// PTY를 `Shutdown`이나 자식 종료까지 돌린다. 끝난 자식의 종료 코드를 돌려주고, 알 수 없으면 None이다.
///
/// `Shutdown`으로 끝낼 때도 자식을 직접 끝내고 기다려 종료 코드를 얻는다.
fn pump(
    mut pty: Pty,
    poller: &Arc<Poller>,
    rx: &Receiver<PtyMsg>,
    mut on_data: impl FnMut(&[u8]),
) -> Option<i32> {
    let mut interest = Event::readable(0);
    let mode = PollMode::Level;
    let mut exit_code: Option<i32> = None;
    // 등록하지 못하면 읽을 수 없으므로 곧바로 끝낸다.
    if unsafe { pty.register(poller, interest, mode) }.is_ok() {
        let mut events = Events::with_capacity(NonZeroUsize::new(64).unwrap());
        let mut buf = vec![0u8; 64 * 1024];
        let mut write_q: VecDeque<Vec<u8>> = VecDeque::new();
        'outer: loop {
            events.clear();
            if let Err(e) = poller.wait(&mut events, None) {
                if e.kind() == ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    PtyMsg::Input(b) => write_q.push_back(b),
                    PtyMsg::Resize(c, r) => pty.on_resize(window_size(c, r)),
                    PtyMsg::Shutdown => {
                        exit_code = terminate_child(&mut pty);
                        break 'outer;
                    }
                }
            }
            for ev in events.iter() {
                match ev.key {
                    poll_key::CHILD => {
                        if let Some(tty::ChildEvent::Exited(status)) = pty.next_child_event() {
                            exit_code = exit_code_of(status);
                            // 남은 출력을 비운 뒤 끝낸다.
                            loop {
                                match pty.reader().read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => on_data(&buf[..n]),
                                    Err(_) => break,
                                }
                            }
                            break 'outer;
                        }
                    }
                    poll_key::READ_WRITE if ev.readable => loop {
                        match pty.reader().read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => on_data(&buf[..n]),
                            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => break,
                            Err(_) => break,
                        }
                    },
                    _ => {}
                }
            }
            // 쓰기 대기열 비우기.
            'w: while let Some(front) = write_q.front_mut() {
                match pty.writer().write(front) {
                    Ok(0) => break 'w,
                    Ok(n) => {
                        if n >= front.len() {
                            write_q.pop_front();
                        } else {
                            front.drain(..n);
                        }
                    }
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => break 'w,
                    Err(_) => break 'w,
                }
            }
            let want_write = !write_q.is_empty();
            if want_write != interest.writable {
                interest.writable = want_write;
                let _ = pty.reregister(poller, interest, mode);
            }
        }
        let _ = pty.deregister(poller);
    }
    exit_code
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_a_panic_while_handling_output_ends_the_terminal_with_an_unknown_exit_code() {
        let spec = ShellSpec {
            program: "bash".to_string(),
            args: vec!["-c".to_string(), "echo hi; sleep 30".to_string()],
            working_directory: None,
            env: Vec::new(),
        };
        let (sender, receiver) = channel();
        let _handle = spawn(
            &spec,
            80,
            24,
            |_data| panic!("output handler failed"),
            move |exit_code| {
                let _ = sender.send(exit_code);
            },
        )
        .expect("spawn");
        // 출력을 다루다 스레드가 죽어도 터미널은 끝난 것으로 알려져야 한다(알리지 않으면 살아 있는 채 출력이 영원히 멎는다).
        let exit_code = receiver.recv_timeout(Duration::from_secs(10)).expect("exit is reported after the panic");
        assert_eq!(exit_code, None);
    }
}
