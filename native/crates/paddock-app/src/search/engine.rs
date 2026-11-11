//! 폴더 검색 엔진: 작업 스레드 하나가 폴더를 훑으며(.gitignore와 `files.exclude`를 지킨다) ripgrep의 검색 라이브러리로 일치 줄을 찾아
//! 100 ms마다 묶음으로 창에 보낸다. 창은 `SearchJob`을 버리거나 `cancel`하는 것으로 검색을 멈춘다.
//!
//! 자식 프로세스(rg.exe)를 쓰지 않으므로 같은 코드가 모든 운영체제에서 돌고, WSL 폴더는 오늘의 파일 목록처럼 공유 경로를 직접 읽는다.
use std::collections::HashSet;
use std::io;
use std::panic::{AssertUnwindSafe, UnwindSafe};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use paddock_core::file_tree::child_path;
use paddock_core::search_model::{
    LineMatch, MAX_FILE_BYTES, MAX_RESULTS, STOPPED_NOTICE, SearchOptions, build_matcher, is_excluded_name, preview_of,
};

/// 찾은 결과를 창으로 묶어 보내는 간격.
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// 검색 요청: 어느 폴더에서 무엇을 어떤 옵션으로 찾는지.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub folder: String,
    pub query: String,
    pub options: SearchOptions,
    /// 훑지 않을 이름(`files.exclude`에서 켜진 이름).
    pub hidden: HashSet<String>,
}

/// 일치가 있는 파일 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHit {
    pub path: String,
    /// 검색 폴더 기준 상대 경로(구분자 `/`).
    pub relative: String,
    pub matches: Vec<LineMatch>,
}

/// 작업 스레드가 창에 보내는 소식.
#[derive(Debug)]
pub enum Event {
    /// 지난 묶음 이후 찾은 파일들.
    Hits(Vec<FileHit>),
    /// 검색을 끝까지 마쳤다(또는 일치가 상한을 넘어 더 찾지 않는다). 취소된 검색은 이 소식을 보내지 않는다.
    Done { n_files: usize },
    /// 질의를 해석하지 못했다(정규식 오류 등). 문장은 사용자에게 보인다.
    Failed(String),
}

/// 돌고 있는 검색 하나. 버리면 취소된다.
pub struct SearchJob {
    cancel: Arc<AtomicBool>,
    events: Receiver<Event>,
}

impl SearchJob {
    /// 작업 스레드를 시작한다. 기다리지 않고 돌아오며, 소식은 `try_recv`로 받는다.
    pub fn start(request: SearchRequest) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (outbox, events) = channel();
        let token = cancel.clone();
        // 스레드를 만들지 못하면 outbox가 버려져 창은 끝났다는 소식 없는 연결 끊김을 실패로 본다.
        let _ = std::thread::Builder::new()
            .name("paddock-search".to_string())
            .spawn(move || guarded(&outbox, AssertUnwindSafe(|| run(&request, &token, &outbox))));
        Self { cancel, events }
    }

    /// 검색을 멈추게 한다. 작업 스레드는 다음 파일을 시작하기 전에 끝나고, 그 뒤로는 소식이 오지 않는다.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// 도착한 소식 하나. 아직 없으면 `Empty`, 작업 스레드가 끝나 더 올 것이 없으면 `Disconnected`다.
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for SearchJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// 작업이 오류(패닉)로 멈추면 창이 "끝났다는 소식 없이 멈춤"을 알 수 있게 실패 소식을 보낸다. 한 번의 검색 실패가 앱으로 번지지 않는다.
fn guarded(
    outbox: &Sender<Event>,
    work: impl FnOnce() + UnwindSafe,
) {
    if std::panic::catch_unwind(work).is_err() {
        let _ = outbox.send(Event::Failed(STOPPED_NOTICE.to_string()));
    }
}

/// 작업 스레드 본체: 질의를 해석하고 폴더를 훑는다.
fn run(
    request: &SearchRequest,
    cancel: &AtomicBool,
    outbox: &Sender<Event>,
) {
    match build_matcher(&request.query, request.options) {
        Ok(matcher) => walk(request, &matcher, cancel, outbox),
        Err(message) => {
            let _ = outbox.send(Event::Failed(message));
        }
    }
}

/// 폴더를 이름순으로 훑으며 파일마다 일치 줄을 모은다. 일치가 `MAX_RESULTS`를 넘기는 첫 줄을 찾으면 멈춘다(상한 안내를 켜기에 충분하다).
fn walk(
    request: &SearchRequest,
    matcher: &RegexMatcher,
    cancel: &AtomicBool,
    outbox: &Sender<Event>,
) {
    let hidden = request.hidden.clone();
    let mut walker = WalkBuilder::new(&request.folder);
    walker
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        // 저장소 폴더가 아니어도 .gitignore를 지킨다(작업 폴더가 저장소 안쪽 폴더일 때 위쪽 규칙도 따른다).
        .require_git(false)
        .follow_links(false)
        .max_filesize(Some(MAX_FILE_BYTES))
        .sort_by_file_name(|left, right| left.cmp(right))
        .filter_entry(move |entry| !entry.file_name().to_str().is_some_and(|name| is_excluded_name(name, &hidden)));
    let mut searcher = SearcherBuilder::new().binary_detection(BinaryDetection::quit(0)).line_number(true).build();
    let mut pending: Vec<FileHit> = Vec::new();
    let mut flushed_at = Instant::now();
    let mut n_matches = 0;
    let mut n_files = 0;
    let mut stopped = false;
    for entry in walker.build() {
        if cancel.load(Ordering::Relaxed) {
            stopped = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        n_files += 1;
        let mut found = Collector { matcher, matches: Vec::new(), n_remaining: MAX_RESULTS + 1 - n_matches };
        // 읽기 오류(권한·잠긴 파일·인코딩)는 그 파일만 건너뛴다.
        let _ = searcher.search_path(matcher, entry.path(), &mut found);
        if !found.matches.is_empty() {
            n_matches += found.matches.len();
            let relative = relative_of(&request.folder, entry.path());
            // 경로는 파일 목록 행과 같은 모양(폴더가 쓰는 구분자)으로 이어, 같은 파일이 두 탭으로 열리지 않게 한다.
            let path = relative.split('/').fold(request.folder.clone(), |parent, name| child_path(&parent, name));
            pending.push(FileHit { path, relative, matches: found.matches });
        }
        if n_matches > MAX_RESULTS {
            break;
        }
        if flushed_at.elapsed() >= FLUSH_INTERVAL && !pending.is_empty() {
            stopped = outbox.send(Event::Hits(std::mem::take(&mut pending))).is_err();
            flushed_at = Instant::now();
        }
        if stopped {
            break;
        }
    }
    if !stopped {
        if !pending.is_empty() {
            stopped = outbox.send(Event::Hits(pending)).is_err();
        }
        if !stopped {
            let _ = outbox.send(Event::Done { n_files });
        }
    }
}

/// 검색 폴더 기준 상대 경로(구분자 `/`).
fn relative_of(
    folder: &str,
    path: &Path,
) -> String {
    let relative = path.strip_prefix(folder).unwrap_or(path);
    relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// 파일 하나의 일치 줄을 모으는 받는 쪽. 줄마다 첫 일치 하나만 미리보기로 남기고, `n_remaining`개를 모으면 멈춘다.
struct Collector<'a> {
    matcher: &'a RegexMatcher,
    matches: Vec<LineMatch>,
    n_remaining: usize,
}

impl Sink for Collector<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        found: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        let line = String::from_utf8_lossy(found.bytes());
        let line = line.trim_end_matches(['\n', '\r']);
        // 글자가 깨진 줄은 대체 글자로 바뀐 글에서 다시 찾는다(바이트 위치가 어긋나지 않게).
        let range = match self.matcher.find(line.as_bytes()) {
            Ok(Some(matched)) => matched.start()..matched.end(),
            _ => 0..0,
        };
        self.matches.push(LineMatch { line: found.line_number().unwrap_or(0), preview: preview_of(line, range) });
        Ok(self.matches.len() < self.n_remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        folder: &Path,
        query: &str,
    ) -> SearchRequest {
        SearchRequest {
            folder: folder.to_string_lossy().to_string(),
            query: query.to_string(),
            options: SearchOptions::default(),
            hidden: [".git".to_string()].into_iter().collect(),
        }
    }

    /// 검색이 끝나(`Done`) 소식이 더 없을 때까지 받아 모든 파일의 일치를 돌려준다. 오류면 그 문장을 Err로 돌려준다.
    fn collect(job: &SearchJob) -> Result<(Vec<FileHit>, usize), String> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut hits = Vec::new();
        let mut outcome = None;
        while outcome.is_none() && Instant::now() < deadline {
            match job.try_recv() {
                Ok(Event::Hits(batch)) => hits.extend(batch),
                Ok(Event::Done { n_files }) => outcome = Some(Ok((hits.clone(), n_files))),
                Ok(Event::Failed(message)) => outcome = Some(Err(message)),
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Disconnected) => outcome = Some(Err("disconnected".to_string())),
            }
        }
        outcome.unwrap_or_else(|| Err("timeout".to_string()))
    }

    fn write(
        root: &Path,
        name: &str,
        text: &str,
    ) {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn test_c_d56_search_respects_gitignore_and_files_exclude() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/a.js", "let x = 1;\nneedle here\n");
        write(root, "b.txt", "no match\n");
        write(root, "ignored/c.js", "needle in an ignored folder\n");
        write(root, "skip.log", "needle in an ignored file\n");
        write(root, ".git/config", "needle in .git\n");
        write(root, ".gitignore", "ignored/\n*.log\n");
        write(root, "node_modules/d.js", "needle in excluded names\n");
        let mut with_exclude = request(root, "needle");
        with_exclude.hidden.insert("node_modules".to_string());
        let (hits, n_files) = collect(&SearchJob::start(with_exclude)).expect("a finished search");
        let found: Vec<(String, u64)> = hits.iter().map(|hit| (hit.relative.clone(), hit.matches[0].line)).collect();
        assert_eq!(found, [("src/a.js".to_string(), 2)]);
        assert!(n_files >= 2, "the walk reads the files that are not ignored");
        // `.gitignore`는 저장소 폴더가 아니어도 지킨다. `files.exclude`에 이름이 없으면 그 폴더도 훑는다.
        let (all, _) = collect(&SearchJob::start(request(root, "needle"))).expect("a finished search");
        let names: Vec<&str> = all.iter().map(|hit| hit.relative.as_str()).collect();
        assert_eq!(names, ["node_modules/d.js", "src/a.js"]);
    }

    #[test]
    fn test_c_d56_options_change_what_matches() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "Needle\nneedles\nneedle\na.b\n");
        let lines = |options: SearchOptions, query: &str| -> Vec<u64> {
            let mut asked = request(dir.path(), query);
            asked.options = options;
            let (hits, _) = collect(&SearchJob::start(asked)).expect("a finished search");
            hits.iter().flat_map(|hit| hit.matches.iter().map(|found| found.line)).collect()
        };
        assert_eq!(lines(SearchOptions::default(), "needle"), [1, 2, 3]);
        assert_eq!(lines(SearchOptions { case_sensitive: true, ..SearchOptions::default() }, "needle"), [2, 3]);
        assert_eq!(lines(SearchOptions { whole_word: true, ..SearchOptions::default() }, "needle"), [1, 3]);
        assert_eq!(lines(SearchOptions::default(), "a.b"), [4], "a dot is a plain dot unless the regex option is on");
        assert_eq!(lines(SearchOptions { regex: true, ..SearchOptions::default() }, "n.dle"), Vec::<u64>::new());
        assert_eq!(lines(SearchOptions { regex: true, ..SearchOptions::default() }, "ne+dle$"), [1, 3]);
    }

    #[test]
    fn test_a_panic_in_the_search_work_is_reported_as_a_failure() {
        let (outbox, events) = channel();
        guarded(&outbox, || panic!("the search work broke"));
        assert!(matches!(events.try_recv(), Ok(Event::Failed(message)) if message == STOPPED_NOTICE));
        guarded(&outbox, || {});
        assert!(events.try_recv().is_err(), "work that ends normally reports nothing here");
    }

    #[test]
    fn test_c_d56_a_broken_regex_is_reported_not_searched() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "x\n");
        let mut asked = request(dir.path(), "(");
        asked.options.regex = true;
        let message = collect(&SearchJob::start(asked)).expect_err("an invalid pattern");
        assert!(message.starts_with("Invalid search pattern"), "{message}");
    }

    #[test]
    fn test_c_d56_binary_files_and_large_files_are_skipped_and_the_limit_stops_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bin.dat", "needle\0binary\n");
        std::fs::write(dir.path().join("big.txt"), format!("needle\n{}", "x".repeat(MAX_FILE_BYTES as usize + 1)))
            .unwrap();
        for index in 0..30 {
            let text: String = (0..100).map(|line| format!("needle {index} {line}\n")).collect();
            write(dir.path(), &format!("f{index:02}.txt"), &text);
        }
        let (hits, _) = collect(&SearchJob::start(request(dir.path(), "needle"))).expect("a finished search");
        let n_matches: usize = hits.iter().map(|hit| hit.matches.len()).sum();
        assert!(hits.iter().all(|hit| hit.relative.starts_with('f')), "binary and oversize files are skipped");
        assert_eq!(n_matches, MAX_RESULTS + 1, "the walk stops at the first match past the limit");
    }

    #[test]
    fn test_c_d56_a_new_query_cancels_the_running_search() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..20_000 {
            write(dir.path(), &format!("d{}/f{index}.txt", index % 50), "alpha\nbeta\n");
        }
        let first = SearchJob::start(request(dir.path(), "alpha"));
        first.cancel();
        // 취소된 검색은 끝났다는 소식 없이 연결이 끊긴다.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_done = false;
        let mut disconnected = false;
        while !disconnected && Instant::now() < deadline {
            match first.try_recv() {
                Ok(Event::Done { .. }) => saw_done = true,
                Ok(_) => {}
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Disconnected) => disconnected = true,
            }
        }
        assert!(disconnected, "the worker thread ends after cancel");
        assert!(!saw_done, "a cancelled search never reports Done");
        let second = SearchJob::start(request(dir.path(), "beta"));
        let (hits, _) = collect(&second).expect("the new search runs to the end");
        assert!(hits.iter().all(|hit| hit.matches.iter().all(|found| found.preview.text == "beta")));
        assert_eq!(hits.len(), MAX_RESULTS + 1, "the new search runs to the limit");
    }
}
