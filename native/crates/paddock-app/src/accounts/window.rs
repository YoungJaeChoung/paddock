//! Accounts 창과 계정 터미널 열기의 창 동작: 창(`WorkWindow`)이 쥔 상태를 바꾸고 계정 목록 파일·터미널 열기에 이어 준다.
//!
//! 목록 파일 쓰기는 `store`, 상태 규칙은 `model`, 그림은 `dialog`가 맡는다. 계정 폴더 안은 건드리지 않는다.
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext, Context, Entity, Window};
use paddock_core::account_profile::NewAccount;
use paddock_core::agent_account::AccountProfile;
use paddock_core::work_model::NewPlace;

use super::launch::account_want;
use super::model::{AccountsModel, Editing, label_error, runtime_choices};
use super::store;
use crate::settings::Settings;
use crate::shell_choice::available_environments;
use crate::window_view::{Overlay, WorkWindow};

/// Accounts 창이 쥔 상태: 모델(창이 닫혀 있으면 None)과 입력칸 둘.
#[derive(Default)]
pub struct AccountsPanel {
    pub model: Option<AccountsModel>,
    /// 추가 양식의 이름 입력칸.
    pub name_input: Option<Entity<InputState>>,
    /// 이름 바꾸기 입력칸.
    pub rename_input: Option<Entity<InputState>>,
}

/// 홈 폴더.
fn home_dir() -> String {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default()
}

/// 이 컴퓨터의 WSL 배포판 이름들(실행 환경 목록에서 WSL 항목만).
fn wsl_distributions() -> Vec<String> {
    available_environments().iter().filter_map(|environment| environment.wsl_distribution.clone()).collect()
}

impl WorkWindow {
    /// 등록 계정 목록(표시용). 읽지 못하면 비어 있다.
    pub(crate) fn registered_accounts(&self) -> Vec<AccountProfile> {
        store::registered_profiles()
    }

    /// 계정 `id`의 터미널을 `place`에 연다. 폴더가 없거나 다른 환경의 계정이면 상태 줄에 이유를 보인다.
    pub(crate) fn open_account_terminal(
        &mut self,
        id: &str,
        place: NewPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.account_terminal_request(id, &place) {
            Ok(want) => self.open_terminal(want, place, window, cx),
            Err(message) => {
                self.status = Some(message);
                cx.notify();
            }
        }
    }

    /// 계정 `id`의 터미널을 `place`의 폴더에서 열기 위한 요청. 폴더는 작업 폴더이거나, 아니면 선택한 터미널의 현재 폴더이거나 홈이다.
    fn account_terminal_request(
        &self,
        id: &str,
        place: &NewPlace,
    ) -> Result<crate::terminal::session::Want, String> {
        let profile = self
            .registered_accounts()
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| "This account is no longer available. Refresh the account list.".to_string())?;
        let folder = match place {
            NewPlace::Folder(key) => Some(key.clone()),
            NewPlace::Group(_) | NewPlace::NewGroup => self.selected_terminal_cwd(),
        };
        account_want(
            &profile,
            folder.as_deref(),
            &home_dir(),
            Settings::load().agent_permissions(),
            cfg!(windows),
            None,
        )
    }

    /// 선택한 터미널이 데몬에서 보고받은 현재 폴더.
    fn selected_terminal_cwd(&self) -> Option<String> {
        let tab = self.workspace.selected().and_then(|id| self.workspace.tab(id))?;
        let state = tab.content.session.grid.shared.state.lock().ok()?.clone()?;
        state.cwd
    }

    /// Accounts 창을 연다. 목록을 읽지 못하면 다시 시도만 보이는 창이 열린다.
    pub(crate) fn open_accounts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settle_composition(window, cx);
        let loaded = store::read_profiles().ok();
        let model = AccountsModel::new(loaded, runtime_choices(cfg!(windows), &wsl_distributions()));
        let initial = model.suggested();
        let show_form = model.form_visible;
        self.accounts.model = Some(model);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) && this.accounts.name_input.as_ref() == Some(input) {
                this.accounts_add(window, cx);
            }
        })
        .detach();
        self.accounts.name_input = Some(input.clone());
        self.accounts.rename_input = None;
        self.open_overlay(Overlay::Accounts, window, cx);
        if show_form {
            input.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        }
    }

    /// 창을 닫을 때 상태를 비운다.
    pub(crate) fn clear_accounts(&mut self) {
        self.accounts = Default::default();
    }

    /// 목록을 다시 읽는다(읽기 실패 뒤 다시 시도).
    pub(crate) fn accounts_retry(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_accounts(window, cx);
    }

    fn accounts_error(
        &mut self,
        message: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(model) = self.accounts.model.as_mut() {
            model.error = Some(message);
        }
        cx.notify();
    }

    /// 양식의 이름 입력칸 값을 바꾼다.
    fn set_name_text(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.accounts.name_input.clone() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    pub(crate) fn accounts_set_provider(
        &mut self,
        provider: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = self.accounts.name_input.as_ref().map(|input| input.read(cx).value().to_string());
        if let Some(model) = self.accounts.model.as_mut() {
            // 추천 이름을 그대로 두었으면 새 도구의 추천 이름으로 따라 바꾼다.
            let was_suggested = typed.as_deref() == Some(model.suggested().as_str());
            model.provider = provider;
            let next = model.suggested();
            if was_suggested {
                self.set_name_text(next, window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn accounts_set_runtime(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(model) = self.accounts.model.as_mut() {
            model.choice_index = index;
        }
        cx.notify();
    }

    pub(crate) fn accounts_show_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggestion = self.accounts.model.as_mut().map(|model| {
            model.form_visible = true;
            model.error = None;
            model.editing = Editing::None;
            model.suggested()
        });
        if let Some(text) = suggestion {
            self.set_name_text(text, window, cx);
            if let Some(input) = self.accounts.name_input.clone() {
                input.update(cx, |state, cx| {
                    state.focus(window, cx);
                    state.select_all(window, cx);
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn accounts_cancel_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(model) = self.accounts.model.as_mut() {
            model.form_visible = false;
            model.error = None;
        }
        window.focus(&self.overlay_focus, cx);
        cx.notify();
    }

    /// 양식으로 계정을 더하고, 더한 계정의 터미널을 연다(`Add & Open`).
    pub(crate) fn accounts_add(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = self.accounts.name_input.as_ref().map(|input| input.read(cx).value().to_string());
        if let (Some(model), Some(typed)) = (self.accounts.model.as_ref(), typed) {
            // 입력 방식이 마지막 글자의 확정을 창이 닫힌 뒤 알려도 터미널로 가지 않게 한다.
            self.already_sent.note_commit_elsewhere(&typed);
            let choice = model.choice().clone();
            let added = match label_error(&model.profiles, &typed, model.provider, None) {
                Some(message) => Err(message.to_string()),
                None => store::add(&NewAccount {
                    provider: model.provider,
                    label: Some(&typed),
                    runtime: choice.runtime,
                    wsl_distribution: choice.wsl_distribution.as_deref(),
                })
                .map_err(|error| error.to_string()),
            };
            match added {
                Ok(profile) => {
                    if let Some(model) = self.accounts.model.as_mut() {
                        model.profiles.push(profile.clone());
                        model.form_visible = false;
                        model.error = None;
                    }
                    self.accounts_open(&profile.id, window, cx);
                }
                Err(message) => self.accounts_error(message, cx),
            }
        }
    }

    /// 계정의 터미널을 지금 자리에 열고, 열렸으면 창을 닫는다.
    pub(crate) fn accounts_open(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let place = self.workspace.new_place();
        match self.account_terminal_request(id, &place) {
            Ok(want) => {
                self.cancel_overlay(window, cx);
                self.open_terminal(want, place, window, cx);
            }
            Err(message) => self.accounts_error(message, cx),
        }
    }

    pub(crate) fn accounts_begin_rename(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self
            .accounts
            .model
            .as_ref()
            .and_then(|model| model.profiles.iter().find(|profile| profile.id == id))
            .map(|profile| profile.label.clone());
        if let Some(label) = label {
            if let Some(model) = self.accounts.model.as_mut() {
                model.editing = Editing::Rename(id.to_string());
                model.error = None;
            }
            self.settle_composition(window, cx);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(label));
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && this.accounts.rename_input.as_ref() == Some(input)
                    && let Some(Editing::Rename(id)) = this.accounts.model.as_ref().map(|model| model.editing.clone())
                {
                    this.accounts_save_rename(&id, window, cx);
                }
            })
            .detach();
            self.accounts.rename_input = Some(input.clone());
            input.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        }
        cx.notify();
    }

    pub(crate) fn accounts_save_rename(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = self.accounts.rename_input.as_ref().map(|input| input.read(cx).value().to_string());
        if let (Some(model), Some(typed)) = (self.accounts.model.as_ref(), typed) {
            self.already_sent.note_commit_elsewhere(&typed);
            let provider =
                model.profiles.iter().find(|profile| profile.id == id).map(|profile| profile.provider.clone());
            let invalid =
                provider.as_deref().and_then(|provider| label_error(&model.profiles, &typed, provider, Some(id)));
            let renamed = match invalid {
                Some(message) => Err(message.to_string()),
                None => store::rename(id, &typed).map_err(|error| error.to_string()),
            };
            match renamed {
                Ok(renamed) => {
                    if let Some(model) = self.accounts.model.as_mut() {
                        for profile in model.profiles.iter_mut().filter(|profile| profile.id == renamed.id) {
                            *profile = renamed.clone();
                        }
                        model.editing = Editing::None;
                        model.error = None;
                    }
                    self.accounts.rename_input = None;
                    window.focus(&self.overlay_focus, cx);
                    cx.notify();
                }
                Err(message) => self.accounts_error(message, cx),
            }
        }
    }

    pub(crate) fn accounts_begin_remove(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(model) = self.accounts.model.as_mut() {
            model.editing = Editing::Remove(id.to_string());
            model.error = None;
        }
        self.accounts.rename_input = None;
        window.focus(&self.overlay_focus, cx);
        cx.notify();
    }

    pub(crate) fn accounts_confirm_remove(
        &mut self,
        id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match store::remove(id) {
            Ok(()) => {
                if let Some(model) = self.accounts.model.as_mut() {
                    let rest: Vec<AccountProfile> =
                        model.profiles.iter().filter(|profile| profile.id != id).cloned().collect();
                    model.replace_profiles(rest);
                    model.error = None;
                }
                cx.notify();
            }
            Err(error) => self.accounts_error(error.to_string(), cx),
        }
    }

    /// 이름 바꾸기·제거 확인을 접는다.
    pub(crate) fn accounts_cancel_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(model) = self.accounts.model.as_mut() {
            model.editing = Editing::None;
            model.error = None;
        }
        self.accounts.rename_input = None;
        window.focus(&self.overlay_focus, cx);
        cx.notify();
    }

    /// 계정이 하나라도 있는지(＋가 메뉴를 열지 바로 열지 정한다).
    pub(crate) fn has_registered_accounts(&self) -> bool {
        !self.registered_accounts().is_empty()
    }

    /// 계정 고르기에 보일 이름: 선택한 터미널에서 실행 중인 도구의 계정, 없으면 `Accounts`.
    pub(crate) fn picker_label(&self) -> String {
        let state = self
            .workspace
            .selected()
            .and_then(|id| self.workspace.tab(id))
            .and_then(|tab| tab.content.session.grid.shared.state.lock().ok().and_then(|state| state.clone()));
        let label = crate::agent_view::account_label(state.as_ref());
        if label.is_empty() { paddock_core::theme::account_picker::EMPTY_LABEL.to_string() } else { label }
    }

    /// 새 터미널 고르기 메뉴를 연다. 등록 계정이 없는 ＋는 메뉴 없이 일반 터미널을 바로 연다.
    pub(crate) fn open_new_terminal_menu(
        &mut self,
        place: NewPlace,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        always: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if always || self.has_registered_accounts() {
            self.open_menu(crate::work::menu_model::MenuKind::NewTerminal(place), position, window, cx);
        } else {
            self.open_plain_terminal(place, window, cx);
        }
    }

    /// 계정 없는 일반 터미널을 `place`에 연다.
    pub(crate) fn open_plain_terminal(
        &mut self,
        place: NewPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match place {
            NewPlace::Folder(key) => self.new_terminal_in_folder(key, window, cx),
            NewPlace::Group(root) => {
                self.open_terminal(crate::startup::new_terminal_want(), NewPlace::Group(root), window, cx)
            }
            NewPlace::NewGroup => self.new_terminal_in_new_group(None, window, cx),
        }
    }
}
