//! Work 목록 메뉴의 항목 구성: 행 우클릭 메뉴(터미널·Unassigned 묶음·작업 폴더)와 머리줄 ⌄의 실행 환경 목록.
//!
//! 화면을 모르는 순수 규칙이라 창 없이 시험한다. 이 결과를 `menu`가 판으로 그린다.
use paddock_core::account_profile::provider_name;
use paddock_core::account_session::{SwitchMode, SwitchSource, item_meta, menu_hint, switch_targets};
use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_core::menu_nav::MenuNav;
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
    /// 계정 고르기(탭 줄 오른쪽)가 보고 있는 터미널 기준으로 하는 일: 같은 대화를 다른 계정으로 이어 열기·이 탭에서 계정 시작·(그 밖은 `NewTerminal`).
    AccountSwitch(SwitchMenu),
    /// 파일 목록의 항목 행(파일 또는 폴더).
    Entry {
        path: String,
        is_dir: bool,
    },
    /// 편집기 우클릭: `Cut`·`Copy`·`Paste`와 검토 메모 항목. `n_notes`는 열 때 모인 메모 수다(0이면 `Send`·`Clear`가 없다).
    Editor {
        n_notes: usize,
    },
    /// 검토 메모를 보낼 에이전트 터미널 고르기(번호와 이름).
    PickAgent(Vec<(u32, String)>),
    /// 탭 줄의 파일 탭 우클릭. 값은 지금 보는 묶음 파일 칸의 탭 번호.
    FileTab(usize),
}

/// 계정 고르기 메뉴의 기준: 보고 있는 터미널과 하는 일, 대화의 출처, 새 터미널을 열 자리.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchMenu {
    pub terminal: u32,
    pub mode: SwitchMode,
    /// `Continue`일 때 대화의 출처(그 CLI의 도구·환경·계정).
    pub source: Option<SwitchSource>,
    /// `New conversation…`·`New terminal…`이 여는 새 터미널 메뉴의 자리.
    pub place: NewPlace,
}

/// 열려 있는 메뉴와 그 자리(창 안 좌표).
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub x: f32,
    pub y: f32,
    pub kind: MenuKind,
    /// 키보드와 마우스가 함께 옮기는 강조 항목.
    pub nav: MenuNav,
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
    /// 터미널 `terminal`의 대화를 계정 `id`로 이어 연다(`Continue with …`).
    ContinueWith {
        terminal: u32,
        id: String,
    },
    /// 빈 셸 터미널 `terminal`에서 계정 `id`를 시작한다(`Start in this terminal with …`).
    StartHere {
        terminal: u32,
        id: String,
    },
    /// 새 터미널을 여는 메뉴(`NewTerminal`)로 바꿔 연다.
    OpenNewTerminalMenu(NewPlace),
    /// 지금 계정처럼 고를 수 없는 항목. 눌러도 아무 일도 없다.
    Noop,
    /// `parent` 폴더 안에 새 파일(`is_dir`이 거짓)이나 새 폴더를 만드는 창을 연다.
    NewEntryIn {
        parent: String,
        is_dir: bool,
    },
    /// 파일 관리자에서 그 경로의 항목을 보여 준다.
    Reveal(String),
    /// 파일 탭으로 연다.
    OpenFile(String),
    /// 클립보드의 파일·그림을 그 행의 폴더(파일 행이면 그 파일이 있는 폴더)에 받는다.
    PasteInto {
        path: String,
        is_dir: bool,
    },
    /// 파일·폴더의 이름을 바꾸는 창을 연다.
    RenameEntry {
        path: String,
        is_dir: bool,
    },
    /// 터미널을 새로 만들지 않고 지금 보이는 다른 칸 옆의 새 칸으로 옮긴다(`Show to the Side`).
    ShowBeside(u32),
    /// 터미널을 탭 하나짜리 새 창으로 옮긴다(`Move to New Window`).
    MoveToNewWindow(u32),
    /// 다른 창에 있는 터미널을 이 창의 첫 칸 끝으로 되돌린다(`Move Back to Main Window`).
    MoveBack(u32),
    /// 지금 보는 묶음 파일 칸의 `index`번째 파일 탭을 탭 하나짜리 새 창으로 옮긴다(`Move to New Window`).
    MoveFileToNewWindow(usize),
    /// 지금 보는 묶음 파일 칸의 `index`번째 파일 탭을 닫는다(저장하지 않은 변경이 있으면 묻고 닫는다).
    CloseFileTab(usize),
    /// 지금 편집기에 잘라내기·복사·붙여넣기를 한다.
    EditorCut,
    EditorCopy,
    EditorPaste,
    /// 고른 줄에 검토 메모를 남기는 창을 연다.
    AddReviewNote,
    /// 모은 검토 메모를 에이전트 터미널에 붙여 넣는다.
    SendReviewNotes,
    ClearReviewNotes,
    /// 검토 메모를 이 터미널(에이전트)에 붙여 넣는다.
    SendNotesTo(u32),
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
/// | 터미널 행          | `Rename terminal`, `Show to the Side`(옆으로 옮길 수 있을 때만), `Close terminal` |
/// | Unassigned 묶음    | 터미널 행과 같다                                                |
/// | 작업 폴더          | `New work terminal here`, `Rename in list`, `Reveal folder`, `Remove from list` |
/// | ⌄                  | 실행 환경마다 하나(보조 글자: 셸이 도는 곳, 기본이면 `default`) |
/// | 파일 목록 파일     | `Open`, `Paste`, `Rename...`, `Reveal in File Explorer`(macOS `Finder`, Linux `File Manager`) |
/// | 편집기             | `Cut`, `Copy`, `Paste`, 구분선 뒤 `Add Review Note…`, 메모가 있으면 `Send Review Notes to Agent (N)`·`Clear Review Notes` |
/// | 에이전트 고르기    | 에이전트 터미널마다 하나(이름)                                  |
/// | 파일 목록 폴더     | `New File...`, `New Folder...`, `Paste`, `Rename...`, `Reveal in File Explorer` |
/// | ＋·계정 고르기     | `Terminal`, 등록 계정마다 `Claude · Main`(WSL이면 보조 글자 `WSL · 배포판`), 구분선 뒤 `Manage accounts…` |
///
/// `beside`는 `Show to the Side`가 옮길 터미널이다. 지금 보이는 묶음의 터미널이고 기준이 될 다른 칸이 있을 때만 호출자가 채우며, None이면 그 항목이 없다.
pub fn menu_items(
    kind: &MenuKind,
    environments: &[Environment],
    profiles: &[AccountProfile],
    beside: Option<u32>,
) -> Vec<MenuItem> {
    let show_beside = beside.map(|id| item("Show to the Side", MenuAction::ShowBeside(id)));
    match kind {
        MenuKind::Terminal(id) => [
            Some(item("Rename terminal", MenuAction::RenameTerminal(*id))),
            show_beside,
            Some(MenuItem { danger: true, ..item("Close terminal", MenuAction::CloseTerminal(*id)) }),
        ]
        .into_iter()
        .flatten()
        .collect(),
        MenuKind::Group(root) => [
            Some(item("Rename terminal", MenuAction::RenameTerminal(*root))),
            show_beside,
            Some(MenuItem { danger: true, ..item("Close terminal", MenuAction::CloseGroup(*root)) }),
        ]
        .into_iter()
        .flatten()
        .collect(),
        MenuKind::Folder(key) => vec![
            item("New work terminal here", MenuAction::NewInFolder(key.clone())),
            item("Rename in list", MenuAction::RenameFolder(key.clone())),
            item("Reveal folder", MenuAction::Reveal(key.clone())),
            MenuItem { danger: true, ..item("Remove from list", MenuAction::RemoveFolder(key.clone())) },
        ],
        MenuKind::Entry { path, is_dir } => {
            let reveal = item(reveal_label(std::env::consts::OS), MenuAction::Reveal(path.clone()));
            let paste = item("Paste", MenuAction::PasteInto { path: path.clone(), is_dir: *is_dir });
            let rename = item("Rename...", MenuAction::RenameEntry { path: path.clone(), is_dir: *is_dir });
            if *is_dir {
                vec![
                    item("New File...", MenuAction::NewEntryIn { parent: path.clone(), is_dir: false }),
                    item("New Folder...", MenuAction::NewEntryIn { parent: path.clone(), is_dir: true }),
                    paste,
                    rename,
                    reveal,
                ]
            } else {
                vec![item("Open", MenuAction::OpenFile(path.clone())), paste, rename, reveal]
            }
        }
        MenuKind::Editor { n_notes } => {
            let mut items = vec![
                item("Cut", MenuAction::EditorCut),
                item("Copy", MenuAction::EditorCopy),
                item("Paste", MenuAction::EditorPaste),
                MenuItem { separated: true, ..item("Add Review Note\u{2026}", MenuAction::AddReviewNote) },
            ];
            if *n_notes > 0 {
                items.push(item(&format!("Send Review Notes to Agent ({n_notes})"), MenuAction::SendReviewNotes));
                items.push(item("Clear Review Notes", MenuAction::ClearReviewNotes));
            }
            items
        }
        MenuKind::FileTab(index) => vec![
            item("Move to New Window", MenuAction::MoveFileToNewWindow(*index)),
            MenuItem { danger: true, ..item("Close", MenuAction::CloseFileTab(*index)) },
        ],
        MenuKind::PickAgent(agents) => {
            agents.iter().map(|(id, name)| item(name, MenuAction::SendNotesTo(*id))).collect()
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
        MenuKind::AccountSwitch(switch) => {
            let choices: Vec<&AccountProfile> = match (&switch.source, switch.mode) {
                (Some(source), SwitchMode::Continue) => switch_targets(profiles, source),
                _ => profiles.iter().collect(),
            };
            let mut items: Vec<MenuItem> = choices
                .iter()
                .map(|profile| {
                    let is_current = switch.mode == SwitchMode::Continue
                        && switch.source.as_ref().and_then(|source| source.account_id.as_deref())
                            == Some(profile.id.as_str());
                    let action = if is_current {
                        MenuAction::Noop
                    } else if switch.mode == SwitchMode::Continue {
                        MenuAction::ContinueWith { terminal: switch.terminal, id: profile.id.clone() }
                    } else {
                        MenuAction::StartHere { terminal: switch.terminal, id: profile.id.clone() }
                    };
                    MenuItem {
                        label: format!("{} \u{b7} {}", provider_name(&profile.provider).unwrap_or(""), profile.label),
                        meta: item_meta(is_current, profile.runtime).to_string(),
                        danger: false,
                        separated: false,
                        action,
                    }
                })
                .collect();
            let create =
                if switch.mode == SwitchMode::Continue { "New conversation\u{2026}" } else { "New terminal\u{2026}" };
            items.push(MenuItem {
                separated: true,
                ..item(create, MenuAction::OpenNewTerminalMenu(switch.place.clone()))
            });
            items.push(MenuItem { separated: true, ..item("Manage accounts\u{2026}", MenuAction::ManageAccounts) });
            items
        }
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

/// 계정 고르기 메뉴 맨 위 안내 글: 이 메뉴가 무엇을 하는지(고를 계정이 없으면 없다는 사실).
pub fn switch_hint(
    switch: &SwitchMenu,
    profiles: &[AccountProfile],
) -> &'static str {
    let n_choices = match (&switch.source, switch.mode) {
        (Some(source), SwitchMode::Continue) => switch_targets(profiles, source).len(),
        _ => profiles.len(),
    };
    menu_hint(switch.mode, n_choices)
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

/// 터미널 메뉴에 창 옮기기 항목을 넣는다: `Show to the Side` 뒤, 위험 항목(`Close terminal`) 앞이다.
///
/// `window_move`는 `MoveToNewWindow`나 `MoveBack`이고, None이면 항목을 넣지 않는다(터미널이 아닌 메뉴, 전환 중처럼 옮길 수 없는 때).
///
/// Examples
/// --------
/// | 항목                                 | window_move       | 결과                                                            |
/// |--------------------------------------|-------------------|-----------------------------------------------------------------|
/// | Rename, Close                        | MoveToNewWindow   | Rename, Move to New Window, Close                               |
/// | Rename, Show to the Side, Close      | MoveBack          | Rename, Show to the Side, Move Back to Main Window, Close       |
/// | Rename, Close                        | None              | Rename, Close                                                   |
pub fn with_window_move(
    items: Vec<MenuItem>,
    window_move: Option<MenuAction>,
) -> Vec<MenuItem> {
    let mut items = items;
    if let Some(action) = window_move {
        let label = match action {
            MenuAction::MoveBack(_) => "Move Back to Main Window",
            _ => "Move to New Window",
        };
        let at = items.iter().position(|entry| entry.danger).unwrap_or(items.len());
        items.insert(at, item(label, action));
    }
    items
}

/// 분리 창 안에서 연 메뉴의 항목: `Move Back to Main Window`, `Close terminal`.
pub fn detached_menu_items(id: u32) -> Vec<MenuItem> {
    vec![
        item("Move Back to Main Window", MenuAction::MoveBack(id)),
        MenuItem { danger: true, ..item("Close terminal", MenuAction::CloseTerminal(id)) },
    ]
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

/// 사이드바 터미널 행·탭을 왼쪽으로 누른 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowClick {
    /// 첫 누름: 그 터미널을 선택한다.
    Select,
    /// 두 번째 누름(더블클릭): 선택한 채 이름 바꾸기 창을 연다.
    SelectAndRename,
}

/// 같은 자리를 연달아 누른 횟수(`click_count`)가 정하는 동작. 더블클릭만 이름 바꾸기이고, 그 밖의 누름은 선택만 한다.
///
/// Examples
/// --------
/// | click_count | 결과              |
/// |-------------|-------------------|
/// | 1           | `Select`          |
/// | 2           | `SelectAndRename` |
/// | 3           | `Select`          |
pub fn row_click(click_count: usize) -> RowClick {
    if click_count == 2 { RowClick::SelectAndRename } else { RowClick::Select }
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
    fn test_c_d77_the_account_switch_menu_lists_only_matching_accounts_marks_the_current_one_and_ends_with_new_and_manage()
     {
        let account = |id: &str, runtime: Runtime, distribution: Option<&str>| AccountProfile {
            id: id.into(),
            provider: "claude".into(),
            label: id.to_uppercase(),
            runtime,
            wsl_distribution: distribution.map(str::to_string),
        };
        let profiles = vec![
            account("a", Runtime::Native, None),
            account("b", Runtime::Native, None),
            account("w", Runtime::Wsl, Some("Ubuntu")),
        ];
        let source = SwitchSource {
            provider: "claude".into(),
            runtime: Runtime::Native,
            wsl_distribution: None,
            account_id: Some("a".into()),
        };
        let menu =
            SwitchMenu { terminal: 7, mode: SwitchMode::Continue, source: Some(source), place: NewPlace::NewGroup };
        let items = menu_items(&MenuKind::AccountSwitch(menu.clone()), &[], &profiles, None);
        let shown: Vec<(&str, &str)> = items.iter().map(|item| (item.label.as_str(), item.meta.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Claude \u{b7} A", "Current"),
                ("Claude \u{b7} B", ""),
                ("New conversation\u{2026}", ""),
                ("Manage accounts\u{2026}", "")
            ]
        );
        assert_eq!(items[0].action, MenuAction::Noop, "the current account cannot be chosen");
        assert_eq!(items[1].action, MenuAction::ContinueWith { terminal: 7, id: "b".into() });
        assert_eq!(items[2].action, MenuAction::OpenNewTerminalMenu(NewPlace::NewGroup));
        assert_eq!(switch_hint(&menu, &profiles), "Continue this conversation with");
        // 빈 셸 탭에서는 계정 전부를 보이고(WSL 보조 글자 포함) 항목이 `StartHere`다.
        let start = SwitchMenu { mode: SwitchMode::Start, source: None, ..menu };
        let items = menu_items(&MenuKind::AccountSwitch(start.clone()), &[], &profiles, None);
        assert_eq!(items.len(), 5);
        assert_eq!((items[2].label.as_str(), items[2].meta.as_str()), ("Claude \u{b7} W", "WSL"));
        assert_eq!(items[0].action, MenuAction::StartHere { terminal: 7, id: "a".into() });
        assert_eq!(items[3].label, "New terminal\u{2026}");
        assert_eq!(switch_hint(&start, &profiles), "Start in this terminal");
        assert_eq!(switch_hint(&start, &[]), "No accounts added yet");
    }

    #[test]
    fn test_c_files_23_file_rows_offer_open_and_reveal_and_folder_rows_add_new_file_and_new_folder() {
        let reveal = crate::files::menu_model::reveal_label(std::env::consts::OS);
        let labels = |is_dir: bool| {
            let kind = MenuKind::Entry { path: "/p/x".to_string(), is_dir };
            menu_items(&kind, &[], &[], None).into_iter().map(|item| item.label).collect::<Vec<_>>()
        };
        assert_eq!(labels(false), ["Open", "Paste", "Rename...", reveal]);
        assert_eq!(labels(true), ["New File...", "New Folder...", "Paste", "Rename...", reveal]);
        let folder = MenuKind::Entry { path: "/p/x".to_string(), is_dir: true };
        let actions: Vec<MenuAction> =
            menu_items(&folder, &[], &[], None).into_iter().map(|item| item.action).collect();
        assert_eq!(actions[0], MenuAction::NewEntryIn { parent: "/p/x".to_string(), is_dir: false });
        assert_eq!(actions[2], MenuAction::PasteInto { path: "/p/x".to_string(), is_dir: true });
        assert_eq!(actions[3], MenuAction::RenameEntry { path: "/p/x".to_string(), is_dir: true });
        assert_eq!(actions[4], MenuAction::Reveal("/p/x".to_string()));
    }

    #[test]
    fn test_row_menus_name_rename_and_close_with_close_marked_dangerous() {
        let labels = |kind: &MenuKind| {
            menu_items(kind, &[], &[], None).into_iter().map(|item| (item.label, item.danger)).collect::<Vec<_>>()
        };
        let terminal = vec![("Rename terminal".to_string(), false), ("Close terminal".to_string(), true)];
        assert_eq!(labels(&MenuKind::Terminal(3)), terminal);
        assert_eq!(labels(&MenuKind::Group(3)), terminal);
        assert_eq!(
            labels(&MenuKind::Folder("/repo".to_string())),
            vec![
                ("New work terminal here".to_string(), false),
                ("Rename in list".to_string(), false),
                ("Reveal folder".to_string(), false),
                ("Remove from list".to_string(), true)
            ]
        );
    }

    #[test]
    fn test_group_menu_acts_on_the_whole_group_but_renames_its_first_terminal() {
        let items = menu_items(&MenuKind::Group(7), &[], &[], None);
        assert_eq!(items[0].action, MenuAction::RenameTerminal(7));
        assert_eq!(items[1].action, MenuAction::CloseGroup(7));
    }

    #[test]
    fn test_environment_menu_has_one_item_per_environment_with_place_and_default_marks() {
        let environments =
            vec![environment("PowerShell", "Windows", true), environment("WSL \u{b7} Ubuntu", "Linux", false)];
        let items = menu_items(&MenuKind::Environments, &environments, &[], None);
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
        let items = menu_items(&MenuKind::NewTerminal(place.clone()), &[], &profiles, None);
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
        assert_eq!(menu_items(&MenuKind::NewTerminal(NewPlace::NewGroup), &[], &[], None).len(), 2);
    }

    #[test]
    fn test_c_sync_18_show_to_the_side_sits_between_rename_and_close_and_only_when_a_terminal_can_move() {
        let labels = |kind: &MenuKind, beside: Option<u32>| {
            menu_items(kind, &[], &[], beside).into_iter().map(|item| item.label).collect::<Vec<_>>()
        };
        assert_eq!(labels(&MenuKind::Terminal(3), Some(3)), ["Rename terminal", "Show to the Side", "Close terminal"]);
        assert_eq!(labels(&MenuKind::Terminal(3), None), ["Rename terminal", "Close terminal"]);
        assert_eq!(labels(&MenuKind::Group(7), Some(9)), ["Rename terminal", "Show to the Side", "Close terminal"]);
        let items = menu_items(&MenuKind::Group(7), &[], &[], Some(9));
        assert_eq!(items[1].action, MenuAction::ShowBeside(9), "the group moves the terminal it last used");
        assert_eq!(items[2].action, MenuAction::CloseGroup(7));
    }

    #[test]
    fn test_c_sync_19_the_folder_menu_reveals_the_folder_in_the_file_manager() {
        let items = menu_items(&MenuKind::Folder("/repo".to_string()), &[], &[], None);
        assert_eq!(items[2].label, "Reveal folder");
        assert_eq!(items[2].action, MenuAction::Reveal("/repo".to_string()));
        assert!(!items[2].danger);
        assert!(items[3].danger, "removing from the list stays last and dangerous");
    }

    #[test]
    fn test_c_d71_double_click_opens_rename_and_single_click_only_selects() {
        assert_eq!(row_click(1), RowClick::Select);
        assert_eq!(row_click(2), RowClick::SelectAndRename);
        assert_eq!(row_click(3), RowClick::Select, "a third click does not reopen the window");
        assert_eq!(row_click(0), RowClick::Select);
    }

    #[test]
    fn test_c_d68_the_editor_menu_lists_clipboard_items_then_review_notes_and_sends_only_when_notes_exist() {
        let labels =
            |kind: &MenuKind| menu_items(kind, &[], &[], None).into_iter().map(|item| item.label).collect::<Vec<_>>();
        assert_eq!(labels(&MenuKind::Editor { n_notes: 0 }), ["Cut", "Copy", "Paste", "Add Review Note\u{2026}"]);
        assert_eq!(
            labels(&MenuKind::Editor { n_notes: 2 }),
            ["Cut", "Copy", "Paste", "Add Review Note\u{2026}", "Send Review Notes to Agent (2)", "Clear Review Notes"]
        );
        let items = menu_items(&MenuKind::Editor { n_notes: 1 }, &[], &[], None);
        assert!(items[3].separated && !items[2].separated);
        let picks =
            menu_items(&MenuKind::PickAgent(vec![(4, "claude".into()), (9, "terminal 2".into())]), &[], &[], None);
        assert_eq!(picks[1].action, MenuAction::SendNotesTo(9));
    }

    #[test]
    fn test_c_d81_the_window_move_item_sits_after_show_to_the_side_and_before_close() {
        let labels = |items: Vec<MenuItem>| items.into_iter().map(|item| item.label).collect::<Vec<_>>();
        let terminal = |beside: Option<u32>| menu_items(&MenuKind::Terminal(3), &[], &[], beside);
        assert_eq!(
            labels(with_window_move(terminal(None), Some(MenuAction::MoveToNewWindow(3)))),
            ["Rename terminal", "Move to New Window", "Close terminal"]
        );
        assert_eq!(
            labels(with_window_move(terminal(Some(3)), Some(MenuAction::MoveBack(3)))),
            ["Rename terminal", "Show to the Side", "Move Back to Main Window", "Close terminal"]
        );
        assert_eq!(labels(with_window_move(terminal(None), None)), ["Rename terminal", "Close terminal"]);
        let moved = with_window_move(terminal(None), Some(MenuAction::MoveToNewWindow(3)));
        assert_eq!(moved[1].action, MenuAction::MoveToNewWindow(3));
        assert!(!moved[1].danger && moved[2].danger, "only the close item stays dangerous");
        // 묶음 메뉴도 같은 자리에 들어간다.
        let group =
            with_window_move(menu_items(&MenuKind::Group(7), &[], &[], None), Some(MenuAction::MoveToNewWindow(9)));
        assert_eq!(group[1].action, MenuAction::MoveToNewWindow(9));
        assert_eq!(group[2].action, MenuAction::CloseGroup(7));
    }

    #[test]
    fn test_c_d86_a_file_tab_menu_offers_move_to_new_window_and_close() {
        let items = menu_items(&MenuKind::FileTab(2), &[], &[], None);
        let rows: Vec<(&str, bool)> = items.iter().map(|item| (item.label.as_str(), item.danger)).collect();
        assert_eq!(rows, [("Move to New Window", false), ("Close", true)]);
        assert_eq!(items[0].action, MenuAction::MoveFileToNewWindow(2));
        assert_eq!(items[1].action, MenuAction::CloseFileTab(2));
    }

    #[test]
    fn test_c_d84_a_menu_inside_the_detached_window_offers_move_back_and_close() {
        let items = detached_menu_items(4);
        let rows: Vec<(&str, bool)> = items.iter().map(|item| (item.label.as_str(), item.danger)).collect();
        assert_eq!(rows, [("Move Back to Main Window", false), ("Close terminal", true)]);
        assert_eq!(items[0].action, MenuAction::MoveBack(4));
        assert_eq!(items[1].action, MenuAction::CloseTerminal(4));
    }
}
