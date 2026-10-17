//! 빠른 설정 패널의 줄 구성과 선택지, 선택이 쓰는 설정 키·값, 패널이 놓일 자리를 정하는 순수 규칙.
//!
//! 패널의 선택값은 모두 기존 앱과 같은 설정 키·같은 값 형식으로 네이티브 설정 파일에 쓴다. 읽기는 두 파일을 합친 값이다.
use paddock_core::settings_text::{SettingValues, default_profile_key, key, terminal_font_setting};
use paddock_core::theme::{Mode, font, quick_settings as size, sidebar, titlebar, view_bar};
use serde_json::Value;

/// 터미널 글꼴 후보. 이 컴퓨터에 설치된 것만 고를 수 있다.
pub const TERMINAL_FONT_CANDIDATES: [&str; 8] =
    ["Consolas", "Cascadia Mono", "JetBrains Mono", "D2Coding", "Menlo", "SF Mono", "Fira Code", "DejaVu Sans Mono"];

/// 창 틀 글꼴 후보. 이 컴퓨터에 설치된 것만 고를 수 있다.
pub const INTERFACE_FONT_CANDIDATES: [&str; 7] =
    ["Segoe UI", "Inter", "Noto Sans", "Noto Sans CJK KR", "Malgun Gothic", "Apple SD Gothic Neo", "Arial"];

/// 패널의 줄 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Theme,
    Zoom,
    FontSize,
    TerminalFont,
    InterfaceFont,
    DefaultShell,
    DoneSound,
    OnQuit,
    RestoreOnRestart,
    AgentPermissions,
    QuitAndStopAll,
}

impl Row {
    /// 패널에 보이는 순서. 마지막 줄은 앱을 끝내는 줄이다.
    pub const ALL: [Self; 11] = [
        Self::Theme,
        Self::Zoom,
        Self::FontSize,
        Self::TerminalFont,
        Self::InterfaceFont,
        Self::DefaultShell,
        Self::DoneSound,
        Self::OnQuit,
        Self::RestoreOnRestart,
        Self::AgentPermissions,
        Self::QuitAndStopAll,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Color theme",
            Self::Zoom => "Zoom",
            Self::FontSize => "Code font size",
            Self::TerminalFont => "Code font",
            Self::InterfaceFont => "Interface font",
            Self::DefaultShell => "Default shell",
            Self::DoneSound => "Agent done sound",
            Self::OnQuit => "On quit",
            Self::RestoreOnRestart => "Restore on restart",
            Self::AgentPermissions => "Agent permissions",
            Self::QuitAndStopAll => "Quit and Stop All Terminals",
        }
    }

    /// 줄 이름 아래에 보이는, 이 줄을 바꾸면 무엇에 언제 적용되는지.
    pub fn applies_to(self) -> &'static str {
        match self {
            Self::Theme | Self::Zoom => "All windows, right away",
            Self::FontSize | Self::TerminalFont => "All terminals and file views, right away",
            Self::InterfaceFont => "Sidebar, tabs and status bar",
            Self::DefaultShell => "New terminals",
            Self::DoneSound => "Next agent completion",
            Self::OnQuit => "Next time you quit",
            Self::RestoreOnRestart => "Next time Paddock starts",
            Self::AgentPermissions => "New agent terminals",
            Self::QuitAndStopAll => "Ends every terminal, then quits",
        }
    }
}

/// 선택상자로 고르는 줄.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Theme,
    TerminalFont,
    InterfaceFont,
    DefaultShell,
    OnQuit,
    RestoreOnRestart,
    AgentPermissions,
}

/// 선택지 하나: 설정 파일에 쓰는 값과 화면에 보이는 이름.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

impl Choice {
    fn new(
        value: &str,
        label: &str,
    ) -> Self {
        Self { value: value.to_string(), label: label.to_string() }
    }
}

/// 열린 패널의 상태.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuickState {
    pub open: bool,
    /// 선택지를 펼친 선택상자. 하나만 펼친다.
    pub expanded: Option<Field>,
}

/// 패널이 놓일 자리(창 안 좌표, 화면 크기 단위).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    /// 패널 높이 한도. 줄이 더 많으면 패널 안에서 스크롤한다.
    pub max_height: f32,
}

/// 보기 줄 톱니 바로 아래에 놓일 패널의 자리. 오른쪽 가장자리를 넘으면 안쪽으로 당기고, 창보다 넓어지지 않는다.
///
/// Examples
/// --------
/// | 창 폭 | 배율 | 왼쪽 | 폭  |
/// |-------|------|------|-----|
/// | 1000  | 1.0  | 234  | 344 |
/// | 500   | 1.0  | 148  | 344 |
/// | 300   | 1.0  | 8    | 284 |
pub fn panel_rect(
    viewport: (f32, f32),
    zoom: f32,
) -> PanelRect {
    let margin = size::SCREEN_MARGIN;
    let width = (size::WIDTH * zoom).min(viewport.0 - 2.0 * margin);
    let gear_left = zoom * (sidebar::WIDTH - view_bar::PADDING_X - view_bar::BUTTON_WIDTH);
    let left = gear_left.min(viewport.0 - width - margin).max(margin);
    let top = zoom * (titlebar::HEIGHT + view_bar::HEIGHT + size::ANCHOR_GAP);
    PanelRect { left, top, width, max_height: viewport.1 - zoom * size::HEIGHT_MARGIN }
}

/// 후보 가운데 이 컴퓨터에 설치된 글꼴(후보 순서). 설정에 이미 있는 글꼴(`current`)은 후보에 없어도 맨 앞에 둬 고른 값이 사라지지 않게 한다.
pub fn font_options(
    installed: &[String],
    candidates: &[&str],
    current: Option<&str>,
) -> Vec<String> {
    let same = |left: &str, right: &str| left.eq_ignore_ascii_case(right);
    let mut options: Vec<String> = candidates
        .iter()
        .filter(|candidate| installed.iter().any(|name| same(name, candidate)))
        .map(|candidate| candidate.to_string())
        .collect();
    if let Some(current) = current.filter(|current| !current.is_empty())
        && !options.iter().any(|option| same(option, current))
    {
        options.insert(0, current.to_string());
    }
    options
}

/// 고정 선택지를 가진 선택상자의 선택지. 글꼴·기본 셸은 이 컴퓨터에서 정해지므로 비어 있다.
pub fn choices(field: Field) -> Vec<Choice> {
    match field {
        Field::Theme => Mode::ALL.iter().map(|mode| Choice::new(mode.theme_name(), mode.theme_name())).collect(),
        Field::OnQuit => {
            vec![Choice::new("ask", "Ask"), Choice::new("keep", "Keep running"), Choice::new("stop", "Stop all")]
        }
        Field::RestoreOnRestart => vec![Choice::new("always", "Always"), Choice::new("never", "Never")],
        Field::AgentPermissions => {
            vec![Choice::new("ask", "Ask"), Choice::new("allowSkip", "Allow skip"), Choice::new("skip", "Skip all")]
        }
        Field::TerminalFont | Field::InterfaceFont | Field::DefaultShell => Vec::new(),
    }
}

/// 선택상자의 지금 값(`choices`의 값 글자). 두 설정 파일을 합친 값이고, 없거나 모르는 값이면 기본값이다.
/// 글꼴은 설정된 글꼴 이름(없으면 빈 글자), 기본 셸은 설정된 프로필 이름(없으면 빈 글자)이다.
pub fn current_value(
    field: Field,
    values: &SettingValues,
) -> String {
    let known = |key: &str, field: Field, default: &str| {
        let wanted = values.get(key).and_then(Value::as_str);
        choices(field)
            .into_iter()
            .map(|choice| choice.value)
            .find(|value| Some(value.as_str()) == wanted)
            .unwrap_or_else(|| default.to_string())
    };
    match field {
        Field::Theme => values.appearance().mode.theme_name().to_string(),
        Field::OnQuit => known(key::ON_QUIT, field, "ask"),
        Field::RestoreOnRestart => known(key::RESTORE_ON_RESTART, field, "always"),
        Field::AgentPermissions => known(key::AGENT_PERMISSIONS, field, "ask"),
        Field::TerminalFont => {
            values.appearance().terminal_font_family.unwrap_or_else(|| font::TERMINAL_FAMILY.to_string())
        }
        Field::InterfaceFont => values.appearance().interface_font_family.unwrap_or_default(),
        Field::DefaultShell => values.default_profile().unwrap_or_default().to_string(),
    }
}

/// 선택값 `value`를 설정 파일에 쓸 (키, 값)으로 바꾼다. 기존 앱과 같은 키·같은 값 형식이다.
pub fn setting_of(
    field: Field,
    value: &str,
) -> (&'static str, Value) {
    match field {
        Field::Theme => (key::COLOR_THEME, Value::from(value)),
        Field::TerminalFont => (key::TERMINAL_FONT_FAMILY, Value::from(terminal_font_setting(value))),
        Field::InterfaceFont => (key::INTERFACE_FONT_FAMILY, Value::from(value)),
        Field::DefaultShell => (default_profile_key(), Value::from(value)),
        Field::OnQuit => (key::ON_QUIT, Value::from(value)),
        Field::RestoreOnRestart => (key::RESTORE_ON_RESTART, Value::from(value)),
        Field::AgentPermissions => (key::AGENT_PERMISSIONS, Value::from(value)),
    }
}

#[cfg(test)]
mod tests {
    use paddock_core::settings_text::SettingValues;
    use paddock_core::theme::Mode;

    use super::*;
    use crate::quit_policy::OnQuit;

    #[test]
    fn test_c_d51_font_rows_say_they_apply_to_terminals_and_file_views() {
        assert_eq!(Row::FontSize.label(), "Code font size");
        assert_eq!(Row::TerminalFont.label(), "Code font");
        for row in [Row::FontSize, Row::TerminalFont] {
            assert_eq!(row.applies_to(), "All terminals and file views, right away");
        }
    }

    #[test]
    fn test_c_d41_panel_stays_inside_a_1000_by_650_window_at_every_zoom() {
        for zoom in [0.58, 1.0, 1.2, 1.728, 2.07] {
            let rect = panel_rect((1000.0, 650.0), zoom);
            assert!(rect.left >= 0.0 && rect.left + rect.width <= 1000.0, "{zoom}: {rect:?}");
            assert!(rect.top >= 0.0 && rect.top + rect.max_height <= 650.0, "{zoom}: {rect:?}");
        }
    }

    #[test]
    fn test_c_d41_panel_opens_under_the_gear_and_is_pulled_in_from_the_right_edge() {
        let wide = panel_rect((1000.0, 650.0), 1.0);
        assert_eq!((wide.left, wide.width), (234.0, 344.0), "left edge of the gear");
        assert_eq!(wide.top, 76.0, "title bar + view bar + 6");
        let narrow = panel_rect((500.0, 650.0), 1.0);
        assert_eq!(narrow.left + narrow.width, 500.0 - 8.0, "8px from the right edge");
        let tiny = panel_rect((300.0, 650.0), 1.0);
        assert_eq!((tiny.left, tiny.width), (8.0, 284.0), "never wider than the window minus 16");
    }

    #[test]
    fn test_c_d41_font_options_keep_the_candidate_order_and_only_installed_ones() {
        let installed: Vec<String> =
            ["Zed Mono", "Fira Code", "Consolas"].iter().map(|name| name.to_string()).collect();
        let candidates = ["Consolas", "Cascadia Mono", "Fira Code"];
        assert_eq!(font_options(&installed, &candidates, None), vec!["Consolas", "Fira Code"]);
        // 설정에 있는 글꼴이 후보에 없어도 고른 값이 사라지지 않도록 맨 앞에 둔다.
        assert_eq!(font_options(&installed, &candidates, Some("Hack")), vec!["Hack", "Consolas", "Fira Code"]);
        assert_eq!(font_options(&installed, &candidates, Some("Fira Code")), vec!["Consolas", "Fira Code"]);
    }

    #[test]
    fn test_c_d41_fixed_choices_use_the_values_the_settings_files_hold() {
        let values = |field: Field| choices(field).into_iter().map(|choice| choice.value).collect::<Vec<_>>();
        assert_eq!(values(Field::Theme), vec!["Mermaid Dark", "Mermaid Light"]);
        assert_eq!(values(Field::OnQuit), vec!["ask", "keep", "stop"]);
        assert_eq!(values(Field::RestoreOnRestart), vec!["always", "never"]);
        assert_eq!(values(Field::AgentPermissions), vec!["ask", "allowSkip", "skip"]);
        for on_quit in [OnQuit::Ask, OnQuit::Keep, OnQuit::Stop] {
            assert!(values(Field::OnQuit).contains(&on_quit.as_str().to_string()));
        }
        assert_eq!(Mode::ALL.map(Mode::theme_name).to_vec(), values(Field::Theme));
    }

    #[test]
    fn test_c_d41_current_value_reads_each_field_from_the_two_files() {
        let old = r#"{"paddock.backend.onQuit": "stop", "workbench.colorTheme": "Mermaid Light"}"#;
        let native = r#"{"paddock.terminal.restoreOnRestart": "never"}"#;
        let values = SettingValues::from_texts(native, old);
        assert_eq!(current_value(Field::OnQuit, &values), "stop");
        assert_eq!(current_value(Field::Theme, &values), "Mermaid Light");
        assert_eq!(current_value(Field::RestoreOnRestart, &values), "never");
        assert_eq!(current_value(Field::AgentPermissions, &values), "ask");
        let empty = SettingValues::from_texts("", "");
        assert_eq!(current_value(Field::Theme, &empty), "Mermaid Dark");
        assert_eq!(current_value(Field::OnQuit, &empty), "ask");
        assert_eq!(current_value(Field::RestoreOnRestart, &empty), "always");
    }

    #[test]
    fn test_c_d41_rows_end_with_quit_and_stop_all_and_each_says_what_it_applies_to() {
        assert_eq!(Row::ALL.last(), Some(&Row::QuitAndStopAll));
        for row in Row::ALL {
            assert!(!row.label().is_empty(), "{row:?}");
            assert!(row == Row::QuitAndStopAll || !row.applies_to().is_empty(), "{row:?}");
        }
        assert_eq!(Row::QuitAndStopAll.label(), "Quit and Stop All Terminals");
    }
    #[test]
    fn test_c_d41_choosing_writes_the_shared_key_in_the_old_apps_value_format() {
        let (key, value) = setting_of(Field::TerminalFont, "Fira Code");
        assert_eq!(key, "terminal.integrated.fontFamily");
        let text = value.as_str().unwrap().to_string();
        assert!(text.starts_with("'Fira Code', ") && text.contains("'Malgun Gothic'"), "{text}");
        assert_eq!(setting_of(Field::InterfaceFont, "Inter"), ("paddock.interfaceFontFamily", "Inter".into()));
        assert_eq!(setting_of(Field::Theme, "Mermaid Light"), ("workbench.colorTheme", "Mermaid Light".into()));
        assert_eq!(setting_of(Field::OnQuit, "keep"), ("paddock.backend.onQuit", "keep".into()));
        assert_eq!(setting_of(Field::RestoreOnRestart, "never"), ("paddock.terminal.restoreOnRestart", "never".into()));
        assert_eq!(setting_of(Field::AgentPermissions, "skip"), ("paddock.agents.permissions", "skip".into()));
        let (shell_key, shell_value) = setting_of(Field::DefaultShell, "Git Bash");
        assert!(shell_key.starts_with("terminal.integrated.defaultProfile."), "{shell_key}");
        assert_eq!(shell_value, "Git Bash");
    }

    /// 소스에서 시험 모듈 앞부분만 잘라, 숫자 리터럴을 바로 넘기는 `px(` 호출(`zpx(`·`gpui_kit::px(` 포함)이 있는 줄을 모은다.
    fn lines_with_literal_px(source: &str) -> Vec<String> {
        let production = source.split("#[cfg(test)]").next().unwrap_or_default();
        production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| {
                line.match_indices("px(").any(|(at, _)| line[at + 3..].starts_with(|c: char| c.is_ascii_digit()))
            })
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn test_sizes_come_from_theme_tokens_not_literals() {
        assert_eq!(lines_with_literal_px(include_str!("panel.rs")), Vec::<String>::new());
        assert_eq!(lines_with_literal_px(include_str!("../window_view.rs")), Vec::<String>::new());
    }
}
