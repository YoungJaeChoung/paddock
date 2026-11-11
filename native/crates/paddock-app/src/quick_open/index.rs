//! 빠른 열기의 색인: 작업 스레드 하나가 폴더를 훑어(.gitignore와 `files.exclude`를 지킨다) 파일의 상대 경로를 모아 100 ms마다 묶음으로 창에 보낸다.
//! 창은 `IndexJob`을 버리는 것으로 훑기를 멈춘다. 색인은 열 때마다 새로 만들고 닫으면 버린다(메모리에 남기지 않는다).
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;
use paddock_core::quick_open::MAX_INDEXED;
use paddock_core::search_model::is_excluded_name;

/// 모은 경로를 창으로 묶어 보내는 간격.
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// 작업 스레드가 창에 보내는 소식.
#[derive(Debug)]
pub enum IndexEvent {
    /// 지난 묶음 이후 찾은 파일의 상대 경로(구분자 `/`).
    Files(Vec<String>),
    /// 다 훑었다. `capped`면 `MAX_INDEXED`에 닿아 더 담지 않았다.
    Done { capped: bool },
}

/// 돌고 있는 색인 하나. 버리면 멈춘다.
pub struct IndexJob {
    cancel: Arc<AtomicBool>,
    events: Receiver<IndexEvent>,
}

impl IndexJob {
    /// 작업 스레드를 시작한다. 기다리지 않고 돌아오며 소식은 `try_recv`로 받는다.
    pub fn start(
        folder: String,
        hidden: HashSet<String>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (outbox, events) = channel();
        let token = cancel.clone();
        // 스레드를 만들지 못하면 outbox가 버려져 창은 연결 끊김을 끝으로 본다.
        let _ = std::thread::Builder::new()
            .name("paddock-quick-open".to_string())
            .spawn(move || walk(&folder, &hidden, &token, &outbox));
        Self { cancel, events }
    }

    /// 도착한 소식 하나. 아직 없으면 `Empty`, 작업이 끝나 더 올 것이 없으면 `Disconnected`다.
    pub fn try_recv(&self) -> Result<IndexEvent, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for IndexJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// 폴더를 이름순으로 훑는다. 검색 엔진과 같은 규칙(.gitignore·`files.exclude`)이고 파일 크기는 따지지 않는다.
fn walk(
    folder: &str,
    hidden: &HashSet<String>,
    cancel: &AtomicBool,
    outbox: &Sender<IndexEvent>,
) {
    let hidden = hidden.clone();
    let mut walker = WalkBuilder::new(folder);
    walker
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_name(|left, right| left.cmp(right))
        .filter_entry(move |entry| !entry.file_name().to_str().is_some_and(|name| is_excluded_name(name, &hidden)));
    let mut pending: Vec<String> = Vec::new();
    let mut n_files = 0;
    let mut flushed_at = Instant::now();
    let mut stopped = false;
    let mut capped = false;
    for entry in walker.build() {
        if cancel.load(Ordering::Relaxed) {
            stopped = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if n_files >= MAX_INDEXED {
            capped = true;
            break;
        }
        n_files += 1;
        pending.push(relative_of(folder, entry.path()));
        if flushed_at.elapsed() >= FLUSH_INTERVAL {
            stopped = outbox.send(IndexEvent::Files(std::mem::take(&mut pending))).is_err();
            flushed_at = Instant::now();
            if stopped {
                break;
            }
        }
    }
    if !stopped {
        if !pending.is_empty() {
            stopped = outbox.send(IndexEvent::Files(pending)).is_err();
        }
        if !stopped {
            let _ = outbox.send(IndexEvent::Done { capped });
        }
    }
}

/// 폴더 기준 상대 경로(구분자 `/`).
fn relative_of(
    folder: &str,
    path: &Path,
) -> String {
    let relative = path.strip_prefix(folder).unwrap_or(path);
    relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(job: &IndexJob) -> (Vec<String>, Option<bool>) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut files = Vec::new();
        let mut done = None;
        while done.is_none() && Instant::now() < deadline {
            match job.try_recv() {
                Ok(IndexEvent::Files(mut more)) => files.append(&mut more),
                Ok(IndexEvent::Done { capped }) => done = Some(capped),
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        (files, done)
    }

    #[test]
    fn test_c_d72_quick_open_index_respects_gitignore_and_excludes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        for file in ["src/a.js", "src/b.js", "README.md", ".git/config", "node_modules/x.js", "target/out", "debug.log"]
        {
            std::fs::write(root.join(file), "x").unwrap();
        }
        let hidden: HashSet<String> = [".git", "node_modules"].iter().map(|name| name.to_string()).collect();
        let job = IndexJob::start(root.to_string_lossy().to_string(), hidden);
        let (files, done) = collect(&job);
        assert_eq!(done, Some(false));
        assert_eq!(files, [".gitignore", "README.md", "src/a.js", "src/b.js"]);
    }

    #[test]
    fn test_c_d72_the_index_stops_at_the_limit_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..(MAX_INDEXED + 5) {
            std::fs::write(dir.path().join(format!("f{index:05}.txt")), "").unwrap();
        }
        let job = IndexJob::start(dir.path().to_string_lossy().to_string(), HashSet::new());
        let (files, done) = collect(&job);
        assert_eq!((files.len(), done), (MAX_INDEXED, Some(true)));
    }
}
