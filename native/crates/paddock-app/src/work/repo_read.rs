//! 작업 폴더의 `.git`을 읽어 폴더가 속한 저장소를 알아내는 일: 디스크 읽기와 백그라운드 읽기 관리.
//!
//! 같은 저장소의 워크트리 폴더를 Work 목록에서 한 묶음으로 보이려면 폴더마다 저장소 key가 필요하다.
//! 읽은 글을 해석하는 규칙과 읽기를 몇 번 다시 시도할지는 `paddock_core::worktree`에 있고, 여기서는 읽기만 한다.
//! WSL 공유 경로나 느린 디스크의 읽기가 화면을 막지 않도록 읽기는 별도 스레드에서 하고 결과만 화면 스레드가 가져간다.
use std::collections::{HashMap, HashSet};
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

/// 폴더의 `.git` 한 항목을 읽는다.
///
/// 디렉터리면 `Directory`, 파일이면 앞부분 글 `Content`다. 폴더는 있는데 `.git`이 없으면 `Missing`이고,
/// 폴더가 없거나(지운 워크트리) 읽지 못하면 `Unreadable`이다.
///
/// Examples
/// --------
/// | 폴더 안                  | 결과                      |
/// |--------------------------|---------------------------|
/// | `.git/` 디렉터리         | `Directory`               |
/// | `.git` 파일 `gitdir: …`  | `Content("gitdir: …")`    |
/// | `.git` 없음              | `Missing`                 |
/// | 폴더 없음                | `Unreadable`              |
pub fn read_dot_git(folder: &str) -> DotGit {
    let git_path = child_path(folder, ".git");
    match fs::metadata(&git_path) {
        Ok(metadata) if metadata.is_dir() => DotGit::Directory,
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
    /// 읽기를 요청했고 아직 결과가 오지 않은 폴더.
    reading: HashSet<String>,
    /// 읽지 못한 폴더가 다시 읽을 수 있게 되는 시각.
    retry_at: HashMap<String, Instant>,
}

impl RepoReader {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self { lookups: RepoLookups::default(), sender, receiver, reading: HashSet::new(), retry_at: HashMap::new() }
    }

    /// 끝난 읽기를 폴더마다 기록하고, 폴더에 새로 기록할 저장소 key를 돌려준다.
    /// `Some(None)`은 저장소가 아니라서 기록을 지우라는 뜻이다. 읽지 못한 폴더는 돌려주지 않아 저장된 값이 그대로 남는다.
    pub fn finished(
        &mut self,
        work: &WorkState,
    ) -> Vec<(String, Option<String>)> {
        let mut updates = Vec::new();
        while let Ok((folder, read)) = self.receiver.try_recv() {
            self.reading.remove(&folder);
            if let Some(repo) = self.lookups.record(&folder, &read, work.folder_repo(&folder)) {
                updates.push((folder, repo));
            } else if read == DotGit::Unreadable {
                self.retry_at.insert(folder, Instant::now() + RETRY_AFTER);
            }
        }
        updates
    }

    /// 아직 저장소를 정하지 못한 폴더의 읽기를 시작한다. 읽는 중이거나 다시 읽을 때가 아닌 폴더는 건너뛴다.
    pub fn request(
        &mut self,
        work: &WorkState,
    ) {
        let keys: Vec<&str> = work.folders.iter().map(|folder| folder.key.as_str()).collect();
        let now = Instant::now();
        let wanted: Vec<String> = self
            .lookups
            .pending(&keys)
            .into_iter()
            .filter(|key| !self.reading.contains(*key) && self.retry_at.get(*key).is_none_or(|at| now >= *at))
            .map(str::to_string)
            .collect();
        self.retry_at.retain(|key, _| keys.contains(&key.as_str()));
        if !wanted.is_empty() {
            self.reading.extend(wanted.iter().cloned());
            let sender = self.sender.clone();
            std::thread::spawn(move || {
                for folder in wanted {
                    let read = read_dot_git(&folder);
                    if sender.send((folder, read)).is_err() {
                        break;
                    }
                }
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
        assert_eq!(read_dot_git(&main), DotGit::Directory);
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
        let root = dir.path().to_str().expect("utf-8 path").to_string();
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
}
