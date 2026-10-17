//! 폴더 읽기: 작업 스레드가 폴더 이름과 종류만 읽어 `Directory` 트리로 만든다.
//!
//! 파일 내용과 수정 시각은 목록에 필요 없다. 수정 시각은 "이 폴더가 그대로인가"를 가르는 열쇠로만 쓴다 —
//! 폴더가 그대로면 이름 목록을 다시 읽지 않아 느린 파일 시스템(WSL 공유)에서도 다시 읽기가 값싸다.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use paddock_core::file_tree::{Directory, Entry, child_path, sort_entries};

/// 폴더를 읽은 시각과 그 폴더의 수정 시각 차이가 이보다 작으면 읽은 목록을 믿지 않는다.
/// 파일 시스템 시각의 해상도가 거칠어서, 같은 시각에 일어난 다음 변경이 수정 시각을 바꾸지 못할 수 있기 때문이다.
pub const SETTLE: Duration = Duration::from_secs(1);

/// 읽어 둔 폴더 하나: 읽을 때의 수정 시각과 이름·폴더 여부 목록.
struct Listing {
    modified: Option<SystemTime>,
    /// 읽은 시각이 수정 시각보다 `SETTLE` 이상 늦었다 — 같은 수정 시각이면 목록이 그대로라고 믿어도 된다.
    settled: bool,
    names: Vec<(String, bool)>,
}

/// 폴더별로 읽어 둔 이름 목록. 이번 읽기에서 쓰이지 않은 폴더(접힌 폴더)는 읽기가 끝날 때 버린다.
#[derive(Default)]
pub struct ListingCache {
    listings: HashMap<String, Listing>,
    n_listings: usize,
}

impl ListingCache {
    /// 지금까지 폴더를 디스크에서 실제로 읽은 횟수.
    pub fn n_listings(&self) -> usize {
        self.n_listings
    }

    /// 읽어 둔 목록을 모두 버린다. 다음 읽기가 모든 폴더를 디스크에서 다시 읽는다.
    pub fn clear(&mut self) {
        self.listings.clear();
    }

    /// 읽어 둔 폴더 중 수정 시각이 달라졌거나 아직 믿을 수 없는 것이 있는지. 파일 시스템 사건이 오지 않는 폴더를 주기로 살필 때 쓴다.
    pub fn is_stale(&self) -> bool {
        self.listings.iter().any(|(path, listing)| {
            !listing.settled || modified_of(path).is_none_or(|now| Some(now) != listing.modified)
        })
    }
}

/// 폴더의 수정 시각. 폴더가 없거나 폴더가 아니거나 시각을 못 읽으면 None이다.
fn modified_of(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).ok().filter(|metadata| metadata.is_dir()).and_then(|metadata| metadata.modified().ok())
}

/// `path`가 있는 폴더인지.
pub fn folder_exists(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// 폴더 안 이름과 폴더 여부. 읽을 수 없으면 None이다. 링크는 가리키는 대상이 폴더인지로 가른다.
fn list_names(path: &str) -> Option<Vec<(String, bool)>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path).ok()?.flatten() {
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => std::fs::metadata(entry.path()).is_ok_and(|target| target.is_dir()),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        names.push((entry.file_name().to_string_lossy().to_string(), is_dir));
    }
    Some(names)
}

/// 폴더 하나의 이름 목록. 수정 시각이 그대로이고 믿을 수 있으면 읽어 둔 목록을, 아니면 디스크에서 읽은 목록을 준다.
fn names_of(
    path: &str,
    cache: &mut ListingCache,
) -> Option<Vec<(String, bool)>> {
    let is_folder = std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir());
    let modified = modified_of(path);
    let reusable = cache
        .listings
        .get(path)
        .filter(|listing| listing.settled && modified.is_some() && listing.modified == modified)
        .map(|listing| listing.names.clone());
    let names = match (is_folder, reusable) {
        (false, _) => None,
        (true, Some(names)) => Some(names),
        (true, None) => {
            cache.n_listings += 1;
            list_names(path).inspect(|names| {
                let settled = modified.is_some_and(|stamp| {
                    SystemTime::now().duration_since(stamp).is_ok_and(|elapsed| elapsed >= SETTLE)
                });
                cache.listings.insert(path.to_string(), Listing { modified, settled, names: names.clone() });
            })
        }
    };
    if names.is_none() {
        cache.listings.remove(path);
    }
    names
}

fn read_directory(
    path: &str,
    expanded: &HashSet<String>,
    hidden: &HashSet<String>,
    cache: &mut ListingCache,
    used: &mut HashSet<String>,
) -> Directory {
    used.insert(path.to_string());
    match names_of(path, cache) {
        None => Directory { found: false, empty: true, children: Vec::new() },
        Some(names) => {
            let empty = names.is_empty();
            let mut children: Vec<Entry> = names
                .into_iter()
                .filter(|(name, _)| !hidden.contains(name))
                .map(|(name, is_dir)| Entry {
                    path: child_path(path, &name),
                    name,
                    is_dir,
                    expanded: false,
                    directory: None,
                })
                .collect();
            sort_entries(&mut children);
            for child in children.iter_mut().filter(|child| child.is_dir && expanded.contains(&child.path)) {
                child.expanded = true;
                child.directory = Some(Box::new(read_directory(&child.path, expanded, hidden, cache, used)));
            }
            Directory { found: true, empty, children }
        }
    }
}

/// `root` 폴더와 그 안의 펼친 폴더를 읽어 트리로 만든다. `hidden` 이름은 뺀다.
///
/// 폴더를 읽지 못하면(없거나 권한이 없음) 그 폴더만 `found: false`다. 숨긴 이름만 든 폴더는 비었다고 보지 않는다.
pub fn read_tree(
    root: &str,
    expanded: &HashSet<String>,
    hidden: &HashSet<String>,
    cache: &mut ListingCache,
) -> Directory {
    let mut used = HashSet::new();
    let directory = read_directory(root, expanded, hidden, cache, &mut used);
    cache.listings.retain(|path, _| used.contains(path));
    directory
}
