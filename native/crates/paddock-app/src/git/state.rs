//! 창이 쥐는 Git 상태: 보이는 폴더의 저장소 읽기 결과, 파일 목록 표시 조회표, 실행 시점 판정(`Gate`), 작업 스레드 창구.
use std::time::Instant;

use paddock_core::file_marks::FileMarks;
use paddock_core::file_tree::{child_path, hidden_names};
use paddock_core::git_args;
use paddock_core::git_paths::{GitTarget, target_of};
use paddock_core::git_status::{Change, Section};
use paddock_core::work_model::{folder_identity, folder_key, is_inside};

use super::gate::{Gate, Trigger};
use super::service::{Event, GitService, Job, WriteKind};
use super::status::{RepoError, RepoStatus};
use crate::settings::Settings;

/// 보이는 폴더의 저장소 읽기 상태.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repo {
    /// 아직 읽지 않았거나 읽는 중이다.
    Reading,
    /// 폴더가 어느 저장소에도 속하지 않는다.
    NoRepository,
    /// git을 실행할 수 없거나 읽기가 실패했다. 값은 사용자에게 보일 문장이다.
    Unavailable(String),
    Ready(RepoStatus),
}

/// 쓰기 일의 결과 한 건. 창이 꺼내 가며 화면에 반영한다.
#[derive(Debug, PartialEq, Eq)]
pub struct WriteResult {
    pub kind: WriteKind,
    pub result: Result<(), String>,
}

/// Git 상태.
pub struct GitState {
    /// 첫 폴더를 볼 때 시작한다. 폴더가 한 번도 보이지 않은 실행에는 스레드를 만들지 않는다.
    service: Option<GitService>,
    folder: Option<String>,
    gate: Gate,
    repo: Repo,
    marks: FileMarks,
    /// status 결과가 바뀔 때마다 오르는 번호. 열려 있는 비교 탭이 낡았는지 가르는 데 쓴다.
    generation: u64,
    /// 이 폴더를 본 뒤 status를 띄운 횟수(자동 점검 진단).
    n_status_runs: usize,
    n_writes_in_flight: usize,
    /// 커밋이 진행 중이다(훅·서명을 기다리는 동안 단추에 진행 표시를 하고 다시 누르지 못하게 한다).
    committing: bool,
    /// 변경을 감시하는 폴더. 저장소를 알면 그 루트이고, 아직 저장소가 아니면 보이는 폴더 자신이다(나중에 `git init`이 되면 `.git/HEAD` 사건으로 알아챈다).
    watching: Option<String>,
    written: Vec<WriteResult>,
    diffs: Vec<(String, Result<String, String>)>,
}

impl Default for GitState {
    fn default() -> Self {
        Self {
            service: None,
            folder: None,
            gate: Gate::new(false),
            repo: Repo::Reading,
            marks: FileMarks::default(),
            generation: 0,
            n_status_runs: 0,
            n_writes_in_flight: 0,
            committing: false,
            watching: None,
            written: Vec::new(),
            diffs: Vec::new(),
        }
    }
}

impl GitState {
    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// 파일 목록 행에 붙일 표시 조회표. 저장소를 읽기 전이나 저장소가 아니면 비어 있다.
    pub fn marks(&self) -> &FileMarks {
        &self.marks
    }

    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn n_status_runs(&self) -> usize {
        self.n_status_runs
    }

    /// 읽은 저장소. 없으면 None이다.
    pub fn ready(&self) -> Option<&RepoStatus> {
        match &self.repo {
            Repo::Ready(status) => Some(status),
            _ => None,
        }
    }

    /// 보기 줄 배지에 보일 변경 수.
    pub fn n_changes(&self) -> usize {
        self.ready().map_or(0, |repo| repo.status.n_changes())
    }

    /// 브랜치 단추에 보일 이름.
    pub fn branch_label(&self) -> Option<String> {
        self.ready().and_then(|repo| repo.status.head_label())
    }

    /// 폴더가 WSL 배포판 안인지.
    pub fn is_wsl(&self) -> bool {
        self.folder.as_deref().is_some_and(|folder| matches!(target_of(&folder_key(folder)), GitTarget::Wsl { .. }))
    }

    /// 보이는 폴더를 정한다. 폴더가 바뀌면 이전 폴더의 저장소 상태를 버리고 새 폴더를 한 번 읽을 준비를 한다.
    pub fn show(
        &mut self,
        folder: Option<&str>,
        now: Instant,
    ) {
        // 이미 읽은 저장소 안의 다른 폴더로 옮긴 것이면 루트·표시·게이트·감시를 그대로 둔다(`cd` 때마다 다시 읽지 않는다).
        let inside_known_repo = self.ready().zip(folder).is_some_and(|(repo, folder)| is_inside(&repo.root, folder));
        if inside_known_repo {
            self.folder = folder.map(str::to_string);
        } else if folder != self.folder.as_deref() {
            self.folder = folder.map(str::to_string);
            self.repo = Repo::Reading;
            self.marks = FileMarks::default();
            self.generation += 1;
            self.n_status_runs = 0;
            self.gate = Gate::new(self.is_wsl());
            if self.watching.take().is_some() {
                self.send(Job::Watch { root: None, hidden: Default::default() });
            }
            if self.folder.is_some() {
                self.gate.request(Trigger::Shown, now);
            }
        }
    }

    /// 사건을 알린다. 폴더가 없으면 무시한다.
    pub fn note(
        &mut self,
        trigger: Trigger,
        now: Instant,
    ) {
        if self.folder.is_some() {
            self.gate.request(trigger, now);
        }
    }

    /// `cwd`(터미널의 현재 폴더)가 보이는 폴더 안인지. 터미널 출력 사건을 이 폴더의 것으로 셀지 정하는 데 쓴다.
    pub fn covers(
        &self,
        cwd: &str,
    ) -> bool {
        let base = self.ready().map(|repo| repo.root.as_str()).or(self.folder.as_deref());
        base.is_some_and(|base| is_inside(base, cwd))
    }

    fn send(
        &mut self,
        job: Job,
    ) {
        self.service.get_or_insert_with(GitService::start).send(job);
    }

    /// 도착한 결과를 반영하고 실행할 때가 되었으면 status를 띄운다. 화면에 보이는 것이 바뀌었으면 true다.
    pub fn tick(
        &mut self,
        now: Instant,
    ) -> bool {
        let mut changed = false;
        while let Some(event) = self.service.as_ref().and_then(GitService::try_recv) {
            changed |= self.apply(event, now);
        }
        if let Some(folder) = self.folder.clone()
            && self.gate.is_due(now)
        {
            self.gate.started(now);
            self.n_status_runs += 1;
            let root = self.ready().map(|repo| repo.root.clone());
            self.send(Job::Status { folder, root });
        }
        changed
    }

    fn apply(
        &mut self,
        event: Event,
        now: Instant,
    ) -> bool {
        let mut changed = false;
        match event {
            Event::Status { folder, result }
                if Some(&folder) == self.folder.as_ref()
                    || self.ready().is_some_and(|repo| is_inside(&repo.root, &folder)) =>
            {
                self.gate.finished();
                let next = match result {
                    Ok(status) => Repo::Ready(status),
                    Err(RepoError::NotRepository) => Repo::NoRepository,
                    Err(RepoError::GitMissing(message)) | Err(RepoError::Failed(message)) => Repo::Unavailable(message),
                };
                if next != self.repo {
                    // 실패는 종류만 기록한다(경로·메시지는 남기지 않는다).
                    match &next {
                        Repo::NoRepository => crate::app_log::event("git: status skipped (not a repository)"),
                        Repo::Unavailable(_) => crate::app_log::event("git: status failed (git unavailable or error)"),
                        Repo::Reading | Repo::Ready(_) => {}
                    }
                    self.marks = match &next {
                        Repo::Ready(status) => FileMarks::new(&status.root, &status.status.changes),
                        _ => FileMarks::default(),
                    };
                    self.generation += 1;
                    self.repo = next;
                    changed = true;
                }
                self.sync_watch();
            }
            Event::Status { .. } => {}
            Event::FsChanged => self.gate.request(Trigger::Change, now),
            Event::Written { kind, result } => {
                self.n_writes_in_flight = self.n_writes_in_flight.saturating_sub(1);
                if kind == WriteKind::Commit {
                    self.committing = false;
                }
                self.gate.request(Trigger::Written, now);
                self.written.push(WriteResult { kind, result });
                changed = true;
            }
            Event::Diff { key, result } => {
                self.diffs.push((key, result));
                changed = true;
            }
        }
        changed
    }

    /// 네이티브 폴더의 감시 대상을 맞춘다: 저장소를 읽었으면 그 루트, 저장소가 아니면 보이는 폴더. 같은 대상이면 아무것도 하지 않는다.
    fn sync_watch(&mut self) {
        let target = match &self.repo {
            Repo::Ready(repo) => Some(repo.root.clone()),
            Repo::NoRepository | Repo::Unavailable(_) => self.folder.clone(),
            Repo::Reading => None,
        };
        if !self.is_wsl() && target.is_some() && target != self.watching {
            let hidden = hidden_names(Settings::load().values());
            self.watching = target.clone();
            self.send(Job::Watch { root: target, hidden });
        }
    }

    /// 쓰기 결과 하나를 꺼낸다.
    pub fn take_written(&mut self) -> Option<WriteResult> {
        (!self.written.is_empty()).then(|| self.written.remove(0))
    }

    /// 커밋이 진행 중이다.
    pub fn is_committing(&self) -> bool {
        self.committing
    }

    /// 쓰기가 진행 중이다(단추를 겹쳐 누르지 못하게 한다).
    pub fn is_writing(&self) -> bool {
        self.n_writes_in_flight > 0
    }

    /// 끝난 diff 읽기 하나를 꺼낸다.
    pub fn take_diff(&mut self) -> Option<(String, Result<String, String>)> {
        (!self.diffs.is_empty()).then(|| self.diffs.remove(0))
    }

    fn write(
        &mut self,
        kind: WriteKind,
        args: Vec<String>,
    ) {
        if let Some(root) = self.ready().map(|repo| repo.root.clone()) {
            self.n_writes_in_flight += 1;
            self.send(Job::Write { kind, root, args });
        }
    }

    fn has_commit(&self) -> bool {
        self.ready().is_some_and(|repo| repo.status.oid.is_some())
    }

    pub fn stage(
        &mut self,
        path: &str,
    ) {
        self.write(WriteKind::Stage, git_args::stage(&[path.to_string()]));
    }

    pub fn stage_all(&mut self) {
        self.write(WriteKind::Stage, git_args::stage_all());
    }

    /// 구획 머리의 `+`: 모두 스테이징하거나(`paths`가 None), 주어진 경로들만 스테이징한다.
    pub fn stage_group(
        &mut self,
        paths: Option<Vec<String>>,
    ) {
        match paths {
            Some(paths) => self.write(WriteKind::Stage, git_args::stage(&paths)),
            None => self.stage_all(),
        }
    }

    pub fn unstage(
        &mut self,
        path: &str,
    ) {
        let args = git_args::unstage(&[path.to_string()], self.has_commit());
        self.write(WriteKind::Unstage, args);
    }

    pub fn unstage_all(&mut self) {
        let args = git_args::unstage_all(self.has_commit());
        self.write(WriteKind::Unstage, args);
    }

    pub fn commit(
        &mut self,
        message: &str,
    ) {
        self.committing = self.ready().is_some();
        self.write(WriteKind::Commit, git_args::commit(message));
    }

    pub fn discard(
        &mut self,
        change: &Change,
    ) {
        self.write(WriteKind::Discard, git_args::discard(change));
    }

    /// 변경 하나의 diff 읽기를 시킨다. 결과는 `take_diff`로 `key`와 함께 온다.
    pub fn request_diff(
        &mut self,
        key: String,
        change: Change,
    ) {
        if let Some(root) = self.ready().map(|repo| repo.root.clone()) {
            self.send(Job::Diff { key, root, change });
        }
    }

    /// 지금 읽은 변경 목록에서 `section`의 `n`번째(0부터) 행.
    pub fn nth_in(
        &self,
        section: Section,
        n: usize,
    ) -> Option<&Change> {
        self.ready().and_then(|repo| repo.status.changes.iter().filter(|change| change.section == section).nth(n))
    }

    /// 절대 경로의 파일이 가진 변경 하나. `staged`가 참이면 스테이징된 변경, 거짓이면 작업 폴더 쪽(병합 충돌 포함)이다.
    pub fn change_for(
        &self,
        path: &str,
        staged: bool,
    ) -> Option<Change> {
        let repo = self.ready()?;
        let wanted = folder_identity(path);
        repo.status
            .changes
            .iter()
            .filter(|change| (change.section == Section::Staged) == staged && change.section != Section::Untracked)
            .find(|change| folder_identity(&child_path(&repo.root, &change.path)) == wanted)
            .cloned()
    }

    /// 파일 경로(절대)가 추적하지 않는 새 파일인지.
    pub fn is_untracked(
        &self,
        path: &str,
    ) -> bool {
        self.marks.file(path).is_some_and(|mark| mark.letter == paddock_core::git_status::Letter::Untracked)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use paddock_core::git_status::Section;

    use super::*;
    use crate::git::status::test_repo::{TestRepo, git_in};
    use crate::test_support::TempHome;

    /// 조건이 될 때까지 20 ms 간격으로 `tick`을 돌린다.
    fn drive(
        state: &mut GitState,
        within: Duration,
        mut done: impl FnMut(&GitState) -> bool,
    ) -> bool {
        let deadline = Instant::now() + within;
        let mut reached = done(state);
        while !reached && Instant::now() < deadline {
            state.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(20));
            reached = done(state);
        }
        reached
    }

    #[test]
    fn test_c_d52_status_runs_once_per_debounce_window() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("tracked.txt", "1\n");
        repo.commit_all();
        let mut state = GitState::default();
        state.show(Some(&repo.root()), Instant::now());
        assert!(drive(&mut state, Duration::from_secs(10), |s| s.ready().is_some()), "the first read");
        assert_eq!(state.n_status_runs(), 1);
        // 감시가 걸릴 시간을 준 뒤 파일 셋을 잇달아 만든다.
        drive(&mut state, Duration::from_millis(600), |_| false);
        for name in ["n1.txt", "n2.txt", "n3.txt"] {
            repo.write(name, "x\n");
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(
            drive(&mut state, Duration::from_secs(10), |s| s
                .ready()
                .is_some_and(|r| r.status.n_in(Section::Untracked) == 3)),
            "the three new files show up"
        );
        assert_eq!(state.n_status_runs(), 2, "three events in one window are one run");
        // 더 이상 변경이 없으면 더 돌지 않는다.
        drive(&mut state, Duration::from_secs(4), |_| false);
        assert_eq!(state.n_status_runs(), 2, "idle means no runs");
    }

    #[test]
    fn test_c_d52_a_stage_from_another_program_shows_up_through_the_index_event() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("a.txt", "1\n");
        repo.commit_all();
        repo.write("a.txt", "2\n");
        let mut state = GitState::default();
        state.show(Some(&repo.root()), Instant::now());
        assert!(drive(&mut state, Duration::from_secs(10), |s| s
            .ready()
            .is_some_and(|r| r.status.n_in(Section::Changes) == 1)));
        drive(&mut state, Duration::from_millis(600), |_| false);
        git_in(repo.path(), &["add", "a.txt"]);
        assert!(
            drive(&mut state, Duration::from_secs(10), |s| s
                .ready()
                .is_some_and(|r| r.status.n_in(Section::Staged) == 1)),
            "a git command typed in a terminal changes .git/index, which is watched"
        );
    }

    #[test]
    fn test_c_review_4_moving_between_folders_of_the_same_repository_keeps_the_root_the_marks_and_the_gate() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("sub/a.txt", "1\n");
        repo.write("other/b.txt", "1\n");
        repo.commit_all();
        repo.write("sub/a.txt", "2\n");
        let mut state = GitState::default();
        state.show(Some(&repo.root()), Instant::now());
        assert!(drive(&mut state, Duration::from_secs(10), |s| s.ready().is_some_and(|r| r.status.n_changes() == 1)));
        let runs = state.n_status_runs();
        let generation = state.generation();
        for folder in ["sub", "other", "sub", "other"] {
            state.show(Some(&format!("{}/{folder}", repo.root())), Instant::now());
            assert!(state.ready().is_some(), "the repository stays read while moving inside it");
            assert!(!state.marks().is_empty());
        }
        drive(&mut state, Duration::from_millis(4_000), |_| false);
        assert_eq!(state.n_status_runs(), runs, "cd between folders of one repository runs nothing");
        assert_eq!(state.generation(), generation);
        assert!(state.covers(&format!("{}/sub", repo.root())) && !state.covers("/elsewhere"));
        // 다른 저장소(또는 저장소 밖) 폴더로 가면 새로 읽는다.
        let other = tempfile::tempdir().unwrap();
        state.show(Some(&other.path().to_string_lossy()), Instant::now());
        assert_eq!(*state.repo(), Repo::Reading);
    }

    #[test]
    fn test_c_d52_a_folder_that_becomes_a_repository_later_is_noticed_through_dot_git_head() {
        let _home = TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let mut state = GitState::default();
        state.show(Some(&folder), Instant::now());
        assert!(
            drive(&mut state, Duration::from_secs(10), |s| *s.repo() == Repo::NoRepository),
            "not a repository yet"
        );
        drive(&mut state, Duration::from_millis(600), |_| false);
        git_in(dir.path(), &["init", "-q", "-b", "main"]);
        assert!(
            drive(&mut state, Duration::from_secs(10), |s| s.ready().is_some()),
            "git init creates .git/HEAD, which is watched"
        );
    }

    #[test]
    fn test_c_d52_a_wsl_folder_runs_only_for_the_gates_and_never_while_idle() {
        let _home = TempHome::new();
        let folder = r"\\wsl.localhost\Ubuntu\home\me\proj";
        let mut state = GitState::default();
        assert!(!state.covers(folder));
        state.show(Some(folder), Instant::now());
        assert!(state.is_wsl());
        assert!(state.covers(r"\\wsl.localhost\Ubuntu\home\me\proj\src"));
        assert!(!state.covers(r"\\wsl.localhost\Ubuntu\home\me\other"));
        drive(&mut state, Duration::from_secs(2), |s| s.n_status_runs() == 1);
        assert_eq!(state.n_status_runs(), 1, "showing the folder reads it once");
        drive(&mut state, Duration::from_millis(3_300), |_| false);
        assert_eq!(state.n_status_runs(), 1, "idle: no further runs");
        state.note(Trigger::Output, Instant::now());
        drive(&mut state, Duration::from_secs(2), |s| s.n_status_runs() == 2);
        assert_eq!(state.n_status_runs(), 2, "terminal output in the folder opens the gate");
    }

    #[test]
    fn test_c_d53_write_results_come_back_and_the_list_updates_after_a_stage() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("a.txt", "1\n");
        let mut state = GitState::default();
        state.show(Some(&repo.root()), Instant::now());
        assert!(drive(&mut state, Duration::from_secs(10), |s| s
            .ready()
            .is_some_and(|r| r.status.n_in(Section::Untracked) == 1)));
        state.stage("a.txt");
        assert!(state.is_writing());
        assert!(drive(&mut state, Duration::from_secs(10), |s| s
            .ready()
            .is_some_and(|r| r.status.n_in(Section::Staged) == 1)));
        let result = state.take_written().expect("a write result");
        assert_eq!((result.kind, result.result), (WriteKind::Stage, Ok(())));
        assert!(!state.is_writing());
    }
}
