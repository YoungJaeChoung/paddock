//! git 작업 스레드: 창이 보낸 일(상태 읽기·쓰기·diff·감시 대상 바꾸기)을 차례로 하고 결과를 사건으로 돌려준다.
//!
//! 프로세스를 띄우는 일은 모두 이 스레드에서 하므로 느린 파일 시스템(WSL 9P)에서도 창이 멈추지 않는다.
//! 한 번에 하나만 실행하고, 같은 일을 겹쳐 띄울지는 창 쪽의 `Gate`가 정한다.
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
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
                            polling = None;
                            watcher = None;
                            if let Some(root) = root.filter(|root| !is_wsl_share(root)) {
                                match FolderWatcher::start_ignoring(
                                    Path::new(&root),
                                    move |bases, path| is_git_noise(bases, path, &hidden),
                                    move || {
                                        let _ = wake.send(Event::FsChanged);
                                    },
                                ) {
                                    Ok(started) => watcher = Some(started),
                                    Err(_) => {
                                        // 감시를 걸지 못하면 삼키지 않고 기록한 뒤 주기 비교로 지켜본다.
                                        crate::app_log::event(
                                            "git: could not watch the repository, checking it periodically",
                                        );
                                        let signature = poll_signature(&root);
                                        polling = Some((root, signature));
                                    }
                                }
                            }
                        }
                    }
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

    /// 도착한 결과 하나. 없으면 None이다.
    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

impl Drop for GitService {
    fn drop(&mut self) {
        // 일을 보내는 쪽을 닫으면 스레드는 지금 하는 일을 마치는 대로 끝난다. 프로세스가 길어질 수 있어 기다리지 않는다.
        self.jobs = None;
        drop(self.worker.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempHome;

    #[test]
    fn test_c_review_9_a_root_that_cannot_be_watched_is_checked_periodically_and_a_change_is_reported() {
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
            reported = matches!(service.try_recv(), Some(Event::FsChanged));
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(reported, "the periodic check noticed the new repository");
        let log = std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs").join("app.log"))
            .unwrap_or_default();
        assert!(log.contains("could not watch the repository"), "{log}");
    }

    #[test]
    fn test_c_review_9_the_poll_signature_changes_with_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let before = poll_signature(&root);
        assert_eq!(before[1], None);
        std::fs::write(dir.path().join(".git").join("index"), "x").unwrap();
        assert_ne!(poll_signature(&root), before);
    }
}
