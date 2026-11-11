//! `git status --porcelain=v2 -z --branch` 출력을 읽어 변경 목록과 브랜치 이름으로 만드는 순수 규칙.
//!
//! 한 파일이 스테이징된 변경과 작업 폴더 변경을 함께 가지면 두 구획에 한 행씩 나온다(Source control 보기와 같다).

/// 변경이 속한 구획. 보이는 순서이자 파일 목록 표시에서 같은 파일이 여러 번 올 때 앞선 것이 이기는 순서다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    /// 병합 충돌.
    Merge,
    /// 스테이징됨(index와 HEAD가 다름).
    Staged,
    /// 작업 폴더에서만 바뀜(추적 중인 파일).
    Changes,
    /// 추적하지 않는 새 파일.
    Untracked,
}

/// 변경 한 행. `index`·`worktree`는 git이 준 상태 글자이고 바뀌지 않았으면 `.`이다(`Untracked`는 `?`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// 저장소 루트 기준 경로(`/` 구분).
    pub path: String,
    /// 이름이 바뀐 파일의 이전 경로.
    pub orig_path: Option<String>,
    pub index: char,
    pub worktree: char,
    pub section: Section,
}

/// 화면에 보이는 상태 글자의 뜻.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Letter {
    Modified,
    Untracked,
    Added,
    Deleted,
    Renamed,
    /// 복사됨(`C`). 기존 화면의 규칙대로 충돌 색을 쓴다.
    Copied,
    /// 파일 종류가 바뀜(`T`).
    Typechange,
    /// 병합 충돌(`!`).
    Conflict,
}

impl Letter {
    /// 행 오른쪽에 그리는 글자: M·U·A·D·R·C.
    pub fn glyph(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::Untracked => 'U',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::Typechange => 'T',
            Self::Conflict => '!',
        }
    }
}

/// status 한 번의 결과.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// 브랜치 이름. 분리된 HEAD면 None이다.
    pub branch: Option<String>,
    pub detached: bool,
    /// 현재 커밋 번호. 아직 커밋이 없으면 None이다.
    pub oid: Option<String>,
    pub changes: Vec<Change>,
}

/// status 출력을 읽는다. 알 수 없는 줄은 건너뛴다.
///
/// Examples
/// --------
/// | 출력(NUL을 `␀`로)                                 | 결과                                          |
/// |----------------------------------------------------|-----------------------------------------------|
/// | `# branch.head main␀1 .M N... … a.rs␀`            | 브랜치 `main`, `a.rs` 작업 폴더 수정 한 행    |
/// | `1 MM N... … a.rs␀`                                | Staged 한 행 + Changes 한 행                  |
/// | `2 R. N... … R100 new␀old␀`                        | Staged 한 행(이름 바뀜, 이전 경로 `old`)      |
/// | `? new.txt␀`                                       | Untracked 한 행                               |
/// | `# branch.head (detached)␀`                        | 분리된 HEAD                                   |
pub fn parse_porcelain_v2_z(bytes: &[u8]) -> Status {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut status = Status::default();
    while let Some(field) = fields.next() {
        if let Some(header) = field.strip_prefix("# ") {
            match header.split_once(' ') {
                Some(("branch.head", "(detached)")) => status.detached = true,
                Some(("branch.head", name)) => status.branch = Some(name.to_string()),
                Some(("branch.oid", oid)) => status.oid = Some(oid.to_string()).filter(|oid| oid != "(initial)"),
                _ => {}
            }
        } else if let Some(path) = field.strip_prefix("? ") {
            status.changes.push(Change {
                path: path.to_string(),
                orig_path: None,
                index: '?',
                worktree: '?',
                section: Section::Untracked,
            });
        } else if let Some(rest) = field.strip_prefix("u ") {
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            let pieces: Vec<&str> = rest.splitn(10, ' ').collect();
            if let (Some(xy), Some(path)) = (pieces.first(), pieces.get(9)) {
                let mut codes = xy.chars();
                status.changes.push(Change {
                    path: path.to_string(),
                    orig_path: None,
                    index: codes.next().unwrap_or('U'),
                    worktree: codes.next().unwrap_or('U'),
                    section: Section::Merge,
                });
            }
        } else if field.starts_with("1 ") || field.starts_with("2 ") {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path> NUL <origPath>
            let is_rename = field.starts_with("2 ");
            let (n_before_path, parts) = if is_rename { (9, 10) } else { (8, 9) };
            let pieces: Vec<&str> = field.splitn(parts, ' ').collect();
            if let (Some(xy), Some(path)) = (pieces.get(1), pieces.get(n_before_path)) {
                let orig_path = if is_rename { fields.next().map(str::to_string) } else { None };
                let mut codes = xy.chars();
                let (index, worktree) = (codes.next().unwrap_or('.'), codes.next().unwrap_or('.'));
                for (section, code) in [(Section::Staged, index), (Section::Changes, worktree)] {
                    if code != '.' {
                        status.changes.push(Change {
                            path: path.to_string(),
                            orig_path: orig_path.clone().filter(|_| section == Section::Staged || code == 'R'),
                            index,
                            worktree,
                            section,
                        });
                    }
                }
            }
        }
    }
    status
}

impl Status {
    /// 변경 행 수(배지 숫자). 두 구획에 걸친 파일은 두 번 센다.
    pub fn n_changes(&self) -> usize {
        self.changes.len()
    }

    /// 한 구획의 행 수.
    pub fn n_in(
        &self,
        section: Section,
    ) -> usize {
        self.changes.iter().filter(|change| change.section == section).count()
    }

    /// 브랜치 단추에 보일 이름: 브랜치 이름, 분리된 HEAD면 커밋 앞 7자리, 커밋이 없는 새 저장소는 브랜치 이름이다.
    pub fn head_label(&self) -> Option<String> {
        match (&self.branch, &self.oid) {
            (Some(branch), _) => Some(branch.clone()),
            (None, Some(oid)) if self.detached => Some(oid.chars().take(7).collect()),
            _ => None,
        }
    }
}

/// 행의 상태 글자. 기존 화면(Source control 확장)과 같다: 병합 충돌은 `!`이되 우리·그쪽이 지운 충돌(`DU`·`UD`·`DD`)은 `D`,
/// 스테이징·작업 폴더 쪽은 index·worktree 글자(`M`·`A`·`D`·`R`·`C`·`T`), 추적 안 함은 `U`다.
pub fn mark_of(change: &Change) -> Letter {
    let code = match change.section {
        Section::Staged => change.index,
        _ => change.worktree,
    };
    match change.section {
        Section::Merge if change.index == 'D' || change.worktree == 'D' => Letter::Deleted,
        Section::Merge => Letter::Conflict,
        Section::Untracked => Letter::Untracked,
        Section::Staged | Section::Changes => match code {
            'A' => Letter::Added,
            'D' => Letter::Deleted,
            'R' => Letter::Renamed,
            'C' => Letter::Copied,
            'T' => Letter::Typechange,
            _ => Letter::Modified,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 줄 끝 `;`를 NUL로 바꿔 실제 `-z` 출력처럼 만든다.
    fn z(text: &str) -> Vec<u8> {
        text.replace(';', "\0").into_bytes()
    }

    const HEX: &str = "N... 100644 100644 100644 1111111 2222222";

    fn parse(entries: &str) -> Status {
        parse_porcelain_v2_z(&z(&format!("# branch.oid abcdef1234567;# branch.head main;{entries}")))
    }

    fn rows(status: &Status) -> Vec<(Section, char, String)> {
        status.changes.iter().map(|c| (c.section, mark_of(c).glyph(), c.path.clone())).collect()
    }

    #[test]
    fn test_c_status_1_a_worktree_modification_is_one_changes_row_with_m() {
        let status = parse(&format!("1 .M {HEX} src/a.rs;"));
        assert_eq!(rows(&status), [(Section::Changes, 'M', "src/a.rs".into())]);
        assert_eq!(status.branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_c_status_2_a_staged_modification_is_one_staged_row() {
        assert_eq!(rows(&parse(&format!("1 M. {HEX} a.rs;"))), [(Section::Staged, 'M', "a.rs".into())]);
    }

    #[test]
    fn test_c_status_3_a_file_changed_in_both_places_gets_a_staged_row_and_a_changes_row() {
        let status = parse(&format!("1 MM {HEX} a.rs;"));
        assert_eq!(rows(&status), [(Section::Staged, 'M', "a.rs".into()), (Section::Changes, 'M', "a.rs".into())]);
        assert_eq!(status.n_changes(), 2);
    }

    #[test]
    fn test_c_status_4_an_added_file_is_a_staged_a() {
        assert_eq!(rows(&parse(&format!("1 A. {HEX} n.txt;"))), [(Section::Staged, 'A', "n.txt".into())]);
    }

    #[test]
    fn test_c_status_5_deletions_are_d_in_the_section_where_they_happened() {
        let status = parse(&format!("1 .D {HEX} gone.txt;1 D. {HEX} gone2.txt;"));
        assert_eq!(
            rows(&status),
            [(Section::Changes, 'D', "gone.txt".into()), (Section::Staged, 'D', "gone2.txt".into())],
            "rows keep the order git printed them in; the view groups them by section"
        );
    }

    #[test]
    fn test_c_status_6_a_rename_keeps_the_old_path() {
        let status = parse(&format!("2 R. {HEX} R100 new name.txt;old name.txt;"));
        assert_eq!(rows(&status), [(Section::Staged, 'R', "new name.txt".into())]);
        assert_eq!(status.changes[0].orig_path.as_deref(), Some("old name.txt"));
    }

    #[test]
    fn test_c_status_7_an_untracked_file_is_u_and_its_name_keeps_spaces_and_hangul() {
        assert_eq!(rows(&parse("? 한글 파일.txt;")), [(Section::Untracked, 'U', "한글 파일.txt".into())]);
    }

    #[test]
    fn test_c_status_8_a_conflict_is_a_merge_row_with_c() {
        let status = parse("u UU N... 100644 100644 100644 100644 1111111 2222222 3333333 c.rs;");
        assert_eq!(rows(&status), [(Section::Merge, '!', "c.rs".into())]);
    }

    #[test]
    fn test_c_status_letters_follow_the_source_control_extension() {
        // (구획, index, worktree, 글자): 충돌 `!`, 복사 `C`, 종류 바뀜 `T`, 우리·그쪽이 지운 충돌 `D`.
        let table = [
            (Section::Staged, 'M', '.', 'M'),
            (Section::Staged, 'A', '.', 'A'),
            (Section::Staged, 'D', '.', 'D'),
            (Section::Staged, 'R', '.', 'R'),
            (Section::Staged, 'C', '.', 'C'),
            (Section::Staged, 'T', '.', 'T'),
            (Section::Changes, '.', 'M', 'M'),
            (Section::Changes, '.', 'D', 'D'),
            (Section::Changes, '.', 'T', 'T'),
            (Section::Changes, '.', 'A', 'A'),
            (Section::Untracked, '?', '?', 'U'),
            (Section::Merge, 'U', 'U', '!'),
            (Section::Merge, 'A', 'A', '!'),
            (Section::Merge, 'A', 'U', '!'),
            (Section::Merge, 'U', 'A', '!'),
            (Section::Merge, 'D', 'U', 'D'),
            (Section::Merge, 'U', 'D', 'D'),
            (Section::Merge, 'D', 'D', 'D'),
        ];
        for (section, index, worktree, glyph) in table {
            let change = Change { path: "p".into(), orig_path: None, index, worktree, section };
            assert_eq!(mark_of(&change).glyph(), glyph, "{section:?} {index}{worktree}");
        }
    }

    #[test]
    fn test_c_status_a_conflict_row_keeps_its_two_codes() {
        let status = parse("u DU N... 100644 100644 100644 100644 1111111 2222222 3333333 c.rs;");
        assert_eq!((status.changes[0].index, status.changes[0].worktree), ('D', 'U'));
        assert_eq!(mark_of(&status.changes[0]).glyph(), 'D');
    }

    #[test]
    fn test_c_status_9_a_detached_head_has_no_branch_and_is_labelled_with_the_short_commit() {
        let status = parse_porcelain_v2_z(&z("# branch.oid abcdef1234567;# branch.head (detached);"));
        assert!(status.detached && status.branch.is_none());
        assert_eq!(status.head_label().as_deref(), Some("abcdef1"));
    }

    #[test]
    fn test_c_status_10_an_empty_new_repository_has_a_branch_name_and_no_commit() {
        let status = parse_porcelain_v2_z(&z("# branch.oid (initial);# branch.head main;"));
        assert_eq!((status.branch.as_deref(), status.oid.as_deref()), (Some("main"), None));
        assert_eq!(status.head_label().as_deref(), Some("main"));
        assert_eq!(status.n_changes(), 0);
    }
}
