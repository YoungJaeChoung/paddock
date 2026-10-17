//! Work 목록 메뉴의 항목 구성: 행 우클릭 메뉴(터미널·Unassigned 묶음·작업 폴더)와 머리줄 ⌄의 실행 환경 목록.
//!
//! 화면을 모르는 순수 규칙이라 창 없이 시험한다. 이 결과를 `menu`가 판으로 그린다.
use paddock_core::account_profile::provider_name;
use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_core::theme::menu as size;
use paddock_core::theme::text;
use paddock_core::work_model::NewPlace;

use crate::files::menu_model::reveal_label;
use crate::shell_choice::Environment;

/// 메뉴가 가리키는 대상.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// 작업 폴더 안의 터미널 행.
    Terminal(u32),
    /// Unassigned 묶음 행. 값은 묶음의 시작 터미널.
    Group(u32),
    Folder(String),
    /// 머리줄 ⌄: 새 Unassigned 묶음을 열 실행 환경 목록.
    Environments,
    /// 탭 줄·폴더 줄의 ＋와 계정 고르기: 일반 터미널과 계정 터미널 중 `NewPlace`에 열 것을 고른다.
    NewTerminal(NewPlace),
    /// 파일 목록의 항목 행(파일 또는 폴더).
    Entry {
        path: String,
        is_dir: bool,
    },
}

/// 열려 있는 메뉴와 그 자리(창 안 좌표).
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub x: f32,
    pub y: f32,
    pub kind: MenuKind,
}

/// 항목을 골랐을 때 할 일.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    RenameTerminal(u32),
    CloseTerminal(u32),
    CloseGroup(u32),
    NewInFolder(String),
    RenameFolder(String),
    RemoveFolder(String),
    /// 실행 환경 목록의 번째 항목으로 새 Unassigned 묶음을 연다.
    NewInEnvironment(usize),
    /// 계정 없는 일반 터미널을 `NewPlace`에 연다.
    NewPlain(NewPlace),
    /// 계정(`id`) 터미널을 `place`에 연다.
    NewAccount {
        id: String,
        place: NewPlace,
    },
    /// Accounts 창을 연다.
    ManageAccounts,
    /// `parent` 폴더 안에 새 파일(`is_dir`이 거짓)이나 새 폴더를 만드는 창을 연다.
    NewEntryIn {
        parent: String,
        is_dir: bool,
    },
    /// 파일 관리자에서 그 경로의 항목을 보여 준다.
    Reveal(String),
    /// 파일 탭으로 연다.
    OpenFile(String),
}

/// 메뉴 항목 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// 이름 오른쪽 보조 글자(`Windows`·`default`).
    pub meta: String,
    /// 되돌릴 수 없는 항목이라 위험색으로 보인다.
    pub danger: bool,
    /// 위 항목들과 구분선으로 나뉜다.
    pub separated: bool,
    pub action: MenuAction,
}

fn item(
    label: &str,
    action: MenuAction,
) -> MenuItem {
    MenuItem { label: label.to_string(), meta: String::new(), danger: false, separated: false, action }
}

/// 메뉴의 항목들. 닫기·목록에서 빼기는 위험 항목이다.
///
/// Examples
/// --------
/// | 대상               | 항목                                                            |
/// |--------------------|-----------------------------------------------------------------|
/// | 터미널 행          | `Rename terminal`, `Close terminal`                             |
/// | Unassigned 묶음    | `Rename terminal`, `Close terminal`                             |
/// | 작업 폴더          | `New work terminal here`, `Rename in list`, `Remove from list`  |
/// | ⌄                  | 실행 환경마다 하나(보조 글자: 셸이 도는 곳, 기본이면 `default`) |
/// | 파일 목록 파일     | `Open`, `Reveal in File Explorer`(macOS `Finder`, Linux `File Manager`) |
/// | 파일 목록 폴더     | `New File...`, `New Folder...`, `Reveal in File Explorer` 등    |
/// | ＋·계정 고르기     | `Terminal`, 등록 계정마다 `Claude · Main`(WSL이면 보조 글자 `WSL · 배포판`), 구분선 뒤 `Manage accounts…` |
pub fn menu_items(
    kind: &MenuKind,
    environments: &[Environment],
    profiles: &[AccountProfile],
) -> Vec<MenuItem> {
    match kind {
        MenuKind::Terminal(id) => vec![
            item("Rename terminal", MenuAction::RenameTerminal(*id)),
            MenuItem { danger: true, ..item("Close terminal", MenuAction::CloseTerminal(*id)) },
        ],
        MenuKind::Group(root) => vec![
            item("Rename terminal", MenuAction::RenameTerminal(*root)),
            MenuItem { danger: true, ..item("Close terminal", MenuAction::CloseGroup(*root)) },
        ],
        MenuKind::Folder(key) => vec![
            item("New work terminal here", MenuAction::NewInFolder(key.clone())),
            item("Rename in list", MenuAction::RenameFolder(key.clone())),
            MenuItem { danger: true, ..item("Remove from list", MenuAction::RemoveFolder(key.clone())) },
        ],
        MenuKind::Entry { path, is_dir } => {
            let reveal = item(reveal_label(std::env::consts::OS), MenuAction::Reveal(path.clone()));
            if *is_dir {
                vec![
                    item("New File...", MenuAction::NewEntryIn { parent: path.clone(), is_dir: false }),
                    item("New Folder...", MenuAction::NewEntryIn { parent: path.clone(), is_dir: true }),
                    reveal,
                ]
            } else {
                vec![item("Open", MenuAction::OpenFile(path.clone())), reveal]
            }
        }
        MenuKind::Environments => environments
            .iter()
            .enumerate()
            .map(|(index, environment)| MenuItem {
                label: environment.label.clone(),
                meta: [environment.place, if environment.is_default { "default" } else { "" }]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" \u{b7} "),
                danger: false,
                separated: false,
                action: MenuAction::NewInEnvironment(index),
            })
            .collect(),
        MenuKind::NewTerminal(place) => {
            let mut items = vec![item("Terminal", MenuAction::NewPlain(place.clone()))];
            items.extend(profiles.iter().map(|profile| MenuItem {
                label: format!("{} \u{b7} {}", provider_name(&profile.provider).unwrap_or(""), profile.label),
                meta: match (profile.runtime, profile.wsl_distribution.as_deref()) {
                    (Runtime::Wsl, Some(distribution)) => format!("WSL \u{b7} {distribution}"),
                    (Runtime::Wsl, None) => "WSL".to_string(),
                    (Runtime::Native, _) => String::new(),
                },
                danger: false,
                separated: false,
                action: MenuAction::NewAccount { id: profile.id.clone(), place: place.clone() },
            }));
            items.push(MenuItem { separated: true, ..item("Manage accounts\u{2026}", MenuAction::ManageAccounts) });
            items
        }
    }
}

/// 폴더 메뉴 아래 안내 글: 목록에서 빼면 닫히는 터미널 수와 디스크의 폴더가 남는다는 사실.
///
/// Examples
/// --------
/// | n_terminals | 결과                                   |
/// |-------------|----------------------------------------|
/// | 0           | `Closes 0 terminals · folder is kept`  |
/// | 1           | `Closes 1 terminal · folder is kept`   |
/// | 3           | `Closes 3 terminals · folder is kept`  |
pub fn folder_note(n_terminals: usize) -> String {
    format!("Closes {n_terminals} terminal{} \u{b7} folder is kept", if n_terminals == 1 { "" } else { "s" })
}

/// 메뉴 판의 예상 높이(논리 px). 창 가장자리에서 안쪽으로 당길 때 쓴다.
pub fn panel_height(
    n_items: usize,
    has_note: bool,
) -> f32 {
    let item_height = size::ITEM_PADDING_Y * 2.0 + size::FONT_SIZE * 1.4;
    let note_height = if has_note {
        size::NOTE_PADDING_TOP + size::NOTE_PADDING_BOTTOM + text::SMALL * size::NOTE_LINE_HEIGHT
    } else {
        0.0
    };
    n_items as f32 * item_height + size::PADDING * 2.0 + note_height
}

/// 판을 `(x, y)`에 열되 창 오른쪽·아래 끝을 넘으면 안쪽으로 당긴 자리.
///
/// Examples
/// --------
/// | 창 크기(폭×높이) | 요청 자리  | 판(폭×높이) | 결과        |
/// |------------------|------------|-------------|-------------|
/// | 1000×600         | (100, 100) | 236×80      | (100, 100)  |
/// | 1000×600         | (900, 580) | 236×80      | (764, 520)  |
pub fn clamped_position(
    window: (f32, f32),
    requested: (f32, f32),
    panel: (f32, f32),
) -> (f32, f32) {
    ((requested.0).min(window.0 - panel.0).max(0.0), (requested.1).min(window.1 - panel.1).max(0.0))
}

/// 폴더 행 오른쪽의 열린 터미널 수 글자.
///
/// Examples
/// --------
/// | n_terminals | 결과   |
/// |-------------|--------|
/// | 0           | `>_ 0` |
/// | 12          | `>_ 12`|
pub fn count_label(n_terminals: usize) -> String {
    format!(">_ {n_terminals}")
}

/// 가려진 터미널을 펼치는 줄의 글자.
pub fn more_label(n_hidden: usize) -> String {
    format!("Show {n_hidden} more")
}

/// 터미널 행의 이름 글자: 이름 뒤에 같은 이름의 번호(`·2`)가 붙는다. 번호는 이름과 한 덩어리로 말줄임한다.
pub fn row_label(
    name: &str,
    suffix: &str,
) -> String {
    format!("{name}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(
        label: &str,
        place: &'static str,
        is_default: bool,
    ) -> Environment {
        Environment { label: label.to_string(), place, shell: "bash", wsl_distribution: None, is_default }
    }

    #[test]
    fn test_c_files_23_file_rows_offer_open_and_reveal_and_folder_rows_add_new_file_and_new_folder() {
        let reveal = crate::files::menu_model::reveal_label(std::env::consts::OS);
        let labels = |is_dir: bool| {
            let kind = MenuKind::Entry { path: "/p/x".to_string(), is_dir };
            menu_items(&kind, &[], &[]).into_iter().map(|item| item.label).collect::<Vec<_>>()
        };
        assert_eq!(labels(false), ["Open", reveal]);
        assert_eq!(labels(true), ["New File...", "New Folder...", reveal]);
        let folder = MenuKind::Entry { path: "/p/x".to_string(), is_dir: true };
        let actions: Vec<MenuAction> = menu_items(&folder, &[], &[]).into_iter().map(|item| item.action).collect();
        assert_eq!(actions[0], MenuAction::NewEntryIn { parent: "/p/x".to_string(), is_dir: false });
        assert_eq!(actions[2], MenuAction::Reveal("/p/x".to_string()));
    }

    #[test]
    fn test_row_menus_name_rename_and_close_with_close_marked_dangerous() {
        let labels = |kind: &MenuKind| {
            menu_items(kind, &[], &[]).into_iter().map(|item| (item.label, item.danger)).collect::<Vec<_>>()
        };
        let terminal = vec![("Rename terminal".to_string(), false), ("Close terminal".to_string(), true)];
        assert_eq!(labels(&MenuKind::Terminal(3)), terminal);
        assert_eq!(labels(&MenuKind::Group(3)), terminal);
        assert_eq!(
            labels(&MenuKind::Folder("/repo".to_string())),
            vec![
                ("New work terminal here".to_string(), false),
                ("Rename in list".to_string(), false),
                ("Remove from list".to_string(), true)
            ]
        );
    }

    #[test]
    fn test_group_menu_acts_on_the_whole_group_but_renames_its_first_terminal() {
        let items = menu_items(&MenuKind::Group(7), &[], &[]);
        assert_eq!(items[0].action, MenuAction::RenameTerminal(7));
        assert_eq!(items[1].action, MenuAction::CloseGroup(7));
    }

    #[test]
    fn test_environment_menu_has_one_item_per_environment_with_place_and_default_marks() {
        let environments =
            vec![environment("PowerShell", "Windows", true), environment("WSL \u{b7} Ubuntu", "Linux", false)];
        let items = menu_items(&MenuKind::Environments, &environments, &[]);
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].label.as_str(), items[0].meta.as_str()), ("PowerShell", "Windows \u{b7} default"));
        assert_eq!((items[1].label.as_str(), items[1].meta.as_str()), ("WSL \u{b7} Ubuntu", "Linux"));
        assert_eq!(items[1].action, MenuAction::NewInEnvironment(1));
    }

    #[test]
    fn test_folder_note_counts_the_terminals_the_removal_closes() {
        assert_eq!(folder_note(0), "Closes 0 terminals \u{b7} folder is kept");
        assert_eq!(folder_note(1), "Closes 1 terminal \u{b7} folder is kept");
        assert_eq!(folder_note(3), "Closes 3 terminals \u{b7} folder is kept");
    }

    #[test]
    fn test_clamped_position_pulls_the_panel_inside_the_window() {
        assert_eq!(clamped_position((1000.0, 600.0), (100.0, 100.0), (236.0, 80.0)), (100.0, 100.0));
        assert_eq!(clamped_position((1000.0, 600.0), (900.0, 580.0), (236.0, 80.0)), (764.0, 520.0));
        assert_eq!(clamped_position((100.0, 50.0), (10.0, 10.0), (236.0, 80.0)), (0.0, 0.0));
    }

    #[test]
    fn test_row_texts() {
        assert_eq!(count_label(3), ">_ 3");
        assert_eq!(more_label(12), "Show 12 more");
        assert_eq!(row_label("claude", "\u{b7}2"), "claude\u{b7}2");
        assert_eq!(row_label("claude", ""), "claude");
    }

    fn profile(
        provider: &str,
        label: &str,
        runtime: Runtime,
    ) -> AccountProfile {
        AccountProfile {
            id: format!("id-{label}"),
            provider: provider.to_string(),
            label: label.to_string(),
            runtime,
            wsl_distribution: (runtime == Runtime::Wsl).then(|| "Ubuntu".to_string()),
        }
    }

    #[test]
    fn test_new_terminal_menu_lists_the_plain_terminal_then_each_account_then_manage_accounts() {
        let place = NewPlace::Folder("/repo".to_string());
        let profiles = vec![profile("claude", "Main", Runtime::Native), profile("codex", "Work", Runtime::Wsl)];
        let items = menu_items(&MenuKind::NewTerminal(place.clone()), &[], &profiles);
        let rows: Vec<(&str, &str, bool)> =
            items.iter().map(|item| (item.label.as_str(), item.meta.as_str(), item.separated)).collect();
        assert_eq!(
            rows,
            vec![
                ("Terminal", "", false),
                ("Claude \u{b7} Main", "", false),
                ("Codex \u{b7} Work", "WSL \u{b7} Ubuntu", false),
                ("Manage accounts\u{2026}", "", true)
            ]
        );
        assert_eq!(items[0].action, MenuAction::NewPlain(place.clone()));
        assert_eq!(items[2].action, MenuAction::NewAccount { id: "id-Work".to_string(), place });
        assert_eq!(items[3].action, MenuAction::ManageAccounts);
        // 계정이 없어도 고르기는 일반 터미널과 계정 관리 둘이다.
        assert_eq!(menu_items(&MenuKind::NewTerminal(NewPlace::NewGroup), &[], &[]).len(), 2);
    }
}
