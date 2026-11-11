//! 작업 폴더의 `.git`을 읽어 폴더가 속한 저장소를 알아내는 일: 디스크 읽기와 백그라운드 읽기 관리.
//!
//! 같은 저장소의 워크트리 폴더를 Work 목록에서 한 묶음으로 보이려면 폴더마다 저장소 key가 필요하다.
//! 읽은 글을 해석하는 규칙과 읽기를 몇 번 다시 시도할지는 `paddock_core::worktree`에 있고, 여기서는 읽기만 한다.
//! WSL 공유 경로나 느린 디스크의 읽기가 화면을 막지 않도록 읽기는 별도 스레드에서 하고 결과만 화면 스레드가 가져간다.
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use paddock_core::paths::child_path;
use paddock_core::work_model::WorkState;
use paddock_core::worktree::{DotGit, RepoLookups};

/// `.git` 파일에서 읽는 앞부분의 크기(바이트). `gitdir:` 한 줄은 이보다 짧다.
const GIT_FILE_PREFIX_BYTES: u64 = 4096;

/// 읽지 못한 폴더를 다시 읽기까지 기다리는 시간.
const RETRY_AFTER: Duration = Duration::from_secs(3);

/// 한 폴더의 읽기가 이 시간 안에 끝나지 않으면 읽지 못한 것으로 본다(응답 없는 공유 경로).
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// 동시에 진행하는 폴더 읽기 수의 상한.
const N_MAX_READING: usize = 8;

/// Windows가 실제 경로 앞에 붙이는 `\\?\` 표기를 떼어 폴더 key와 같은 철자로 맞춘다.
///
/// Examples
/// --------
/// | 경로                                   | 결과                              |
/// |----------------------------------------|-----------------------------------|
/// | `\\?\C:\dev\main`                      | `C:\dev\main`                     |
/// | `\\?\UNC\wsl.localhost\Ubuntu\h`       | `\\wsl.localhost\Ubuntu\h`        |
/// | `/private/tmp/m`                       | `/private/tmp/m`                  |
fn without_verbatim_prefix(path: &str) -> String {
    match path.strip_prefix("\\\\?\\UNC\\") {
        Some(share) => format!("\\\\{share}"),
        None => path.strip_prefix("\\\\?\\").unwrap_or(path).to_string(),
    }
}

/// 폴더의 `.git` 한 항목을 읽는다.
///
/// 디렉터리면 `Directory`, 파일이면 앞부분 글 `Content`다. 폴더는 있는데 `.git`이 없으면 `Missing`이고,
/// 폴더가 없거나(지운 워크트리) 읽지 못하면 `Unreadable`이다.
///
/// Examples
/// --------
/// | 폴더 안                  | 결과                      |
/// |--------------------------|---------------------------|
/// | `.git/` 디렉터리         | `Directory(실제 경로)`    |
/// | `.git` 파일 `gitdir: …`  | `Content("gitdir: …")`    |
/// | `.git` 없음              | `Missing`                 |
/// | 폴더 없음                | `Unreadable`              |
pub fn read_dot_git(folder: &str) -> DotGit {
    let git_path = child_path(folder, ".git");
    match fs::metadata(&git_path) {
        // 심볼릭 링크를 푼 폴더의 실제 경로를 함께 싣는다. 풀지 못하면(공유 경로 등) 폴더 key를 그대로 쓰게 None이다.
        Ok(metadata) if metadata.is_dir() => {
            DotGit::Directory(fs::canonicalize(folder).ok().and_then(|real| real.to_str().map(without_verbatim_prefix)))
        }
        Ok(_) => {
            let mut text = String::new();
            let read =
                fs::File::open(&git_path).and_then(|file| file.take(GIT_FILE_PREFIX_BYTES).read_to_string(&mut text));
            match read {
                Ok(_) => DotGit::Content(text),
                Err(_) => DotGit::Unreadable,
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if fs::metadata(folder).is_ok_and(|metadata| metadata.is_dir()) {
                DotGit::Missing
            } else {
                DotGit::Unreadable
            }
        }
        Err(_) => DotGit::Unreadable,
    }
}

/// 작업 폴더의 `.git` 읽기를 백그라운드로 요청하고 끝난 결과를 모으는 도구.
pub struct RepoReader {
    lookups: RepoLookups,
    sender: Sender<(String, DotGit)>,
    receiver: Receiver<(String, DotGit)>,
    /// 읽기를 요청했고 아직 결과가 오지 않은 폴더와 요청한 시각.
    reading: HashMap<String, Instant>,
    /// 읽지 못한 폴더가 다시 읽을 수 있게 되는 시각.
    retry_at: HashMap<String, Instant>,
}

impl RepoReader {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self { lookups: RepoLookups::default(), sender, receiver, reading: HashMap::new(), retry_at: HashMap::new() }
    }

    /// 끝난 읽기를 폴더마다 기록하고, 폴더에 새로 기록할 저장소 key를 돌려준다.
    /// `Some(None)`은 저장소가 아니라서 기록을 지우라는 뜻이다. 읽지 못한 폴더는 돌려주지 않아 저장된 값이 그대로 남는다.
    /// 시간 안에 끝나지 않은 읽기는 읽지 못한 것으로 기록하고, 뒤늦게 온 결과는 버린다.
    pub fn finished(
        &mut self,
        work: &WorkState,
    ) -> Vec<(String, Option<String>)> {
        self.finished_at(work, Instant::now())
    }

    fn finished_at(
        &mut self,
        work: &WorkState,
        now: Instant,
    ) -> Vec<(String, Option<String>)> {
        let mut updates = Vec::new();
        let mut results: Vec<(String, DotGit)> = Vec::new();
        while let Ok((folder, read)) = self.receiver.try_recv() {
            // 시간 초과로 이미 실패 처리한 폴더의 뒤늦은 결과는 버린다.
            if self.reading.remove(&folder).is_some() {
                results.push((folder, read));
            }
        }
        let timed_out: Vec<String> = self
            .reading
            .iter()
            .filter(|(_, started)| now.duration_since(**started) >= READ_TIMEOUT)
            .map(|(folder, _)| folder.clone())
            .collect();
        for folder in timed_out {
            self.reading.remove(&folder);
            results.push((folder, DotGit::Unreadable));
        }
        for (folder, read) in results {
            if let Some(repo) = self.lookups.record(&folder, &read, work.folder_repo(&folder)) {
                updates.push((folder, repo));
            } else if read == DotGit::Unreadable {
                self.retry_at.insert(folder, now + RETRY_AFTER);
            }
        }
        updates
    }

    /// 아직 저장소를 정하지 못한 폴더의 읽기를 시작한다. 읽는 중이거나 다시 읽을 때가 아닌 폴더는 건너뛰고,
    /// 동시에 읽는 폴더는 `N_MAX_READING`개까지다(남은 폴더는 다음 호출에서 시작한다). 폴더마다 스레드가 따로라 한 폴더가 막혀도 다른 폴더는 읽힌다.
    pub fn request(
        &mut self,
        work: &WorkState,
    ) {
        let keys: Vec<&str> = work.folders.iter().map(|folder| folder.key.as_str()).collect();
        let now = Instant::now();
        let n_free = N_MAX_READING.saturating_sub(self.reading.len());
        let wanted: Vec<String> = self
            .lookups
            .pending(&keys)
            .into_iter()
            .filter(|key| !self.reading.contains_key(*key) && self.retry_at.get(*key).is_none_or(|at| now >= *at))
            .take(n_free)
            .map(str::to_string)
            .collect();
        self.retry_at.retain(|key, _| keys.contains(&key.as_str()));
        for folder in wanted {
            self.reading.insert(folder.clone(), now);
            let sender = self.sender.clone();
            std::thread::spawn(move || {
                let read = read_dot_git(&folder);
                let _ = sender.send((folder, read));
            });
        }
    }
}

impl Default for RepoReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_repo_read_1_a_git_directory_a_git_file_a_missing_git_and_a_missing_folder_read_as_four_different_things()
    {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_str().expect("utf-8 path").to_string();
        let main = format!("{root}/main");
        let linked = format!("{root}/wt");
        let bare = format!("{root}/bare");
        fs::create_dir_all(format!("{main}/.git")).unwrap();
        fs::create_dir_all(&linked).unwrap();
        fs::write(format!("{linked}/.git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
        fs::create_dir_all(&bare).unwrap();
        assert!(matches!(read_dot_git(&main), DotGit::Directory(Some(_))));
        assert_eq!(read_dot_git(&linked), DotGit::Content("gitdir: ../main/.git/worktrees/wt\n".to_string()));
        assert_eq!(read_dot_git(&bare), DotGit::Missing);
        assert_eq!(read_dot_git(&format!("{root}/deleted")), DotGit::Unreadable);
    }

    #[test]
    fn test_c_repo_read_2_only_the_first_bytes_of_a_huge_git_file_are_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().to_str().expect("utf-8 path").to_string();
        fs::write(format!("{folder}/.git"), "x".repeat(100_000)).unwrap();
        assert!(matches!(read_dot_git(&folder), DotGit::Content(text) if text.len() == GIT_FILE_PREFIX_BYTES as usize));
    }

    #[test]
    fn test_c_repo_read_3_the_reader_reports_the_repository_of_each_folder_once_and_keeps_the_saved_one_of_a_deleted_folder()
     {
        let dir = tempfile::tempdir().expect("temp dir");
        // 임시 폴더 자체가 심볼릭 링크 아래일 수 있어(맥의 /var) 실제 경로로 시작한다.
        let root = fs::canonicalize(dir.path()).unwrap().to_str().expect("utf-8 path").to_string();
        let (main, wt, gone) = (format!("{root}/main"), format!("{root}/wt"), format!("{root}/gone"));
        fs::create_dir_all(format!("{main}/.git")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(format!("{wt}/.git"), "gitdir: ../main/.git/worktrees/wt").unwrap();
        let mut work = WorkState::default();
        for key in [&main, &wt, &gone] {
            work.ensure_folder(key);
        }
        work.set_folder_repo(&gone, Some(&main));
        let mut reader = RepoReader::new();
        reader.request(&work);
        let mut updates = Vec::new();
        let started = Instant::now();
        while updates.len() < 2 && started.elapsed() < Duration::from_secs(10) {
            updates.extend(reader.finished(&work));
            std::thread::sleep(Duration::from_millis(5));
        }
        updates.sort();
        assert_eq!(updates, [(main.clone(), Some(main.clone())), (wt.clone(), Some(main.clone()))]);
        // 지운 폴더는 저장된 값이 있으므로 더 읽지 않고 기록을 바꾸지도 않는다.
        std::thread::sleep(Duration::from_millis(50));
        assert!(reader.finished(&work).is_empty());
        reader.request(&work);
        std::thread::sleep(Duration::from_millis(50));
        assert!(reader.finished(&work).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_c_repo_read_4_a_folder_opened_through_a_symlink_reads_with_its_real_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let real = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(real.join("main/.git")).unwrap();
        std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
        let through_link = format!("{}/link/main", real.display());
        let real_main = format!("{}/main", real.display());
        assert_eq!(read_dot_git(&through_link), DotGit::Directory(Some(real_main.clone())));
        assert_eq!(paddock_core::worktree::repo_key_of(&through_link, &read_dot_git(&through_link)), Some(real_main));
    }

    #[test]
    fn test_c_repo_read_5_the_windows_verbatim_prefix_is_removed() {
        assert_eq!(without_verbatim_prefix("\\\\?\\C:\\dev\\main"), "C:\\dev\\main");
        assert_eq!(without_verbatim_prefix("\\\\?\\UNC\\wsl.localhost\\Ubuntu\\h"), "\\\\wsl.localhost\\Ubuntu\\h");
        assert_eq!(without_verbatim_prefix("/private/tmp/m"), "/private/tmp/m");
    }

    #[test]
    fn test_c_repo_read_6_a_read_that_never_answers_counts_as_unreadable_and_is_asked_again_later() {
        let mut work = WorkState::default();
        work.ensure_folder("/r/stuck");
        let mut reader = RepoReader::new();
        let started = Instant::now();
        reader.reading.insert("/r/stuck".to_string(), started);
        assert!(reader.finished_at(&work, started + READ_TIMEOUT - Duration::from_millis(1)).is_empty());
        assert!(reader.reading.contains_key("/r/stuck"), "still waiting before the limit");
        assert!(reader.finished_at(&work, started + READ_TIMEOUT).is_empty());
        assert!(!reader.reading.contains_key("/r/stuck"), "given up at the limit");
        assert!(reader.retry_at.contains_key("/r/stuck"), "asked again after the retry wait");
    }

    #[test]
    fn test_c_repo_read_7_a_late_answer_after_the_timeout_is_ignored_and_reads_run_at_most_eight_at_once() {
        let mut work = WorkState::default();
        for index in 0..12 {
            work.ensure_folder(&format!("/nowhere/{index}"));
        }
        let mut reader = RepoReader::new();
        reader.sender.send(("/late".to_string(), DotGit::Directory(None))).unwrap();
        assert!(reader.finished(&work).is_empty(), "no request was waiting for it");
        reader.request(&work);
        assert_eq!(reader.reading.len(), N_MAX_READING);
    }
}
