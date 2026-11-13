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
    // -- 폴더 검색: 일치 글자 바탕·켠 옵션 단추 바탕·상한 안내 바탕 --
    pub search_match_bg: u32,
    pub search_option_on_bg: u32,
    pub search_notice_bg: u32,
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
    /// 게이지 막대의 채움색. 75% 이상은 `warning`, 90% 이상은 `danger`가 대신한다. 빈 부분(막대 바탕)은 이 색의 22% 투명이다.
    pub meter_fill: u32,
    /// 누를 수 있는 상태 줄 묶음 위에 마우스를 올렸을 때의 바탕(0xRRGGBBAA).
    pub status_hover_bg: u32,
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
        search_match_bg: 0x673819,
        search_option_on_bg: 0x245779,
        search_notice_bg: 0x352a05,
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
        meter_fill: 0x0e70c0,
        status_hover_bg: 0xffffff1f,
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
        search_match_bg: 0xf0c1a2,
        search_option_on_bg: 0xcce9fc,
        search_notice_bg: 0xf6f5d2,
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
        meter_fill: 0x0e70c0,
        status_hover_bg: 0xffffff1f,
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
    /// macOS는 모든 맥에 있는 Menlo를 쓴다(Consolas·DejaVu Sans Mono가 없어 대체 글꼴로 그려지면 글자 간격이 넓어진다).
    #[cfg(windows)]
    pub const TERMINAL_FAMILY: &str = "Consolas";
    #[cfg(target_os = "macos")]
    pub const TERMINAL_FAMILY: &str = "Menlo";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const TERMINAL_FAMILY: &str = "DejaVu Sans Mono";
    #[cfg(windows)]
    pub const TERMINAL_FALLBACK_FAMILY: &str = "Malgun Gothic";
    #[cfg(target_os = "macos")]
    pub const TERMINAL_FALLBACK_FAMILY: &str = "Apple SD Gothic Neo";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const TERMINAL_FALLBACK_FAMILY: &str = "Noto Sans CJK KR";
    /// 창 틀 글꼴. 없는 플랫폼은 시스템 기본 글꼴로 대신한다.
    #[cfg(windows)]
    pub const UI_FAMILY: &str = "Segoe UI";
    #[cfg(target_os = "macos")]
    pub const UI_FAMILY: &str = "Helvetica Neue";
    #[cfg(not(any(windows, target_os = "macos")))]
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
    /// 터미널 행 왼쪽 여백 = 22 + 사이드바 들여쓰기(설정 `paddock.sidebarIndent`, 기본 12). 여기에는 22만 있다.
    pub const TERMINAL_ROW_PADDING_BASE: f32 = 22.0;
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
    /// 저장소 묶음 행 아래의 폴더 행과 그 터미널 행이 묶음 행보다 더 들여 쓰는 크기.
    pub const REPO_MEMBER_INDENT: f32 = 12.0;
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
    /// 폴더 한 단계 안쪽의 들여쓰기는 상수가 아니라 설정 값이다(사이드바 들여쓰기, 기본 12).
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

/// 이미지 탭: 칸 둘레 여백, 아래 상태 줄, 탭 줄 오른쪽 `Fit`·`1:1` 단추 묶음.
pub mod image_view {
    /// 탭 줄 오른쪽 구역에서 `Fit`·`1:1` 묶음 몫으로 비워 두는 폭(두 단추 글자 폭 + 바깥 여백). 단추 모양은 `markdown_view_actions`와 같다.
    pub const ACTIONS_RESERVED_WIDTH: f32 = 90.0;
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
    /// 탭 줄 오른쪽 구역에서 `−`·`+`·`Fit`·`Open with default app` 몫으로 비워 두는 폭.
    pub const ACTIONS_RESERVED_WIDTH: f32 = 260.0;
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
    /// 단계 목록의 왼쪽 여백 = 20 + 사이드바 들여쓰기(설정, 기본 12). 여기에는 20만 있다.
    pub const STEPS_PADDING_BASE: f32 = 20.0;
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

/// 편집기: 저장하지 않은 점·디스크 변경 띠·상태 줄 저장 표시. 캐럿은 `fg_strong`, 선택 바탕은 `selected_bg`를 편집기 테마(`highlight_theme`)에 싣는다.
pub mod editor {
    /// 저장하지 않은 파일 탭의 점(`●`). 탭의 `×` 자리에 보이고 마우스를 올리면 `×`가 된다. 크기는 `tab::CLOSE_GLYPH`와 같다.
    pub const UNSAVED_DOT: &str = "\u{25cf}";
    /// 디스크 변경 띠(`This file changed on disk.` + `Reload` + `Keep mine`): 파일 내용 위에 붙는 줄. 전체 테두리가 아니라 바탕색으로 알린다.
    pub const CHANGED_BAR_HEIGHT: f32 = 30.0;
    pub const CHANGED_BAR_PADDING_X: f32 = 12.0;
    pub const CHANGED_BAR_GAP: f32 = 8.0;
    /// 띠 속 단추(`Reload`·`Keep mine`)와 상태 줄 `Save` 단추의 높이·가로 여백·모서리.
    pub const BUTTON_HEIGHT: f32 = 20.0;
    pub const BUTTON_PADDING_X: f32 = 8.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
    /// 상태 줄 오른쪽 저장 표시(`Unsaved changes` + `Save`) 속 간격.
    pub const STATUS_GAP: f32 = 8.0;
    /// 줄로 가기 창(Ctrl+G) 입력칸의 힌트.
    pub const GO_TO_LINE_TITLE: &str = "Go to line";
}

/// 폴더 검색의 바꾸기 줄(`⇄`로 여는 둘째 입력칸)과 `Replace`·`Replace All` 단추. 입력칸 틀은 검색 입력칸(`search`)과 같다.
pub mod search_replace {
    /// 바꾸기 줄을 여닫는 단추 글자.
    pub const TOGGLE_GLYPH: &str = "\u{21c4}";
    /// 단추(`Replace`·`Replace All`)의 높이·가로 여백·모서리. 상태 줄 `Save` 단추(`editor`)와 같은 크기다.
    pub const BUTTON_HEIGHT: f32 = 20.0;
    pub const BUTTON_PADDING_X: f32 = 8.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
}

/// 파일 빠른 열기(Ctrl+Shift+P): 가운데 위의 목록 창. 창 틀은 대화상자(`dialog`)와 같고 폭만 다르다.
pub mod quick_open {
    pub const WIDTH: f32 = 560.0;
    pub const ROW_HEIGHT: f32 = 26.0;
    /// 한 번에 보이는 줄 수(넘으면 목록 안에서 스크롤한다).
    pub const VISIBLE_ROWS: usize = 12;
    /// 이름과 폴더 사이.
    pub const NAME_GAP: f32 = 8.0;
    pub const ROW_PADDING_X: f32 = 8.0;
    pub const ROW_RADIUS: f32 = 4.0;
}

/// 폴더 검색(파일 목록 자리): 입력칸과 옵션 단추, 범위 안내, 결과 트리, 상한 안내.
pub mod search {
    /// 입력칸 묶음 둘레 여백: 왼쪽은 위젯 안쪽 18, 아래는 간격 6의 두 배.
    pub const HEADER_PADDING_TOP: f32 = 6.0;
    pub const HEADER_PADDING_BOTTOM: f32 = 12.0;
    pub const HEADER_PADDING_LEFT: f32 = 18.0;
    pub const HEADER_PADDING_RIGHT: f32 = 6.0;
    /// 입력칸(테두리 1px 포함)과 안쪽 좌우 여백, 모서리.
    pub const INPUT_HEIGHT: f32 = 26.0;
    pub const INPUT_PADDING_X: f32 = 6.0;
    pub const INPUT_RADIUS: f32 = 2.0;
    /// 입력칸 오른쪽의 옵션 단추(`Aa`·`ab`·`.*`) 하나의 크기와 좌우 바깥 여백, 꺼졌을 때 불투명도.
    pub const OPTION_SIZE: f32 = 21.0;
    pub const OPTION_MARGIN_X: f32 = 1.0;
    pub const OPTION_OFF_OPACITY: f32 = 0.7;
    /// 범위 안내(`Searching in <폴더>`, 글자는 `text::TINY`)의 안쪽 여백.
    pub const SCOPE_PADDING_TOP: f32 = 2.0;
    pub const SCOPE_PADDING_X: f32 = 12.0;
    pub const SCOPE_PADDING_BOTTOM: f32 = 4.0;
    /// 결과 개수 줄(글자는 `text::SMALL`)의 높이와 왼쪽 여백.
    pub const COUNT_HEIGHT: f32 = 22.0;
    pub const COUNT_PADDING_LEFT: f32 = 18.0;
    /// 결과 트리 행 높이(파일 행·일치 줄 행 모두)와 좌우 여백, 요소 간격.
    pub const ROW_HEIGHT: f32 = 22.0;
    pub const ROW_PADDING_LEFT: f32 = 8.0;
    pub const ROW_PADDING_RIGHT: f32 = 12.0;
    pub const ROW_GAP: f32 = 4.0;
    /// 일치 줄 행의 들여쓰기(파일 행 이름 열 아래).
    pub const MATCH_INDENT: f32 = 28.0;
    /// 접기 표지가 차지하는 폭.
    pub const CHEVRON_WIDTH: f32 = 12.0;
    /// 파일 행의 폴더 경로(흐림)와 일치 줄 번호 글자: 앞 간격과 불투명도. 줄 번호 글자 크기는 본문의 0.9배다.
    pub const PATH_MARGIN_LEFT: f32 = 3.0;
    pub const LINE_NUMBER_MARGIN_LEFT: f32 = 7.0;
    pub const LINE_NUMBER_MARGIN_RIGHT: f32 = 4.0;
    pub const LINE_NUMBER_OPACITY: f32 = 0.7;
    pub const LINE_NUMBER_SCALE: f32 = 0.9;
    /// 상한 안내 상자의 안쪽 여백과 바깥 여백.
    pub const NOTICE_PADDING: f32 = 3.0;
    pub const NOTICE_MARGIN: f32 = 6.0;
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

/// 탭을 끌 때 커서를 따라다니는 미리보기 상자: 바탕은 팝업 바탕, 글자는 본문 글자색(안내 글은 `fg_muted`), 전체 1px 테두리.
pub mod tab_drag {
    pub const PADDING_X: f32 = 12.0;
    pub const PADDING_Y: f32 = 8.0;
    pub const BORDER_WIDTH: f32 = 1.0;
    pub const RADIUS: f32 = 6.0;
    pub const TEXT_SIZE: f32 = 12.0;
    /// 커서에서 상자 왼쪽 위까지의 간격.
    pub const CURSOR_OFFSET: f32 = 12.0;
    /// 마우스를 누른 뒤 이만큼(px) 움직여야 끌기가 시작된다.
    pub const START_DISTANCE: f32 = 8.0;
}

/// 탭을 놓을 자리 미리보기: 칸 위의 상자(2px `accent` 테두리 + `accent` 18% 채움)와 탭 줄에 끼울 자리의 세로선(2px `accent`).
pub mod dock_preview {
    pub const BORDER_WIDTH: f32 = 2.0;
    /// 채움의 `accent` 투명도(0~1).
    pub const FILL_ALPHA: f32 = 0.18;
    pub const INSERT_LINE_WIDTH: f32 = 2.0;
}

/// 다른 창에 있는 탭을 이 창 탭 줄에 흐리게 남길 때(칸을 나누지 않은 줄): 글자색은 `tab_inactive_fg`의 60%다.
pub mod tab_detached {
    pub const FG_ALPHA: f32 = 0.6;
}

/// 다른 창에 있는 터미널 행의 오른쪽 표지(`other window`).
pub mod row_remote_meta {
    pub const TEXT: &str = "other window";
    pub const TEXT_SIZE: f32 = 11.0;
    pub const MAX_WIDTH: f32 = 92.0;
}

/// 탭을 분리해 연 창. 크기는 기본 900×600이고, 메뉴로 열면 본 창에서 (40, 40) 떨어진 자리, 끌어 놓으면 놓은 지점에서 (-60, -16)만큼 옮긴 자리에 연다.
pub mod detached_window {
    pub const WIDTH: f32 = 900.0;
    pub const HEIGHT: f32 = 600.0;
    pub const MIN_WIDTH: f32 = 320.0;
    pub const MIN_HEIGHT: f32 = 200.0;
    /// 메뉴로 열 때 본 창의 왼쪽 위에서 떨어진 거리.
    pub const MENU_OFFSET: f32 = 40.0;
    /// 끌어 놓을 때 놓은 지점에서 창 왼쪽 위까지의 거리(왼쪽·위로 이만큼).
    pub const DROP_OFFSET_X: f32 = 60.0;
    pub const DROP_OFFSET_Y: f32 = 16.0;
}

/// 사이드바 터미널 행을 끌어 순서를 바꿀 때 놓을 자리를 알리는 선: 행 위 또는 아래의 가로 전체 선이다(한쪽 띠가 아니다).
pub mod drop_marker {
    pub const LINE_WIDTH: f32 = 2.0;
}

/// 탭 줄에 초점이 있을 때(빈 자리를 눌렀을 때): 줄 바탕이 호버 바탕으로 바뀌고 고른 탭에 초점 테두리(강조색)가 둘린다.
pub mod tab_bar_focus {
    pub const OUTLINE_WIDTH: f32 = 1.0;
}

/// 밖에서 끌어 온 파일이 놓일 폴더 행·목록의 강조: 전체 1px `accent` 테두리와 `hover_bg` 바탕(한쪽 띠가 아니다).
pub mod drop_target {
    pub const BORDER_WIDTH: f32 = 1.0;
}

/// 터미널 안 찾기 띠: 터미널 칸 오른쪽 위에 뜨는 상자(입력칸 · `N of M` · ‹ › · ×). 바탕은 팝업 바탕이고 테두리는 전체 테두리다.
pub mod terminal_find {
    pub const WIDTH: f32 = 340.0;
    pub const HEIGHT: f32 = 36.0;
    pub const RADIUS: f32 = 6.0;
    pub const PADDING_X: f32 = 8.0;
    pub const GAP: f32 = 6.0;
    /// 띠가 터미널 칸의 위·오른쪽 끝에서 떨어진 거리.
    pub const MARGIN_TOP: f32 = 8.0;
    pub const MARGIN_RIGHT: f32 = 16.0;
    /// `N of M` 글자 칸의 최소 폭(자리가 바뀌어도 단추가 흔들리지 않게).
    pub const COUNT_MIN_WIDTH: f32 = 64.0;
    pub const COUNT_SIZE: f32 = 12.0;
    /// ‹ › × 단추 하나의 크기.
    pub const BUTTON_SIZE: f32 = 24.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
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
    /// 상태 칸(점 + 글자)의 고정 폭. 글자가 바뀌어도 오른쪽 열이 흔들리지 않는다. 가장 긴 글자 `Needs input`이 잘리지 않는 폭이다.
    pub const STATUS_WIDTH: f32 = 76.0;
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

/// 상태 줄 오른쪽의 사용량 묶음(Claude·Codex·Memory): 앞 구분선, 표지·이름, 창별 게이지.
pub mod usage_group {
    /// 묶음 왼쪽 여백(앞 구분선이 이 안에 선다)과 오른쪽 여백.
    pub const PADDING_LEFT: f32 = 16.0;
    pub const PADDING_RIGHT: f32 = 2.0;
    /// 묶음 안 표지·이름·계정 이름·게이지 사이.
    pub const GAP: f32 = 5.0;
    /// 묶음 사이.
    pub const GROUP_GAP: f32 = 8.0;
    /// 앞 구분선: 폭 1, 높이 14.
    pub const DIVIDER_HEIGHT: f32 = 14.0;
    /// 출처 표지(`✱`·`◎`)의 글자 크기. Claude 표지는 굵게 12다.
    pub const MARK_SIZE: f32 = 11.0;
    pub const CLAUDE_MARK_SIZE: f32 = 12.0;
    /// 묶음에서 첫 게이지 앞 간격과 출처 이름 바로 뒤 게이지 앞 간격.
    pub const FIRST_METER_MARGIN: f32 = 10.0;
    pub const AFTER_NAME_METER_MARGIN: f32 = 4.0;
    /// 계정 이름의 최대 폭(넘으면 줄임).
    pub const ACCOUNT_NAME_MAX_WIDTH: f32 = 120.0;
    /// 값이 없을 때(`Loading…`·`Unavailable`·`No usage yet`·`Not reported`)의 글자 크기.
    pub const STATE_SIZE: f32 = 12.0;
    /// 오래된 값 옆의 경과 시간(`12m ago`) 글자 크기, 그리고 오래된 값의 게이지 투명도.
    pub const AGE_SIZE: f32 = 11.0;
    pub const STALE_OPACITY: f32 = 0.55;
    pub const NAME_SIZE: f32 = 12.0;
}

/// 게이지 하나: `5h ▰▰▱▱ 23%`. 막대는 사용한 만큼 채운다.
pub mod meter {
    /// 막대 폭: Claude·Codex 28, Memory 36. 높이 4, 모서리 3.
    pub const BAR_WIDTH: f32 = 28.0;
    pub const MEMORY_BAR_WIDTH: f32 = 36.0;
    pub const BAR_HEIGHT: f32 = 4.0;
    pub const BAR_RADIUS: f32 = 3.0;
    /// 이름표·막대·퍼센트 사이.
    pub const GAP: f32 = 5.0;
    /// 이름표·퍼센트 글자 크기(모노).
    pub const TEXT_SIZE: f32 = 12.0;
    /// 이 퍼센트 이상이면 막대·퍼센트가 `warning`, `CRITICAL_PERCENT` 이상이면 `danger` 색이다.
    pub const HIGH_PERCENT: i64 = 75;
    pub const CRITICAL_PERCENT: i64 = 90;
    /// 막대 바탕: 채움색의 22% 투명(0xAA).
    pub const TRACK_ALPHA: u32 = 0x38;
}

/// 좁은 폭에서 사용량 묶음 전부를 접는 `Usage` 단추와, 접을지 정하는 폭 추정.
pub mod usage_compact {
    /// 단추 글자와 가로 여백.
    pub const LABEL: &str = "Usage";
    pub const PADDING_X: f32 = 8.0;
    /// 글자 하나의 추정 폭(상태 줄 12px 글자 평균, 모노는 `MONO_CHAR_WIDTH`).
    pub const CHAR_WIDTH: f32 = 6.4;
    pub const MONO_CHAR_WIDTH: f32 = 7.2;
    /// 상태 줄 왼쪽 글자(`Claude · Main` 등)가 우선 쥐는 최소 폭: 오른쪽이 이 만큼 남기고 접힌다.
    pub const LEFT_MIN_WIDTH: f32 = 160.0;
}

/// 사용량 목록(팝업): 상태 줄 묶음을 누르면 오른쪽 아래에 뜨는 창. 도구별 절 안에 계정마다 한 행(이름·배지·환경·창별 게이지·`Open in new terminal`).
pub mod usage_popup {
    pub const WIDTH: f32 = 560.0;
    pub const MAX_HEIGHT: f32 = 640.0;
    /// 창 오른쪽 가장자리에서 12, 아래(상태 줄 위)에서 34 떨어진 자리에 붙는다. 위·옆으로 이 여백만큼 창 안에 남는다.
    pub const RIGHT_MARGIN: f32 = 12.0;
    pub const BOTTOM_MARGIN: f32 = 34.0;
    pub const VIEWPORT_HEIGHT_MARGIN: f32 = 72.0;
    pub const PADDING: f32 = 14.0;
    pub const RADIUS: f32 = 8.0;
    /// 머리·힌트·검색·절 사이.
    pub const GAP: f32 = 10.0;
    pub const TITLE_SIZE: f32 = 15.0;
    pub const HINT_SIZE: f32 = 12.0;
    pub const BUTTON_HEIGHT: f32 = 30.0;
    pub const BUTTON_PADDING_X: f32 = 9.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
    pub const SEARCH_HEIGHT: f32 = 32.0;
    pub const SECTION_PADDING_X: f32 = 10.0;
    pub const SECTION_PADDING_Y: f32 = 8.0;
    pub const SECTION_RADIUS: f32 = 6.0;
    pub const SECTION_GAP: f32 = 12.0;
    pub const SECTION_TITLE_SIZE: f32 = 13.0;
    pub const ROW_PADDING_Y: f32 = 12.0;
    pub const ROW_GAP: f32 = 6.0;
    pub const NAME_SIZE: f32 = 13.0;
    pub const BADGE_SIZE: f32 = 10.0;
    pub const BADGE_PADDING_X: f32 = 5.0;
    pub const BADGE_RADIUS: f32 = 3.0;
    pub const ENVIRONMENT_SIZE: f32 = 11.0;
    /// 값 칸의 최소 높이와 게이지 줄 간격.
    pub const VALUES_MIN_HEIGHT: f32 = 42.0;
    pub const VALUES_GAP: f32 = 8.0;
    /// 게이지 격자: 이름표 44 · 막대(남는 폭, 높이 6) · 퍼센트 42(오른쪽 맞춤).
    pub const GRID_LABEL_WIDTH: f32 = 44.0;
    pub const GRID_PERCENT_WIDTH: f32 = 42.0;
    pub const GRID_BAR_HEIGHT: f32 = 6.0;
}

/// 메모리 90% 경고 띠: 창 오른쪽 아래(상태 줄 위)에 뜨는 알림. 전체 테두리(경고색)로 알리고 `Go to <이름>` 단추와 닫기가 있다.
pub mod memory_notice {
    pub const WIDTH: f32 = 440.0;
    pub const RIGHT_MARGIN: f32 = 12.0;
    pub const BOTTOM_MARGIN: f32 = 34.0;
    pub const PADDING: f32 = 12.0;
    pub const GAP: f32 = 10.0;
    pub const RADIUS: f32 = 6.0;
    pub const TEXT_SIZE: f32 = 13.0;
    /// 경고 표지.
    pub const ICON: &str = "\u{26a0}";
    /// 닫기 단추 글자.
    pub const CLOSE: &str = "\u{d7}";
    /// 단추 높이·가로 여백·모서리(대화상자 단추보다 작다).
    pub const BUTTON_HEIGHT: f32 = 26.0;
    pub const BUTTON_PADDING_X: f32 = 12.0;
    pub const BUTTON_RADIUS: f32 = 4.0;
}

/// WSL 터미널에도 Claude 사용량을 보일지 묻는 창: 폭만 대화상자 기본과 다르다. 단추는 `Ask me later`(처음 초점)·`No thanks`·`Turn on`.
pub mod wsl_launcher_dialog {
    pub const WIDTH: f32 = 520.0;
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
        assert_eq!((agent_row::STATUS_WIDTH, agent_row::STATUS_DOT_SIZE, agent_row::STATUS_GAP), (76.0, 5.0, 5.0));
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
        assert_eq!((file_row::HEIGHT, file_row::FILE_ICON_MARGIN_LEFT), (26.0, 18.0));
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

    #[test]
    fn test_sync_sizes_match_the_design_tokens() {
        assert_eq!((sidebar::TERMINAL_ROW_PADDING_BASE, work_empty::STEPS_PADDING_BASE), (22.0, 20.0));
        assert_eq!((drop_marker::LINE_WIDTH, tab_bar_focus::OUTLINE_WIDTH), (2.0, 1.0));
        assert_eq!(drop_target::BORDER_WIDTH, 1.0);
        assert_eq!((terminal_find::WIDTH, terminal_find::HEIGHT, terminal_find::BUTTON_SIZE), (340.0, 36.0, 24.0));
    }

    #[test]
    fn test_tab_drag_and_detached_window_sizes_match_the_design_tokens() {
        assert_eq!(
            (tab_drag::PADDING_X, tab_drag::PADDING_Y, tab_drag::BORDER_WIDTH, tab_drag::RADIUS),
            (12.0, 8.0, 1.0, 6.0)
        );
        assert_eq!(
            (dock_preview::BORDER_WIDTH, dock_preview::FILL_ALPHA, dock_preview::INSERT_LINE_WIDTH),
            (2.0, 0.18, 2.0)
        );
        assert_eq!(tab_detached::FG_ALPHA, 0.6);
        assert_eq!(
            (row_remote_meta::TEXT, row_remote_meta::TEXT_SIZE, row_remote_meta::MAX_WIDTH),
            ("other window", 11.0, 92.0)
        );
        assert_eq!((detached_window::WIDTH, detached_window::HEIGHT), (900.0, 600.0));
        assert_eq!((detached_window::MIN_WIDTH, detached_window::MIN_HEIGHT), (320.0, 200.0));
        assert_eq!(detached_window::MENU_OFFSET, 40.0);
        assert_eq!((detached_window::DROP_OFFSET_X, detached_window::DROP_OFFSET_Y), (60.0, 16.0));
    }
}
