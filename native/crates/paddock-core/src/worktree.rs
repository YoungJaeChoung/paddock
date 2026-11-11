//! 작업 폴더가 어느 git 저장소의 워크트리인지 `.git` 한 항목만 보고 정하는 순수 계산.
//!
//! Work 목록이 같은 저장소의 워크트리 폴더를 한 묶음으로 보이려면 폴더마다 "같은 저장소"를 가리키는 키가 필요하다.
//! git 프로세스를 띄우지 않고 `.git`이 디렉터리인지, 파일이면 `gitdir:` 한 줄이 무엇을 가리키는지로 판정한다.
//! 결과 키는 폴더 key와 같은 철자 규칙(`paths::folder_key`)을 따라, 주 워크트리 폴더의 key와 그대로 같아진다.
//! `.git`을 읽는 일은 호출자가 하고, 여기서는 읽은 결과를 해석하고 읽기를 몇 번 다시 시도할지만 정한다(파일 시스템은 모른다).
use std::collections::{HashMap, HashSet};

use crate::git_paths::parse_wsl_path;
use crate::paths::folder_key;

/// 폴더의 `.git`을 읽은 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DotGit {
    /// `.git`이 디렉터리다. 이 폴더가 주 워크트리(저장소 루트)다.
    /// 값은 호출자가 심볼릭 링크를 풀어 알아낸 폴더의 실제 경로이고, 알아내지 못했으면 None이다.
    Directory(Option<String>),
    /// `.git`이 파일이다. 값은 파일의 앞부분 글이다.
    Content(String),
    /// 폴더는 있는데 `.git`이 없다.
    Missing,
    /// 폴더가 없거나 읽지 못했다. 지금은 알 수 없다는 뜻이라 `Missing`과 다르다.
    Unreadable,
}

/// 읽지 못한 폴더를 다시 읽어 보는 최대 횟수(처음 읽기 포함).
pub const N_MAX_LOOKUPS: u32 = 5;

/// 경로 글이 `C:/…`처럼 드라이브 글자로 시작하는지.
fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes.len() == 2 || bytes[2] == b'/')
}

/// 폴더 key를 (접두, `/`로 시작하는 나머지 경로)로 나눈다. 접두는 상대 경로를 풀 때 넘어가지 않는 뿌리다.
///
/// 접두는 WSL 공유면 `\\<호스트>\<배포판>`, 일반 공유면 `\\<서버>\<공유>`, 드라이브면 `C:`, 리눅스·맥 경로면 빈 글자다.
fn split_prefix(folder: &str) -> (String, String) {
    if let Some((host, distro, linux)) = parse_wsl_path(folder) {
        (format!("\\\\{host}\\{distro}"), linux)
    } else if let Some(share) = folder.strip_prefix("\\\\") {
        let parts: Vec<&str> = share.splitn(3, '\\').collect();
        let prefix = format!("\\\\{}", parts[..parts.len().min(2)].join("\\"));
        (prefix, format!("/{}", parts.get(2).map_or(String::new(), |rest| rest.replace('\\', "/"))))
    } else if is_drive_path(folder) {
        (folder[..2].to_string(), format!("/{}", folder[2..].trim_start_matches('/')))
    } else {
        (String::new(), folder.to_string())
    }
}

/// 접두와 `/` 경로를 이어 폴더 key로 맞춘다. 공유 경로는 역슬래시로 쓴다.
fn join_key(
    prefix: &str,
    path: &str,
) -> String {
    let joined = if prefix.starts_with("\\\\") {
        format!("{prefix}{}", path.replace('/', "\\"))
    } else {
        format!("{prefix}{path}")
    };
    folder_key(&joined)
}

/// `base`(`/`로 시작) 아래에서 상대 경로를 풀어 `.`·`..`을 정리한 절대 경로로 만든다.
fn resolve_relative(
    base: &str,
    relative: &str,
) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('/').chain(relative.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

/// `.git` 파일 글에서 연결된 워크트리의 저장소 루트를 뽑는다. `gitdir: <루트>/.git/worktrees/<이름>`이 아니면 None이다.
/// 서브모듈(`.git/modules/…`)과 그 밖의 모양은 맞지 않는다. 역슬래시는 `/`로 본다.
fn linked_root(text: &str) -> Option<String> {
    let target = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:").map(str::trim).filter(|target| !target.is_empty()))?
        .replace('\\', "/");
    let (head, name) = target.trim_end_matches('/').rsplit_once('/')?;
    let root = head.strip_suffix("/.git/worktrees").filter(|root| !root.is_empty() && !name.is_empty())?;
    Some(root.to_string())
}

/// 두 접두(`split_prefix`의 앞부분)가 같은 뿌리를 가리키는지. 대소문자는 가리지 않는다.
fn same_prefix(
    left: &str,
    right: &str,
) -> bool {
    left.eq_ignore_ascii_case(right)
}

/// 폴더의 `.git` 읽기 결과로 저장소 key를 정한다. 같은 저장소의 폴더는 같은 key를 얻어 한 묶음이 된다.
/// 묶지 않는 경우(서브모듈·`.git` 없음·읽기 실패·알 수 없는 모양)는 None이다.
///
/// WSL 폴더의 gitdir는 리눅스 경로라 폴더 key의 `\\wsl.localhost\<배포판>` 접두를 붙여 같은 철자로 맞춘다.
/// 드라이브 폴더에서 WSL의 git이 만든 워크트리는 gitdir가 `/mnt/<문자>/…`라서 드라이브 경로로 되돌린다.
/// 상대 gitdir는 폴더 경로(WSL이면 배포판 안의 경로) 기준으로 푼다.
///
/// 연결된 워크트리의 key는 git이 `.git` 파일에 적은 실제 경로다. 폴더가 심볼릭 링크를 거쳐 열렸으면(맥의 `/tmp`→`/private/tmp`)
/// 주 워크트리도 같은 철자여야 한 묶음이 되므로, `Directory`에 실린 실제 경로를 key로 쓴다. 실제 경로의 접두(드라이브·공유·배포판)가
/// 폴더와 다르면 믿지 않고 폴더 key를 쓴다.
///
/// Examples
/// --------
/// | 폴더                           | .git                                              | 결과                           |
/// |--------------------------------|---------------------------------------------------|--------------------------------|
/// | `/r/main`                      | 디렉터리                                          | `/r/main`                      |
/// | `/tmp/m`(실제 `/private/tmp/m`)| 디렉터리                                          | `/private/tmp/m`               |
/// | `/r/wt`                        | `gitdir: /r/main/.git/worktrees/wt`               | `/r/main`                      |
/// | `/r/wt`                        | `gitdir: ../main/.git/worktrees/wt`               | `/r/main`                      |
/// | `\\wsl.localhost\Ubuntu\h\wt`  | `gitdir: /h/main/.git/worktrees/wt`               | `\\wsl.localhost\Ubuntu\h\main`|
/// | `C:/dev/wt`                    | `gitdir: /mnt/c/dev/main/.git/worktrees/wt`       | `C:/dev/main`                  |
/// | `/r/sub`                       | `gitdir: ../.git/modules/sub`                     | None                           |
/// | `/r/x`                         | 읽지 못함                                         | None                           |
pub fn repo_key_of(
    folder: &str,
    dot_git: &DotGit,
) -> Option<String> {
    let folder = folder_key(folder);
    match dot_git {
        DotGit::Directory(real) => {
            let real_key = real.as_deref().filter(|real| !real.is_empty()).map(folder_key);
            Some(real_key.filter(|real| same_prefix(&split_prefix(real).0, &split_prefix(&folder).0)).unwrap_or(folder))
        }
        DotGit::Content(text) => linked_root(text).map(|root| {
            let (prefix, path) = split_prefix(&folder);
            let mount = root.strip_prefix("/mnt/").filter(|_| is_drive_path(&prefix)).and_then(|rest| {
                let (letter, tail) = rest.split_at(rest.find('/')?);
                (letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic()).then(|| format!("{letter}:{tail}"))
            });
            if is_drive_path(&format!("{root}/")) || root.starts_with("//") {
                // Windows 드라이브 경로나 공유 경로가 그대로 적혀 있다.
                folder_key(&root)
            } else if let Some(drive_path) = mount {
                folder_key(&drive_path)
            } else if root.starts_with('/') {
                // 리눅스 경로. WSL 폴더면 폴더 key의 `\\<호스트>\<배포판>` 접두를 붙인다.
                join_key(&prefix, &root)
            } else {
                // 상대 경로. 폴더 경로 안에서 푼다.
                join_key(&prefix, &resolve_relative(&path, &root))
            }
        }),
        DotGit::Missing | DotGit::Unreadable => None,
    }
}

/// 폴더마다 `.git` 읽기를 끝냈는지와 읽지 못한 횟수를 기억하는 재시도 규칙.
///
/// 읽은 폴더는 한 번으로 끝난다. 읽지 못한 폴더(앱 시작 직후 WSL 경로 지연·연결 끊김·디스크에서 지운 워크트리)는 다시 읽되
/// `N_MAX_LOOKUPS`번까지만 읽는다. 지운 워크트리처럼 저장된 저장소 key가 이미 있으면 더 읽지 않는다 — 그래야 폴더를 지워도 묶음 아래에 남는다.
///
/// 한계: 한 번 읽은 폴더는 이번 실행 동안 다시 읽지 않는다. 폴더를 연 뒤 `git init`이나 `git worktree repair`로 `.git`이 바뀌면 다음 실행에야 반영된다.
#[derive(Debug, Default)]
pub struct RepoLookups {
    attempts: HashMap<String, u32>,
    done: HashSet<String>,
}

impl RepoLookups {
    /// 아직 읽어야 하는 폴더(`folders` 중 끝나지 않은 것). 목록에서 빠진 폴더의 기록은 버린다.
    pub fn pending<'a>(
        &mut self,
        folders: &[&'a str],
    ) -> Vec<&'a str> {
        let known: HashSet<&str> = folders.iter().copied().collect();
        self.attempts.retain(|key, _| known.contains(key.as_str()));
        self.done.retain(|key| known.contains(key.as_str()));
        folders.iter().copied().filter(|key| !self.done.contains(*key)).collect()
    }

    /// 읽은 결과를 기록한다. 폴더에 저장소 key를 새로 기록해야 하면 `Some(key)`다(`Some(None)`은 기록을 지우라는 뜻).
    /// 읽지 못했으면 None이라 저장된 값을 그대로 둔다. `saved`는 폴더에 이미 저장된 저장소 key다.
    pub fn record(
        &mut self,
        folder: &str,
        read: &DotGit,
        saved: Option<&str>,
    ) -> Option<Option<String>> {
        let mut update = None;
        if *read == DotGit::Unreadable {
            let n_lookups = self.attempts.entry(folder.to_string()).or_insert(0);
            *n_lookups += 1;
            if saved.is_some() || *n_lookups >= N_MAX_LOOKUPS {
                self.done.insert(folder.to_string());
            }
        } else {
            self.done.insert(folder.to_string());
            update = Some(repo_key_of(folder, read));
        }
        update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> DotGit {
        DotGit::Content(text.to_string())
    }

    #[test]
    fn test_c_wt_1_a_git_directory_makes_the_folder_the_repository_root_and_the_trailing_slash_is_dropped() {
        assert_eq!(repo_key_of("/tmp/main", &DotGit::Directory(None)).as_deref(), Some("/tmp/main"));
        assert_eq!(repo_key_of("/tmp/main/", &DotGit::Directory(None)).as_deref(), Some("/tmp/main"));
    }

    #[test]
    fn test_c_wt_2_a_linked_worktree_git_file_points_at_the_main_repository_folder() {
        for text in [
            "gitdir: /tmp/main/.git/worktrees/wt1\n",
            "gitdir: /tmp/main/.git/worktrees/wt1",
            "gitdir: /tmp/main/.git/worktrees/wt1\r\n",
            "gitdir: /tmp/main/.git/worktrees/wt1/",
        ] {
            assert_eq!(repo_key_of("/tmp/wt1", &file(text)).as_deref(), Some("/tmp/main"), "{text:?}");
        }
    }

    #[test]
    fn test_c_wt_3_a_relative_gitdir_is_resolved_from_the_folder() {
        assert_eq!(repo_key_of("/tmp/wt1", &file("gitdir: ../main/.git/worktrees/wt1")).as_deref(), Some("/tmp/main"));
        assert_eq!(
            repo_key_of("/a/b/wt", &file("gitdir: ../../../c/main/.git/worktrees/wt")).as_deref(),
            Some("/c/main")
        );
    }

    #[test]
    fn test_c_wt_4_submodules_other_git_files_a_missing_git_and_unreadable_folders_are_not_grouped() {
        for read in [
            file("gitdir: ../.git/modules/sub"),
            file("gitdir: /tmp/main/.git"),
            file("not a gitdir file"),
            file(""),
            file("gitdir:"),
            DotGit::Missing,
            DotGit::Unreadable,
        ] {
            assert_eq!(repo_key_of("/tmp/x", &read), None, "{read:?}");
        }
    }

    #[test]
    fn test_c_wt_5_a_wsl_folders_linux_gitdir_gets_the_distribution_prefix_of_the_folder_key() {
        let folder = "\\\\wsl.localhost\\Ubuntu\\home\\me\\wt1";
        let main = Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\main");
        assert_eq!(repo_key_of(folder, &file("gitdir: /home/me/main/.git/worktrees/wt1")).as_deref(), main);
        assert_eq!(repo_key_of("\\\\wsl.localhost\\Ubuntu\\home\\me\\main", &DotGit::Directory(None)).as_deref(), main);
        // 상대 경로는 배포판 안의 경로에서 푼다. 배포판 접두를 넘어가지 않는다.
        assert_eq!(repo_key_of(folder, &file("gitdir: ../main/.git/worktrees/wt1")).as_deref(), main);
        assert_eq!(
            repo_key_of("\\\\wsl.localhost\\Ubuntu\\wt", &file("gitdir: ../../../main/.git/worktrees/wt")).as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\main")
        );
        // 슬래시로 적힌 폴더도 같은 key가 된다.
        assert_eq!(
            repo_key_of("//wsl.localhost/Ubuntu/home/me/wt1", &file("gitdir: /home/me/main/.git/worktrees/wt1"))
                .as_deref(),
            main
        );
        // 배포판이 다른 폴더의 같은 리눅스 경로는 다른 저장소다.
        assert_ne!(
            repo_key_of("\\\\wsl.localhost\\Debian\\home\\me\\wt1", &file("gitdir: /home/me/main/.git/worktrees/wt1")),
            repo_key_of(folder, &file("gitdir: /home/me/main/.git/worktrees/wt1"))
        );
    }

    #[test]
    fn test_c_wt_6_windows_drive_paths_come_back_in_the_folder_key_spelling() {
        let main = Some("C:/dev/main");
        assert_eq!(repo_key_of("C:/dev/wt", &file("gitdir: C:/dev/main/.git/worktrees/wt")).as_deref(), main);
        assert_eq!(repo_key_of("C:/dev/wt", &file("gitdir: C:\\dev\\main\\.git\\worktrees\\wt")).as_deref(), main);
        assert_eq!(repo_key_of("C:/dev/wt", &file("gitdir: ../main/.git/worktrees/wt")).as_deref(), main);
        // 드라이브 글자는 대소문자가 달라도 폴더 key처럼 대문자로 쓴다.
        assert_eq!(repo_key_of("c:\\dev\\wt", &file("gitdir: c:/dev/main/.git/worktrees/wt")).as_deref(), main);
        assert_eq!(repo_key_of("C:/dev/main", &DotGit::Directory(None)).as_deref(), main);
        // 공백이 든 경로도 그대로다.
        assert_eq!(
            repo_key_of("/home/me/my wt", &file("gitdir: /home/me/my main/.git/worktrees/my wt")).as_deref(),
            Some("/home/me/my main")
        );
    }

    #[test]
    fn test_c_wt_7_a_wsl_dollar_share_is_a_wsl_folder_too() {
        let folder = "\\\\wsl$\\Ubuntu\\home\\me\\wt1";
        let main = Some("\\\\wsl$\\Ubuntu\\home\\me\\main");
        assert_eq!(repo_key_of(folder, &file("gitdir: /home/me/main/.git/worktrees/wt1")).as_deref(), main);
        assert_eq!(repo_key_of(folder, &file("gitdir: ../main/.git/worktrees/wt1")).as_deref(), main);
        assert_eq!(repo_key_of("\\\\wsl$\\Ubuntu\\home\\me\\main", &DotGit::Directory(None)).as_deref(), main);
    }

    #[test]
    fn test_c_wt_8_a_worktree_made_by_wsl_git_in_a_drive_folder_comes_back_as_a_drive_key() {
        assert_eq!(
            repo_key_of("C:/dev/wt", &file("gitdir: /mnt/c/dev/main/.git/worktrees/wt")).as_deref(),
            Some("C:/dev/main")
        );
        assert_eq!(
            repo_key_of("D:/dev/wt", &file("gitdir: /mnt/c/dev/main/.git/worktrees/wt")).as_deref(),
            Some("C:/dev/main")
        );
        // 드라이브 폴더가 아니면 /mnt/c는 그냥 리눅스 경로다.
        assert_eq!(
            repo_key_of("/home/me/wt", &file("gitdir: /mnt/c/dev/main/.git/worktrees/wt")).as_deref(),
            Some("/mnt/c/dev/main")
        );
    }

    #[test]
    fn test_c_wt_9_a_folder_that_could_not_be_read_is_asked_again_until_the_limit() {
        let mut lookups = RepoLookups::default();
        let folders = ["/r/wt"];
        for _ in 0..N_MAX_LOOKUPS - 1 {
            assert_eq!(lookups.record("/r/wt", &DotGit::Unreadable, None), None);
            assert_eq!(lookups.pending(&folders), vec!["/r/wt"]);
        }
        assert_eq!(lookups.record("/r/wt", &DotGit::Unreadable, None), None);
        assert!(lookups.pending(&folders).is_empty());
    }

    #[test]
    fn test_c_wt_10_a_deleted_worktree_keeps_its_saved_repository_and_is_not_asked_again() {
        let mut lookups = RepoLookups::default();
        assert_eq!(lookups.record("/r/wt", &DotGit::Unreadable, Some("/r/main")), None);
        assert!(lookups.pending(&["/r/wt"]).is_empty());
    }

    #[test]
    fn test_c_wt_11_a_read_folder_is_done_in_one_lookup_and_a_missing_git_clears_the_record() {
        let mut lookups = RepoLookups::default();
        let linked = file("gitdir: /r/main/.git/worktrees/wt");
        assert_eq!(lookups.record("/r/wt", &linked, None), Some(Some("/r/main".to_string())));
        assert_eq!(lookups.record("/r/x", &DotGit::Missing, Some("/r/main")), Some(None));
        assert!(lookups.pending(&["/r/wt", "/r/x"]).is_empty());
    }

    #[test]
    fn test_c_wt_12_folders_removed_from_the_list_are_forgotten_and_asked_again_when_they_return() {
        let mut lookups = RepoLookups::default();
        lookups.record("/r/wt", &DotGit::Directory(None), None);
        assert!(lookups.pending(&["/r/wt"]).is_empty());
        assert!(lookups.pending(&[]).is_empty());
        assert_eq!(lookups.pending(&["/r/wt"]), vec!["/r/wt"]);
    }

    #[test]
    fn test_c_wt_13_a_main_folder_opened_through_a_symlink_uses_its_real_path_like_the_linked_worktrees() {
        // 맥에서 `/tmp/r/main`으로 연 폴더의 실제 경로는 `/private/tmp/r/main`이고, 연결 워크트리의 .git 파일도 실제 경로를 적는다.
        let main = repo_key_of("/tmp/r/main", &DotGit::Directory(Some("/private/tmp/r/main".to_string())));
        let linked = repo_key_of("/tmp/r/wt", &file("gitdir: /private/tmp/r/main/.git/worktrees/wt"));
        assert_eq!(main.as_deref(), Some("/private/tmp/r/main"));
        assert_eq!(main, linked);
    }

    #[test]
    fn test_c_wt_14_a_real_path_with_another_prefix_is_not_trusted_and_the_folder_key_is_used() {
        for (folder, real) in [
            ("\\\\wsl.localhost\\Ubuntu\\home\\me\\main", "/home/me/main"),
            ("\\\\wsl.localhost\\Ubuntu\\home\\me\\main", "\\\\wsl.localhost\\Debian\\home\\me\\main"),
            ("C:/dev/main", "D:/dev/main"),
            ("/tmp/main", "C:/dev/main"),
            ("/tmp/main", ""),
        ] {
            assert_eq!(
                repo_key_of(folder, &DotGit::Directory(Some(real.to_string()))),
                repo_key_of(folder, &DotGit::Directory(None)),
                "{folder} {real}"
            );
        }
        // 접두가 같으면 대소문자만 다른 드라이브 경로도 실제 경로를 따른다.
        assert_eq!(
            repo_key_of("c:/dev/main", &DotGit::Directory(Some("C:/Dev/main".to_string()))).as_deref(),
            Some("C:/Dev/main")
        );
    }
}
