//! 저장소 읽기·쓰기 일: 폴더가 속한 저장소와 변경 목록 읽기, 스테이징·커밋·되돌리기, 변경 하나의 diff 읽기.
//!
//! 모두 작업 스레드에서 부르는 동기 함수다. 실행 위치와 경로 표기는 `git_paths`가, 인자는 `git_args`가 정한다.
use paddock_core::git_args;
use paddock_core::git_paths::{GitTarget, target_of, to_windows_output};
use paddock_core::git_status::{Change, Status, parse_porcelain_v2_z};
use paddock_core::work_model::folder_key;

use super::runner::{self, Output};

/// diff 글자가 이보다 크면 보이지 않고 안내만 한다.
pub const DIFF_LIMIT: usize = 2 * 1024 * 1024;

/// 저장소를 읽지 못한 이유.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoError {
    /// 폴더가 어느 저장소에도 속하지 않는다.
    NotRepository,
    /// git(WSL이면 그 배포판의 git)을 실행할 수 없다. 값은 사용자에게 보일 문장이다.
    GitMissing(String),
    /// 그 밖의 실패. 값은 git이 알린 첫 줄이다.
    Failed(String),
}

/// 읽은 저장소: 루트(폴더 key 꼴)와 변경 목록.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoStatus {
    pub root: String,
    pub status: Status,
}

fn missing_text(target: &GitTarget) -> String {
    match target {
        GitTarget::Wsl { distro, .. } => format!("Git is not available in {distro}"),
        GitTarget::Local { .. } => "Git is not available".to_string(),
    }
}

/// 실행 결과가 "git을 실행할 수 없음"인지 판정한다.
fn is_missing(output: &Output) -> bool {
    let lower = output.stderr.to_lowercase();
    output.code == Some(127)
        || lower.contains("execvpe(git)")
        || lower.contains("git: not found")
        || lower.contains("command not found")
}

/// `folder`가 속한 저장소의 루트를 묻는다.
fn read_root(
    target: &GitTarget,
    folder: &str,
) -> Result<String, RepoError> {
    match runner::run(target, folder, &git_args::toplevel(), Some(runner::TIME_LIMIT)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(RepoError::GitMissing(match target {
            GitTarget::Wsl { .. } => "WSL is not available".to_string(),
            GitTarget::Local { .. } => missing_text(target),
        })),
        Err(error) => Err(RepoError::Failed(error.to_string())),
        Ok(output) if output.code == Some(0) => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let converted = match target.wsl_root() {
                Some(root) => to_windows_output(&text, &root),
                None => text,
            };
            Ok(folder_key(&converted))
        }
        Ok(output) if output.stderr.contains("not a git repository") => Err(RepoError::NotRepository),
        Ok(output) if is_missing(&output) => Err(RepoError::GitMissing(missing_text(target))),
        Ok(output) if output.timed_out => Err(RepoError::Failed("git did not answer in time".to_string())),
        Ok(output) => Err(RepoError::Failed(output.first_error_line())),
    }
}

/// `folder`의 저장소와 변경 목록을 읽는다. `known_root`가 있으면 루트를 다시 묻지 않는다(저장소가 아니게 되면 호출자가 버린다).
pub fn read_repo(
    folder: &str,
    known_root: Option<&str>,
) -> Result<RepoStatus, RepoError> {
    let first = target_of(&folder_key(folder));
    let root = match known_root {
        Some(root) => Ok(root.to_string()),
        None => read_root(&first, folder),
    };
    root.and_then(|root| {
        let target = target_of(&root);
        match runner::run(&target, &root, &git_args::status(), Some(runner::TIME_LIMIT)) {
            Err(error) => Err(RepoError::Failed(error.to_string())),
            Ok(output) if output.code == Some(0) => {
                Ok(RepoStatus { root: root.clone(), status: parse_porcelain_v2_z(&output.stdout) })
            }
            Ok(output) if output.stderr.contains("not a git repository") => Err(RepoError::NotRepository),
            Ok(output) if is_missing(&output) => Err(RepoError::GitMissing(missing_text(&target))),
            Ok(output) => Err(RepoError::Failed(output.first_error_line())),
        }
    })
}

/// 저장소에 쓰는 git 명령 하나를 실행한다. 실패하면 사용자에게 보일 첫 줄을 돌려준다.
pub fn write(
    root: &str,
    args: &[String],
    limit: Option<std::time::Duration>,
) -> Result<(), String> {
    let target = target_of(root);
    match runner::run(&target, root, args, limit) {
        Err(error) => Err(error.to_string()),
        Ok(output) if output.code == Some(0) => Ok(()),
        Ok(output) if output.timed_out => Err("git did not answer in time".to_string()),
        Ok(output) if is_missing(&output) => Err(missing_text(&target)),
        Ok(output) => {
            let line = output.first_error_line();
            Err(if line.is_empty() { "git failed".to_string() } else { line })
        }
    }
}

/// 변경 하나의 diff 글자. 너무 크면 안내 문장을 오류로 돌려준다.
pub fn read_diff(
    root: &str,
    change: &Change,
) -> Result<String, String> {
    let target = target_of(root);
    match runner::run(&target, root, &git_args::diff(change), Some(runner::TIME_LIMIT)) {
        Err(error) => Err(error.to_string()),
        Ok(output) if output.code == Some(0) && output.stdout.len() > DIFF_LIMIT => {
            Err("This diff is too large to show here.".to_string())
        }
        Ok(output) if output.code == Some(0) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Ok(output) => Err(output.first_error_line()),
    }
}

#[cfg(test)]
pub(crate) mod test_repo {
    use std::path::{Path, PathBuf};

    /// 임시 저장소. 작성자는 저장소 로컬 설정이고 전역 설정을 건드리지 않는다.
    pub struct TestRepo {
        pub dir: tempfile::TempDir,
    }

    pub fn git_in(
        dir: &Path,
        args: &[&str],
    ) -> String {
        let output = std::process::Command::new("git").args(args).current_dir(dir).output().expect("run git");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    impl TestRepo {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            git_in(dir.path(), &["init", "-q", "-b", "main"]);
            git_in(dir.path(), &["config", "user.name", "Test"]);
            git_in(dir.path(), &["config", "user.email", "test@example.com"]);
            git_in(dir.path(), &["config", "core.autocrlf", "false"]);
            Self { dir }
        }

        pub fn path(&self) -> &Path {
            self.dir.path()
        }

        pub fn root(&self) -> String {
            super::folder_key(&self.dir.path().canonicalize().unwrap().to_string_lossy().replace("\\\\?\\", ""))
        }

        pub fn write(
            &self,
            name: &str,
            text: &str,
        ) -> PathBuf {
            let path = self.dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }

        pub fn commit_all(&self) {
            git_in(self.path(), &["add", "-A"]);
            git_in(self.path(), &["commit", "-q", "-m", "base"]);
        }
    }
}

#[cfg(test)]
mod tests {
    use paddock_core::git_status::{Section, mark_of};

    use super::test_repo::{TestRepo, git_in};
    use super::*;
    use crate::test_support::TempHome;

    fn rows(status: &RepoStatus) -> Vec<(Section, char, String)> {
        status.status.changes.iter().map(|c| (c.section, mark_of(c).glyph(), c.path.clone())).collect()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|text| text.to_string()).collect()
    }

    #[test]
    fn test_c_d53_status_parses_a_real_repository() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("mod.txt", "1\n");
        repo.write("del.txt", "1\n");
        repo.write("ren.txt", "rename me please, this is long enough to be detected\n");
        repo.commit_all();
        repo.write("mod.txt", "2\n");
        std::fs::remove_file(repo.path().join("del.txt")).unwrap();
        git_in(repo.path(), &["mv", "ren.txt", "ren2.txt"]);
        repo.write("added.txt", "a\n");
        git_in(repo.path(), &["add", "added.txt"]);
        repo.write("sub dir/새 파일.txt", "n\n");
        let status = read_repo(&repo.root(), None).expect("a repository");
        let got = rows(&status);
        assert!(got.contains(&(Section::Changes, 'M', "mod.txt".into())), "{got:?}");
        assert!(got.contains(&(Section::Changes, 'D', "del.txt".into())), "{got:?}");
        assert!(got.contains(&(Section::Staged, 'R', "ren2.txt".into())), "{got:?}");
        assert!(got.contains(&(Section::Staged, 'A', "added.txt".into())), "{got:?}");
        assert!(got.contains(&(Section::Untracked, 'U', "sub dir/새 파일.txt".into())), "{got:?}");
        assert_eq!(status.status.branch.as_deref(), Some("main"));
        assert_eq!(status.root, repo.root());
    }

    #[test]
    fn test_c_d53_a_folder_outside_any_repository_is_reported_as_not_a_repository() {
        let _home = TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        let result = read_repo(&dir.path().to_string_lossy(), None);
        // 임시 폴더가 어떤 저장소 안에 있는 환경이면 건너뛴다(저장소 밖이 보장되지 않는다).
        if !matches!(result, Err(RepoError::NotRepository)) {
            assert!(result.is_ok(), "{result:?}");
        }
    }

    #[test]
    fn test_c_d53_stage_and_unstage_round_trip() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("-rf", "x\n");
        repo.write("a b.txt", "1\n");
        repo.commit_all();
        repo.write("a b.txt", "2\n");
        repo.write("-n", "new\n");
        let root = repo.root();
        write(&root, &git_args::stage(&strings(&["a b.txt", "-n"])), Some(runner::TIME_LIMIT)).unwrap();
        let staged = read_repo(&root, Some(&root)).unwrap();
        assert!(rows(&staged).contains(&(Section::Staged, 'M', "a b.txt".into())));
        assert!(rows(&staged).contains(&(Section::Staged, 'A', "-n".into())), "{:?}", rows(&staged));
        write(&root, &git_args::unstage(&strings(&["a b.txt", "-n"]), true), Some(runner::TIME_LIMIT)).unwrap();
        let after = read_repo(&root, Some(&root)).unwrap();
        assert!(rows(&after).contains(&(Section::Changes, 'M', "a b.txt".into())));
        assert!(rows(&after).contains(&(Section::Untracked, 'U', "-n".into())));
        write(&root, &git_args::stage_all(), Some(runner::TIME_LIMIT)).unwrap();
        assert_eq!(read_repo(&root, Some(&root)).unwrap().status.n_in(Section::Staged), 2);
        write(&root, &git_args::unstage_all(true), Some(runner::TIME_LIMIT)).unwrap();
        assert_eq!(read_repo(&root, Some(&root)).unwrap().status.n_in(Section::Staged), 0);
    }

    #[test]
    fn test_c_d53_unstaging_in_a_repository_without_commits_uses_rm_cached() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("first.txt", "1\n");
        let root = repo.root();
        write(&root, &git_args::stage(&strings(&["first.txt"])), Some(runner::TIME_LIMIT)).unwrap();
        let status = read_repo(&root, Some(&root)).unwrap();
        assert!(status.status.oid.is_none(), "no commit yet");
        write(&root, &git_args::unstage(&strings(&["first.txt"]), false), Some(runner::TIME_LIMIT)).unwrap();
        assert_eq!(read_repo(&root, Some(&root)).unwrap().status.n_in(Section::Staged), 0);
    }

    #[test]
    fn test_c_d53_commit_uses_the_message_argument_not_a_shell() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("a.txt", "1\n");
        let root = repo.root();
        write(&root, &git_args::stage_all(), Some(runner::TIME_LIMIT)).unwrap();
        write(&root, &git_args::commit("first; $(touch pwned) `touch pwned2` \"q\" 한글"), None).unwrap();
        let subject = git_in(repo.path(), &["log", "-1", "--format=%s"]);
        assert_eq!(subject.trim_end(), "first; $(touch pwned) `touch pwned2` \"q\" 한글");
        assert!(!repo.path().join("pwned").exists() && !repo.path().join("pwned2").exists());
    }

    #[test]
    fn test_c_d53_a_commit_failure_comes_back_as_the_first_git_line() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("a.txt", "1\n");
        let root = repo.root();
        // 아무것도 스테이징하지 않은 커밋은 실패한다.
        let error = write(&root, &git_args::commit("nothing"), None).unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn test_c_d53_git_env_disables_prompts() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        let target = target_of(&repo.root());
        let args = strings(&["-c", "alias.e=!printenv GIT_TERMINAL_PROMPT GIT_OPTIONAL_LOCKS", "e"]);
        let output = runner::run(&target, &repo.root(), &args, Some(runner::TIME_LIMIT)).unwrap();
        assert_eq!(output.code, Some(0), "{}", output.stderr);
        assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\n0\n");
    }

    #[test]
    fn test_c_d53_discard_restores_tracked_and_deletes_untracked() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("t.txt", "base\n");
        repo.commit_all();
        repo.write("t.txt", "staged\n");
        git_in(repo.path(), &["add", "t.txt"]);
        repo.write("t.txt", "worktree\n");
        repo.write("-u.txt", "untracked\n");
        let root = repo.root();
        let status = read_repo(&root, Some(&root)).unwrap();
        let tracked = status.status.changes.iter().find(|c| c.section == Section::Changes).unwrap().clone();
        let untracked = status.status.changes.iter().find(|c| c.section == Section::Untracked).unwrap().clone();
        write(&root, &git_args::discard(&tracked), Some(runner::TIME_LIMIT)).unwrap();
        assert_eq!(std::fs::read_to_string(repo.path().join("t.txt")).unwrap().replace("\r\n", "\n"), "staged\n");
        assert!(git_in(repo.path(), &["diff", "--cached", "--stat"]).contains("t.txt"), "the staged content stays");
        write(&root, &git_args::discard(&untracked), Some(runner::TIME_LIMIT)).unwrap();
        assert!(!repo.path().join("-u.txt").exists());
    }

    #[test]
    fn test_c_d53_diff_shows_the_staged_and_the_unstaged_side_apart() {
        let _home = TempHome::new();
        let repo = TestRepo::new();
        repo.write("d.txt", "one\n");
        repo.commit_all();
        repo.write("d.txt", "two\n");
        git_in(repo.path(), &["add", "d.txt"]);
        repo.write("d.txt", "three\n");
        let root = repo.root();
        let status = read_repo(&root, Some(&root)).unwrap();
        let staged = status.status.changes.iter().find(|c| c.section == Section::Staged).unwrap();
        let worktree = status.status.changes.iter().find(|c| c.section == Section::Changes).unwrap();
        let staged_text = read_diff(&root, staged).unwrap();
        let worktree_text = read_diff(&root, worktree).unwrap();
        assert!(staged_text.contains("-one") && staged_text.contains("+two"), "{staged_text}");
        assert!(worktree_text.contains("-two") && worktree_text.contains("+three"), "{worktree_text}");
    }
}
