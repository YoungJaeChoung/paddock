//! git 작업 스레드: 창이 보낸 일(상태 읽기·쓰기·diff·감시 대상 바꾸기)을 차례로 하고 결과를 사건으로 돌려준다.
//!
//! 프로세스를 띄우는 일은 모두 이 스레드에서 하므로 느린 파일 시스템(WSL 9P)에서도 창이 멈추지 않는다.
//! 한 번에 하나만 실행하고, 같은 일을 겹쳐 띄울지는 창 쪽의 `Gate`가 정한다.
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, UnwindSafe};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use paddock_core::git_status::Change;

use super::status::{RepoError, RepoStatus, read_diff, read_repo, write};
use crate::files::watcher::{FolderWatcher, is_git_noise, is_wsl_share};

/// 파일 사건 감시를 걸지 못했을 때 저장소 루트를 주기로 살피는 간격(파일 목록의 주기 비교와 같다).
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// 주기 비교의 표지: 루트 폴더·`.git/index`·`.git/HEAD`의 수정 시각. 달라지면 변경이 있었다고 본다.
/// 파일 안 내용만 바뀐 변경은 이 표지에 나타나지 않는다(파일 목록의 주기 비교와 같은 한계).
pub fn poll_signature(root: &str) -> [Option<SystemTime>; 3] {
    let modified = |path: std::path::PathBuf| std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
    let base = Path::new(root);
    [modified(base.to_path_buf()), modified(base.join(".git").join("index")), modified(base.join(".git").join("HEAD"))]
}

/// 작업이 오류(패닉)로 멈췄을 때 창이 사용자에게 보일 안내.
pub const STOPPED_NOTICE: &str = "Git stopped unexpectedly.";

/// 쓰기 일의 종류. 결과를 받은 창이 무엇을 할지(커밋 입력칸 비우기 등) 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    Stage,
    Unstage,
    Commit,
    Discard,
}

/// 창이 작업 스레드에 보내는 일.
pub enum Job {
    /// `folder`가 속한 저장소의 변경을 읽는다. `root`를 알면 다시 묻지 않는다.
    Status {
        folder: String,
        root: Option<String>,
    },
    Write {
        kind: WriteKind,
        root: String,
        args: Vec<String>,
    },
    Diff {
        key: String,
        root: String,
        change: Change,
    },
    /// 저장소 루트의 변경 감시를 바꾼다. None이면 끈다. WSL 폴더는 파일 사건이 오지 않아 감시하지 않는다.
    Watch {
        root: Option<String>,
        hidden: HashSet<String>,
    },
}

impl Job {
    /// 이 일이 오류로 멈췄을 때 창에 알릴 실패 표지. 일을 실행하기 전에 만든다(실행이 일을 소비한다).
    fn failure(&self) -> FailedJob {
        match self {
            Job::Status { folder, .. } => FailedJob::Status { folder: folder.clone() },
            Job::Write { kind, .. } => FailedJob::Write { kind: *kind },
            Job::Diff { key, .. } => FailedJob::Diff { key: key.clone() },
            Job::Watch { .. } => FailedJob::Watch,
        }
    }
}

/// 오류로 멈춘 일. 창이 그 일을 기다리는 표지(진행 중 쓰기·읽기 중 게이트)를 풀 수 있게 일의 종류와 열쇠를 담는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailedJob {
    Status { folder: String },
    Write { kind: WriteKind },
    Diff { key: String },
    Watch,
}

/// 작업 스레드가 창에 돌려주는 결과.
pub enum Event {
    Status {
        folder: String,
        result: Result<RepoStatus, RepoError>,
    },
    Written {
        kind: WriteKind,
        result: Result<(), String>,
    },
    Diff {
        key: String,
        result: Result<String, String>,
    },
    /// 감시 중인 저장소 안에서 표시에 영향을 주는 변경이 있었다.
    FsChanged,
    /// 일 하나가 오류로 멈췄다. 한 번의 실패가 다른 일과 앱으로 번지지 않는다.
    Failed(FailedJob),
}

/// 작업 스레드와 주고받는 창구.
pub struct GitService {
    jobs: Option<Sender<Job>>,
    events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}

impl GitService {
    /// 작업 스레드를 시작한다. 일이 오기 전에는 아무것도 띄우지 않는다.
    pub fn start() -> Self {
        let (jobs, inbox) = channel::<Job>();
        let (outbox, events) = channel::<Event>();
        let worker = std::thread::Builder::new()
            .name("paddock-git".to_string())
            .spawn(move || {
                let mut watcher: Option<FolderWatcher> = None;
                // 감시를 걸지 못한 루트와 마지막으로 본 표지. 있으면 주기로 살핀다.
                let mut polling: Option<(String, [Option<SystemTime>; 3])> = None;
                loop {
                    let received = match &polling {
                        Some(_) => inbox.recv_timeout(POLL_INTERVAL),
                        None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    let job = match received {
                        Ok(job) => job,
                        Err(RecvTimeoutError::Timeout) => {
                            if let Some((root, last)) = polling.as_mut() {
                                let now = poll_signature(root);
                                if now != *last {
                                    *last = now;
                                    let _ = outbox.send(Event::FsChanged);
                                }
                            }
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let failure = job.failure();
                    guarded(&outbox, failure, AssertUnwindSafe(|| handle(job, &outbox, &mut watcher, &mut polling)));
                }
                drop(watcher);
            })
            .ok();
        Self { jobs: Some(jobs), events, worker }
    }

    /// 일을 보낸다. 기다리지 않고 돌아온다.
    pub fn send(
        &self,
        job: Job,
    ) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
    }

    /// 도착한 결과 하나. 아직 없으면 `Empty`, 작업 스레드가 사라져 더 올 것이 없으면 `Disconnected`다(스레드를 만들지 못한 경우 포함).
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        self.events.try_recv()
    }

    /// 작업 스레드가 없는 서비스. 스레드가 오류로 사라진 상태를 시험에서 만든다.
    #[cfg(test)]
    pub fn stopped() -> Self {
        let (_, events) = channel::<Event>();
        Self { jobs: None, events, worker: None }
    }
}

impl Drop for GitService {
    fn drop(&mut self) {
        // 일을 보내는 쪽을 닫으면 스레드는 지금 하는 일을 마치는 대로 끝난다. 프로세스가 길어질 수 있어 기다리지 않는다.
        self.jobs = None;
        drop(self.worker.take());
    }
}

/// 일 하나를 실행하고 결과를 `outbox`로 돌려준다. 감시자와 주기 비교 표지는 작업 스레드가 쥔 것을 빌려 바꾼다.
fn handle(
    job: Job,
    outbox: &Sender<Event>,
    watcher: &mut Option<FolderWatcher>,
    polling: &mut Option<(String, [Option<SystemTime>; 3])>,
) {
    match job {
        Job::Status { folder, root } => {
            let result = read_repo(&folder, root.as_deref());
            let _ = outbox.send(Event::Status { folder, result });
        }
        Job::Write { kind, root, args } => {
            // 커밋은 훅·서명 입력을 기다릴 수 있어 시간 한도를 두지 않는다.
            let limit = (kind != WriteKind::Commit).then_some(super::runner::TIME_LIMIT);
            let result = write(&root, &args, limit);
            let _ = outbox.send(Event::Written { kind, result });
        }
        Job::Diff { key, root, change } => {
            let result = read_diff(&root, &change);
            let _ = outbox.send(Event::Diff { key, result });
        }
        Job::Watch { root, hidden } => {
            let wake = outbox.clone();
            *polling = None;
            *watcher = None;
            if let Some(root) = root.filter(|root| !is_wsl_share(root)) {
                match FolderWatcher::start_ignoring(
                    Path::new(&root),
                    move |bases, path| is_git_noise(bases, path, &hidden),
                    move || {
                        let _ = wake.send(Event::FsChanged);
                    },
                ) {
                    Ok(started) => *watcher = Some(started),
                    Err(_) => {
                        // 감시를 걸지 못하면 삼키지 않고 기록한 뒤 주기 비교로 지켜본다.
                        crate::app_log::event("git: could not watch the repository, checking it periodically");
                        let signature = poll_signature(&root);
                        *polling = Some((root, signature));
                    }
                }
            }
        }
    }
}

/// 일이 오류(패닉)로 멈추면 창이 그 일을 기다리지 않게 실패 소식을 보낸다. 작업 스레드는 다음 일을 계속 받는다.
fn guarded(
    outbox: &Sender<Event>,
    failure: FailedJob,
    work: impl FnOnce() + UnwindSafe,
) {
    if std::panic::catch_unwind(work).is_err() {
        let _ = outbox.send(Event::Failed(failure));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempHome;

    #[test]
    fn test_a_root_that_cannot_be_watched_is_checked_periodically_and_a_change_is_reported() {
        let _home = TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("not-yet");
        let service = GitService::start();
        // 아직 없는 폴더는 파일 사건 감시를 걸 수 없다.
        service.send(Job::Watch { root: Some(root.to_string_lossy().to_string()), hidden: HashSet::new() });
        std::thread::sleep(Duration::from_millis(300));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut reported = false;
        while !reported && std::time::Instant::now() < deadline {
            reported = matches!(service.try_recv(), Ok(Event::FsChanged));
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(reported, "the periodic check noticed the new repository");
        let log = std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs").join("app.log"))
            .unwrap_or_default();
        assert!(log.contains("could not watch the repository"), "{log}");
    }

    #[test]
    fn test_the_poll_signature_changes_with_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let before = poll_signature(&root);
        assert_eq!(before[1], None);
        std::fs::write(dir.path().join(".git").join("index"), "x").unwrap();
        assert_ne!(poll_signature(&root), before);
    }

    #[test]
    fn test_a_panic_in_one_git_job_is_reported_for_that_job_only() {
        let (outbox, events) = channel();
        let failure = Job::Write { kind: WriteKind::Commit, root: "/r".to_string(), args: Vec::new() }.failure();
        guarded(&outbox, failure, AssertUnwindSafe(|| panic!("the git work broke")));
        assert!(matches!(events.try_recv(), Ok(Event::Failed(FailedJob::Write { kind: WriteKind::Commit }))));
        guarded(&outbox, FailedJob::Watch, AssertUnwindSafe(|| {}));
        assert!(events.try_recv().is_err(), "work that ends normally reports nothing here");
    }
}
