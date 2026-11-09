//! 파일 빠른 열기의 규칙: 작업 폴더의 상대 경로 목록에서 입력한 글자로 파일을 골라 점수 순으로 늘어놓는다.
//!
//! 입력한 글자가 경로 안에 순서대로(띄엄띄엄) 들어 있으면 일치로 본다. 점수는 연속해서 맞을수록, 이름 첫머리·구분자 뒤·낙타 표기 대문자에서 맞을수록,
//! 폴더가 아니라 파일 이름 안에서 맞을수록, 경로가 짧을수록 높다. 색인(폴더 훑기)은 앱이 하고 여기서는 이미 모은 목록만 다룬다.
use std::cmp::Reverse;

/// 목록에 보이는 최대 줄 수.
pub const LIMIT: usize = 50;

/// 색인에 담는 파일 수의 상한. 넘으면 더 담지 않고 안내를 보인다.
pub const MAX_INDEXED: usize = 20_000;

/// 색인이 상한에 닿았을 때 목록 아래에 보이는 안내.
pub const CAPPED_NOTICE: &str = "Only the first 20,000 files are listed. Type more of the name to narrow it down.";

/// 입력칸의 안내 글.
pub const PLACEHOLDER: &str = "Go to file by name";

/// 일치하는 파일이 하나도 없을 때의 안내.
pub const NO_MATCH: &str = "No matching files.";

/// 구분자·점·밑줄·가로줄·빈칸 뒤인가(이름 조각의 첫머리).
fn is_boundary(previous: Option<char>) -> bool {
    previous.is_none_or(|c| matches!(c, '/' | '\\' | '.' | '_' | '-' | ' '))
}

/// `text`(소문자로 바꾸지 않은 원문) 안에서 `query`(소문자)가 순서대로 들어 있는지 앞에서부터 훑어 점수를 매긴다. 들어 있지 않으면 None이다.
fn greedy_score(
    query: &[char],
    text: &str,
) -> Option<i64> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let mut score = 0i64;
    let mut previous_match: Option<usize> = None;
    for wanted in query {
        let found = (at..chars.len()).find(|&index| chars[index].to_lowercase().next() == Some(*wanted))?;
        let before = if found == 0 { None } else { Some(chars[found - 1]) };
        let camel = chars[found].is_uppercase() && before.is_some_and(char::is_lowercase);
        score += 10;
        if previous_match.is_some_and(|last| last + 1 == found) {
            score += 15;
        }
        if is_boundary(before) || camel {
            score += 12;
        }
        previous_match = Some(found);
        at = found + 1;
    }
    Some(score)
}

/// `query`가 `path`(작업 폴더 기준 상대 경로, 구분자 `/`)에 맞는 정도. 맞지 않으면 None이고, 클수록 더 맞는다. 대소문자는 구별하지 않는다.
///
/// Examples
/// --------
/// | query  | path              | 결과                                  |
/// |--------|-------------------|---------------------------------------|
/// | `a.j`  | `src/a.js`        | 큰 점수(이름 첫머리부터 이어서 맞음)  |
/// | `a.j`  | `lib/data.x/jar.txt`| 작은 점수(띄엄띄엄)                   |
/// | `zzz`  | `src/a.js`        | None                                  |
/// | ``     | 무엇이든          | `Some(0)`                             |
pub fn score(
    query: &str,
    path: &str,
) -> Option<u32> {
    let wanted: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let name = path.rsplit('/').next().unwrap_or(path);
    let in_path = greedy_score(&wanted, path);
    // 파일 이름 안에서 맞으면 폴더를 가로질러 맞은 것보다 훨씬 높게 친다.
    let in_name = greedy_score(&wanted, name).map(|raw| raw + 40);
    let raw = match (wanted.is_empty(), in_name, in_path) {
        (true, ..) => Some(0),
        (false, Some(name_score), Some(path_score)) => Some(name_score.max(path_score)),
        (false, Some(name_score), None) => Some(name_score),
        (false, None, path_score) => path_score,
    };
    raw.map(|raw| u32::try_from((raw * 4 - i64::try_from(path.len().min(200)).unwrap_or(200)).max(1)).unwrap_or(1))
}

/// `paths` 중 `query`에 맞는 것을 점수가 높은 순(같으면 짧은 경로, 그다음 사전순)으로 최대 `limit`개 골라 목록의 번호를 돌려준다.
/// 질의가 비면 앞에서부터 `limit`개다.
///
/// Examples
/// --------
/// | query | paths                              | limit | 결과(번호)  |
/// |-------|------------------------------------|-------|-------------|
/// | `a.j` | `lib/data.x/jar.txt`, `src/a.js`     | 50    | `[1, 0]`    |
/// | `zzz` | `src/a.js`                         | 50    | `[]`        |
/// | ``    | `a`, `b`, `c`                      | 2     | `[0, 1]`    |
pub fn rank(
    query: &str,
    paths: &[String],
    limit: usize,
) -> Vec<usize> {
    if query.trim().is_empty() {
        (0..paths.len().min(limit)).collect()
    } else {
        let mut scored: Vec<(usize, u32)> = paths
            .iter()
            .enumerate()
            .filter_map(|(index, path)| score(query, path).map(|score| (index, score)))
            .collect();
        // 정렬 열쇠(경로 복사본)는 후보마다 한 번만 만든다(비교마다 만들지 않는다).
        scored.sort_by_cached_key(|(index, score)| (Reverse(*score), paths[*index].len(), paths[*index].clone()));
        scored.into_iter().take(limit).map(|(index, _)| index).collect()
    }
}

/// 목록 한 줄에 보일 이름과 폴더. 폴더가 없으면 빈 글자다.
///
/// Examples
/// --------
/// | path            | 결과                    |
/// |-----------------|-------------------------|
/// | `src/lib/a.js`  | (`a.js`, `src/lib`)     |
/// | `README.md`     | (`README.md`, ``)       |
pub fn display(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((folder, name)) => (name.to_string(), folder.to_string()),
        None => (path.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn test_c_quick_1_the_file_name_beats_a_scattered_match_in_folders() {
        let list = paths(&["lib/data.x/jar.txt", "src/a.js", "docs/x.md"]);
        assert_eq!(rank("a.j", &list, LIMIT), vec![1, 0]);
        assert!(score("a.j", "src/a.js") > score("a.j", "lib/data.x/jar.txt"));
        assert_eq!(score("zzz", "src/a.js"), None);
    }

    #[test]
    fn test_c_quick_2_case_is_ignored_and_boundaries_and_camel_humps_score_higher() {
        assert!(score("READ", "docs/readme.md").is_some());
        assert!(score("fb", "src/FooBar.js") > score("fb", "src/fxxbxx.js"));
        assert!(score("ab", "x/a-b.txt") > score("ab", "x/axxb.txt"));
        // 한글 이름도 글자 단위로 맞는다.
        assert!(score("한글", "문서/한글 문서.md").is_some());
    }

    #[test]
    fn test_c_quick_3_the_list_is_capped_and_an_empty_query_keeps_the_given_order() {
        let many: Vec<String> = (0..200).map(|n| format!("src/file{n}.rs")).collect();
        assert_eq!(rank("file", &many, LIMIT).len(), LIMIT);
        assert_eq!(rank("", &paths(&["a", "b", "c"]), 2), vec![0, 1]);
        assert_eq!(rank("   ", &paths(&["a"]), 5), vec![0]);
        // 점수가 같으면 짧은 경로가 먼저다.
        assert_eq!(rank("a", &paths(&["bb/a", "a"]), 5), vec![1, 0]);
    }

    #[test]
    fn test_c_quick_4_each_line_shows_the_name_then_the_folder() {
        assert_eq!(display("src/lib/a.js"), ("a.js".to_string(), "src/lib".to_string()));
        assert_eq!(display("README.md"), ("README.md".to_string(), String::new()));
    }

    #[test]
    fn test_c_quick_5_ranking_twenty_thousand_paths_is_fast_enough_for_one_keystroke() {
        let many: Vec<String> =
            (0..MAX_INDEXED).map(|n| format!("packages/pkg{}/src/components/Widget{n}.tsx", n % 97)).collect();
        let started = std::time::Instant::now();
        let top = rank("wid5", &many, LIMIT);
        assert!(!top.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "{:?}", started.elapsed());
    }
}
