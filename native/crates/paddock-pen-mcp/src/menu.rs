//! 메뉴로 저장하기: Pen 문서 창의 File 메뉴를 접근성 기능(MSAA)으로 열고 Save 항목을 고른다. 키 입력은 쓰지 않는다.
//!
//! 메뉴의 Save는 그 메뉴가 달린 창의 문서가 아니라 앱의 활성 문서를 저장한다. 그래서 호출자는 대상을 활성 문서로 만든 뒤 부르고,
//! 이 모듈은 메뉴가 열린 뒤 Save를 고르기 직전에 호출자가 준 확인 함수로 활성 문서를 한 번 더 확인한다.
//!
//! ```text
//! 편집 창 찾기 ─(최소화면)→ 활성화 없이 되살림
//!   → (File 메뉴가 아직 없으면) File 단추 기본 동작 → File 메뉴 팝업 창 대기 → 활성 문서 재확인 → Save 항목 기본 동작
//!   → (File 메뉴가 남았으면) 메뉴 닫기 ─(최소화였으면)→ 활성화 없이 다시 최소화
//! ```
//! Save 항목을 고르기 전의 실패는 다시 시도해도 안전하다(아무것도 저장되지 않았다). 고른 뒤의 실패는 다시 시도하지 않는다.
//! 이 순서는 `save_by_menu_with`가 쥐고, 창·접근성 조작은 `MenuSteps`의 구현(Windows의 `driver`)이 맡는다.
use crate::retry::SaveError;

/// Pen 메뉴의 글자.
pub struct MenuLabel;

impl MenuLabel {
    /// 메뉴 막대의 File 단추 이름.
    pub const FILE: &'static str = "File";
    /// File 메뉴의 저장 항목 이름의 첫 단어. 항목 이름 뒤에는 단축키 표기가 붙는다(`Save Ctrl+S`).
    pub const SAVE: &'static str = "Save";
    /// `Save As...` 항목 이름의 둘째 단어 머리. 이 항목이 든 팝업만 File 메뉴로 본다.
    pub const AS: &'static str = "As";
}

/// 메뉴 항목 이름이 저장 항목인지: 첫 단어가 `Save`이고 나머지가 모두 단축키 표기(`+`가 든 낱말)여야 한다.
///
/// Examples
/// --------
/// | 항목 이름                  | 결과  |
/// |----------------------------|-------|
/// | `Save Ctrl+S`              | true  |
/// | `Save`                     | true  |
/// | `Save As...`               | false |
/// | `Save As... Ctrl+Shift+S`  | false |
/// | `Save All`                 | false |
/// | `Saved`                    | false |
/// | `Open Ctrl+O`              | false |
pub fn is_save_item_name(name: &str) -> bool {
    let mut words = name.split_whitespace();
    words.next() == Some(MenuLabel::SAVE) && words.all(|word| word.contains('+'))
}

/// 메뉴 항목 이름이 `Save As...` 항목인지: 첫 단어가 `Save`이고 둘째 단어가 `As`로 시작한다.
///
/// 왜: 문서 창이 띄우는 제목 없는 팝업은 File 메뉴만이 아니다(Edit 메뉴, 드롭다운). 이 항목이 든 팝업만 File 메뉴로 본다.
///
/// Examples
/// --------
/// | 항목 이름                  | 결과  |
/// |----------------------------|-------|
/// | `Save As...`               | true  |
/// | `Save As... Ctrl+Shift+S`  | true  |
/// | `Save Ctrl+S`              | false |
/// | `Undo Ctrl+Z`              | false |
pub fn is_save_as_item_name(name: &str) -> bool {
    let mut words = name.split_whitespace();
    words.next() == Some(MenuLabel::SAVE) && words.next().is_some_and(|word| word.starts_with(MenuLabel::AS))
}

/// 메뉴로 저장하지 못한 까닭.
#[derive(Debug, PartialEq, Eq)]
pub enum MenuFailure {
    /// 제목이 파일 이름인 Pen 편집 창이 없다.
    NoWindow,
    /// 제목이 파일 이름인 Pen 편집 창이 둘 이상이라 어느 창인지 모른다.
    AmbiguousWindows,
    /// 창의 접근성 정보를 읽지 못했다(사유 문장).
    Unreadable(String),
    /// 창에 File 메뉴 단추가 없다.
    NoFileButton,
    /// File 단추를 눌렀는데 File 메뉴 팝업 창이 뜨지 않았다(다른 메뉴·드롭다운이 열려 있던 경우 포함).
    MenuNotOpened,
    /// 메뉴가 열린 사이 활성 문서가 대상이 아니게 됐다(그때의 활성 문서).
    ActiveChanged(String),
    /// File 메뉴의 저장 항목이 하나가 아니다(찾은 개수).
    SaveItemCount(usize),
    /// 저장 항목의 기본 동작이 실패했다(사유 문장). 저장됐는지 알 수 없다.
    SaveInvokeFailed(String),
}

impl MenuFailure {
    /// 다시 시도해도 되는 실패인지. Save 항목을 고르기 전에 났고 잠깐 뒤 풀릴 수 있는 실패만 그렇다.
    ///
    /// Save 항목이 하나도 없는 메뉴는 아직 다 그려지지 않은 메뉴일 수 있어 다시 시도한다. 둘 이상이면 어느 것인지 몰라 다시 해도 같다.
    ///
    /// Examples
    /// --------
    /// | 실패                                                                                       | 결과  |
    /// |--------------------------------------------------------------------------------------------|-------|
    /// | `NoWindow`·`Unreadable`·`NoFileButton`·`MenuNotOpened`·`ActiveChanged`·`SaveItemCount(0)` | true  |
    /// | `AmbiguousWindows`·`SaveItemCount(2)`·`SaveInvokeFailed`                                   | false |
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::NoWindow
                | Self::Unreadable(_)
                | Self::NoFileButton
                | Self::MenuNotOpened
                | Self::ActiveChanged(_)
                | Self::SaveItemCount(0)
        )
    }

    /// 에이전트에게 보일 문장을 붙여 저장 실패로 바꾼다. 다시 시도해도 되는 실패는 `Retryable`, 나머지는 `Fatal`이다.
    pub fn into_save_error(
        self,
        file_name: &str,
    ) -> SaveError {
        let message = match &self {
            Self::NoWindow => format!("Could not find a Pen window titled {file_name}, so Save was not chosen."),
            Self::AmbiguousWindows => format!(
                "There are several Pen windows named {file_name}, so Save was not chosen. Close the other window and call again."
            ),
            Self::Unreadable(reason) => {
                format!("Could not read the menu of the Pen window {file_name} ({reason}), so Save was not chosen.")
            }
            Self::NoFileButton => {
                format!("The Pen window {file_name} has no File menu button, so Save was not chosen.")
            }
            Self::MenuNotOpened => {
                format!(
                    "The File menu of the Pen window {file_name} did not open (another menu or drop-down may be open), so Save was not chosen."
                )
            }
            Self::ActiveChanged(active) => format!(
                "The active document changed ({active}) while the File menu was open, so Save was not chosen: {file_name}"
            ),
            Self::SaveItemCount(n_items) => {
                format!("The File menu of {file_name} has {n_items} Save items instead of one, so Save was not chosen.")
            }
            Self::SaveInvokeFailed(reason) => format!(
                "Choosing Save in the File menu of {file_name} failed ({reason}). It may or may not have been saved, so it was not tried again."
            ),
        };
        if self.is_retryable() { SaveError::Retryable(message) } else { SaveError::Fatal(message) }
    }
}

/// 메뉴로 저장하는 데 드는 창·접근성 조작. 조작의 순서는 `save_by_menu_with`가 정하고, 구현은 각 조작만 한다.
pub trait MenuSteps {
    /// 문서 창이나 메뉴 팝업 창을 가리키는 값.
    type Window: Copy;

    /// 파일의 편집 창 하나를 찾는다(없음·여럿이면 그 실패).
    fn find_window(&self) -> Result<Self::Window, MenuFailure>;
    fn is_minimized(
        &self,
        window: Self::Window,
    ) -> bool;
    /// 최소화된 창을 활성화 없이 되살리고, 메뉴를 누를 수 있을 때까지 기다린다.
    fn show_without_activating(
        &self,
        window: Self::Window,
    );
    fn minimize_without_activating(
        &self,
        window: Self::Window,
    );
    /// File 메뉴 팝업 창을 연다. 이미 열린 File 메뉴가 있으면 그것을 쓴다.
    fn open_file_menu(
        &self,
        window: Self::Window,
    ) -> Result<Self::Window, MenuFailure>;
    /// 팝업의 저장 항목 하나를 고른다. 하나가 아니면 아무것도 고르지 않는다.
    fn choose_save(
        &self,
        popup: Self::Window,
    ) -> Result<(), MenuFailure>;
    /// File 메뉴가 남아 있으면 닫는다. 다른 팝업은 건드리지 않고, 닫지 못해도 결과를 바꾸지 않는다.
    fn close_menu(
        &self,
        window: Self::Window,
    );
}

/// 파일의 편집 창에서 File > Save를 고른다. `Ok`면 Save 항목을 골랐다는 뜻이고, 디스크에 쓰였는지는 호출자가 확인한다.
///
/// 왜: 새 문서 창은 모니터 밖에 열리기도 해 단축키가 듣지 않고, 앞 창을 바꾸는 방식은 운영체제가 막기도 한다. 메뉴 기본 동작은 창이
/// 다른 창 뒤에 있거나 화면 밖에 있어도 듣고, 대상이 이미 활성 문서면 앞 창도 바꾸지 않는다.
///
/// `check_active`는 대상이 지금 활성 문서인지 확인해, 아니면 그때의 활성 문서 표기를 오류로 돌려준다. File 메뉴가 열린 뒤 Save를
/// 고르기 직전에 한 번 부르고, 실패하면 Save를 고르지 않는다. 최소화된 창은 활성화 없이 되살려 메뉴를 열고 끝나면 다시 최소화한다.
/// 최소화 여부는 이 함수에 들어온 때의 상태다: 호출자가 대상을 활성 문서로 만들며 창이 이미 되살아났으면 여기서는 최소화로 보지 않으므로,
/// 그 창을 다시 최소화하는 일은 호출자 몫이다.
/// 실패의 종류는 `MenuFailure`이고, 어느 실패든 File 메뉴는 닫기를 시도한 뒤 돌아온다(닫기가 늦으면 다음 시도가 열린 File 메뉴를 그대로 쓴다).
///
/// Examples
/// --------
/// | 상황                                     | 조작 순서                                             | 결과                  |
/// |------------------------------------------|-------------------------------------------------------|-----------------------|
/// | 보이는 창, 대상이 활성                   | 창 찾기 → 메뉴 열기 → 확인 → Save 고르기 → 메뉴 닫기  | `Ok`                  |
/// | 메뉴가 열린 사이 활성 문서가 바뀜        | 창 찾기 → 메뉴 열기 → 확인 → 메뉴 닫기                | `Err(ActiveChanged)`  |
/// | 최소화된 창                              | 되살리기 → … → 메뉴 닫기 → 다시 최소화                | 위와 같다             |
pub fn save_by_menu_with<M: MenuSteps>(
    steps: &M,
    check_active: impl Fn() -> Result<(), String>,
) -> Result<(), MenuFailure> {
    let window = steps.find_window()?;
    let was_minimized = steps.is_minimized(window);
    if was_minimized {
        steps.show_without_activating(window);
    }
    let chosen = steps.open_file_menu(window).and_then(|popup| {
        check_active().map_err(MenuFailure::ActiveChanged)?;
        steps.choose_save(popup)
    });
    steps.close_menu(window);
    if was_minimized {
        steps.minimize_without_activating(window);
    }
    chosen
}

#[cfg(windows)]
pub use driver::save_by_menu;

/// 실제 창을 다루는 부분(Windows 전용).
#[cfg(windows)]
mod driver {
    use std::time::Instant;

    use ::windows::Win32::UI::Accessibility::{ROLE_SYSTEM_BUTTONMENU, ROLE_SYSTEM_MENUITEM};

    use super::{MenuFailure, MenuLabel, MenuSteps, is_save_as_item_name, is_save_item_name, save_by_menu_with};
    use crate::accessible::{self, ComApartment, Element};
    use crate::limits::Limits;
    use crate::titles::Found;
    use crate::windows::{self, Window};

    /// 메뉴 막대의 File 단추.
    fn file_button(window: Window) -> Result<Element, MenuFailure> {
        accessible::elements(window)
            .map_err(MenuFailure::Unreadable)?
            .into_iter()
            .find(|element| element.role == ROLE_SYSTEM_BUTTONMENU && element.name == MenuLabel::FILE)
            .ok_or(MenuFailure::NoFileButton)
    }

    /// 팝업 창이 File 메뉴인지: `Save As...` 항목이 들어 있어야 한다. 접근성 정보를 못 읽으면 아니다.
    fn is_file_menu(popup: Window) -> bool {
        accessible::elements(popup).is_ok_and(|items| {
            items.iter().any(|element| element.role == ROLE_SYSTEM_MENUITEM && is_save_as_item_name(&element.name))
        })
    }

    /// 문서 창이 띄운 팝업 가운데 File 메뉴. 다른 메뉴·드롭다운 팝업만 있으면 None이다.
    fn file_menu_popup(window: Window) -> Option<Window> {
        windows::menu_popups(window).into_iter().find(|popup| is_file_menu(*popup))
    }

    /// File 메뉴 팝업 창이 뜨기를 `MENU_OPEN_DEADLINE`까지 기다린다.
    fn wait_for_file_menu(window: Window) -> Option<Window> {
        let deadline = Instant::now() + Limits::MENU_OPEN_DEADLINE;
        let mut popup = file_menu_popup(window);
        while popup.is_none() && Instant::now() < deadline {
            std::thread::sleep(Limits::WINDOW_INTERVAL);
            popup = file_menu_popup(window);
        }
        popup
    }

    /// 이 파일의 Pen 편집 창을 다루는 실제 조작.
    struct PenMenu<'a> {
        file_name: &'a str,
    }

    impl MenuSteps for PenMenu<'_> {
        type Window = Window;

        fn find_window(&self) -> Result<Window, MenuFailure> {
            match windows::wait_for_editor_window(self.file_name) {
                Found::None => Err(MenuFailure::NoWindow),
                Found::Ambiguous => Err(MenuFailure::AmbiguousWindows),
                Found::One(window) => Ok(window),
            }
        }

        fn is_minimized(
            &self,
            window: Window,
        ) -> bool {
            windows::is_minimized(window)
        }

        fn show_without_activating(
            &self,
            window: Window,
        ) {
            windows::show_without_activating(window);
            std::thread::sleep(Limits::RESTORE_SETTLE);
        }

        fn minimize_without_activating(
            &self,
            window: Window,
        ) {
            windows::minimize_without_activating(window);
        }

        /// 앞 시도가 남긴 File 메뉴는 그대로 쓴다. File 메뉴가 아닌 팝업(Edit 메뉴·드롭다운)이 열려 있어도 File 단추를 누르고,
        /// File 메뉴가 뜨지 않으면 다시 시도할 수 있는 실패(`MenuNotOpened`)다.
        fn open_file_menu(
            &self,
            window: Window,
        ) -> Result<Window, MenuFailure> {
            match file_menu_popup(window) {
                Some(popup) => Ok(popup),
                None => {
                    file_button(window)?.do_default_action().map_err(|_| MenuFailure::MenuNotOpened)?;
                    wait_for_file_menu(window).ok_or(MenuFailure::MenuNotOpened)
                }
            }
        }

        fn choose_save(
            &self,
            popup: Window,
        ) -> Result<(), MenuFailure> {
            let items = accessible::elements(popup).map_err(MenuFailure::Unreadable)?;
            let saves: Vec<&Element> = items
                .iter()
                .filter(|element| element.role == ROLE_SYSTEM_MENUITEM && is_save_item_name(&element.name))
                .collect();
            match saves.as_slice() {
                [only] => only.do_default_action().map_err(MenuFailure::SaveInvokeFailed),
                others => Err(MenuFailure::SaveItemCount(others.len())),
            }
        }

        /// File 메뉴가 남아 있으면 File 단추를 한 번 더 눌러 닫고, 사라지기를 `MENU_CLOSE_DEADLINE`까지 기다린다. File 메뉴가 아닌
        /// 팝업은 그대로 둔다: 그때 File 단추를 누르면 File 메뉴를 새로 열어 남기게 된다.
        fn close_menu(
            &self,
            window: Window,
        ) {
            if file_menu_popup(window).is_some() {
                if let Ok(button) = file_button(window) {
                    let _ = button.do_default_action();
                }
                let deadline = Instant::now() + Limits::MENU_CLOSE_DEADLINE;
                while file_menu_popup(window).is_some() && Instant::now() < deadline {
                    std::thread::sleep(Limits::WINDOW_INTERVAL);
                }
            }
        }
    }

    /// 파일 이름이 제목인 Pen 편집 창에서 File > Save를 고른다(`save_by_menu_with`를 실제 창에 적용한다).
    pub fn save_by_menu(
        file_name: &str,
        check_active: impl Fn() -> Result<(), String>,
    ) -> Result<(), MenuFailure> {
        // 접근성 객체는 모두 아래 조작 안에서 버려지므로 COM 구간이 가장 오래 산다.
        let _apartment = ComApartment::enter();
        save_by_menu_with(&PenMenu { file_name }, check_active)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn test_menu_1_only_the_plain_save_item_matches() {
        assert!(is_save_item_name("Save Ctrl+S"));
        assert!(is_save_item_name("Save"));
        assert!(is_save_item_name("Save\tCtrl+S"));
        assert!(is_save_item_name("  Save  Ctrl+S "));
        assert!(!is_save_item_name("Save As..."));
        assert!(!is_save_item_name("Save As... Ctrl+Shift+S"));
        assert!(!is_save_item_name("Save All"));
        assert!(!is_save_item_name("Saved"));
        assert!(!is_save_item_name("save Ctrl+S"));
        assert!(!is_save_item_name("Open Ctrl+O"));
        assert!(!is_save_item_name(""));
    }

    #[test]
    fn test_menu_2_failures_before_save_is_chosen_are_retryable_and_the_rest_are_fatal() {
        let retryable = [
            MenuFailure::NoWindow,
            MenuFailure::Unreadable("E_FAIL".into()),
            MenuFailure::NoFileButton,
            MenuFailure::MenuNotOpened,
            MenuFailure::ActiveChanged("/C:/w/b.pen".into()),
            MenuFailure::SaveItemCount(0),
        ];
        for failure in retryable {
            assert!(matches!(failure.into_save_error("a.pen"), SaveError::Retryable(_)));
        }
        let fatal =
            [MenuFailure::AmbiguousWindows, MenuFailure::SaveItemCount(2), MenuFailure::SaveInvokeFailed("x".into())];
        for failure in fatal {
            assert!(matches!(failure.into_save_error("a.pen"), SaveError::Fatal(_)));
        }
    }

    #[test]
    fn test_menu_3_failure_messages_name_the_file_and_the_cause() {
        let SaveError::Retryable(message) = MenuFailure::ActiveChanged("/C:/w/b.pen".into()).into_save_error("a.pen")
        else {
            panic!("retryable expected")
        };
        assert!(message.contains("/C:/w/b.pen") && message.ends_with("a.pen"), "{message}");
        let SaveError::Fatal(message) = MenuFailure::SaveItemCount(2).into_save_error("a.pen") else {
            panic!("fatal expected")
        };
        assert!(message.contains("has 2 Save items"), "{message}");
    }

    #[test]
    fn test_menu_4_only_a_popup_with_save_as_is_the_file_menu() {
        assert!(is_save_as_item_name("Save As..."));
        assert!(is_save_as_item_name("Save As... Ctrl+Shift+S"));
        assert!(!is_save_as_item_name("Save Ctrl+S"));
        assert!(!is_save_as_item_name("Save"));
        assert!(!is_save_as_item_name("Undo Ctrl+Z"));
        assert!(!is_save_as_item_name(""));
    }

    /// 조작마다 이름을 남기는 가짜 창. 메뉴 열기·Save 고르기의 실패는 필드로 정한다(None이면 성공).
    struct FakeMenu {
        log: RefCell<Vec<&'static str>>,
        is_minimized: bool,
        open_failure: RefCell<Option<MenuFailure>>,
        choose_failure: RefCell<Option<MenuFailure>>,
    }

    impl FakeMenu {
        fn new() -> Self {
            Self {
                log: RefCell::new(Vec::new()),
                is_minimized: false,
                open_failure: RefCell::new(None),
                choose_failure: RefCell::new(None),
            }
        }

        fn note(
            &self,
            step: &'static str,
        ) {
            self.log.borrow_mut().push(step);
        }

        /// 가짜 창과 주어진 활성 문서 확인으로 메뉴 저장을 돌리고, 결과와 조작 순서를 돌려준다.
        fn run(
            &self,
            check: Result<(), String>,
        ) -> (Result<(), MenuFailure>, Vec<&'static str>) {
            let outcome = save_by_menu_with(self, || {
                self.note("check_active");
                check.clone()
            });
            (outcome, self.log.borrow().clone())
        }
    }

    impl MenuSteps for FakeMenu {
        type Window = ();

        fn find_window(&self) -> Result<(), MenuFailure> {
            self.note("find_window");
            Ok(())
        }

        fn is_minimized(
            &self,
            _window: (),
        ) -> bool {
            self.is_minimized
        }

        fn show_without_activating(
            &self,
            _window: (),
        ) {
            self.note("show");
        }

        fn minimize_without_activating(
            &self,
            _window: (),
        ) {
            self.note("minimize");
        }

        fn open_file_menu(
            &self,
            _window: (),
        ) -> Result<(), MenuFailure> {
            self.note("open_file_menu");
            self.open_failure.borrow_mut().take().map_or(Ok(()), Err)
        }

        fn choose_save(
            &self,
            _popup: (),
        ) -> Result<(), MenuFailure> {
            self.note("choose_save");
            self.choose_failure.borrow_mut().take().map_or(Ok(()), Err)
        }

        fn close_menu(
            &self,
            _window: (),
        ) {
            self.note("close_menu");
        }
    }

    #[test]
    fn test_menu_5_the_active_document_is_checked_after_the_menu_opens_and_right_before_save() {
        let (outcome, log) = FakeMenu::new().run(Ok(()));
        assert_eq!(outcome, Ok(()));
        assert_eq!(log, ["find_window", "open_file_menu", "check_active", "choose_save", "close_menu"]);
    }

    #[test]
    fn test_menu_6_a_failed_check_closes_the_menu_without_choosing_save_and_can_be_retried() {
        let (outcome, log) = FakeMenu::new().run(Err("/C:/w/b.pen".into()));
        assert_eq!(outcome, Err(MenuFailure::ActiveChanged("/C:/w/b.pen".into())));
        assert_eq!(log, ["find_window", "open_file_menu", "check_active", "close_menu"]);
        assert!(outcome.unwrap_err().is_retryable());
    }

    #[test]
    fn test_menu_7_a_menu_that_did_not_open_is_neither_checked_nor_chosen() {
        let fake = FakeMenu { open_failure: RefCell::new(Some(MenuFailure::MenuNotOpened)), ..FakeMenu::new() };
        let (outcome, log) = fake.run(Ok(()));
        assert_eq!(outcome, Err(MenuFailure::MenuNotOpened));
        assert_eq!(log, ["find_window", "open_file_menu", "close_menu"]);
    }

    #[test]
    fn test_menu_8_a_failure_after_save_is_chosen_is_fatal() {
        let fake = FakeMenu {
            choose_failure: RefCell::new(Some(MenuFailure::SaveInvokeFailed("E_FAIL".into()))),
            ..FakeMenu::new()
        };
        let (outcome, log) = fake.run(Ok(()));
        assert!(matches!(outcome.unwrap_err().into_save_error("a.pen"), SaveError::Fatal(_)));
        assert_eq!(log.last(), Some(&"close_menu"));
    }

    #[test]
    fn test_menu_9_a_minimized_window_is_shown_first_and_minimized_again_after_the_menu_closes() {
        let fake = FakeMenu { is_minimized: true, ..FakeMenu::new() };
        let (outcome, log) = fake.run(Ok(()));
        assert_eq!(outcome, Ok(()));
        assert_eq!(
            log,
            ["find_window", "show", "open_file_menu", "check_active", "choose_save", "close_menu", "minimize"]
        );
    }
}
