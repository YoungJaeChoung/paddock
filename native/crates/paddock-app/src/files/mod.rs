//! 사이드바 파일 목록: 폴더를 작업 스레드에서 읽고 변경을 감시하는 서비스(`FileService`)와, 목록 그리기·우클릭 메뉴.
//!
//! 읽기·감시·메뉴 규칙은 창 없이 시험할 수 있어 항상 빌드되고, 그리기는 `gui` 기능일 때만 빌드된다.
//! 파일 시스템 일은 모두 서비스의 작업 스레드가 하고 창은 완성된 `Snapshot`만 받는다 — 느린 파일 시스템에서도 창이 멈추지 않는다.
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use paddock_core::file_tree::{Directory, child_path};

pub mod active;
#[cfg(feature = "gui")]
pub mod clipboard;
/// 파일 편집 흐름(저장·저장 여부 묻기·디스크 변경 띠·줄로 가기).
#[cfg(feature = "gui")]
pub mod editing;
/// 자동 점검용 편집 명령·상태 글.
#[cfg(all(feature = "gui", feature = "automation"))]
pub mod editing_automation;
/// 편집 흐름의 화면 조각(상태 줄 저장 표시·디스크 변경 띠).
#[cfg(feature = "gui")]
pub mod editing_view;
pub mod image_load;
#[cfg(feature = "gui")]
pub mod image_view;
#[cfg(feature = "gui")]
pub mod images;
#[cfg(feature = "gui")]
pub mod list_view;
#[cfg(feature = "gui")]
pub mod markdown_view;
pub mod menu_model;
#[cfg(feature = "gui")]
pub mod notice_view;
pub mod open;
pub mod ops;
#[cfg(feature = "gui")]
pub mod pane;
pub mod pdf_render;
#[cfg(feature = "gui")]
pub mod pdf_view;
/// `.pen` 파일을 탭 대신 Pen 앱에 넘기는 흐름.
#[cfg(feature = "gui")]
pub mod pen;
pub mod reader;
/// 검토 메모(고른 줄에 메모를 남겨 에이전트에게 한 번에 보낸다).
#[cfg(feature = "gui")]
pub mod review_notes;
#[cfg(feature = "gui")]
pub mod row;
pub mod state;
pub mod svg_load;
#[cfg(feature = "gui")]
pub mod tab_flow;
#[cfg(feature = "gui")]
pub mod text_view;
#[cfg(feature = "gui")]
pub mod transfer;
pub mod watcher;
pub mod write_service;

use reader::{ListingCache, folder_exists, read_tree};
use watcher::{FolderWatcher, is_wsl_share};

/// 읽기 작업 스레드가 오류로 멈췄을 때 목록 자리에 보이는 안내.
pub const STOPPED_NOTICE: &str = "The file list stopped unexpectedly. Refresh to try again.";

/// 파일 사건이 처음 온 뒤 목록을 다시 읽기까지 모으는 시간.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// 파일 사건이 오지 않는 폴더(WSL 공유)의 수정 시각을 비교하는 주기. 사건 감시 중인 폴더는 이 주기마다 폴더가 아직 있는지만 본다.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 서비스에 보내는 요청: 어느 폴더의 어떤 펼침 상태를 읽을지.
#[derive(Clone, Debug)]
pub struct Request {
    /// 목록이 보여 줄 폴더. None이면 감시를 멈추고 쉰다.
    pub folder: Option<String>,
    /// 펼친 폴더의 경로들.
    pub expanded: HashSet<String>,
    /// 목록에서 뺄 이름.
    pub hidden: HashSet<String>,
    /// 읽어 둔 목록을 모두 버리고 디스크에서 다시 읽는다(Refresh).
    pub force: bool,
}

/// 서비스가 창에 돌려주는 읽기 결과.
pub struct Snapshot {
    pub folder: String,
    pub directory: Directory,
    /// 서비스가 시작한 뒤 폴더를 디스크에서 실제로 읽은 횟수.
    pub n_listings: usize,
    /// 읽기를 한 스레드. 창 스레드가 아님을 진단 기록에서 확인하는 데 쓴다.
    pub read_thread: ThreadId,
    /// 이번 읽기에 걸린 시간.
    pub elapsed: Duration,
    /// 폴더를 파일 사건 대신 주기 비교로 지켜보는 중이다(WSL 공유이거나 감시를 시작하지 못함).
    pub polled: bool,
}

enum Command {
    Request(Request),
    /// 감시가 폴더 안 변경을 알렸다.
    Changed,
    /// 창이 가려졌다(true)·다시 보인다(false).
    Pause(bool),
    Stop,
}

/// 폴더 읽기·감시를 맡은 작업 스레드와 주고받는 창구.
pub struct FileService {
    commands: Sender<Command>,
    snapshots: Receiver<Snapshot>,
    worker: Option<JoinHandle<()>>,
    /// 감시가 폴더 안 변경(파일 내용만 바뀐 것 포함)을 알린 횟수. 목록이 달라지지 않아도 올라가므로 열린 파일의 변경 확인 신호로 쓴다.
    events: Arc<AtomicUsize>,
}

impl FileService {
    /// 작업 스레드를 시작한다. 요청이 오기 전에는 아무것도 읽지 않는다.
    pub fn start() -> Self {
        let (commands, inbox) = channel();
        let (outbox, snapshots) = channel();
        let wake = commands.clone();
        let events = Arc::new(AtomicUsize::new(0));
        let counted = events.clone();
        let worker = std::thread::Builder::new()
            .name("paddock-files".to_string())
            .spawn(move || {
                // 읽기 중 오류(패닉)는 이 스레드 안에서 끝낸다. 결과를 보내는 쪽이 닫히므로 창은 `try_recv`의 `Disconnected`로 알아챈다.
                if catch_unwind(AssertUnwindSafe(|| Worker::new(wake, outbox, counted).run(&inbox))).is_err() {
                    crate::app_log::event("files: the folder reader stopped unexpectedly");
                }
            })
            .ok();
        Self { commands, snapshots, worker, events }
    }

    /// 감시가 지금까지 알린 변경 횟수. 값이 달라졌으면 감시하는 폴더 안 어딘가가 바뀐 것이다(목록에 보이는 이름이 같아도).
    pub fn n_events(&self) -> usize {
        self.events.load(Ordering::Relaxed)
    }

    /// 요청을 보낸다. 기다리지 않고 돌아오며, 결과는 `try_recv`로 받는다.
    pub fn request(
        &self,
        request: Request,
    ) {
        let _ = self.commands.send(Command::Request(request));
    }

    /// 창이 가려진 동안(최소화) 주기 비교를 멈추고, 다시 보이면 한 번 바로 확인한다. 파일 사건 감시는 그대로 둔다.
    pub fn set_paused(
        &self,
        paused: bool,
    ) {
        let _ = self.commands.send(Command::Pause(paused));
    }

    /// 도착한 읽기 결과 하나. 아직 없으면 `Empty`, 작업 스레드가 사라져 더 올 것이 없으면 `Disconnected`다(스레드를 만들지 못한 경우 포함).
    pub fn try_recv(&self) -> Result<Snapshot, TryRecvError> {
        self.snapshots.try_recv()
    }

    /// 작업 스레드가 없는 서비스. 스레드가 오류로 사라진 상태를 시험에서 만든다.
    #[cfg(test)]
    pub fn stopped() -> Self {
        let (commands, _) = channel();
        let (_, snapshots) = channel();
        Self { commands, snapshots, worker: None, events: Arc::new(AtomicUsize::new(0)) }
    }
}

impl Drop for FileService {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        // 읽는 중일 수 있으니 기다리지 않는다. 스레드는 Stop을 받는 즉시 끝난다.
        drop(self.worker.take());
    }
}

/// 작업 스레드의 상태.
struct Worker {
    wake: Sender<Command>,
    outbox: Sender<Snapshot>,
    cache: ListingCache,
    request: Option<Request>,
    watcher: Option<FolderWatcher>,
    polled: bool,
    /// 창이 가려져 있어 주기 비교를 멈춘 상태.
    paused: bool,
    /// 마지막으로 보낸 읽기 결과. 같은 결과를 되풀이해 보내지 않으려고 쥔다.
    last: Option<(String, Directory)>,
    /// 파일 사건을 처음 받은 시각. 모으는 시간이 지나면 읽는다.
    changed_at: Option<Instant>,
    /// 감시가 사건을 알릴 때마다 올리는 횟수(`FileService::n_events`와 같은 값).
    events: Arc<AtomicUsize>,
}

impl Worker {
    fn new(
        wake: Sender<Command>,
        outbox: Sender<Snapshot>,
        events: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            wake,
            events,
            outbox,
            cache: ListingCache::default(),
            request: None,
            watcher: None,
            polled: false,
            paused: false,
            last: None,
            changed_at: None,
        }
    }

    fn run(
        mut self,
        inbox: &Receiver<Command>,
    ) {
        loop {
            let wait = match (&self.changed_at, &self.request) {
                (Some(since), _) => Some((*since + DEBOUNCE).saturating_duration_since(Instant::now())),
                (None, Some(request)) if request.folder.is_some() && !(self.polled && self.paused) => {
                    Some(POLL_INTERVAL)
                }
                _ => None,
            };
            let received = match wait {
                Some(wait) => inbox.recv_timeout(wait),
                None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Command::Request(request)) => self.on_request(request),
                Ok(Command::Pause(paused)) => {
                    self.paused = paused;
                    if !paused {
                        self.poll_now();
                    }
                }
                Ok(Command::Changed) => {
                    self.changed_at.get_or_insert_with(Instant::now);
                }
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => self.on_timeout(),
            }
        }
    }

    /// 새 요청: 폴더가 바뀌면 감시 대상을 바꾸고, 어느 경우든 결과를 한 번 보낸다.
    fn on_request(
        &mut self,
        request: Request,
    ) {
        let same_target =
            self.request.as_ref().is_some_and(|old| old.folder == request.folder && old.hidden == request.hidden);
        // 새로 고침은 죽은 감시도 다시 세운다(폴더가 지워졌다 다시 생기면 감시가 끊겨 있다).
        if !same_target || request.force {
            self.watch(&request);
        }
        if !same_target {
            self.last = None;
        }
        if request.force {
            self.cache.clear();
        }
        self.changed_at = None;
        self.request = Some(request);
        self.read(true);
    }

    /// 모으는 시간이 끝났거나 주기 비교 시각이 되었을 때.
    fn on_timeout(&mut self) {
        if self.changed_at.take().is_some() {
            self.read(false);
        } else if self.polled && !self.paused {
            self.poll_now();
        } else if !self.polled && self.root_changed() {
            // 파일 사건 감시 중에도 폴더가 없어진 것은 사건으로 오지 않을 수 있다(Windows는 열린 감시가 삭제를 늦춘다).
            self.read(false);
        }
    }

    /// 주기 비교 한 번: 지켜보는 폴더가 다시 생겼으면 파일 사건 감시를 다시 시도하고,
    /// 폴더가 생기거나 없어졌거나 읽어 둔 폴더의 수정 시각이 달라졌으면 다시 읽는다.
    fn poll_now(&mut self) {
        if self.polled {
            if let Some(request) = self.request.clone()
                && request.folder.as_deref().is_some_and(|folder| !is_wsl_share(folder) && folder_exists(folder))
                && self.watcher.is_none()
            {
                self.watch(&request);
            }
            if self.root_changed() || self.cache.is_stale() {
                self.read(false);
            }
        }
    }

    /// 지켜보는 폴더가 있는지가 마지막으로 보낸 결과와 달라졌는지.
    fn root_changed(&self) -> bool {
        let found = self.last.as_ref().map(|(_, directory)| directory.found);
        self.request
            .as_ref()
            .and_then(|request| request.folder.as_deref())
            .is_some_and(|folder| Some(folder_exists(folder)) != found)
    }

    /// `request.folder`의 감시를 시작한다. WSL 공유이거나 시작하지 못하면 감시 없이 주기 비교로 지켜본다.
    fn watch(
        &mut self,
        request: &Request,
    ) {
        self.watcher = None;
        self.polled = false;
        if let Some(folder) = &request.folder {
            let wake = self.wake.clone();
            let events = self.events.clone();
            let watching = if is_wsl_share(folder) {
                None
            } else {
                FolderWatcher::start(Path::new(folder), request.hidden.clone(), move || {
                    events.fetch_add(1, Ordering::Relaxed);
                    let _ = wake.send(Command::Changed);
                })
                .ok()
            };
            self.polled = watching.is_none();
            self.watcher = watching;
        }
    }

    /// 지금 요청대로 폴더를 읽어 결과를 보낸다. `always`가 거짓이면 이전 결과와 같을 때는 보내지 않는다.
    fn read(
        &mut self,
        always: bool,
    ) {
        if let Some(Request { folder: Some(folder), expanded, hidden, .. }) = &self.request {
            let started = Instant::now();
            let directory = read_tree(folder, expanded, hidden, &mut self.cache);
            // 폴더가 없어지면 파일 사건 감시는 죽는다. 주기 비교로 내려가 다시 생기기를 기다린다.
            if !directory.found && self.watcher.take().is_some() {
                self.polled = true;
            }
            let unchanged =
                self.last.as_ref().is_some_and(|(last_folder, last)| last_folder == folder && *last == directory);
            if always || !unchanged {
                self.last = Some((folder.clone(), directory.clone()));
                let _ = self.outbox.send(Snapshot {
                    folder: folder.clone(),
                    directory,
                    n_listings: self.cache.n_listings(),
                    read_thread: std::thread::current().id(),
                    elapsed: started.elapsed(),
                    polled: self.polled,
                });
            }
        }
    }
}

/// `parent` 폴더에 새 파일이나 폴더를 만들어 그 경로를 돌려준다. 같은 이름이 이미 있으면 덮어쓰지 않고 안내 글을 돌려준다.
///
/// 파일 시스템 일이라 창 스레드가 아닌 곳에서 부른다. 이름 규칙 검사는 부르기 전에 끝나 있어야 한다.
pub fn create_entry(
    parent: &str,
    name: &str,
    is_dir: bool,
) -> Result<String, String> {
    let path = child_path(parent, name);
    let created = if is_dir {
        std::fs::create_dir(&path)
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map(drop)
    };
    match created {
        Ok(()) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(format!("\u{201c}{name}\u{201d} already exists. Choose another name."))
        }
        Err(error) => Err(format!("Could not create \u{201c}{name}\u{201d}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;

    /// 시험이 만든 폴더를 읽기 스레드가 "안정된" 폴더로 보게 하려고 기다리는 시간(`reader::SETTLE`보다 길다).
    const SETTLED: Duration = Duration::from_millis(1300);

    fn request_for(
        folder: &Path,
        force: bool,
    ) -> Request {
        Request {
            folder: Some(folder.to_string_lossy().to_string()),
            expanded: HashSet::new(),
            hidden: HashSet::new(),
            force,
        }
    }

    /// 조건을 만족하는 스냅샷이 올 때까지 기다린다(없으면 마지막으로 받은 것을 돌려준다).
    fn wait_for(
        service: &FileService,
        within: Duration,
        mut done: impl FnMut(&Snapshot) -> bool,
    ) -> Option<Snapshot> {
        let deadline = Instant::now() + within;
        let mut last = None;
        let mut finished = false;
        while !finished && Instant::now() < deadline {
            while let Ok(snapshot) = service.try_recv() {
                finished = finished || done(&snapshot);
                last = Some(snapshot);
                if finished {
                    break;
                }
            }
            if !finished {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        last
    }

    fn names(snapshot: &Snapshot) -> Vec<String> {
        snapshot.directory.children.iter().map(|entry| entry.name.clone()).collect()
    }

    #[test]
    fn test_c_d52_a_new_file_appears_after_one_debounce() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let service = FileService::start();
        service.request(request_for(dir.path(), false));
        let first = wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        assert_eq!(names(&first), ["a.txt"]);
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let after = wait_for(&service, Duration::from_secs(1), |snapshot| names(snapshot).len() == 2)
            .expect("a snapshot within one second");
        assert_eq!(names(&after), ["a.txt", "b.txt"]);
    }

    #[test]
    fn test_c_d52_unchanged_folder_is_not_reread() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::thread::sleep(SETTLED);
        let service = FileService::start();
        service.request(request_for(dir.path(), false));
        let first = wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        assert_eq!(first.n_listings, 1);
        service.request(request_for(dir.path(), false));
        let second = wait_for(&service, Duration::from_secs(5), |_| true).expect("second snapshot");
        assert_eq!(second.n_listings, 1, "the same modification time reads no folder again");
        service.request(request_for(dir.path(), true));
        let forced = wait_for(&service, Duration::from_secs(5), |_| true).expect("forced snapshot");
        assert_eq!(forced.n_listings, 2, "Refresh reads from the disk again");
    }

    #[test]
    fn test_c_d52_reading_runs_off_the_ui_thread() {
        let dir = tempfile::tempdir().unwrap();
        let service = FileService::start();
        service.request(request_for(dir.path(), false));
        let snapshot = wait_for(&service, Duration::from_secs(5), |_| true).expect("snapshot");
        assert_ne!(snapshot.read_thread, std::thread::current().id());
        assert!(snapshot.directory.found && snapshot.directory.empty);
    }

    #[test]
    fn test_c_d52_changes_under_a_hidden_name_do_not_cause_a_new_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let service = FileService::start();
        let mut request = request_for(dir.path(), false);
        request.hidden = [".git".to_string()].into_iter().collect();
        service.request(request);
        let first = wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        assert_eq!(names(&first), ["a.txt"]);
        std::fs::write(dir.path().join(".git").join("index"), "x").unwrap();
        let none = wait_for(&service, Duration::from_millis(900), |_| true);
        assert!(none.is_none(), "a .git write must not re-read the folder");
    }

    #[test]
    fn test_c_files_27_a_folder_that_does_not_exist_yet_shows_up_when_it_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let later = dir.path().join("later");
        let service = FileService::start();
        service.request(request_for(&later, false));
        let first = wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        assert!(!first.directory.found);
        std::fs::create_dir(&later).unwrap();
        std::fs::write(later.join("a.txt"), "a").unwrap();
        let after = wait_for(&service, Duration::from_secs(8), |snapshot| snapshot.directory.found)
            .expect("a snapshot once the folder exists");
        assert_eq!(names(&after), ["a.txt"]);
    }

    #[test]
    fn test_c_files_28_a_watched_folder_that_is_deleted_and_made_again_shows_up_again() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let service = FileService::start();
        service.request(request_for(&root, false));
        wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        std::fs::remove_dir_all(&root).unwrap();
        let gone = wait_for(&service, Duration::from_secs(8), |snapshot| !snapshot.directory.found);
        assert!(gone.is_some_and(|snapshot| !snapshot.directory.found), "the deletion is reported");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("b.txt"), "b").unwrap();
        let back = wait_for(&service, Duration::from_secs(8), |snapshot| snapshot.directory.found)
            .expect("a snapshot once the folder is back");
        assert_eq!(names(&back), ["b.txt"]);
    }

    #[test]
    fn test_c_files_29_refresh_sets_up_a_dead_watch_again() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let service = FileService::start();
        service.request(request_for(&root, false));
        wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        std::fs::remove_dir_all(&root).unwrap();
        wait_for(&service, Duration::from_secs(8), |snapshot| !snapshot.directory.found);
        std::fs::create_dir(&root).unwrap();
        service.request(request_for(&root, true));
        let refreshed =
            wait_for(&service, Duration::from_secs(5), |snapshot| snapshot.directory.found).expect("refresh snapshot");
        assert!(!refreshed.polled, "the refresh watches the folder with file events again");
        std::fs::write(root.join("c.txt"), "c").unwrap();
        let after = wait_for(&service, Duration::from_secs(1), |snapshot| names(snapshot).len() == 1)
            .expect("an event-driven snapshot within a second");
        assert_eq!(names(&after), ["c.txt"]);
    }

    #[test]
    fn test_c_files_31_polling_stops_while_the_window_is_hidden_and_checks_once_when_it_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let later = dir.path().join("later");
        let service = FileService::start();
        service.request(request_for(&later, false));
        let first = wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        assert!(first.polled && !first.directory.found);
        service.set_paused(true);
        std::fs::create_dir(&later).unwrap();
        assert!(
            wait_for(&service, Duration::from_millis(4500), |_| true).is_none(),
            "no polling while the window is hidden"
        );
        service.set_paused(false);
        let shown = wait_for(&service, Duration::from_secs(1), |snapshot| snapshot.directory.found);
        assert!(shown.is_some_and(|snapshot| snapshot.directory.found), "showing the window checks right away");
    }

    #[test]
    fn test_c_files_16_expanded_folders_are_read_in_place_and_hidden_names_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(dir.path().join("__pycache__")).unwrap();
        std::fs::write(dir.path().join("src").join("a.js"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let src = paddock_core::file_tree::child_path(&root, "src");
        let hidden: HashSet<String> = [".git", "__pycache__"].iter().map(|name| name.to_string()).collect();
        let mut cache = reader::ListingCache::default();
        let closed = reader::read_tree(&root, &HashSet::new(), &hidden, &mut cache);
        let rows: Vec<String> = paddock_core::file_tree::rows(&closed).into_iter().map(|row| row.name).collect();
        assert_eq!(rows, ["src", "README.md"]);
        let expanded: HashSet<String> = [src].into_iter().collect();
        let open = reader::read_tree(&root, &expanded, &hidden, &mut cache);
        let rows: Vec<(usize, String)> =
            paddock_core::file_tree::rows(&open).into_iter().map(|row| (row.depth, row.name)).collect();
        assert_eq!(rows, [(0, "src".to_string()), (1, "a.js".to_string()), (0, "README.md".to_string())]);
    }

    #[test]
    fn test_c_files_17_a_missing_folder_reads_as_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone").to_string_lossy().to_string();
        let mut cache = reader::ListingCache::default();
        let directory = reader::read_tree(&missing, &HashSet::new(), &HashSet::new(), &mut cache);
        assert!(!directory.found);
        assert_eq!(
            paddock_core::file_tree::rows(&directory)[0].notice,
            Some(paddock_core::file_tree::NOT_FOUND_NOTICE)
        );
    }

    #[test]
    fn test_c_files_18_creating_an_entry_refuses_a_name_that_exists_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().to_string_lossy().to_string();
        let made = create_entry(&parent, "n.txt", false).expect("new file");
        assert!(Path::new(&made).is_file());
        std::fs::write(&made, "keep").unwrap();
        let error = create_entry(&parent, "n.txt", false).unwrap_err();
        assert_eq!(error, "\u{201c}n.txt\u{201d} already exists. Choose another name.");
        assert_eq!(std::fs::read_to_string(&made).unwrap(), "keep");
        let folder = create_entry(&parent, "sub", true).expect("new folder");
        assert!(Path::new(&folder).is_dir());
        assert!(create_entry(&parent, "sub", true).is_err());
    }

    #[test]
    fn test_c_files_19_wsl_shares_are_polled_instead_of_watched() {
        assert!(watcher::is_wsl_share(r"\\wsl.localhost\Ubuntu\home\me"));
        assert!(watcher::is_wsl_share(r"\\WSL$\Ubuntu"));
        assert!(watcher::is_wsl_share("//wsl.localhost/Ubuntu/home"));
        assert!(!watcher::is_wsl_share(r"C:\Users\me"));
        assert!(!watcher::is_wsl_share(r"\\server\share"));
        assert!(!watcher::is_wsl_share("/home/me"));
    }

    /// 폴더 `a`(안에 `b/`)를 펼친 목록에서 `a`의 이름을 바꾼 뒤 `a2/b/`에 파일을 만들면 한 디바운스 안에 그 행이 나타나는지.
    fn watch_follows_rename(rename: impl Fn(&Path, &Path)) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a").join("b")).unwrap();
        std::fs::write(dir.path().join("a").join("b").join("old.txt"), "o").unwrap();
        // 펼친 폴더의 경로는 목록 행이 쓰는 철자(`child_path`: 부모의 구분자 철자)와 같아야 한다 — Windows에서 `join("a/b")`는 구분자가 섞인다.
        let expanded = |names: &[&str]| -> HashSet<String> {
            names
                .iter()
                .map(|name| {
                    name.split('/')
                        .fold(dir.path().to_string_lossy().to_string(), |parent, part| child_path(&parent, part))
                })
                .collect()
        };
        let service = FileService::start();
        let mut request = request_for(dir.path(), false);
        request.expanded = expanded(&["a", "a/b"]);
        service.request(request.clone());
        wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        rename(&dir.path().join("a"), &dir.path().join("a2"));
        // 앱은 이름을 바꾼 직후 펼침 기억을 새 경로로 옮겨 다시 요청한다.
        request.expanded = expanded(&["a2", "a2/b"]);
        service.request(request);
        wait_for(&service, Duration::from_secs(5), |snapshot| names(snapshot) == ["a2"]).expect("renamed folder row");
        std::fs::write(dir.path().join("a2").join("b").join("c.txt"), "c").unwrap();
        let seen = wait_for(&service, Duration::from_secs(1), |snapshot| {
            let leaf = |snapshot: &Snapshot| {
                snapshot
                    .directory
                    .children
                    .first()
                    .and_then(|a2| a2.directory.as_ref())
                    .and_then(|a2| a2.children.first())
                    .and_then(|b| b.directory.as_ref())
                    .map(|b| b.children.iter().map(|e| e.name.clone()).collect::<Vec<_>>())
            };
            leaf(snapshot).is_some_and(|files| files.contains(&"c.txt".to_string()))
        })
        .expect("a snapshot within one second of creating the file");
        assert!(seen.directory.children.first().is_some_and(|a2| a2.name == "a2"));
    }

    #[test]
    fn test_c_d70_watch_follows_a_renamed_folder_two_levels_down() {
        // 터미널 `mv`와 같은 운영체제 이름 바꾸기, 그리고 앱의 이름 바꾸기(`ops::rename_entry`) 둘 다.
        watch_follows_rename(|from, to| std::fs::rename(from, to).unwrap());
        watch_follows_rename(|from, to| {
            let new_name = to.file_name().unwrap().to_string_lossy().to_string();
            ops::rename_entry(&from.to_string_lossy(), &new_name).unwrap();
        });
    }

    #[test]
    fn test_c_live_2_a_change_that_only_edits_a_files_content_still_counts_as_a_watch_event() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "one").unwrap();
        let service = FileService::start();
        service.request(request_for(dir.path(), false));
        wait_for(&service, Duration::from_secs(5), |_| true).expect("first snapshot");
        std::thread::sleep(Duration::from_millis(300));
        let before = service.n_events();
        std::fs::write(dir.path().join("a.txt"), "two, longer").unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while service.n_events() == before && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(service.n_events() > before, "the editor of an open file is told within a second");
        // 이름 목록이 같아 새 읽기 결과는 오지 않는다 — 그래서 목록 변화만 보면 열린 파일의 변경을 놓친다.
        assert!(wait_for(&service, Duration::from_millis(600), |_| true).is_none());
    }
}
