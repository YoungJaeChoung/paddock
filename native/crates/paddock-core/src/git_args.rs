//! 앱이 git에 시키는 일마다의 인자 목록. 셸을 거치지 않는 인자 배열이고, 사용자 파일 이름은 항상 `--` 뒤에 둔다(`-rf` 같은
//! 이름의 파일이 옵션으로 읽히지 않게). 커밋 메시지는 `-m` 뒤 인자 하나로만 넘긴다.
use crate::git_status::{Change, Section};

fn owned(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| arg.to_string()).collect()
}

/// `prefix` 뒤에 `--`와 경로들을 붙인다. 경로는 항상 `--` 뒤에 온다.
fn with_paths(
    prefix: &[&str],
    paths: &[String],
) -> Vec<String> {
    let mut args = owned(prefix);
    args.push("--".to_string());
    args.extend(paths.iter().cloned());
    args
}

/// 저장소 루트를 묻는다.
pub fn toplevel() -> Vec<String> {
    owned(&["rev-parse", "--show-toplevel"])
}

/// 변경 목록과 브랜치를 한 번에 읽는다. 추적하지 않는 파일은 폴더가 아니라 파일 하나씩 낸다.
pub fn status() -> Vec<String> {
    owned(&["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"])
}

/// 경로들을 스테이징한다.
pub fn stage(paths: &[String]) -> Vec<String> {
    with_paths(&["add"], paths)
}

/// 모든 변경을 스테이징한다.
pub fn stage_all() -> Vec<String> {
    owned(&["add", "-A"])
}

/// 경로들의 스테이징을 푼다. 아직 커밋이 없는 저장소(`has_commit`이 거짓)는 HEAD가 없어 `restore --staged`를 쓸 수 없으므로 `rm --cached`다.
pub fn unstage(
    paths: &[String],
    has_commit: bool,
) -> Vec<String> {
    if has_commit {
        with_paths(&["restore", "--staged"], paths)
    } else {
        with_paths(&["rm", "--cached", "-r", "-q"], paths)
    }
}

/// 모든 스테이징을 푼다.
pub fn unstage_all(has_commit: bool) -> Vec<String> {
    unstage(&[".".to_string()], has_commit)
}

/// 스테이징된 변경을 커밋한다. 메시지는 인자 하나로 넘기므로 `;`·`$(…)`가 그대로 제목이 된다.
pub fn commit(message: &str) -> Vec<String> {
    vec!["commit".to_string(), "-m".to_string(), message.to_string()]
}

/// 작업 폴더의 변경을 버린다. 추적 중인 파일은 index 내용으로 되돌리고(스테이징한 내용은 그대로) 추적하지 않는 파일은 지운다.
pub fn discard(change: &Change) -> Vec<String> {
    let path = [change.path.clone()];
    match change.section {
        Section::Untracked => with_paths(&["clean", "-f", "-q"], &path),
        _ => with_paths(&["restore"], &path),
    }
}

/// 변경 하나의 diff. 스테이징된 쪽은 index와 HEAD를, 아닌 쪽은 작업 폴더와 index를 비교한다. 이름이 바뀐 파일은 이전 경로도 함께 준다.
pub fn diff(change: &Change) -> Vec<String> {
    let mut paths = Vec::new();
    paths.extend(change.orig_path.clone().filter(|_| change.section == Section::Staged));
    paths.push(change.path.clone());
    let head: &[&str] = if change.section == Section::Staged {
        if paths.len() > 1 { &["--cached", "-M"] } else { &["--cached"] }
    } else {
        &[]
    };
    let mut prefix = vec!["diff", "--no-color", "--no-ext-diff"];
    prefix.extend(head);
    with_paths(&prefix, &paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(
        path: &str,
        section: Section,
    ) -> Change {
        Change { path: path.into(), orig_path: None, index: '.', worktree: 'M', section }
    }

    fn after_dashes(args: &[String]) -> Vec<&str> {
        let at = args.iter().position(|arg| arg == "--").expect("a -- separator");
        args[at + 1..].iter().map(String::as_str).collect()
    }

    #[test]
    fn test_c_t_injection_paths_follow_double_dash() {
        let evil = vec!["-rf".to_string(), "--force".to_string(), "-n".to_string()];
        for args in [stage(&evil), unstage(&evil, true), unstage(&evil, false)] {
            assert_eq!(after_dashes(&args), ["-rf", "--force", "-n"], "{args:?}");
            assert!(args[..args.iter().position(|arg| arg == "--").unwrap()].iter().all(|arg| !arg.starts_with("-rf")));
        }
        for section in [Section::Changes, Section::Untracked, Section::Staged] {
            let args = if section == Section::Staged {
                diff(&change("-rf", section))
            } else {
                discard(&change("-rf", section))
            };
            assert_eq!(after_dashes(&args), ["-rf"], "{args:?}");
        }
    }

    #[test]
    fn test_c_d53_commit_message_is_one_argument_after_dash_m() {
        assert_eq!(commit("first; $(x) `y` \"z\""), ["commit", "-m", "first; $(x) `y` \"z\""]);
        assert_eq!(commit("-n"), ["commit", "-m", "-n"], "a message that looks like an option stays the value of -m");
    }

    #[test]
    fn test_c_d53_stage_unstage_and_all_variants() {
        let one = vec!["a b.txt".to_string()];
        assert_eq!(stage(&one), ["add", "--", "a b.txt"]);
        assert_eq!(stage_all(), ["add", "-A"]);
        assert_eq!(unstage(&one, true), ["restore", "--staged", "--", "a b.txt"]);
        assert_eq!(unstage(&one, false), ["rm", "--cached", "-r", "-q", "--", "a b.txt"]);
        assert_eq!(unstage_all(true), ["restore", "--staged", "--", "."]);
        assert_eq!(unstage_all(false), ["rm", "--cached", "-r", "-q", "--", "."]);
    }

    #[test]
    fn test_c_d53_discard_restores_tracked_files_and_cleans_untracked_ones() {
        assert_eq!(discard(&change("a.rs", Section::Changes)), ["restore", "--", "a.rs"]);
        assert_eq!(discard(&change("n.txt", Section::Untracked)), ["clean", "-f", "-q", "--", "n.txt"]);
    }

    #[test]
    fn test_c_d53_diff_compares_the_index_with_head_for_staged_rows_and_includes_the_old_name_of_a_rename() {
        assert_eq!(diff(&change("a.rs", Section::Changes)), ["diff", "--no-color", "--no-ext-diff", "--", "a.rs"]);
        assert_eq!(
            diff(&change("a.rs", Section::Staged)),
            ["diff", "--no-color", "--no-ext-diff", "--cached", "--", "a.rs"]
        );
        let renamed = Change { orig_path: Some("old.rs".into()), ..change("new.rs", Section::Staged) };
        assert_eq!(diff(&renamed), ["diff", "--no-color", "--no-ext-diff", "--cached", "-M", "--", "old.rs", "new.rs"]);
    }
}
