//! Accounts 창의 상태와 문구 규칙: 목록·추가 양식·이름 바꾸기·제거 확인 중 무엇이 열려 있는지, 기본 이름, 이름 검사, 실행 환경 선택지.
//!
//! 화면을 모르는 순수 규칙이라 창 없이 시험한다. 그리기는 `dialog`가, 파일 쓰기는 `store`가 맡는다.
use paddock_core::account_profile::{checked_label, provider_name};
use paddock_core::agent_account::{AccountProfile, Runtime};

/// 양식에서 고를 수 있는 도구.
pub const PROVIDERS: [&str; 2] = ["claude", "codex"];

/// 이 컴퓨터에서 계정을 돌릴 곳 하나(이 컴퓨터 또는 WSL 배포판).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeChoice {
    pub label: String,
    pub runtime: Runtime,
    pub wsl_distribution: Option<String>,
}

/// 양식의 "Run in" 선택지: 첫 항목은 Windows에서 `Windows`, 그 밖의 OS에서 `This device`(기존 앱과 같다). Windows에서는 설치된 WSL 배포판마다 `WSL · 이름`이 더해진다.
///
/// Examples
/// --------
/// | is_windows | 배포판            | 선택지                                    |
/// |------------|-------------------|-------------------------------------------|
/// | false      | (무관)            | `This device`                             |
/// | true       | Ubuntu, Debian    | `Windows`, `WSL · Ubuntu`, `WSL · Debian` |
/// | true       | (없음)            | `Windows`                                 |
pub fn runtime_choices(
    is_windows: bool,
    distributions: &[String],
) -> Vec<RuntimeChoice> {
    let first = if is_windows { "Windows" } else { "This device" };
    let mut choices =
        vec![RuntimeChoice { label: first.to_string(), runtime: Runtime::Native, wsl_distribution: None }];
    if is_windows {
        choices.extend(distributions.iter().map(|name| RuntimeChoice {
            label: format!("WSL \u{b7} {name}"),
            runtime: Runtime::Wsl,
            wsl_distribution: Some(name.clone()),
        }));
    }
    choices
}

/// 목록 행의 실행 환경 글자(`This device`·`WSL · Ubuntu`).
pub fn runtime_text(profile: &AccountProfile) -> String {
    match (profile.runtime, profile.wsl_distribution.as_deref()) {
        (Runtime::Native, _) => "This device".to_string(),
        (Runtime::Wsl, Some(distribution)) => format!("WSL \u{b7} {distribution}"),
        (Runtime::Wsl, None) => "WSL".to_string(),
    }
}

/// 행 제목(`Claude · Main`).
pub fn title_of(profile: &AccountProfile) -> String {
    format!("{} \u{b7} {}", provider_name(&profile.provider).unwrap_or(""), profile.label)
}

/// 같은 도구에서 겹치지 않는 추천 이름: `Main`, 있으면 `Extra`, 그다음 `Account 3`….
///
/// Examples
/// --------
/// | 이미 있는 이름(claude) | 결과        |
/// |------------------------|-------------|
/// | (없음)                 | `Main`      |
/// | Main                   | `Extra`     |
/// | Main, Extra            | `Account 3` |
pub fn suggest_label(
    profiles: &[AccountProfile],
    provider: &str,
) -> String {
    let taken = |label: &str| {
        profiles.iter().any(|item| item.provider == provider && item.label.to_lowercase() == label.to_lowercase())
    };
    let mut label = "Main".to_string();
    let mut n_accounts = 2;
    while taken(&label) {
        label = if n_accounts == 2 { "Extra".to_string() } else { format!("Account {n_accounts}") };
        n_accounts += 1;
    }
    label
}

/// 이름 입력의 문제를 알리는 문장. 괜찮으면 None. `id`가 있으면 그 계정 자신과의 겹침은 무시한다(이름 바꾸기).
pub fn label_error(
    profiles: &[AccountProfile],
    label: &str,
    provider: &str,
    id: Option<&str>,
) -> Option<&'static str> {
    match checked_label(label) {
        Err(_) => Some("Enter an account name between 1 and 64 characters."),
        Ok(label) => profiles
            .iter()
            .any(|item| {
                Some(item.id.as_str()) != id
                    && item.provider == provider
                    && item.label.to_lowercase() == label.to_lowercase()
            })
            .then_some("This tool already has an account with that name. Choose another name."),
    }
}

/// 창에 열려 있는 것.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Editing {
    /// 목록만 보인다.
    None,
    /// 계정 `id`의 이름을 입력칸으로 바꾸는 중이다.
    Rename(String),
    /// 계정 `id`를 목록에서 뺄지 묻는 중이다.
    Remove(String),
}

/// Accounts 창의 상태.
#[derive(Clone, Debug)]
pub struct AccountsModel {
    pub profiles: Vec<AccountProfile>,
    /// 저장된 목록을 읽지 못했다. 이때는 추가·수정을 막고 다시 시도만 보인다.
    pub load_failed: bool,
    pub form_visible: bool,
    pub provider: &'static str,
    pub choices: Vec<RuntimeChoice>,
    pub choice_index: usize,
    pub editing: Editing,
    pub error: Option<String>,
}

impl AccountsModel {
    /// 목록을 읽은 결과로 창 상태를 만든다. 목록이 비었으면 추가 양식을 바로 보인다.
    pub fn new(
        loaded: Option<Vec<AccountProfile>>,
        choices: Vec<RuntimeChoice>,
    ) -> Self {
        let load_failed = loaded.is_none();
        let profiles = loaded.unwrap_or_default();
        Self {
            form_visible: profiles.is_empty() && !load_failed,
            profiles,
            load_failed,
            provider: PROVIDERS[0],
            choices,
            choice_index: 0,
            editing: Editing::None,
            error: load_failed.then(|| "Could not load your accounts. Try again.".to_string()),
        }
    }

    /// 지금 양식의 추천 이름.
    pub fn suggested(&self) -> String {
        suggest_label(&self.profiles, self.provider)
    }

    /// 지금 고른 실행 환경.
    pub fn choice(&self) -> &RuntimeChoice {
        &self.choices[self.choice_index.min(self.choices.len() - 1)]
    }

    /// 목록이 바뀐 뒤 상태를 정리한다: 비었으면 양식을 보이고, 열려 있던 수정은 닫는다.
    pub fn replace_profiles(
        &mut self,
        profiles: Vec<AccountProfile>,
    ) {
        self.form_visible = profiles.is_empty();
        self.profiles = profiles;
        self.editing = Editing::None;
    }
}

#[cfg(test)]
mod tests {
    /// 크기는 theme 토큰으로만 쓴다: 대화상자 소스에 `zpx(숫자)`가 없어야 한다.
    #[test]
    fn test_c_theme_accounts_dialog_uses_size_tokens_only() {
        let source = include_str!("dialog.rs");
        let literal = source.lines().find(|line| {
            line.split("zpx(")
                .skip(1)
                .any(|rest| rest.chars().next().is_some_and(|character| character.is_ascii_digit()))
        });
        assert_eq!(literal, None);
    }

    use super::*;

    fn profile(
        provider: &str,
        label: &str,
    ) -> AccountProfile {
        AccountProfile {
            id: format!("id-{provider}-{label}"),
            provider: provider.to_string(),
            label: label.to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        }
    }

    #[test]
    fn test_suggested_names_count_up_per_tool() {
        let mut profiles = vec![];
        assert_eq!(suggest_label(&profiles, "claude"), "Main");
        profiles.push(profile("claude", "main"));
        assert_eq!(suggest_label(&profiles, "claude"), "Extra");
        assert_eq!(suggest_label(&profiles, "codex"), "Main");
        profiles.push(profile("claude", "Extra"));
        assert_eq!(suggest_label(&profiles, "claude"), "Account 3");
    }

    #[test]
    fn test_label_errors_cover_blank_long_control_and_duplicate_names_but_not_the_accounts_own_name() {
        let profiles = vec![profile("claude", "Main")];
        let long = "a".repeat(65);
        for label in ["", "  ", "a\nb", long.as_str()] {
            assert_eq!(
                label_error(&profiles, label, "claude", None),
                Some("Enter an account name between 1 and 64 characters.")
            );
        }
        assert!(label_error(&profiles, " MAIN ", "claude", None).unwrap().contains("already has an account"));
        assert_eq!(label_error(&profiles, "Main", "codex", None), None);
        assert_eq!(label_error(&profiles, "Main", "claude", Some("id-claude-Main")), None);
    }

    #[test]
    fn test_runtime_choices_add_wsl_distributions_only_on_windows_and_rows_say_where_they_run() {
        let distributions = vec!["Ubuntu".to_string(), "Debian".to_string()];
        assert_eq!(runtime_choices(false, &distributions).len(), 1);
        assert_eq!(runtime_choices(false, &distributions)[0].label, "This device");
        let windows = runtime_choices(true, &distributions);
        let labels: Vec<&str> = windows.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Windows", "WSL \u{b7} Ubuntu", "WSL \u{b7} Debian"],
            "the old app says Windows for the first choice on Windows"
        );
        assert_eq!(windows[2].wsl_distribution.as_deref(), Some("Debian"));
        let wsl = AccountProfile {
            runtime: Runtime::Wsl,
            wsl_distribution: Some("Ubuntu".into()),
            ..profile("codex", "Work")
        };
        assert_eq!((runtime_text(&wsl), title_of(&wsl)), ("WSL \u{b7} Ubuntu".to_string(), "Codex \u{b7} Work".into()));
        assert_eq!(runtime_text(&profile("claude", "Main")), "This device");
    }

    #[test]
    fn test_the_window_opens_on_the_form_when_there_are_no_accounts_and_on_a_retry_message_when_the_list_is_unreadable()
    {
        let choices = runtime_choices(false, &[]);
        let empty = AccountsModel::new(Some(vec![]), choices.clone());
        assert!(empty.form_visible && !empty.load_failed && empty.error.is_none());
        assert_eq!(empty.suggested(), "Main");
        let some = AccountsModel::new(Some(vec![profile("claude", "Main")]), choices.clone());
        assert!(!some.form_visible);
        let broken = AccountsModel::new(None, choices);
        assert!(broken.load_failed && !broken.form_visible);
        assert_eq!(broken.error.as_deref(), Some("Could not load your accounts. Try again."));
    }
}
