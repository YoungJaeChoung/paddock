//! 변경 비교 탭: `git diff` 결과를 읽기 전용 비교 보기(gpui-component `Diff`)로 보여 준다.
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::{AppContext, Context, Entity, IntoElement, ParentElement, Styled, div, relative};

use crate::window_view::WorkWindow;

/// 열어 둔 비교 하나: 비교 보기 상태와 상태 글에 보일 요약.
pub struct DiffBody {
    pub state: Entity<DiffState>,
    pub n_files: usize,
    pub n_added: usize,
    pub n_removed: usize,
}

/// `git diff` 글자를 파일별 비교로 읽는다. 큰 비교는 읽는 데 시간이 걸리므로 창 스레드가 아닌 곳에서 부른다.
pub fn parse_patch(patch: &str) -> Result<Vec<DiffFile>, String> {
    match DiffFile::parse(patch) {
        Ok(files) if files.is_empty() => Err("No changes.".to_string()),
        Ok(files) => Ok(files),
        Err(error) => Err(format!("Could not read this comparison: {error}")),
    }
}

/// 읽어 둔 비교로 비교 보기를 만든다(창 스레드에서 부른다).
pub fn build_diff(
    files: Vec<DiffFile>,
    cx: &mut Context<WorkWindow>,
) -> DiffBody {
    let (n_added, n_removed) = files.iter().fold((0, 0), |(a, r), file| (a + file.additions(), r + file.deletions()));
    let n_files = files.len();
    let state = cx.new(|cx| DiffState::new(files, cx));
    DiffBody { state, n_files, n_added, n_removed }
}

/// 비교 보기 그리기. 읽기 전용이다.
pub fn diff_view(body: &DiffBody) -> impl IntoElement {
    div().size_full().child(Diff::new(&body.state).h(relative(1.0)))
}
