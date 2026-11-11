//! 폴더 변경 감시: 운영체제 파일 사건(notify)으로 폴더 안 변경을 알린다.
//!
//! WSL 공유(`\\wsl.localhost\…`)는 파일 사건이 오지 않으므로 감시하지 않고, 서비스가 주기로 수정 시각을 비교한다.
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// 경로가 WSL 배포판의 파일 공유(`\\wsl.localhost\…`·`\\wsl$\…`)인지. 슬래시 철자와 대소문자 차이는 같은 공유다.
///
/// Examples
/// --------
/// | 경로                              | 결과  |
/// |-----------------------------------|-------|
/// | `\\wsl.localhost\Ubuntu\home\me`  | true  |
/// | `//WSL$/Ubuntu`                   | true  |
/// | `C:\Users\me`·`\\server\share`    | false |
pub fn is_wsl_share(path: &str) -> bool {
    let normalized = path.replace('/', "\\").to_ascii_lowercase();
    normalized.starts_with("\\\\wsl.localhost\\") || normalized.starts_with("\\\\wsl$\\")
}

/// 감시 중인 폴더. 버리면 감시가 끝난다.
pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
}

impl FolderWatcher {
    /// `root` 아래 변경(재귀)이 있을 때마다 `wake`를 부른다. 숨긴 이름 아래의 변경과 읽기 사건은 알리지 않는다.
    ///
    /// 숨긴 이름(`.git` 등) 안은 git 작업이 쉴 새 없이 바꾸므로, 알리면 목록을 쓸데없이 계속 다시 읽는다.
    pub fn start(
        root: &Path,
        hidden: HashSet<String>,
        wake: impl Fn() + Send + 'static,
    ) -> notify::Result<Self> {
        Self::start_ignoring(root, move |bases, path| is_hidden_path(bases, path, &hidden), wake)
    }

    /// `root` 아래 변경(재귀)이 있을 때마다 `wake`를 부르되, `ignored`가 참인 경로의 사건만 있으면 알리지 않는다.
    /// `ignored`는 감시 기준 경로 목록(원래 경로와 실제 경로)과 사건 경로를 받는다.
    pub fn start_ignoring(
        root: &Path,
        ignored: impl Fn(&[PathBuf], &Path) -> bool + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> notify::Result<Self> {
        // 사건 경로가 원래 경로가 아니라 실제 경로(링크·`/private/var` 같은 별칭을 푼 것)로 올 수 있어 둘 다 기준으로 삼는다.
        let mut bases = vec![root.to_path_buf()];
        bases.extend(root.canonicalize().ok().filter(|real| real != root));
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && !matches!(event.kind, EventKind::Access(_))
                && event.paths.iter().any(|path| !ignored(&bases, path))
            {
                wake();
            }
        })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher })
    }
}

/// Git 변경 표시를 새로 읽을 필요가 없는 경로인지: 숨긴 이름(`.git` 제외) 아래이거나, `.git` 안에서 `index`·`HEAD`가 아닌 것.
///
/// 스테이징·커밋·브랜치 변경은 `.git/index`·`.git/HEAD`를 바꾸므로 터미널에서 한 git 명령도 표시에 반영된다.
/// 나머지 `.git` 안(객체·로그·잠금 파일)은 git이 쉴 새 없이 쓰므로 알리면 status가 status를 부르는 고리가 된다.
pub fn is_git_noise(
    bases: &[PathBuf],
    path: &Path,
    hidden: &HashSet<String>,
) -> bool {
    bases.iter().filter_map(|base| path.strip_prefix(base).ok()).any(|relative| {
        let names: Vec<String> = relative
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();
        match names.iter().position(|name| name == ".git") {
            Some(at) => names.get(at + 1).is_none_or(|next| next != "index" && next != "HEAD") || names.len() > at + 2,
            None => names.iter().any(|name| hidden.contains(name)),
        }
    })
}

/// `path`가 `bases` 중 어느 폴더 아래에서 숨긴 이름을 지나는지.
fn is_hidden_path(
    bases: &[PathBuf],
    path: &Path,
    hidden: &HashSet<String>,
) -> bool {
    bases.iter().filter_map(|base| path.strip_prefix(base).ok()).any(|relative| {
        relative
            .components()
            .any(|part| matches!(part, Component::Normal(name) if hidden.contains(name.to_string_lossy().as_ref())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_d52_git_noise_keeps_the_index_and_head_and_drops_the_rest_of_dot_git() {
        let hidden: HashSet<String> = ["node_modules".to_string()].into_iter().collect();
        let bases = [PathBuf::from("/r")];
        assert!(!is_git_noise(&bases, Path::new("/r/.git/index"), &hidden));
        assert!(!is_git_noise(&bases, Path::new("/r/.git/HEAD"), &hidden));
        assert!(is_git_noise(&bases, Path::new("/r/.git/index.lock"), &hidden));
        assert!(is_git_noise(&bases, Path::new("/r/.git/objects/ab/cdef"), &hidden));
        assert!(is_git_noise(&bases, Path::new("/r/.git/logs/HEAD"), &hidden));
        assert!(is_git_noise(&bases, Path::new("/r/node_modules/x/y.js"), &hidden));
        assert!(!is_git_noise(&bases, Path::new("/r/src/a.rs"), &hidden));
    }

    #[test]
    fn test_c_files_30_hidden_paths_are_recognised_under_the_given_or_the_real_base() {
        let hidden: HashSet<String> = [".git".to_string()].into_iter().collect();
        let bases = [PathBuf::from("/var/proj"), PathBuf::from("/private/var/proj")];
        assert!(is_hidden_path(&bases, Path::new("/var/proj/.git/index"), &hidden));
        assert!(is_hidden_path(&bases, Path::new("/private/var/proj/src/.git/x"), &hidden));
        assert!(!is_hidden_path(&bases, Path::new("/private/var/proj/src/a.rs"), &hidden));
        assert!(!is_hidden_path(&bases, Path::new("/elsewhere/.git/x"), &hidden));
    }
}
