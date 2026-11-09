//! 폴더 검색: 파일 목록 자리에 입력칸과 결과 트리를 보인다. 찾기는 `engine`의 작업 스레드가, 결과 모델과 규칙은 `core::search_model`이,
//! 그리기는 `view`가, 창을 움직이는 동작은 `actions`가 맡는다.
use std::collections::HashSet;
use std::sync::mpsc::TryRecvError;

#[cfg(feature = "gui")]
use gpui_kit::Entity;
#[cfg(feature = "gui")]
use gpui_kit::component::input::InputState;
use paddock_core::search_model::{ResultRow, Results, STOPPED_NOTICE, SearchOptions, rows};

#[cfg(feature = "gui")]
pub mod actions;
#[cfg(all(feature = "gui", feature = "automation"))]
pub mod automation;
pub mod engine;
#[cfg(feature = "gui")]
pub mod view;

use engine::{Event, SearchJob, SearchRequest};

/// 창이 쥐는 폴더 검색 상태.
#[derive(Default)]
pub struct SearchState {
    /// 검색 보기가 열려 있다.
    pub open: bool,
    /// 질의 입력칸. 검색 보기를 처음 열 때 만든다.
    #[cfg(feature = "gui")]
    pub input: Option<Entity<InputState>>,
    pub options: SearchOptions,
    /// 바꾸기 줄(`⇄`)이 열려 있다.
    pub replace_open: bool,
    /// 바꿀 글 입력칸. 바꾸기 줄을 처음 열 때 만든다.
    #[cfg(feature = "gui")]
    pub replace_input: Option<Entity<InputState>>,
    /// 훑을 폴더. 파일 구획 폴더나 선택한 터미널의 폴더가 바뀌면 따라 바뀌고, 그때 결과는 비운다.
    scope: Option<String>,
    /// 마지막으로 시작한 검색의 질의. 결과가 어느 질의의 것인지 보이는 데 쓴다.
    query: String,
    job: Option<SearchJob>,
    results: Results,
    collapsed: HashSet<usize>,
    rows: Vec<ResultRow>,
    running: bool,
    /// 검색을 끝까지 마쳤다(결과가 모두 모였다).
    finished: bool,
    /// 질의를 해석하지 못한 이유.
    failure: Option<String>,
    n_files_searched: usize,
    /// 지금까지 시작한 검색 수(자동 점검용 진단 값).
    n_started: usize,
}

impl SearchState {
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &Results {
        &self.results
    }

    /// 화면에 그릴 결과 행(파일 행과 접히지 않은 파일의 일치 줄 행).
    pub fn rows(&self) -> &[ResultRow] {
        &self.rows
    }

    /// 검색 작업이 돌고 있다.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// 검색을 끝까지 마쳤다.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn n_files_searched(&self) -> usize {
        self.n_files_searched
    }

    pub fn n_started(&self) -> usize {
        self.n_started
    }

    /// 훑을 폴더를 정한다. 폴더가 달라지면 도는 검색과 결과를 버린다(다른 폴더의 결과가 새 폴더의 것처럼 보이지 않게). 바뀌었으면 true다.
    pub fn set_scope(
        &mut self,
        scope: Option<String>,
    ) -> bool {
        let changed = scope != self.scope;
        if changed {
            self.scope = scope;
            self.clear_results();
        }
        changed
    }

    /// 새 검색을 시작한다. 돌던 검색은 멈추고 결과는 비운다. 질의가 비었으면 시작하지 않고 결과만 비운다.
    pub fn start(
        &mut self,
        request: Option<SearchRequest>,
    ) {
        self.clear_results();
        if let Some(request) = request {
            self.query = request.query.clone();
            self.job = Some(SearchJob::start(request));
            self.running = true;
            self.n_started += 1;
        }
    }

    /// 돌던 검색을 멈추고 결과·오류를 비운다. 닫은 뒤에는 결과가 메모리에 남지 않는다.
    pub fn clear_results(&mut self) {
        self.job = None;
        self.results = Results::default();
        self.collapsed.clear();
        self.rows.clear();
        self.running = false;
        self.finished = false;
        self.failure = None;
        self.n_files_searched = 0;
        self.query.clear();
    }

    /// 작업 스레드가 보낸 소식을 모두 반영한다. 화면에 보이는 것이 바뀌었으면 true다.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        let mut ended = false;
        while let Some(job) = self.job.as_ref()
            && !ended
        {
            match job.try_recv() {
                Ok(Event::Hits(files)) => {
                    for file in files {
                        self.results.add_file(file.path, file.relative, file.matches);
                    }
                    changed = true;
                }
                Ok(Event::Done { n_files }) => {
                    self.n_files_searched = n_files;
                    self.finished = true;
                    ended = true;
                    changed = true;
                }
                Ok(Event::Failed(message)) => {
                    self.failure = Some(message);
                    ended = true;
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                // 끝났다는 소식 없이 작업 스레드가 사라졌다(만들지 못함 포함): 결과를 믿을 수 없으니 실패로 알린다.
                Err(TryRecvError::Disconnected) => {
                    self.failure = Some(STOPPED_NOTICE.to_string());
                    ended = true;
                    changed = true;
                }
            }
        }
        if ended {
            self.job = None;
            self.running = false;
        }
        if changed {
            self.rows = rows(&self.results, &self.collapsed);
        }
        changed
    }

    /// 파일 `file`의 일치 줄을 접거나 펼친다.
    pub fn toggle_file(
        &mut self,
        file: usize,
    ) {
        if !self.collapsed.remove(&file) {
            self.collapsed.insert(file);
        }
        self.rows = rows(&self.results, &self.collapsed);
    }

    /// 질의 옵션 하나를 뒤집는다: `case`·`word`·`regex`. 알 수 없는 이름이면 false다.
    pub fn toggle_option(
        &mut self,
        name: &str,
    ) -> bool {
        let known = matches!(name, "case" | "word" | "regex");
        match name {
            "case" => self.options.case_sensitive = !self.options.case_sensitive,
            "word" => self.options.whole_word = !self.options.whole_word,
            "regex" => self.options.regex = !self.options.regex,
            _ => {}
        }
        known
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn request(
        folder: &std::path::Path,
        query: &str,
    ) -> SearchRequest {
        SearchRequest {
            folder: folder.to_string_lossy().to_string(),
            query: query.to_string(),
            options: SearchOptions::default(),
            hidden: HashSet::new(),
        }
    }

    fn pump_until_finished(state: &mut SearchState) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while state.is_running() && Instant::now() < deadline {
            state.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn test_a_second_search_replaces_the_first_one_and_its_results() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..300 {
            std::fs::write(dir.path().join(format!("f{index}.txt")), "alpha\nbeta\n").unwrap();
        }
        let mut state = SearchState::default();
        state.start(Some(request(dir.path(), "alpha")));
        state.start(Some(request(dir.path(), "beta")));
        pump_until_finished(&mut state);
        assert_eq!(state.n_started(), 2);
        assert!(state.is_finished());
        assert_eq!(state.query(), "beta");
        assert_eq!(state.results().n_matches(), 300);
        assert!(state.results().files().iter().all(|file| file.matches.iter().all(|found| found.line == 2)));
        // 닫으면 결과가 메모리에서 사라진다.
        state.clear_results();
        assert_eq!((state.results().n_matches(), state.rows().len(), state.is_running()), (0, 0, false));
    }

    #[test]
    fn test_changing_the_scope_drops_the_results_of_the_old_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "needle\n").unwrap();
        let mut state = SearchState::default();
        assert!(state.set_scope(Some(dir.path().to_string_lossy().to_string())));
        state.start(Some(request(dir.path(), "needle")));
        pump_until_finished(&mut state);
        assert_eq!(state.rows().len(), 2);
        assert!(!state.set_scope(Some(dir.path().to_string_lossy().to_string())), "the same folder keeps its results");
        assert_eq!(state.rows().len(), 2);
        assert!(state.set_scope(None));
        assert_eq!(state.rows().len(), 0);
    }

    #[test]
    fn test_collapsing_a_file_hides_its_lines_and_options_toggle_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "needle\nneedle\n").unwrap();
        let mut state = SearchState::default();
        state.start(Some(request(dir.path(), "needle")));
        pump_until_finished(&mut state);
        assert_eq!(state.rows().len(), 3);
        state.toggle_file(0);
        assert_eq!(state.rows().len(), 1);
        state.toggle_file(0);
        assert_eq!(state.rows().len(), 3);
        assert!(state.toggle_option("regex") && state.options.regex);
        assert!(state.toggle_option("regex") && !state.options.regex);
        assert!(!state.toggle_option("fuzzy"));
    }

    #[test]
    fn test_an_invalid_pattern_ends_the_search_with_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = SearchState::default();
        let mut asked = request(dir.path(), "(");
        asked.options.regex = true;
        state.start(Some(asked));
        pump_until_finished(&mut state);
        assert!(!state.is_running() && !state.is_finished());
        assert!(state.failure().is_some_and(|message| message.starts_with("Invalid search pattern")));
    }
}
