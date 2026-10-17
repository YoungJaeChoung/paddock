//! 화면 모양 값: 색·글꼴·크기·여백. 앱의 모든 화면 값이 여기서 나오며 그리기 도구를 모르는 순수 값이다.
//!
//! 기존 Electron판 Paddock 화면(창 틀 색은 Theia 기본 다크·라이트, 터미널 색은 VS Code 기본)과 같은 모양으로 그리기 위한
//! 값이다. 크기는 100% 배율의 논리 px이고, 실제 배율은 `zoom_factor`가 정한다.
//! 색은 다크·라이트 두 벌(`Colors::DARK`·`Colors::LIGHT`)이고 크기는 두 모드가 같다.

/// 화면 색 모드.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    /// 고를 수 있는 모드 전부(빠른 설정의 테마 목록 순서).
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];

    /// 설정 파일에 쓰는 색 테마 이름. `from_theme_name`이 이 이름에서 같은 모드를 찾는다.
    pub fn theme_name(self) -> &'static str {
        match self {
            Self::Dark => "Mermaid Dark",
            Self::Light => "Mermaid Light",
        }
    }

    /// 설정의 색 테마 이름에서 모드를 정한다. 이 앱이 가진 테마 이름(`ALL`)과 정확히 같을 때만 그 모드이고,
    /// 설정이 없거나 모르는 이름이면 다크다(이름에 `light`가 들어 있어도 이 앱에 없는 테마면 다크다).
    ///
    /// Examples
    /// --------
    /// | 테마 이름          | 결과    |
    /// |--------------------|---------|
    /// | `Mermaid Light`    | `Light` |
    /// | `Mermaid Dark`     | `Dark`  |
    /// | `Solarized Light`  | `Dark`  |
    /// | (설정 없음)        | `Dark`  |
    pub fn from_theme_name(name: Option<&str>) -> Self {
        Self::ALL.into_iter().find(|mode| Some(mode.theme_name()) == name).unwrap_or(Self::Dark)
    }

    /// 이 모드의 색 묶음.
    pub fn colors(self) -> &'static Colors {
        match self {
            Self::Dark => &Colors::DARK,
            Self::Light => &Colors::LIGHT,
        }
    }
}

/// 화면 배율: 설정의 `window.zoomLevel`을 VS Code·Theia 규칙(1.2의 거듭제곱)으로 바꾼다.
///
/// Examples
/// --------
/// | zoomLevel | 배율  |
/// |-----------|-------|
/// | 0         | 1.0   |
/// | 3         | 1.728 |
/// | -1        | 0.833 |
pub fn zoom_factor(level: f64) -> f32 {
    1.2_f64.powf(level) as f32
}

/// 화면 색(0xRRGGBB). 이름은 쓰이는 자리의 구획을 앞에 둔다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    // -- 전역 --
    pub app_bg: u32,
    pub fg: u32,
    pub fg_strong: u32,
    pub fg_muted: u32,
    /// 칸·창 경계선.
    pub border: u32,
    pub hover_bg: u32,
    /// 메뉴 항목 위에 마우스를 올렸을 때의 바탕.
    pub selected_bg: u32,
    pub current_bg: u32,
    pub accent: u32,
    pub warning: u32,
    /// 되돌릴 수 없는 메뉴 항목의 글자색.
    pub danger: u32,
    // -- 제목줄·창 버튼 --
    pub titlebar_bg: u32,
    pub titlebar_fg: u32,
    pub control_hover_bg: u32,
    pub control_close_hover_bg: u32,
    pub control_close_hover_fg: u32,
    // -- 사이드바 --
    pub sidebar_bg: u32,
    /// 사이드바 오른쪽 선. 기존 화면은 테마와 무관한 고정색이다.
    pub sidebar_border: u32,
    /// 사이드바 안의 구분선(머리줄 아래). 사이드바 바탕 위에 글자색 20%를 섞은 색이다.
    pub border_subtle: u32,
    /// 사이드바와 본문 사이 손잡이 선. 본문 바탕 위에 글자색 20%를 섞은 색이다.
    pub split_line: u32,
    /// 파일 목록 행의 글자색. 호버·현재 행은 `fg_strong`이다.
    pub file_row_fg: u32,
    // -- 텍스트 보기 --
    /// 줄 번호 글자색.
    pub gutter_fg: u32,
    /// 구문 강조 색 여섯: 키워드·문자열·주석·숫자·타입·함수. 그 밖의 글자는 `terminal_fg`와 같은 본문 색이다.
    pub syntax_keyword: u32,
    pub syntax_string: u32,
    pub syntax_comment: u32,
    pub syntax_number: u32,
    pub syntax_type: u32,
    pub syntax_function: u32,
    // -- Markdown 미리보기: 코드 블록 바탕·인용 바탕·링크 글자 --
    pub markdown_code_bg: u32,
    pub markdown_quote_bg: u32,
    pub markdown_link: u32,
    // -- Git 변경 표시: 파일·폴더 이름과 상태 글자의 색 --
    pub git_modified: u32,
    pub git_untracked: u32,
    pub git_added: u32,
    pub git_deleted: u32,
    pub git_renamed: u32,
    pub git_conflicting: u32,
    // -- 보기 줄 배지(Source control 변경 수) --
    pub badge_bg: u32,
    pub badge_fg: u32,
    // -- 탭 줄 --
    pub tab_bar_bg: u32,
    pub tab_active_bg: u32,
    pub tab_active_fg: u32,
    pub tab_inactive_bg: u32,
    pub tab_inactive_fg: u32,
    // -- 터미널 --
    pub terminal_bg: u32,
    pub terminal_fg: u32,
    pub terminal_cursor: u32,
    /// 블록 커서 위에 놓는 글자색.
    pub terminal_cursor_text: u32,
    pub terminal_selection: u32,
    pub terminal_ansi: [u32; 16],
    // -- 상태 줄 --
    pub status_bar_bg: u32,
    pub status_bar_fg: u32,
    pub status_bar_border: u32,
    // -- 대화상자 --
    pub popup_bg: u32,
    /// 대화상자 안 입력칸·목록 행의 바탕.
    pub control_bg: u32,
    /// 빠른 설정의 선택상자·± 단추 테두리. 다크는 바탕과 같아 보이지 않는다.
    pub control_border: u32,
    pub popup_border: u32,
    pub button_primary_bg: u32,
    pub button_primary_fg: u32,
    pub button_secondary_bg: u32,
    pub button_secondary_fg: u32,
    pub button_destructive_bg: u32,
    pub button_destructive_fg: u32,
    /// 대화상자 뒤를 덮는 막(0xRRGGBBAA).
    pub dialog_scrim: u32,
}

impl Colors {
    /// 다크: 기존 화면에 실제로 적용된 Mermaid Dark(창 틀은 Theia 기본 다크 값).
    pub const DARK: Self = Self {
        app_bg: 0x1e1e1e,
        fg: 0xcccccc,
        fg_strong: 0xffffff,
        fg_muted: 0xa1a1a2,
        border: 0x444444,
        hover_bg: 0x2a2d2e,
        selected_bg: 0x094771,
        current_bg: 0x37373d,
        accent: 0x007fd4,
        warning: 0xcca700,
        danger: 0xf48771,
        titlebar_bg: 0x3c3c3c,
        titlebar_fg: 0xcccccc,
        control_hover_bg: 0x2a2d2e,
        control_close_hover_bg: 0xe81123,
        control_close_hover_fg: 0xffffff,
        sidebar_bg: 0x252526,
        sidebar_border: 0x263036,
        border_subtle: 0x474747,
        split_line: 0x414141,
        file_row_fg: 0xa1a1a2,
        gutter_fg: 0x858585,
        syntax_keyword: 0x569cd6,
        syntax_string: 0xce9178,
        syntax_comment: 0x6a9955,
        syntax_number: 0xb5cea8,
        syntax_type: 0x4ec9b0,
        syntax_function: 0xdcdcaa,
        markdown_code_bg: 0x161616,
        markdown_quote_bg: 0x282828,
        markdown_link: 0x3794ff,
        git_modified: 0xe2c08d,
        git_untracked: 0x73c991,
        git_added: 0x81b88b,
        git_deleted: 0xc74e39,
        git_renamed: 0x73c991,
        git_conflicting: 0xe4676b,
        badge_bg: 0x4d4d4d,
        badge_fg: 0xffffff,
        tab_bar_bg: 0x252526,
        tab_active_bg: 0x1e1e1e,
        tab_active_fg: 0xffffff,
        tab_inactive_bg: 0x2d2d2d,
        tab_inactive_fg: 0x969696,
        terminal_bg: 0x1e1e1e,
        terminal_fg: 0xcccccc,
        terminal_cursor: 0xcccccc,
        terminal_cursor_text: 0x1e1e1e,
        terminal_selection: 0x264f78,
        terminal_ansi: [
            0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666, 0xf14c4c,
            0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xe5e5e5,
        ],
        status_bar_bg: 0x3c3c3c,
        status_bar_fg: 0xcccccc,
        status_bar_border: 0x444444,
        popup_bg: 0x252526,
        control_bg: 0x3c3c3c,
        control_border: 0x3c3c3c,
        popup_border: 0x454545,
        button_primary_bg: 0x0e639c,
        button_primary_fg: 0xffffff,
        button_secondary_bg: 0x3a3d41,
        button_secondary_fg: 0xffffff,
        button_destructive_bg: 0x6a1d1d,
        button_destructive_fg: 0xffffff,
        dialog_scrim: 0x0000004d,
    };

    /// 라이트: Mermaid Light(Theia 라이트 기본 값에 Mermaid Light 덮어쓰기를 얹은 계산값).
    pub const LIGHT: Self = Self {
        app_bg: 0xffffff,
        fg: 0x616161,
        fg_strong: 0x333333,
        fg_muted: 0x6f6f6f,
        border: 0xe7e7e7,
        hover_bg: 0xe8e8e8,
        selected_bg: 0xe8e8e8,
        current_bg: 0xe4e6f1,
        accent: 0x0090f1,
        warning: 0xbf8803,
        danger: 0xa1260d,
        titlebar_bg: 0xdddddd,
        titlebar_fg: 0x616161,
        control_hover_bg: 0xe8e8e8,
        control_close_hover_bg: 0xe81123,
        control_close_hover_fg: 0xffffff,
        sidebar_bg: 0xf3f3f3,
        sidebar_border: 0xe7e7e7,
        border_subtle: 0xdadada,
        split_line: 0xe1e1e1,
        file_row_fg: 0x6f6f6f,
        gutter_fg: 0x237893,
        syntax_keyword: 0x0000ff,
        syntax_string: 0xa31515,
        syntax_comment: 0x008000,
        syntax_number: 0x098658,
        syntax_type: 0x267f99,
        syntax_function: 0x795e26,
        markdown_code_bg: 0xf1f1f1,
        markdown_quote_bg: 0xf2f2f2,
        markdown_link: 0x006ab1,
        git_modified: 0x895503,
        git_untracked: 0x007100,
        git_added: 0x587c0c,
        git_deleted: 0xad0707,
        git_renamed: 0x007100,
        git_conflicting: 0xad0707,
        badge_bg: 0xc4c4c4,
        badge_fg: 0x333333,
        tab_bar_bg: 0xf3f3f3,
        tab_active_bg: 0xffffff,
        tab_active_fg: 0x333333,
        tab_inactive_bg: 0xececec,
        tab_inactive_fg: 0x6b6b6b,
        terminal_bg: 0xffffff,
        terminal_fg: 0x333333,
        terminal_cursor: 0x333333,
        terminal_cursor_text: 0xffffff,
        terminal_selection: 0xadd6ff,
        terminal_ansi: [
            0x000000, 0xcd3131, 0x00bc00, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555, 0x666666, 0xcd3131,
            0x14ce14, 0xb5ba00, 0x0451a5, 0xbc05bc, 0x0598bc, 0xa5a5a5,
        ],
        status_bar_bg: 0xdddddd,
        status_bar_fg: 0x616161,
        status_bar_border: 0xc8c8c8,
        popup_bg: 0xffffff,
        control_bg: 0xffffff,
        control_border: 0xcecece,
        popup_border: 0xd4d4d4,
        button_primary_bg: 0x007acc,
        button_primary_fg: 0xffffff,
        button_secondary_bg: 0x5f6a79,
        button_secondary_fg: 0xffffff,
        button_destructive_bg: 0xc72e0f,
        button_destructive_fg: 0xffffff,
        dialog_scrim: 0x0000004d,
    };
}

/// 글꼴.
pub mod font {
    /// 터미널 글꼴 순서: 앞쪽이 주 글꼴, 한글·기호는 뒤쪽 대체 글꼴이 맡는다. 플랫폼에 없는 글꼴은 건너뛴다.
    #[cfg(windows)]
    pub const TERMINAL_FAMILY: &str = "Consolas";
    #[cfg(not(windows))]
    pub const TERMINAL_FAMILY: &str = "DejaVu Sans Mono";
    #[cfg(windows)]
    pub const TERMINAL_FALLBACK_FAMILY: &str = "Malgun Gothic";
    #[cfg(not(windows))]
    pub const TERMINAL_FALLBACK_FAMILY: &str = "Noto Sans CJK KR";
    /// 창 틀 글꼴. 없는 플랫폼은 시스템 기본 글꼴로 대신한다.
    #[cfg(windows)]
    pub const UI_FAMILY: &str = "Segoe UI";
    #[cfg(not(windows))]
    pub const UI_FAMILY: &str = "Noto Sans";
    /// 창 버튼 그림 글꼴(Windows 기본 포함).
    pub const CONTROL_ICON_FAMILY: &str = "Segoe MDL2 Assets";
    /// 창 버튼 글리프: 최소화·최대화·되돌리기·닫기.
    pub const CONTROL_MINIMIZE: &str = "\u{e921}";
    pub const CONTROL_MAXIMIZE: &str = "\u{e922}";
    pub const CONTROL_RESTORE: &str = "\u{e923}";
    pub const CONTROL_CLOSE: &str = "\u{e8bb}";
}

/// 글자 크기(px).
pub mod text {
    /// 사이드바 행·메뉴 기본.
    pub const UI: f32 = 13.0;
    /// 탭 이름·보조 글자·상태 줄·섹션 머리.
    pub const SMALL: f32 = 12.0;
    /// Unassigned 머리·터미널 행 표지.
    pub const TINY: f32 = 11.0;
    pub const TITLEBAR_BRAND: f32 = 12.0;
    pub const CONTROL_ICON: f32 = 10.0;
    pub const DIALOG_TITLE: f32 = 14.0;
    pub const DIALOG_BODY: f32 = 14.0;
    pub const DIALOG_BUTTON: f32 = 13.0;
    /// 터미널 칸 높이 = 글꼴 자체 줄 높이 × 이 배수.
    pub const TERMINAL_LINE_HEIGHT: f32 = 1.1;
    /// 대화상자 본문 줄 높이 배수.
    pub const DIALOG_LINE_HEIGHT: f32 = 1.5;
}

/// 제목줄과 창 버튼.
pub mod titlebar {
    /// 아래 선 1px 포함.
    pub const HEIGHT: f32 = 34.0;
    pub const BRAND_PADDING_X: f32 = 16.0;
    pub const CONTROL_WIDTH: f32 = 48.0;
    pub const BORDER_WIDTH: f32 = 1.0;
}

/// 사이드바와 Work 목록.
pub mod sidebar {
    pub const WIDTH: f32 = 272.0;
    /// 사이드바와 본문 사이 선.
    pub const SPLIT_WIDTH: f32 = 1.0;
    /// WORK 머리줄(아래 선 1px 포함).
    pub const HEADER_HEIGHT: f32 = 32.0;
    pub const HEADER_PADDING_LEFT: f32 = 8.0;
    pub const HEADER_PADDING_RIGHT: f32 = 10.0;
    pub const HEADER_GAP: f32 = 4.0;
    pub const HEADER_LETTER_SPACING: f32 = 0.3;
    /// 머리줄의 ＋ 같은 24x24 단추.
    pub const ACTION_SIZE: f32 = 24.0;
    pub const ACTION_RADIUS: f32 = 4.0;
    pub const LIST_PADDING_Y: f32 = 4.0;
    pub const ROW_HEIGHT: f32 = 28.0;
    pub const ROW_PADDING_RIGHT: f32 = 8.0;
    /// 터미널 행 왼쪽 여백 = 22 + 들여쓰기 12.
    pub const TERMINAL_ROW_PADDING_LEFT: f32 = 34.0;
    pub const UNASSIGNED_PADDING_TOP: f32 = 10.0;
    pub const UNASSIGNED_PADDING_X: f32 = 14.0;
    pub const UNASSIGNED_PADDING_BOTTOM: f32 = 4.0;
    /// 행 안 표지(`>_`)와 이름 사이.
    pub const ROW_GAP: f32 = 5.0;
    /// 닫힘 X 단추.
    pub const CLOSE_SIZE: f32 = 24.0;
    /// 머리줄의 ＋와 환경 고르기(⌄) 사이.
    pub const ACTIONS_GAP: f32 = 2.0;
    /// 머리줄 단추 묶음의 오른쪽 여백.
    pub const ACTIONS_PADDING_RIGHT: f32 = 8.0;
    /// 파일 목록이 함께 보일 때 Work 목록이 차지할 수 있는 최대 높이(사이드바 높이 대비 비율).
    pub const WORK_MAX_HEIGHT_WITH_FILES: f32 = 0.5;
}

/// 작업 폴더 행. 앞의 두 그림(펼침 표지·폴더)은 16px 상자이고 이름 열은 터미널 행과 다르다.
pub mod folder_row {
    /// 행 왼쪽 가장자리에서 펼침 표지까지(기존 앱은 8px 안쪽 여백에 2px 투명 띠가 더해진 값).
    pub const PADDING_LEFT: f32 = 10.0;
    /// 펼침 표지·폴더 그림의 상자 크기.
    pub const ICON_BOX: f32 = 16.0;
    /// 그림 상자와 이름 사이.
    pub const GAP: f32 = 5.0;
    /// 폴더 그림의 몸통 크기.
    pub const FOLDER_WIDTH: f32 = 14.0;
    pub const FOLDER_HEIGHT: f32 = 10.0;
    pub const FOLDER_TAB_WIDTH: f32 = 6.0;
    pub const FOLDER_TAB_HEIGHT: f32 = 3.0;
    pub const FOLDER_RADIUS: f32 = 2.0;
    /// 호버 때 보이는 ＋·⋯ 단추.
    pub const ACTION_SIZE: f32 = 20.0;
    pub const ACTION_MARGIN_LEFT: f32 = 4.0;
    /// 현재 폴더 이름의 굵기(CSS 600).
    pub const CURRENT_WEIGHT: f32 = 600.0;
}

/// 파일 목록 머리줄(`<폴더> FILES`): 접기 표지·제목과 오른쪽의 새 파일·새 폴더·새로 고침·검색 단추.
pub mod files_header {
    /// 아래 선 1px 포함.
    pub const HEIGHT: f32 = 30.0;
    pub const PADDING_LEFT: f32 = 8.0;
    pub const PADDING_RIGHT: f32 = 6.0;
    pub const GAP: f32 = 4.0;
    /// 제목 글자 크기는 `text::TINY`, 굵기는 CSS 700이다.
    pub const LETTER_SPACING: f32 = 0.3;
    /// 오른쪽 단추 하나의 크기.
    pub const ACTION_SIZE: f32 = 24.0;
    pub const ACTION_RADIUS: f32 = 4.0;
    /// 비활성 단추의 불투명도.
    pub const DISABLED_OPACITY: f32 = 0.4;
    /// 단추 안 파일·폴더 그림 옆의 `+` 글자 크기.
    pub const PLUS_SIZE: f32 = 9.0;
}

/// 파일 목록 행. 한 줄 높이가 모두 같아서 가상 목록으로 그린다.
pub mod file_row {
    pub const HEIGHT: f32 = 26.0;
    pub const GAP: f32 = 5.0;
    /// 깊이 0 행의 왼쪽 여백.
    pub const PADDING_LEFT: f32 = 8.0;
    pub const PADDING_RIGHT: f32 = 12.0;
    /// 폴더 한 단계 안쪽의 들여쓰기.
    pub const INDENT: f32 = 20.0;
    /// 파일 행의 파일 그림 앞 빈 칸(폴더 행의 펼침 표지 자리).
    pub const FILE_ICON_MARGIN_LEFT: f32 = 18.0;
    /// 파일 그림(쪽 모양)의 크기.
    pub const FILE_WIDTH: f32 = 10.0;
    pub const FILE_HEIGHT: f32 = 13.0;
    pub const FILE_RADIUS: f32 = 2.0;
    /// Git 변경 표시 글자 칸.
    pub const MARK_WIDTH: f32 = 14.0;
    pub const MARK_SIZE: f32 = 11.0;
    /// 안내 글(`Empty folder.`)의 왼쪽 여백 = 파일 행 이름 열과 같은 깊이 + 안쪽 여백.
    pub const NOTICE_PADDING_LEFT: f32 = 22.0;
}

/// 파일 탭(탭 줄 마지막 열): 터미널 탭과 같은 폭·높이·닫기에 파일 그림이 앞에 붙는다.
pub mod file_tab {
    /// 탭 왼쪽에서 파일 그림까지.
    pub const PADDING_LEFT: f32 = 10.0;
    /// 파일 그림(쪽 모양)과 이름 사이.
    pub const ICON_GAP: f32 = 6.0;
    pub const ICON_WIDTH: f32 = 10.0;
    pub const ICON_HEIGHT: f32 = 13.0;
    pub const ICON_RADIUS: f32 = 2.0;
}

/// 텍스트 보기: 읽기 전용 편집기(줄 번호 거터 + 구문 강조). 글꼴과 크기는 터미널 글꼴 설정을 따른다.
pub mod file_view {
    /// 본문 위·아래 여백.
    pub const PADDING_Y: f32 = 6.0;
    /// 줄 높이 = 글꼴 크기 × 이 배수.
    pub const LINE_HEIGHT: f32 = 1.5;
}

/// 탭 줄 오른쪽의 `Open in external editor` 단추(↗ 글리프). 파일 탭이 활성일 때만 보인다.
pub mod external_editor_button {
    pub const WIDTH: f32 = 24.0;
    pub const HEIGHT: f32 = 22.0;
    pub const RADIUS: f32 = 5.0;
}

/// 볼 수 없는 파일을 알리는 안내 탭: 이름·크기·문구·동작 단추가 가운데 세로로 놓인다.
pub mod notice_tab {
    pub const MAX_WIDTH: f32 = 360.0;
    pub const PADDING: f32 = 16.0;
    pub const GAP: f32 = 6.0;
    /// 이름 글자 크기(굵기 CSS 600).
    pub const NAME_SIZE: f32 = 14.0;
    /// 크기·문구 글자 크기.
    pub const BODY_SIZE: f32 = 12.0;
    /// 문구 위 여백.
    pub const MESSAGE_MARGIN_TOP: f32 = 6.0;
    /// 단추 줄 위 여백과 단추 사이.
    pub const ACTIONS_MARGIN_TOP: f32 = 10.0;
    pub const ACTIONS_GAP: f32 = 8.0;
    pub const BUTTON_HEIGHT: f32 = 26.0;
    pub const BUTTON_PADDING_X: f32 = 12.0;
    pub const BUTTON_RADIUS: f32 = 5.0;
    pub const BUTTON_TEXT_SIZE: f32 = 12.0;
}

/// Markdown 미리보기 본문: 읽을 수 있는 폭, 글자 크기·줄 높이, 제목 배수, 문단 간격, 코드 블록.
pub mod markdown {
    /// 본문 최대 폭. 칸이 이보다 넓어도 본문은 이 폭으로 가운데에 놓인다.
    pub const CONTENT_MAX_WIDTH: f32 = 760.0;
    pub const PADDING_X: f32 = 26.0;
    pub const PADDING_Y: f32 = 16.0;
    pub const TEXT_SIZE: f32 = 14.0;
    /// 줄 높이 = 글자 크기 × 이 배수.
    pub const LINE_HEIGHT: f32 = 1.6;
    /// 제목 1~6단계의 글자 크기 배수.
    pub const HEADING_SCALE: [f32; 6] = [2.0, 1.5, 1.25, 1.0, 0.875, 0.85];
    /// 문단·목록·표 사이 간격(rem).
    pub const PARAGRAPH_GAP: f32 = 1.0;
    pub const CODE_BLOCK_PADDING: f32 = 16.0;
    pub const CODE_BLOCK_RADIUS: f32 = 3.0;
    /// 인용 블록 상자의 안쪽 여백(바탕색 + 전체 테두리로 그린다).
    pub const QUOTE_PADDING: f32 = 10.0;
    /// 나란히 보기에서 원문 쪽이 차지하는 비율.
    pub const BOTH_SOURCE_RATIO: f32 = 0.5;
}

/// 탭 줄 오른쪽의 Markdown 보기 전환 단추 묶음(`File`·`Preview`·`Both`).
pub mod markdown_view_actions {
    /// 탭 줄 오른쪽 구역에서 이 묶음 몫으로 비워 두는 폭(세 단추 글자 폭의 상한 + 바깥 여백).
    pub const RESERVED_WIDTH: f32 = 160.0;
    pub const PADDING: f32 = 2.0;
    pub const GAP: f32 = 2.0;
    pub const MARGIN_RIGHT: f32 = 8.0;
    pub const RADIUS: f32 = 6.0;
    pub const BUTTON_HEIGHT: f32 = 20.0;
    pub const BUTTON_PADDING_X: f32 = 7.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
    pub const BUTTON_TEXT_SIZE: f32 = 11.0;
}

/// 이미지 탭: 칸 둘레 여백과 아래 상태 줄.
pub mod image_view {
    pub const PADDING: f32 = 16.0;
    pub const STATUS_HEIGHT: f32 = 22.0;
    pub const STATUS_PADDING_X: f32 = 12.0;
    pub const STATUS_TEXT_SIZE: f32 = 11.0;
}

/// PDF 탭: 쪽 종이 색, 쪽 테두리, 아래 상태 줄, 탭 줄 오른쪽 단추.
pub mod pdf_view {
    /// 종이 색(PDF 쪽은 테마와 무관하게 흰 바탕에 그려진다).
    pub const PAPER: u32 = 0xffffff;
    pub const PAGE_BORDER: f32 = 1.0;
    pub const STATUS_HEIGHT: f32 = 22.0;
    pub const STATUS_PADDING_X: f32 = 12.0;
    pub const STATUS_TEXT_SIZE: f32 = 11.0;
    pub const STATUS_GAP: f32 = 12.0;
    /// 탭 줄 오른쪽의 `−`·`+` 단추 한 칸.
    pub const ZOOM_BUTTON_WIDTH: f32 = 24.0;
    pub const ZOOM_BUTTON_HEIGHT: f32 = 22.0;
    pub const ZOOM_BUTTON_RADIUS: f32 = 5.0;
    /// 탭 줄 오른쪽 구역에서 `−`·`+`·`Open with default app` 몫으로 비워 두는 폭.
    pub const ACTIONS_RESERVED_WIDTH: f32 = 220.0;
    /// `Open with default app` 글자 단추.
    pub const OPEN_BUTTON_HEIGHT: f32 = 22.0;
    pub const OPEN_BUTTON_PADDING_X: f32 = 8.0;
    pub const OPEN_BUTTON_RADIUS: f32 = 5.0;
    pub const OPEN_BUTTON_TEXT_SIZE: f32 = 11.0;
}

/// 가려진 터미널을 펼치는 `Show N more` 줄.
pub mod more_row {
    pub const GAP: f32 = 6.0;
}

/// 작업 폴더가 없을 때 Work 목록에 보이는 첫 실행 안내.
pub mod work_empty {
    pub const TITLE_MARGIN_TOP: f32 = 12.0;
    pub const TITLE_MARGIN_X: f32 = 14.0;
    pub const TITLE_MARGIN_BOTTOM: f32 = 2.0;
    pub const TEXT_PADDING_Y: f32 = 6.0;
    pub const TEXT_PADDING_X: f32 = 14.0;
    /// 단계 목록의 왼쪽 여백 = 20 + 들여쓰기 12.
    pub const STEPS_PADDING_LEFT: f32 = 32.0;
    pub const STEPS_PADDING_RIGHT: f32 = 14.0;
    pub const STEPS_PADDING_BOTTOM: f32 = 6.0;
    pub const STEP_PADDING_Y: f32 = 4.0;
    /// 단계 번호가 차지하는 폭.
    pub const STEP_NUMBER_WIDTH: f32 = 18.0;
    pub const STEP_LINE_HEIGHT: f32 = 1.5;
    pub const NOTE_LINE_HEIGHT: f32 = 1.6;
    pub const COMMAND_MARGIN_TOP: f32 = 6.0;
}

/// 보기 단추 위의 개수 배지.
pub mod view_badge {
    pub const MIN_WIDTH: f32 = 14.0;
    pub const HEIGHT: f32 = 14.0;
    pub const PADDING_X: f32 = 3.0;
    pub const TOP: f32 = 1.0;
    pub const LEFT: f32 = 16.0;
    /// 글자 크기(굵기 CSS 700).
    pub const TEXT_SIZE: f32 = 10.0;
    /// 이 수를 넘으면 `99+`로 줄여 보인다.
    pub const MAX_COUNT: usize = 99;
}

/// 탭 줄 오른쪽의 브랜치 단추(이름 표시만).
pub mod branch_button {
    pub const HEIGHT: f32 = 24.0;
    pub const PADDING_X: f32 = 8.0;
    pub const RADIUS: f32 = 5.0;
    pub const TEXT_SIZE: f32 = 11.0;
    pub const GAP: f32 = 4.0;
    /// 이름이 길어도 이 폭 안에서 말줄임한다.
    pub const MAX_WIDTH: f32 = 140.0;
    /// 가지 모양 그림의 크기.
    pub const ICON_SIZE: f32 = 13.0;
}

/// 추적하지 않는 새 파일의 읽기 전용 보기 위에 붙는 `Untracked` 표지 줄.
pub mod untracked_bar {
    pub const HEIGHT: f32 = 22.0;
    pub const PADDING_X: f32 = 12.0;
}

/// Source control 보기: 저장소 줄·커밋 입력칸·`Commit` 단추·구획 머리·변경 행.
pub mod scm {
    /// 저장소 이름·브랜치 줄.
    pub const REPO_HEIGHT: f32 = 22.0;
    pub const PADDING_X: f32 = 8.0;
    /// 커밋 입력칸 둘레 여백: 왼쪽은 위젯 안쪽 18, 나머지 6.
    pub const COMMIT_PADDING_LEFT: f32 = 18.0;
    pub const COMMIT_PADDING: f32 = 6.0;
    /// 입력칸은 두 줄 높이가 최소다(줄 높이 22).
    pub const INPUT_LINES: f32 = 2.0;
    pub const INPUT_LINE_HEIGHT: f32 = 22.0;
    pub const INPUT_PADDING_X: f32 = 6.0;
    pub const BUTTON_HEIGHT: f32 = 28.0;
    pub const BUTTON_GAP: f32 = 4.0;
    pub const BUTTON_DISABLED_OPACITY: f32 = 0.5;
    /// 커밋 실패·안내 한 줄.
    pub const NOTE_PADDING_Y: f32 = 4.0;
    /// 구획 머리.
    pub const SECTION_HEIGHT: f32 = 22.0;
    pub const SECTION_PADDING_X: f32 = 6.0;
    pub const COUNT_MIN_WIDTH: f32 = 18.0;
    pub const COUNT_HEIGHT: f32 = 16.0;
    pub const COUNT_PADDING_X: f32 = 4.0;
    /// 접기 표지(▸·▾)가 차지하는 폭.
    pub const CHEVRON_WIDTH: f32 = 12.0;
    /// 변경 행.
    pub const ROW_HEIGHT: f32 = 22.0;
    pub const ROW_PADDING_X: f32 = 3.0;
    pub const ROW_GAP: f32 = 4.0;
    /// 폴더 경로(흐림) 앞 간격과 불투명도.
    pub const PATH_MARGIN_LEFT: f32 = 6.0;
    pub const PATH_OPACITY: f32 = 0.7;
    /// 상태 글자 칸과 오른쪽 여백.
    pub const STATUS_WIDTH: f32 = 14.0;
    pub const STATUS_MARGIN_RIGHT: f32 = 6.0;
    /// 행에 올렸을 때 보이는 인라인 단추.
    pub const ACTION_SIZE: f32 = 18.0;
    pub const ACTION_GAP: f32 = 2.0;
    pub const ACTION_RADIUS: f32 = 3.0;
}

/// 사이드바 맨 위의 보기 줄: 왼쪽에 보기 단추, 오른쪽 끝에 빠른 설정 톱니.
pub mod view_bar {
    /// 아래 선 1px 포함.
    pub const HEIGHT: f32 = 36.0;
    pub const PADDING_X: f32 = 8.0;
    pub const GAP: f32 = 4.0;
    pub const BUTTON_WIDTH: f32 = 30.0;
    pub const BUTTON_HEIGHT: f32 = 28.0;
    pub const BUTTON_RADIUS: f32 = 6.0;
    /// 단추 안 그림의 크기.
    pub const ICON_SIZE: f32 = 16.0;
    /// 목록 모양 그림: 줄 굵기와 줄 사이, 들여쓴 줄의 들여쓰기 폭.
    pub const LIST_LINE_HEIGHT: f32 = 2.0;
    pub const LIST_LINE_GAP: f32 = 3.0;
    pub const LIST_INDENT: f32 = 4.0;
    /// 가지 모양 그림: 동그라미 크기와 왼쪽·오른쪽 동그라미의 가로 자리.
    pub const BRANCH_CIRCLE: f32 = 5.0;
    pub const BRANCH_LEFT: f32 = 2.0;
    pub const BRANCH_RIGHT: f32 = 9.0;
}

/// 빠른 설정 패널: 보기 줄 톱니 아래에 뜨는 설정 모음.
pub mod quick_settings {
    pub const WIDTH: f32 = 344.0;
    pub const PADDING: f32 = 16.0;
    pub const RADIUS: f32 = 8.0;
    pub const FONT_SIZE: f32 = 14.0;
    /// 톱니를 단 보기 줄 아래에서 패널까지.
    pub const ANCHOR_GAP: f32 = 6.0;
    /// 창 가장자리와 패널 사이의 최소 간격.
    pub const SCREEN_MARGIN: f32 = 8.0;
    /// 패널 높이가 창 높이에서 비우는 값. 넘치면 패널 안에서 스크롤한다.
    pub const HEIGHT_MARGIN: f32 = 120.0;
    pub const TITLE_MARGIN_BOTTOM: f32 = 10.0;
    pub const ROW_MIN_HEIGHT: f32 = 38.0;
    pub const ROW_GAP: f32 = 10.0;
    /// 줄 위아래의 안쪽 여백.
    pub const ROW_PADDING_Y: f32 = 2.0;
    pub const LABEL_SIZE: f32 = 13.0;
    /// 줄 이름 아래의 적용 대상 안내 글.
    pub const HINT_SIZE: f32 = 11.0;
    pub const STEP_SIZE: f32 = 28.0;
    pub const STEP_RADIUS: f32 = 5.0;
    pub const VALUE_MIN_WIDTH: f32 = 40.0;
    pub const VALUE_SIZE: f32 = 13.0;
    pub const SELECT_WIDTH: f32 = 158.0;
    pub const SELECT_HEIGHT: f32 = 30.0;
    pub const SELECT_RADIUS: f32 = 5.0;
    pub const SELECT_PADDING_X: f32 = 6.0;
    /// 선택상자 안의 값 글과 ▾ 사이.
    pub const SELECT_CONTENT_GAP: f32 = 4.0;
    /// 펼친 선택지 목록과 아래 줄 사이.
    pub const OPTION_LIST_MARGIN_BOTTOM: f32 = 4.0;
    /// 선택지 한 줄 안의 ✓ 칸과 이름 사이.
    pub const OPTION_GAP: f32 = 6.0;
    /// 선택지 한 줄 맨 앞 ✓ 칸의 폭.
    pub const OPTION_CHECK_WIDTH: f32 = 12.0;
    /// 소리 체크박스 둘레의 눌림 영역 여백.
    pub const SOUND_HIT_PADDING: f32 = 6.0;
    /// 펼친 선택지 한 줄의 높이.
    pub const OPTION_HEIGHT: f32 = 30.0;
    /// 마지막 줄(`Quit and Stop All Terminals`) 위의 간격과 안쪽 여백.
    pub const LAST_ROW_MARGIN_TOP: f32 = 12.0;
    pub const LAST_ROW_PADDING_TOP: f32 = 12.0;
    pub const LAST_ROW_PADDING_X: f32 = 8.0;
    /// 마지막 줄 단추 안쪽 위 여백.
    pub const LAST_ROW_BUTTON_PADDING_TOP: f32 = 4.0;
    pub const LAST_ROW_PADDING_BOTTOM: f32 = 8.0;
    pub const LAST_ROW_RADIUS: f32 = 5.0;
}

/// 우클릭·환경 고르기 메뉴 판.
pub mod menu {
    pub const MIN_WIDTH: f32 = 236.0;
    pub const PADDING: f32 = 6.0;
    pub const RADIUS: f32 = 8.0;
    pub const ITEM_PADDING_Y: f32 = 9.0;
    pub const ITEM_PADDING_X: f32 = 12.0;
    pub const ITEM_GAP: f32 = 10.0;
    pub const ITEM_RADIUS: f32 = 4.0;
    /// 항목 오른쪽 보조 글자(`Windows`·`default`).
    pub const ITEM_META_SIZE: f32 = 12.0;
    pub const FONT_SIZE: f32 = 14.0;
    /// 메뉴 아래 안내 글(`Closes N terminals · folder is kept`)의 위치.
    pub const NOTE_PADDING_TOP: f32 = 2.0;
    pub const NOTE_PADDING_BOTTOM: f32 = 8.0;
    pub const NOTE_PADDING_X: f32 = 12.0;
    pub const NOTE_LINE_HEIGHT: f32 = 1.45;
    /// 머리줄 아래에 열 때 단추와의 간격.
    pub const ANCHOR_GAP: f32 = 4.0;
    /// 일반/계정 터미널 고르기 메뉴(＋·계정 고르기)의 최소 폭.
    pub const ACCOUNT_WIDTH: f32 = 284.0;
    /// 구분선으로 나뉜 항목(`Manage accounts…`) 위의 간격.
    pub const SEPARATOR_MARGIN_TOP: f32 = 6.0;
}

/// 탭 줄 오른쪽의 계정 고르기: 계정 그림 + `Claude · Main` + 아래 화살표.
pub mod account_picker {
    pub const HEIGHT: f32 = 24.0;
    pub const PADDING_X: f32 = 8.0;
    pub const GAP: f32 = 6.0;
    pub const MAX_WIDTH: f32 = 210.0;
    pub const RADIUS: f32 = 4.0;
    /// 계정 그림(머리 원 + 어깨)의 크기.
    pub const ICON_SIZE: f32 = 14.0;
    pub const HEAD_SIZE: f32 = 6.0;
    pub const BODY_WIDTH: f32 = 10.0;
    pub const BODY_HEIGHT: f32 = 5.0;
    /// 머리와 어깨 사이.
    pub const ICON_GAP: f32 = 1.0;
    /// 이름이 없을 때(현재 터미널에 계정이 없을 때) 보이는 글자.
    pub const EMPTY_LABEL: &str = "Accounts";
}

/// Accounts 창: 계정 목록 행과 추가 양식.
pub mod accounts_dialog {
    pub const WIDTH: f32 = 640.0;
    /// 대화상자 본문 안쪽 여백(`dialog::CONTENT_PADDING` 12)에 더해 기존 창의 좌우 20·위 16이 되게 하는 값.
    pub const PADDING_X: f32 = 8.0;
    pub const PADDING_TOP: f32 = 4.0;
    pub const SECTION_GAP: f32 = 16.0;
    pub const LIST_GAP: f32 = 8.0;
    pub const ROW_PADDING: f32 = 12.0;
    pub const ROW_RADIUS: f32 = 6.0;
    pub const ROW_GAP: f32 = 12.0;
    pub const SUMMARY_GAP: f32 = 4.0;
    pub const DETAIL_SIZE: f32 = 12.0;
    pub const BUTTON_HEIGHT: f32 = 30.0;
    pub const BUTTON_PADDING_X: f32 = 12.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
    pub const BUTTON_GAP: f32 = 6.0;
    pub const FIELD_GAP: f32 = 12.0;
    pub const FIELD_LABEL_GAP: f32 = 6.0;
    pub const FIELD_LABEL_SIZE: f32 = 12.0;
    pub const FIELD_HEIGHT: f32 = 32.0;
    pub const FIELD_RADIUS: f32 = 4.0;
    pub const FIELD_PADDING_X: f32 = 8.0;
    /// 양식 칸(도구·이름 등) 하나의 최소 폭.
    pub const FIELD_MIN_WIDTH: f32 = 120.0;
    /// 계정 행에서 이름·상세 글이 줄어들 수 있는 최소 폭.
    pub const SUMMARY_MIN_WIDTH: f32 = 140.0;
    pub const FORM_HEADING_SIZE: f32 = 13.0;
    pub const HINT_SIZE: f32 = 12.0;
    pub const ERROR_PADDING_X: f32 = 12.0;
    pub const ERROR_PADDING_Y: f32 = 10.0;
    /// 화면 높이에서 창이 비우는 여백. 본문이 넘치면 이 안에서 스크롤한다.
    pub const SCREEN_MARGIN: f32 = 48.0;
}

/// 이름 바꾸기 창의 입력칸.
pub mod rename {
    pub const INPUT_HEIGHT: f32 = 32.0;
    pub const INPUT_RADIUS: f32 = 4.0;
}

/// 터미널 행 오른쪽의 에이전트 정보: 프로그램 요약, 상태 점·글자, 이름 뒤 미확인 점.
pub mod agent_row {
    /// 프로그램·계정 요약의 최대 폭. 넘치면 말줄임이다.
    pub const META_MAX_WIDTH: f32 = 70.0;
    /// 요약과 이름(또는 앞 칸) 사이.
    pub const META_MARGIN_LEFT: f32 = 6.0;
    /// 상태 칸(점 + 글자)의 고정 폭. 글자가 바뀌어도 오른쪽 열이 흔들리지 않는다.
    pub const STATUS_WIDTH: f32 = 65.0;
    pub const STATUS_MARGIN_LEFT: f32 = 8.0;
    /// 상태 점의 지름.
    pub const STATUS_DOT_SIZE: f32 = 5.0;
    /// 상태 점과 글자 사이.
    pub const STATUS_GAP: f32 = 5.0;
    /// 읽지 않은 완료 알림 점의 지름. 행 이름 뒤와 탭 이름 뒤에 같은 점이 붙는다.
    pub const UNSEEN_DOT_SIZE: f32 = 6.0;
    pub const UNSEEN_DOT_MARGIN_LEFT: f32 = 6.0;
}

/// 탭 줄.
pub mod tab {
    /// 아래 선 1px 포함.
    pub const BAR_HEIGHT: f32 = 30.0;
    pub const BAR_PADDING_LEFT: f32 = 8.0;
    pub const HEIGHT: f32 = 26.0;
    pub const WIDTH: f32 = 150.0;
    pub const MIN_WIDTH: f32 = 72.0;
    pub const GAP: f32 = 2.0;
    pub const RADIUS_TOP: f32 = 5.0;
    /// 탭 왼쪽에서 글자 시작까지.
    pub const TEXT_PADDING_LEFT: f32 = 16.0;
    pub const PADDING_RIGHT: f32 = 6.0;
    pub const CLOSE_SIZE: f32 = 18.0;
    pub const CLOSE_RADIUS: f32 = 4.0;
    /// 닫기 글리프 크기.
    pub const CLOSE_GLYPH: f32 = 10.0;
    pub const ADD_WIDTH: f32 = 24.0;
    pub const ADD_HEIGHT: f32 = 22.0;
    pub const ADD_RADIUS: f32 = 5.0;
    /// ＋ 단추와 탭 줄 아래 선 사이.
    pub const ADD_MARGIN_BOTTOM: f32 = 2.0;
}

/// 탭 줄 왼쪽의 사이드바 접기 단추. 탭 시작 x = `tab::BAR_PADDING_LEFT` + 좌우 여백 + 폭 = 44.
pub mod sidebar_toggle {
    pub const WIDTH: f32 = 28.0;
    pub const HEIGHT: f32 = 24.0;
    pub const MARGIN_X: f32 = 4.0;
    pub const RADIUS: f32 = 4.0;
    /// 단추 안 그림(사이드바가 왼쪽에 있는 화면 모양)의 크기.
    pub const ICON_WIDTH: f32 = 14.0;
    pub const ICON_HEIGHT: f32 = 12.0;
    pub const ICON_RADIUS: f32 = 2.0;
    /// 그림 왼쪽 구획의 폭(사이드바를 뜻하는 부분).
    pub const ICON_SIDEBAR_WIDTH: f32 = 5.0;
}

/// 탭이 줄을 넘칠 때: 가장자리 흐림, 넘김 단추, 가려진 쪽에 알릴 탭이 있을 때의 점.
pub mod tab_overflow {
    pub const FADE_WIDTH: f32 = 32.0;
    pub const BUTTON_SIZE: f32 = 22.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
    pub const MARK_SIZE: f32 = 6.0;
    /// 점의 단추 모서리에서의 거리.
    pub const MARK_OFFSET: f32 = 3.0;
    /// 넘김 단추를 한 번 누를 때 움직이는 거리.
    pub const SCROLL_STEP: f32 = 150.0;
}

/// 탭 줄 오른쪽의 칸 나누기 단추 둘(옆 칸·아래 칸).
pub mod split_buttons {
    pub const WIDTH: f32 = 24.0;
    pub const HEIGHT: f32 = 22.0;
    pub const RADIUS: f32 = 5.0;
    pub const GAP: f32 = 2.0;
    pub const PADDING_RIGHT: f32 = 8.0;
    /// 단추 안 그림(칸 나눈 모양)의 크기.
    pub const ICON_SIZE: f32 = 12.0;
    pub const ICON_RADIUS: f32 = 2.0;
    /// 그림 안 칸 나눔선의 굵기. 1px 선은 배율을 곱하지 않고 장치 픽셀에 맞춘다.
    pub const DIVIDER_WIDTH: f32 = 1.0;
}

/// 본문의 칸 사이 경계선.
pub mod split {
    /// 칸 사이에 보이는 선.
    pub const LINE_WIDTH: f32 = 1.0;
    /// 끌어서 크기를 바꾸는 잡는 영역의 폭(선 양옆 4px).
    pub const HIT_WIDTH: f32 = 8.0;
    /// 호버·끌기 중에 선 가운데에 칠하는 강조 폭.
    pub const HOVER_WIDTH: f32 = 2.0;
    /// 끌어서 줄일 수 있는 칸의 최소 크기.
    pub const MIN_PANE_SIZE: f32 = 120.0;
}

/// 터미널 영역.
pub mod terminal {
    pub const PADDING_X: f32 = 20.0;
    pub const PADDING_Y: f32 = 14.0;
    /// 밑줄·취소선·비블록 커서 두께.
    pub const LINE_THICKNESS: f32 = 1.0;
    /// 막대 커서 폭.
    pub const BEAM_WIDTH: f32 = 1.0;
    /// 안내 띠(재생 뒤 안내 등)의 여백.
    pub const BANNER_PADDING_X: f32 = 8.0;
    pub const BANNER_MARGIN: f32 = 4.0;
    pub const BANNER_PADDING_Y: f32 = 2.0;
}

/// 상태 줄.
pub mod status_bar {
    /// 위 선 1px 포함.
    pub const HEIGHT: f32 = 27.0;
    pub const PADDING_X: f32 = 13.0;
    pub const BORDER_WIDTH: f32 = 1.0;
}

/// 대화상자(종료 묻기·닫기 확인·안내).
pub mod dialog {
    pub const WIDTH: f32 = 580.0;
    pub const RADIUS: f32 = 8.0;
    pub const BORDER_WIDTH: f32 = 1.0;
    pub const TITLE_HEIGHT: f32 = 36.0;
    pub const TITLE_PADDING_LEFT: f32 = 12.0;
    pub const CONTENT_PADDING: f32 = 12.0;
    pub const CONTENT_GAP: f32 = 8.0;
    pub const BUTTON_MIN_WIDTH: f32 = 65.0;
    pub const BUTTON_PADDING_X: f32 = 14.0;
    pub const BUTTON_PADDING_Y: f32 = 7.0;
    pub const BUTTON_RADIUS: f32 = 5.0;
    pub const BUTTON_GAP: f32 = 12.0;
    pub const CHECKBOX_SIZE: f32 = 15.0;
    pub const CHECKBOX_RADIUS: f32 = 3.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoom_factor_follows_the_one_point_two_power_rule() {
        assert_eq!(zoom_factor(0.0), 1.0);
        assert!((zoom_factor(3.0) - 1.728).abs() < 1e-4);
        assert!((zoom_factor(-1.0) - 0.8333).abs() < 1e-3);
    }

    #[test]
    fn test_agent_row_sizes_match_the_measured_old_app() {
        assert_eq!((agent_row::STATUS_WIDTH, agent_row::STATUS_DOT_SIZE, agent_row::STATUS_GAP), (65.0, 5.0, 5.0));
        assert_eq!((agent_row::UNSEEN_DOT_SIZE, agent_row::META_MAX_WIDTH), (6.0, 70.0));
    }

    #[test]
    fn test_mode_comes_from_the_color_theme_name() {
        assert_eq!(Mode::from_theme_name(Some("Mermaid Light")), Mode::Light);
        assert_eq!(Mode::from_theme_name(Some("Mermaid Dark")), Mode::Dark);
        assert_eq!(Mode::from_theme_name(None), Mode::Dark);
    }

    #[test]
    fn test_mode_ignores_theme_names_that_only_contain_light() {
        // 이름 목록과 정확히 같을 때만 라이트다: 이 앱에 없는 테마는 이름에 light가 들어 있어도 기본(다크)이다.
        for name in ["Solarized Light", "Light+ (default light)", "Mermaid Lighter", "mermaid light", ""] {
            assert_eq!(Mode::from_theme_name(Some(name)), Mode::Dark, "{name:?}");
        }
    }

    #[test]
    fn test_dark_matches_the_measured_old_app_colors() {
        let dark = Mode::Dark.colors();
        // 기존 앱 캡처에서 측정한 값.
        assert_eq!((dark.app_bg, dark.fg, dark.fg_muted), (0x1e1e1e, 0xcccccc, 0xa1a1a2));
        assert_eq!((dark.titlebar_bg, dark.sidebar_bg, dark.tab_bar_bg), (0x3c3c3c, 0x252526, 0x252526));
        assert_eq!((dark.current_bg, dark.accent, dark.border), (0x37373d, 0x007fd4, 0x444444));
    }

    #[test]
    fn test_c_d53_git_colors_are_the_six_decoration_colors_of_each_mode() {
        let dark = Mode::Dark.colors();
        assert_eq!(
            [
                dark.git_modified,
                dark.git_untracked,
                dark.git_added,
                dark.git_deleted,
                dark.git_renamed,
                dark.git_conflicting
            ],
            [0xe2c08d, 0x73c991, 0x81b88b, 0xc74e39, 0x73c991, 0xe4676b]
        );
        let light = Mode::Light.colors();
        assert_eq!(
            [
                light.git_modified,
                light.git_untracked,
                light.git_added,
                light.git_deleted,
                light.git_renamed,
                light.git_conflicting
            ],
            [0x895503, 0x007100, 0x587c0c, 0xad0707, 0x007100, 0xad0707]
        );
    }

    #[test]
    fn test_light_and_dark_differ_where_the_surface_changes() {
        let (dark, light) = (Mode::Dark.colors(), Mode::Light.colors());
        assert_ne!(dark.app_bg, light.app_bg);
        assert_ne!(dark.terminal_fg, light.terminal_fg);
        assert_ne!(dark.terminal_ansi, light.terminal_ansi);
    }

    #[test]
    fn test_c_d41_every_theme_name_selects_its_own_mode() {
        assert_eq!(Mode::ALL, [Mode::Dark, Mode::Light]);
        for mode in Mode::ALL {
            assert_eq!(Mode::from_theme_name(Some(mode.theme_name())), mode);
        }
        assert_eq!((Mode::Dark.theme_name(), Mode::Light.theme_name()), ("Mermaid Dark", "Mermaid Light"));
    }

    #[test]
    fn test_file_list_sizes_match_the_design_tokens() {
        assert_eq!((files_header::HEIGHT, files_header::ACTION_SIZE), (30.0, 24.0));
        assert_eq!((file_row::HEIGHT, file_row::INDENT, file_row::FILE_ICON_MARGIN_LEFT), (26.0, 20.0, 18.0));
        assert_eq!((file_row::MARK_WIDTH, file_row::MARK_SIZE), (14.0, 11.0));
        assert_eq!(Mode::Dark.colors().file_row_fg, Mode::Dark.colors().fg_muted);
        assert_eq!(Mode::Light.colors().file_row_fg, Mode::Light.colors().fg_muted);
    }

    #[test]
    fn test_view_bar_and_quick_settings_sizes_match_the_design_tokens() {
        assert_eq!((view_bar::HEIGHT, view_bar::PADDING_X, view_bar::GAP), (36.0, 8.0, 4.0));
        assert_eq!((view_bar::BUTTON_WIDTH, view_bar::BUTTON_HEIGHT, view_bar::BUTTON_RADIUS), (30.0, 28.0, 6.0));
        assert_eq!((quick_settings::WIDTH, quick_settings::PADDING, quick_settings::RADIUS), (344.0, 16.0, 8.0));
        assert_eq!((quick_settings::ROW_MIN_HEIGHT, quick_settings::STEP_SIZE), (38.0, 28.0));
        assert_eq!((quick_settings::SELECT_WIDTH, quick_settings::SELECT_HEIGHT), (158.0, 30.0));
    }
}
