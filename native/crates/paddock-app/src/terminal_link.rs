//! 터미널 경로 링크의 앱 쪽: 화면 줄에서 찾은 경로꼴 글자를 칸 범위로 옮기고, 터미널의 현재 폴더 기준으로 실제 파일을 가리키는지 정한다.
//!
//! 글자 찾기와 경로 만들기 규칙은 `core::terminal_link`에 있고, 이 모듈은 화면 격자와 파일 시스템을 잇는다.
//! 파일 존재 확인은 느린 파일 시스템(WSL 공유)에서 창을 멈출 수 있어 창 스레드에서 부르지 않는다(자동 점검은 예외).
use std::ops::Range;

use alacritty_terminal::grid::Dimensions;

use paddock_core::terminal_link::{LinkSpan, find_paths, resolve};
use unicode_width::UnicodeWidthChar;

use crate::terminal::grid::Grid;

/// 화면 한 줄 안의 경로꼴 글자 하나와 그것이 차지하는 칸 범위.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowLink {
    pub span: LinkSpan,
    /// 글자가 놓인 칸(왼쪽 포함, 오른쪽 제외).
    pub columns: Range<usize>,
}

/// 링크가 가리키는 실제 경로와 줄 번호.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: String,
    pub line: Option<u32>,
}

/// 줄의 글자(`text`)와 글자마다의 시작 칸(`columns`)에서 경로꼴 글자를 모두 찾는다. 칸 범위의 끝은 마지막 글자의 너비까지 포함한다.
///
/// Examples
/// --------
/// | 글자           | 링크 글자    | 칸 범위 |
/// |----------------|--------------|---------|
/// | `go a.js:3`    | `a.js:3`     | 3..9    |
/// | `한글 a/b.md`  | `a/b.md`     | 7..13   |
pub fn links_in_row(
    text: &str,
    columns: &[usize],
) -> Vec<RowLink> {
    let chars: Vec<char> = text.chars().collect();
    find_paths(text)
        .into_iter()
        .filter_map(|span| {
            let first = columns.get(span.start).copied()?;
            let last_index = span.start + span.len - 1;
            let last = columns.get(last_index).copied()?;
            let width = chars.get(last_index).and_then(|c| c.width()).unwrap_or(1).max(1);
            Some(RowLink { span, columns: first..last + width })
        })
        .collect()
}

/// `column`칸 위에 놓인 링크.
pub fn link_at(
    links: &[RowLink],
    column: usize,
) -> Option<&RowLink> {
    links.iter().find(|link| link.columns.contains(&column))
}

/// 링크를 실제 경로로 만든다. 현재 폴더·홈을 몰라 만들 수 없으면 None이다.
pub fn target_of(
    link: &RowLink,
    terminal_cwd: Option<&str>,
    home: Option<&str>,
) -> Option<Target> {
    resolve(&link.span.path, terminal_cwd, home).map(|path| Target { path, line: link.span.line })
}

/// 경로가 실제 파일(폴더가 아님)인지. 파일 시스템을 읽으므로 창 스레드 밖에서 부른다.
pub fn is_file(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

/// 터미널이 마지막으로 알려 온 현재 폴더.
pub fn terminal_cwd(grid: &Grid) -> Option<String> {
    grid.shared.state.lock().ok().and_then(|state| state.clone()).and_then(|state| state.cwd)
}

/// 사용자 홈 폴더.
pub fn home_folder() -> Option<String> {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()
}

/// 지금 화면의 경로꼴 글자를 실제 경로로 만들어 위에서 아래, 왼쪽에서 오른쪽 순서로 모은다. 파일인지는 확인하지 않는다(`is_file`은 창 스레드 밖에서).
pub fn screen_targets(grid: &Grid) -> Vec<Target> {
    let cwd = terminal_cwd(grid);
    let home = home_folder();
    let n_rows = grid.term.lock().screen_lines() as i32;
    let mut targets = Vec::new();
    for row in 0..n_rows {
        let (text, columns) = grid.row_cells(row);
        targets.extend(
            links_in_row(&text, &columns).iter().filter_map(|link| target_of(link, cwd.as_deref(), home.as_deref())),
        );
    }
    targets
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;

    fn grid_with(text: &str) -> Grid {
        let grid = Grid::new(40, 4, |_| {}, |_, _| {});
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *grid.term.lock(), text.as_bytes());
        grid
    }

    #[test]
    fn test_links_get_the_cell_columns_of_their_characters_including_wide_ones() {
        let grid = grid_with("go a.js:3\r\n\u{d55c}\u{ae00} a/b.md");
        let (text, columns) = grid.row_cells(0);
        let links = links_in_row(&text, &columns);
        assert_eq!(links.len(), 1);
        assert_eq!(
            (links[0].span.path.as_str(), links[0].span.line, links[0].columns.clone()),
            ("a.js", Some(3), 3..9)
        );
        assert!(link_at(&links, 4).is_some());
        assert!(link_at(&links, 2).is_none());
        assert!(link_at(&links, 9).is_none());
        let (text, columns) = grid.row_cells(1);
        let links = links_in_row(&text, &columns);
        assert_eq!(links[0].columns, 5..11, "the two wide characters take four cells");
    }

    #[test]
    fn test_screen_targets_are_resolved_candidates_and_only_real_files_pass_the_file_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("a.js"), "x").unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let grid = grid_with("src/a.js:12 src/missing.js src/\r\n");
        *grid.shared.state.lock().unwrap() = Some(paddock_protocol::message::TerminalState {
            id: 1,
            cwd: Some(root.clone()),
            program: None,
            is_agent: false,
            activity: None,
            finished: false,
            account: None,
            conversation: None,
            signal: None,
        });
        let targets: Vec<Target> = screen_targets(&grid).into_iter().filter(|target| is_file(&target.path)).collect();
        assert_eq!(
            targets,
            [Target {
                path: std::path::Path::new(&root).join("src").join("a.js").to_string_lossy().to_string(),
                line: Some(12)
            }],
            "a missing file and a folder are not links"
        );
    }
}
