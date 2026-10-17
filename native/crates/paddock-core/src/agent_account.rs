//! 터미널에서 실행 중인 claude·codex가 어느 계정 폴더로 돌고 있는지 정하는 순수 규칙.
//!
//! 계정은 "어느 버튼으로 연 터미널인가"가 아니라 에이전트 프로세스 자신의 환경 변수로 정한다.
//! 사용자가 터미널에서 `export CODEX_HOME=…; codex`처럼 직접 지정해도, 계정 터미널에서 다른 도구를 실행해도 같은 규칙이 적용된다.
//! 데몬(프로세스 환경 판정)과 앱(표시할 범위 고르기)이 함께 쓰므로 운영체제 호출이 없다.
use serde::{Deserialize, Serialize};

/// 도구별로 설정 폴더를 정하는 환경 변수와 기본 폴더. 이 값이 없으면 도구는 `$HOME/<기본 폴더>`를 쓴다.
fn default_folder(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some(".claude"),
        "codex" => Some(".codex"),
        _ => None,
    }
}

/// 미등록 폴더로 실행한 도구의 범위 이름.
const CUSTOM_NAME: &str = "Unregistered folder";
/// 기본 CLI 범위의 이름.
const CURRENT_NAME: &str = "Default";

/// 계정이 도는 실행 환경.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Native,
    Wsl,
}

/// 등록된 계정 하나. 계정 목록 파일(`account-profiles.json`)의 항목과 같은 모양이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub runtime: Runtime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wsl_distribution: Option<String>,
}

/// 계정 목록 파일 내용에서 계정을 읽는다. 모양이 깨진 항목은 그 항목만 건너뛰고, 파일이 깨졌으면 빈 목록이다.
///
/// 판정에 필요한 값(id·provider·label·runtime·wslDistribution)만 읽는 가벼운 읽기이며 형식 검사는 하지 않는다.
pub fn parse_account_profiles(text: &str) -> Vec<AccountProfile> {
    let parsed = serde_json::from_str::<serde_json::Value>(text).ok();
    let items = parsed.as_ref().and_then(|value| value.get("profiles")).and_then(serde_json::Value::as_array);
    items
        .map(|items| items.iter().filter_map(|item| serde_json::from_value(item.clone()).ok()).collect())
        .unwrap_or_default()
}

/// 프로세스 환경에서 꺼내는 세 값. 나머지 변수(로그인 키 등)는 값을 읽지 않고 버린다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountEnv {
    pub home: Option<String>,
    pub codex_home: Option<String>,
    pub claude_config_dir: Option<String>,
}

impl AccountEnv {
    /// 프로세스 환경에서 읽는 이름 목록.
    pub const READ: [&'static str; 3] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "HOME"];

    /// 도구의 설정 폴더 변수 값. 모르는 도구나 값이 없으면 None이다.
    fn config_dir_of(
        &self,
        provider: &str,
    ) -> Option<&str> {
        match provider {
            "claude" => self.claude_config_dir.as_deref(),
            "codex" => self.codex_home.as_deref(),
            _ => None,
        }
    }
}

/// 환경 목록(`이름=값`을 구분자로 이은 글자, Linux `/proc/<pid>/environ`은 NUL)에서 계정 판정에 쓰는 세 값만 꺼낸다.
/// 빈 값은 없는 것으로 본다. 다른 변수는 결과에 들어가지 않는다.
///
/// Examples
/// --------
/// | 입력                                               | 결과                                     |
/// |----------------------------------------------------|------------------------------------------|
/// | `HOME=/home/me`, `CODEX_HOME=/x`, `OPENAI_API_KEY=k` (NUL로 이음) | `home`·`codex_home`만 채워짐 |
/// | `CLAUDE_CONFIG_DIR=`                               | 모두 없음                                |
pub fn parse_environ(
    text: &str,
    separator: char,
) -> AccountEnv {
    let mut env = AccountEnv::default();
    for entry in text.split(separator) {
        if let Some((name, value)) = entry.split_once('=')
            && !name.is_empty()
            && !value.is_empty()
        {
            match name {
                "HOME" => env.home = Some(value.to_string()),
                "CODEX_HOME" => env.codex_home = Some(value.to_string()),
                "CLAUDE_CONFIG_DIR" => env.claude_config_dir = Some(value.to_string()),
                _ => {}
            }
        }
    }
    env
}

/// Linux 경로를 비교용으로 다듬는다: 겹친 `/`, `.`, `..`, 끝 `/`를 정리한다. 절대 경로가 아니면 빈 글자다.
/// 심볼릭 링크 해석은 하지 않는다 — 실제 경로가 필요하면 호출하는 쪽이 `real_path`로 넘긴다.
pub fn normalize_path(file: &str) -> String {
    let mut result = String::new();
    if file.starts_with('/') {
        let mut parts: Vec<&str> = Vec::new();
        for part in file.split('/') {
            match part {
                ".." => {
                    parts.pop();
                }
                "" | "." => {}
                other => parts.push(other),
            }
        }
        result = format!("/{}", parts.join("/"));
    }
    result
}

/// `attribute_account`의 판정 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detected {
    /// 기본 CLI. `profile`은 그 로그인이 등록 계정 하나와 같다고 확인된 경우에만 채워진다(이 모듈은 채우지 않는다).
    Default { profile: Option<AccountProfile> },
    /// 등록 계정.
    Profile(AccountProfile),
    /// 등록되지 않은 폴더.
    Custom,
}

/// `attribute_account`의 입력.
pub struct AttributeInput<'a> {
    /// `claude` 또는 `codex`. 다른 값이면 결과는 None이다.
    pub provider: &'a str,
    /// 에이전트가 도는 환경.
    pub runtime: Runtime,
    /// WSL 배포판 이름. 모르면 빈 글자이고, 그때는 배포판을 비교하지 않는다.
    pub wsl_distribution: &'a str,
    pub env: &'a AccountEnv,
    pub profiles: &'a [AccountProfile],
    /// 계정 → 그 실행 환경에서 본 계정 폴더 경로.
    pub profile_directory: &'a dyn Fn(&AccountProfile) -> String,
    /// `$HOME` 기본 폴더 말고도 기본 CLI로 볼 폴더(앱이 물려준 `CODEX_HOME` 등).
    pub default_directories: &'a [String],
    /// 경로 → 실제 경로. 읽을 수 없으면 입력을 그대로 돌려줘야 한다.
    pub real_path: &'a dyn Fn(&str) -> String,
}

/// 실행 중인 에이전트가 쓰는 계정을 정한다. 계정 표시·사용량·대화 식별이 모두 이 결과를 따른다.
///
/// 판정 규칙(앞에서 맞는 것이 결과):
/// 1. 계정 변수가 없거나 그 실행 환경의 기본 폴더(`$HOME/.codex`·`$HOME/.claude`, 또는 `default_directories`)와 같으면 기본 CLI.
/// 2. 등록 계정 폴더와 실제 경로가 같고 도구·실행 환경(native/wsl)·WSL 배포판이 맞으면 그 계정.
///    배포판을 모르면(빈 글자) 배포판은 비교하지 않는다.
/// 3. 그 밖의 폴더는 미등록 폴더. 기본 CLI의 사용량이나 계정을 빌려 쓰지 않는다.
///
/// Examples
/// --------
/// | CODEX_HOME                                        | 결과              |
/// |---------------------------------------------------|-------------------|
/// | (없음)                                            | `Default`         |
/// | `/home/me/.codex/`                                | `Default`         |
/// | `<config>/agent-profiles/<codex 계정 id>/codex`    | `Profile(계정)`   |
/// | `<config>/agent-profiles/<claude 계정 id>/claude`  | `Custom`          |
/// | `/tmp/other`                                      | `Custom`          |
pub fn attribute_account(input: &AttributeInput) -> Option<Detected> {
    let folder = default_folder(input.provider)?;
    let canonical = |file: &str| {
        let normalized = normalize_path(file);
        if normalized.is_empty() { normalized } else { normalize_path(&(input.real_path)(&normalized)) }
    };
    let variable = input.env.config_dir_of(input.provider).filter(|value| !value.is_empty());
    let configured = variable.map(canonical).unwrap_or_default();
    let home = normalize_path(input.env.home.as_deref().unwrap_or(""));
    let home_default = (!home.is_empty()).then(|| format!("{}/{folder}", if home == "/" { "" } else { &home }));
    let defaults: Vec<String> = home_default
        .iter()
        .chain(input.default_directories.iter())
        .map(|directory| canonical(directory))
        .filter(|directory| !directory.is_empty())
        .collect();
    let profile = (!configured.is_empty())
        .then(|| {
            input.profiles.iter().find(|item| {
                item.provider == input.provider
                    && item.runtime == input.runtime
                    && (input.runtime != Runtime::Wsl
                        || input.wsl_distribution.is_empty()
                        || item.wsl_distribution.as_deref().unwrap_or("").to_lowercase()
                            == input.wsl_distribution.to_lowercase())
                    && canonical(&(input.profile_directory)(item)) == configured
            })
        })
        .flatten();
    let detected = if variable.is_none() || (!configured.is_empty() && defaults.contains(&configured)) {
        Detected::Default { profile: None }
    } else if let Some(profile) = profile {
        Detected::Profile(profile.clone())
    } else {
        Detected::Custom
    };
    Some(detected)
}

/// 기본 CLI의 로그인이 등록 계정 하나와 같다고 확인된 연결.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linked {
    pub id: String,
    pub label: String,
}

/// 화면(상태 줄·사이드바)이 이 터미널에 대해 보일 계정 범위.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub provider: String,
    /// 등록 계정. 있으면 이름과 대화 기록이 그 계정을 따른다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<AccountProfile>,
    /// 등록되지 않은 폴더로 실행했다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
    /// 기본 CLI인데 로그인이 등록 계정 하나와 같다. 이름·사용량만 그 계정이고 범위는 기본 CLI 그대로다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked: Option<Linked>,
}

impl Scope {
    /// 등록 계정 범위.
    pub fn of_profile(profile: &AccountProfile) -> Self {
        Self { provider: profile.provider.clone(), profile: Some(profile.clone()), custom: false, linked: None }
    }

    /// 기본 CLI 범위.
    pub fn provider(provider: &str) -> Self {
        Self { provider: provider.to_string(), profile: None, custom: false, linked: None }
    }

    /// 미등록 폴더 범위.
    pub fn custom(provider: &str) -> Self {
        Self { custom: true, ..Self::provider(provider) }
    }
}

/// `running_scope`의 입력.
pub struct ScopeInput<'a> {
    /// 터미널의 앞쪽 프로그램 이름. 모르면 None.
    pub program: Option<&'a str>,
    /// 계정 버튼으로 연 터미널의 계정. 일반 터미널이면 None.
    pub launch_profile: Option<&'a AccountProfile>,
    /// 데몬이 에이전트 프로세스 환경으로 판정한 결과. 환경을 읽지 못했으면 None.
    pub detected: Option<&'a Detected>,
}

/// 화면이 이 터미널에 대해 보일 계정 범위를 고른다.
/// 실행 중인 에이전트가 있으면 그 에이전트의 판정 결과가 이기고, 없으면 터미널을 연 계정이다.
///
/// Examples
/// --------
/// | program | launch_profile | detected                  | 결과                               |
/// |---------|----------------|---------------------------|------------------------------------|
/// | bash    | Work Claude    | -                         | Work Claude                        |
/// | claude  | Work Claude    | Profile(Work Claude)      | Work Claude                        |
/// | codex   | Work Claude    | Default                   | codex 기본 CLI                     |
/// | codex   | -              | Default + Work Codex 로그인 | codex 기본 CLI + `linked`          |
/// | claude  | Work Claude    | (읽지 못함)               | Work Claude                        |
/// | codex   | -              | Custom                    | codex 미등록 폴더                  |
/// | bash    | -              | -                         | None                               |
pub fn running_scope(input: &ScopeInput) -> Option<Scope> {
    let mut scope = input.launch_profile.map(Scope::of_profile);
    if let Some(program) = input.program.filter(|program| default_folder(program).is_some()) {
        let launch_provider_differs = input.launch_profile.is_none_or(|profile| profile.provider != program);
        match input.detected {
            Some(Detected::Profile(profile)) if profile.provider == program => {
                // 같은 계정이면 터미널이 가진 계정 객체 그대로다(이름 변경이 반영된 쪽).
                let own = input.launch_profile.filter(|launch| launch.id == profile.id).unwrap_or(profile);
                scope = Some(Scope::of_profile(own));
            }
            Some(Detected::Custom) => scope = Some(Scope::custom(program)),
            Some(Detected::Default { profile: Some(profile) }) if profile.provider == program => {
                scope = Some(Scope {
                    linked: Some(Linked { id: profile.id.clone(), label: profile.label.clone() }),
                    ..Scope::provider(program)
                });
            }
            Some(Detected::Default { .. }) => scope = Some(Scope::provider(program)),
            _ if launch_provider_differs => scope = Some(Scope::provider(program)),
            _ => {}
        }
    }
    scope
}

/// 범위의 계정 이름: 등록 계정은 그 이름, 미등록 폴더는 `Unregistered folder`, 기본 범위는 `Default`.
/// 기본 범위라도 로그인이 등록 계정 하나와 같다고 확인됐으면(`linked`) 그 계정 이름이다.
pub fn scope_name(scope: Option<&Scope>) -> String {
    match scope {
        Some(Scope { profile: Some(profile), .. }) => profile.label.clone(),
        Some(Scope { custom: true, .. }) => CUSTOM_NAME.to_string(),
        Some(Scope { linked: Some(linked), .. }) => linked.label.clone(),
        _ => CURRENT_NAME.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "/home/me/.paddock/config";

    fn profile(
        id: &str,
        provider: &str,
        label: &str,
        runtime: Runtime,
        wsl_distribution: Option<&str>,
    ) -> AccountProfile {
        AccountProfile {
            id: id.to_string(),
            provider: provider.to_string(),
            label: label.to_string(),
            runtime,
            wsl_distribution: wsl_distribution.map(str::to_string),
        }
    }

    fn work_codex() -> AccountProfile {
        profile("11111111-1111-4111-8111-111111111111", "codex", "Work Codex", Runtime::Native, None)
    }

    fn work_claude() -> AccountProfile {
        profile("22222222-2222-4222-8222-222222222222", "claude", "Work Claude", Runtime::Native, None)
    }

    fn wsl_codex() -> AccountProfile {
        profile("33333333-3333-4333-8333-333333333333", "codex", "WSL Codex", Runtime::Wsl, Some("Ubuntu"))
    }

    fn profiles() -> Vec<AccountProfile> {
        vec![work_codex(), work_claude(), wsl_codex()]
    }

    fn native_directory(profile: &AccountProfile) -> String {
        format!("{CONFIG}/agent-profiles/{}/{}", profile.id, profile.provider)
    }

    fn env(pairs: &[(&str, &str)]) -> AccountEnv {
        let mut env = AccountEnv::default();
        for (name, value) in pairs {
            match *name {
                "HOME" => env.home = Some(value.to_string()),
                "CODEX_HOME" => env.codex_home = Some(value.to_string()),
                "CLAUDE_CONFIG_DIR" => env.claude_config_dir = Some(value.to_string()),
                other => panic!("unexpected variable {other}"),
            }
        }
        env
    }

    fn attribute(
        provider: &str,
        env: &AccountEnv,
        default_directories: &[String],
        real_path: &dyn Fn(&str) -> String,
    ) -> Option<Detected> {
        attribute_account(&AttributeInput {
            provider,
            runtime: Runtime::Native,
            wsl_distribution: "",
            env,
            profiles: &profiles(),
            profile_directory: &native_directory,
            default_directories,
            real_path,
        })
    }

    fn identity(path: &str) -> String {
        path.to_string()
    }

    fn attribute_plain(
        provider: &str,
        pairs: &[(&str, &str)],
    ) -> Option<Detected> {
        attribute(provider, &env(pairs), &[], &identity)
    }

    #[test]
    fn test_aa01_environ_keeps_the_three_values_and_drops_login_keys() {
        let parsed =
            parse_environ("HOME=/home/me\0CODEX_HOME=/x\0OPENAI_API_KEY=secret\0CLAUDE_CONFIG_DIR=\0PATH=/bin\0", '\0');
        assert_eq!(parsed, env(&[("HOME", "/home/me"), ("CODEX_HOME", "/x")]));
        assert_eq!(
            parse_environ("CLAUDE_CONFIG_DIR=/c\x1fANTHROPIC_API_KEY=k\x1f", '\x1f'),
            env(&[("CLAUDE_CONFIG_DIR", "/c")])
        );
        assert_eq!(parse_environ("", '\0'), AccountEnv::default());
    }

    #[test]
    fn test_aa02_comparison_paths_lose_repeated_separators_dots_and_trailing_slash() {
        assert_eq!(normalize_path("/home//me/./.codex/"), "/home/me/.codex");
        assert_eq!(normalize_path("/home/me/x/../.codex"), "/home/me/.codex");
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("relative/.codex"), "");
        assert_eq!(normalize_path(""), "");
    }

    #[test]
    fn test_aa03_no_variable_or_a_default_folder_is_the_default_cli() {
        let default = Some(Detected::Default { profile: None });
        assert_eq!(attribute_plain("codex", &[("HOME", "/home/me")]), default);
        assert_eq!(attribute_plain("codex", &[]), default);
        assert_eq!(attribute_plain("codex", &[("HOME", "/home/me"), ("CODEX_HOME", "/home/me/.codex/")]), default);
        assert_eq!(
            attribute_plain("claude", &[("HOME", "/home/me"), ("CLAUDE_CONFIG_DIR", "/home/me/.claude")]),
            default
        );
        // 앱이 물려준 기본 폴더(앱 자체의 CODEX_HOME)도 기본 CLI다.
        let inherited = env(&[("HOME", "/home/me"), ("CODEX_HOME", "/opt/codex")]);
        assert_eq!(attribute("codex", &inherited, &["/opt/codex".to_string()], &identity), default);
        // 다른 도구의 변수만 있으면 이 도구는 기본 폴더를 쓴다.
        let claude_dir = native_directory(&work_claude());
        assert_eq!(attribute_plain("codex", &[("HOME", "/home/me"), ("CLAUDE_CONFIG_DIR", &claude_dir)]), default);
    }

    #[test]
    fn test_aa04_a_registered_folder_with_the_same_real_path_is_that_account() {
        let codex_dir = native_directory(&work_codex());
        assert_eq!(
            attribute_plain("codex", &[("HOME", "/home/me"), ("CODEX_HOME", &codex_dir)]),
            Some(Detected::Profile(work_codex()))
        );
        let claude_dir = format!("{}/", native_directory(&work_claude()));
        assert_eq!(
            attribute_plain("claude", &[("CLAUDE_CONFIG_DIR", &claude_dir)]),
            Some(Detected::Profile(work_claude()))
        );
        // 심볼릭 링크로 가리킨 폴더는 실제 경로로 비교한다.
        let target = native_directory(&work_codex());
        let real_path = |file: &str| if file == "/home/me/link" { target.clone() } else { file.to_string() };
        assert_eq!(
            attribute("codex", &env(&[("CODEX_HOME", "/home/me/link")]), &[], &real_path),
            Some(Detected::Profile(work_codex()))
        );
    }

    #[test]
    fn test_aa05_other_folders_are_unregistered_and_other_tools_or_runtimes_are_not_borrowed() {
        let custom = Some(Detected::Custom);
        assert_eq!(attribute_plain("codex", &[("HOME", "/home/me"), ("CODEX_HOME", "/tmp/other")]), custom);
        assert_eq!(attribute_plain("codex", &[("CODEX_HOME", "relative/codex")]), custom);
        // Claude 계정 폴더를 CODEX_HOME으로 지정한 경우.
        let claude_as_codex = format!("{CONFIG}/agent-profiles/{}/codex", work_claude().id);
        assert_eq!(attribute_plain("codex", &[("CODEX_HOME", &claude_as_codex)]), custom);
        // WSL 계정은 native 환경에서 같은 경로라도 맞지 않는다.
        let wsl_dir = native_directory(&wsl_codex());
        assert_eq!(attribute_plain("codex", &[("CODEX_HOME", &wsl_dir)]), custom);
        let codex_dir = native_directory(&work_codex());
        assert_eq!(attribute_plain("bash", &[("CODEX_HOME", &codex_dir)]), None);
        assert_eq!(attribute_plain("", &[]), None);
    }

    #[test]
    fn test_aa06_wsl_accounts_match_under_the_distributions_home_and_other_distributions_are_unregistered() {
        let wsl_directory =
            |profile: &AccountProfile| format!("/home/u/.paddock/agent-profiles/{}/{}", profile.id, profile.provider);
        let wsl_env = env(&[("HOME", "/home/u"), ("CODEX_HOME", &wsl_directory(&wsl_codex()))]);
        let all = profiles();
        let run = |distribution: &str, env: &AccountEnv| {
            attribute_account(&AttributeInput {
                provider: "codex",
                runtime: Runtime::Wsl,
                wsl_distribution: distribution,
                env,
                profiles: &all,
                profile_directory: &wsl_directory,
                default_directories: &[],
                real_path: &identity,
            })
        };
        assert_eq!(run("ubuntu", &wsl_env), Some(Detected::Profile(wsl_codex())));
        assert_eq!(run("", &wsl_env), Some(Detected::Profile(wsl_codex())));
        assert_eq!(run("Debian", &wsl_env), Some(Detected::Custom));
        assert_eq!(run("", &env(&[("HOME", "/home/u")])), Some(Detected::Default { profile: None }));
    }

    #[test]
    fn test_aa07_the_screen_scope_follows_the_running_tool_else_the_account_that_opened_the_terminal() {
        let default_detected = Detected::Default { profile: None };
        // [program, launch_profile, detected, expected profile id or provider-only marker]
        #[allow(clippy::type_complexity)]
        let cases: Vec<(Option<&str>, Option<AccountProfile>, Option<Detected>, Option<Scope>)> = vec![
            (Some("bash"), Some(work_claude()), None, Some(Scope::of_profile(&work_claude()))),
            (
                Some("claude"),
                Some(work_claude()),
                Some(Detected::Profile(work_claude())),
                Some(Scope::of_profile(&work_claude())),
            ),
            (Some("codex"), Some(work_claude()), Some(default_detected.clone()), Some(Scope::provider("codex"))),
            (Some("codex"), Some(work_claude()), None, Some(Scope::provider("codex"))),
            (Some("claude"), Some(work_claude()), None, Some(Scope::of_profile(&work_claude()))),
            (Some("codex"), None, Some(Detected::Profile(work_codex())), Some(Scope::of_profile(&work_codex()))),
            (
                Some("codex"),
                Some(work_claude()),
                Some(Detected::Profile(work_codex())),
                Some(Scope::of_profile(&work_codex())),
            ),
            (Some("codex"), None, Some(Detected::Custom), Some(Scope::custom("codex"))),
            (Some("claude"), None, Some(default_detected.clone()), Some(Scope::provider("claude"))),
            (Some("claude"), None, None, Some(Scope::provider("claude"))),
            (Some("bash"), None, None, None),
            (None, None, None, None),
        ];
        for (program, launch_profile, detected, expected) in cases {
            let scope = running_scope(&ScopeInput {
                program,
                launch_profile: launch_profile.as_ref(),
                detected: detected.as_ref(),
            });
            assert_eq!(scope, expected, "{program:?} / {launch_profile:?} / {detected:?}");
        }
    }

    #[test]
    fn test_aa08_the_scope_name_is_the_account_name_unregistered_folder_or_default() {
        assert_eq!(scope_name(Some(&Scope::of_profile(&work_codex()))), "Work Codex");
        assert_eq!(scope_name(Some(&Scope::custom("codex"))), "Unregistered folder");
        assert_eq!(scope_name(Some(&Scope::provider("codex"))), "Default");
        assert_eq!(scope_name(None), "Default");
    }

    #[test]
    fn test_aa09_the_same_tool_with_another_config_folder_follows_detection_not_the_opening_account() {
        let other_claude =
            profile("44444444-4444-4444-8444-444444444444", "claude", "Other Claude", Runtime::Native, None);
        let run = |detected: Detected| {
            running_scope(&ScopeInput {
                program: Some("claude"),
                launch_profile: Some(&work_claude()),
                detected: Some(&detected),
            })
        };
        assert_eq!(run(Detected::Default { profile: None }), Some(Scope::provider("claude")));
        assert_eq!(run(Detected::Profile(other_claude.clone())), Some(Scope::of_profile(&other_claude)));
        assert_eq!(run(Detected::Custom), Some(Scope::custom("claude")));
    }

    #[test]
    fn test_aa10_a_default_login_matching_one_account_keeps_the_scope_but_takes_its_name() {
        let detected = Detected::Default { profile: Some(work_codex()) };
        let scope =
            running_scope(&ScopeInput { program: Some("codex"), launch_profile: None, detected: Some(&detected) })
                .expect("a scope");
        assert_eq!(scope.linked, Some(Linked { id: work_codex().id, label: "Work Codex".to_string() }));
        assert!(scope.profile.is_none(), "conversation history and account switching use the default folder");
        assert_eq!(scope_name(Some(&scope)), "Work Codex");
        // 계정 터미널에서 기본 codex를 실행해도 같다.
        let from_account_terminal = running_scope(&ScopeInput {
            program: Some("codex"),
            launch_profile: Some(&work_claude()),
            detected: Some(&detected),
        })
        .expect("a scope");
        assert_eq!(from_account_terminal.linked.map(|linked| linked.id), Some(work_codex().id));
        // 다른 도구의 계정은 연결 근거가 아니다.
        assert_eq!(
            running_scope(&ScopeInput { program: Some("claude"), launch_profile: None, detected: Some(&detected) }),
            Some(Scope::provider("claude"))
        );
    }

    #[test]
    fn test_account_profiles_file_is_read_leniently() {
        let text = r#"{"version":1,"profiles":[
            {"id":"a","provider":"claude","label":"Main","runtime":"native"},
            {"id":"b","provider":"codex","label":"WSL","runtime":"wsl","wslDistribution":"Ubuntu"},
            {"id":"broken"}
        ]}"#;
        let parsed = parse_account_profiles(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].wsl_distribution.as_deref(), Some("Ubuntu"));
        assert!(parse_account_profiles("{oops").is_empty());
        assert!(parse_account_profiles("{}").is_empty());
    }
}
