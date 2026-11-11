//! 등록 계정 목록의 형식과 검사: 이름·식별자·항목 검사, 목록 읽기와 쓰기 글자, 추가·이름 바꾸기·제거.
//!
//! 목록 파일(`account-profiles.json`)은 기존 앱과 같은 파일을 같은 형식으로 읽고 쓰는 공유 계약이다.
//! 그래서 검사 규칙과 쓰는 글자(2칸 들여쓰기, 끝 줄바꿈, 항목 안 키 순서)를 기존 앱과 글자 하나까지 맞춘다.
//! 파일 입출력은 하지 않는다 — 읽고 쓰는 쪽(앱)이 이 모듈의 규칙을 부른다.
use std::fmt;

use serde::Serialize;
use serde_json::Value;

use crate::agent_account::{AccountProfile, Runtime};

/// 계정 이름의 최대 길이(UTF-16 코드 단위). 기존 앱이 자바스크립트 문자열 길이로 세는 것과 같다.
const MAX_LABEL_UNITS: usize = 64;

/// 계정 목록 파일의 형식 번호.
const LIST_VERSION: u64 = 1;

/// 계정 검사나 목록 편집이 거부된 이유. 문장은 Accounts 창에 그대로 보인다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    InvalidProvider,
    InvalidRuntime,
    InvalidLabel,
    InvalidId,
    InvalidDistribution,
    /// 같은 도구에 같은 이름(대소문자 무시)의 계정이 이미 있다.
    DuplicateLabel,
    /// 목록에 없는 계정이다.
    NotFound,
    /// 저장된 목록이 깨졌거나 형식이 맞지 않는다. 덮어쓰지 않고 멈춘다.
    UnreadableList,
}

impl fmt::Display for ProfileError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let text = match self {
            Self::InvalidProvider => "Choose Claude or Codex as the account provider.",
            Self::InvalidRuntime => "Choose a native or WSL account environment.",
            Self::InvalidLabel => "Enter an account name between 1 and 64 characters without control characters.",
            Self::InvalidId => "The account identifier is invalid. Select an account from the list.",
            Self::InvalidDistribution => "The saved WSL distribution is invalid. Add the account again.",
            Self::DuplicateLabel => "This provider already has an account with that name. Choose a different name.",
            Self::NotFound => "This account is no longer available. Refresh the account list.",
            Self::UnreadableList => {
                "The saved account list cannot be read. Restore account-profiles.json before changing accounts."
            }
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for ProfileError {}

/// 새 계정 요청. 이름이 없으면 `Claude 1`처럼 도구 이름과 비어 있는 번호로 정한다.
#[derive(Clone, Debug)]
pub struct NewAccount<'a> {
    pub provider: &'a str,
    pub label: Option<&'a str>,
    pub runtime: Runtime,
    /// `runtime`이 WSL일 때 계정이 도는 배포판.
    pub wsl_distribution: Option<&'a str>,
}

/// 도구 이름(`claude` → `Claude`). 모르는 도구면 None이다.
pub fn provider_name(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("Claude"),
        "codex" => Some("Codex"),
        _ => None,
    }
}

/// 자바스크립트 `String.prototype.trim`이 지우는 글자인지.
fn is_js_space(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// 제어 문자(`U+0000`~`U+001F`, `U+007F`)인지.
pub fn is_control(character: char) -> bool {
    character <= '\u{1f}' || character == '\u{7f}'
}

/// 계정 이름을 다듬어 돌려준다: 앞뒤 공백을 지우고, 비었거나 64칸(UTF-16)을 넘거나 제어 문자가 있으면 거부한다.
///
/// 제어 문자는 다듬기 전 글자에서 찾는다 — 끝의 줄바꿈도 거부 대상이다.
///
/// Examples
/// --------
/// | 입력        | 결과                 |
/// |-------------|----------------------|
/// | `" Main "`  | `Ok("Main")`         |
/// | `" "`       | `Err(InvalidLabel)`  |
/// | `"A\nB"`    | `Err(InvalidLabel)`  |
/// | 65칸 글자   | `Err(InvalidLabel)`  |
pub fn checked_label(value: &str) -> Result<String, ProfileError> {
    let trimmed = value.trim_matches(is_js_space);
    if trimmed.is_empty() || trimmed.encode_utf16().count() > MAX_LABEL_UNITS || value.chars().any(is_control) {
        Err(ProfileError::InvalidLabel)
    } else {
        Ok(trimmed.to_string())
    }
}

/// 계정 식별자(소문자 UUID v4)인지 검사한다.
pub fn checked_id(value: &str) -> Result<&str, ProfileError> {
    let groups: Vec<&str> = value.split('-').collect();
    let lengths_match = groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12]);
    let is_hex = value.chars().all(|character| character == '-' || matches!(character, '0'..='9' | 'a'..='f'));
    let is_v4 = groups.get(2).is_some_and(|group| group.starts_with('4'))
        && groups.get(3).is_some_and(|group| group.starts_with(['8', '9', 'a', 'b']));
    if lengths_match && is_hex && is_v4 { Ok(value) } else { Err(ProfileError::InvalidId) }
}

/// 파일 항목 하나를 검사해 계정으로 만든다. 모르는 키는 버린다.
pub fn checked_profile_value(value: &Value) -> Result<AccountProfile, ProfileError> {
    let text_of = |key: &str| value.get(key).and_then(Value::as_str);
    let provider = text_of("provider")
        .filter(|provider| provider_name(provider).is_some())
        .ok_or(ProfileError::InvalidProvider)?;
    let runtime = match text_of("runtime") {
        Some("native") => Ok(Runtime::Native),
        Some("wsl") => Ok(Runtime::Wsl),
        _ => Err(ProfileError::InvalidRuntime),
    }?;
    let id = checked_id(text_of("id").unwrap_or(""))?;
    let label = checked_label(text_of("label").unwrap_or(""))?;
    let wsl_distribution = if runtime == Runtime::Wsl {
        let distribution = text_of("wslDistribution")
            .filter(|name| !name.trim_matches(is_js_space).is_empty() && !name.chars().any(is_control))
            .ok_or(ProfileError::InvalidDistribution)?;
        Some(distribution.to_string())
    } else {
        None
    };
    Ok(AccountProfile { id: id.to_string(), provider: provider.to_string(), label, runtime, wsl_distribution })
}

/// 목록 안에서 같은 도구의 이름이 겹치는지 본다(대소문자 무시, `profile` 자신은 제외).
fn assert_unique_label(
    profiles: &[AccountProfile],
    profile: &AccountProfile,
) -> Result<(), ProfileError> {
    let is_taken = profiles.iter().any(|item| {
        item.id != profile.id
            && item.provider == profile.provider
            && item.label.to_lowercase() == profile.label.to_lowercase()
    });
    if is_taken { Err(ProfileError::DuplicateLabel) } else { Ok(()) }
}

/// 목록 파일 글자를 읽어 검사한다. 깨진 JSON·다른 형식 번호·잘못된 항목·겹친 식별자나 이름이 하나라도 있으면 `UnreadableList`다.
///
/// 읽지 못한 목록을 빈 목록으로 보고 덮어쓰면 등록 계정이 사라지므로, 호출자는 이 오류에서 쓰기를 멈춘다.
pub fn parse_profile_list(text: &str) -> Result<Vec<AccountProfile>, ProfileError> {
    let parsed: Option<Vec<AccountProfile>> = serde_json::from_str::<Value>(text).ok().and_then(|saved| {
        let is_current = saved.get("version").and_then(Value::as_u64) == Some(LIST_VERSION);
        let items = saved.get("profiles").and_then(Value::as_array)?;
        let profiles = items.iter().map(checked_profile_value).collect::<Result<Vec<_>, _>>().ok()?;
        let has_unique_ids = profiles.iter().enumerate().all(|(index, profile)| {
            profiles[..index].iter().all(|earlier| earlier.id != profile.id)
                && assert_unique_label(&profiles[..index], profile).is_ok()
        });
        (is_current && has_unique_ids).then_some(profiles)
    });
    parsed.ok_or(ProfileError::UnreadableList)
}

/// 목록 파일의 JSON 모양. 필드 순서가 곧 파일 안 키 순서다.
#[derive(Serialize)]
struct ListFile<'a> {
    version: u64,
    profiles: &'a [AccountProfile],
}

/// 목록을 파일에 쓸 글자로 만든다: 2칸 들여쓰기, 끝 줄바꿈, 항목 키 순서 `id`·`provider`·`label`·`runtime`·`wslDistribution`.
pub fn serialize_profile_list(profiles: &[AccountProfile]) -> String {
    let mut text = serde_json::to_string_pretty(&ListFile { version: LIST_VERSION, profiles }).unwrap_or_default();
    text.push('\n');
    text
}

/// 이름을 주지 않은 새 계정의 기본 이름: `Claude 1`, 이미 있으면 비어 있는 다음 번호.
pub fn default_label(
    profiles: &[AccountProfile],
    provider: &str,
) -> String {
    let name = provider_name(provider).unwrap_or("");
    let mut n_accounts = 1;
    while profiles.iter().any(|item| {
        item.provider == provider && item.label.to_lowercase() == format!("{name} {n_accounts}").to_lowercase()
    }) {
        n_accounts += 1;
    }
    format!("{name} {n_accounts}")
}

/// 새 계정을 목록 끝에 더한 목록. 이름이 겹치거나 항목이 규칙에 맞지 않으면 거부한다. `id`는 호출자가 새로 뽑은 UUID v4다.
pub fn add_profile(
    profiles: &[AccountProfile],
    input: &NewAccount,
    id: &str,
) -> Result<Vec<AccountProfile>, ProfileError> {
    let label = input.label.map_or_else(|| default_label(profiles, input.provider), str::to_string);
    let candidate = serde_json::json!({
        "id": id,
        "provider": input.provider,
        "label": label,
        "runtime": if input.runtime == Runtime::Wsl { "wsl" } else { "native" },
        "wslDistribution": input.wsl_distribution,
    });
    let profile = checked_profile_value(&candidate)?;
    assert_unique_label(profiles, &profile)?;
    let mut next = profiles.to_vec();
    next.push(profile);
    Ok(next)
}

/// 계정 이름을 바꾼 목록. 식별자는 그대로라 계정 폴더 위치도 그대로다.
pub fn rename_profile(
    profiles: &[AccountProfile],
    id: &str,
    label: &str,
) -> Result<Vec<AccountProfile>, ProfileError> {
    checked_id(id)?;
    let current = profiles.iter().find(|item| item.id == id).ok_or(ProfileError::NotFound)?;
    let renamed = AccountProfile { label: checked_label(label)?, ..current.clone() };
    assert_unique_label(profiles, &renamed)?;
    Ok(profiles.iter().map(|item| if item.id == id { renamed.clone() } else { item.clone() }).collect())
}

/// 계정을 목록에서만 뺀 목록. 계정 폴더·로그인 자료는 건드리지 않는다.
pub fn remove_profile(
    profiles: &[AccountProfile],
    id: &str,
) -> Result<Vec<AccountProfile>, ProfileError> {
    checked_id(id)?;
    if profiles.iter().any(|item| item.id == id) {
        Ok(profiles.iter().filter(|item| item.id != id).cloned().collect())
    } else {
        Err(ProfileError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_ID: &str = "7f0c2a3e-4b1d-4c8e-9a5f-0123456789ab";
    const SECOND_ID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-89abcdef0123";

    /// 기존 앱이 같은 두 계정을 쓴 파일 내용(`JSON.stringify(…, null, 2)` + 줄바꿈).
    const SAMPLE: &str = r#"{
  "version": 1,
  "profiles": [
    {
      "id": "7f0c2a3e-4b1d-4c8e-9a5f-0123456789ab",
      "provider": "claude",
      "label": "Main",
      "runtime": "native"
    },
    {
      "id": "0a1b2c3d-4e5f-4a6b-8c7d-89abcdef0123",
      "provider": "codex",
      "label": "Work 😀 \"q\" \\ x",
      "runtime": "wsl",
      "wslDistribution": "Ubuntu 22.04"
    }
  ]
}
"#;

    fn new_account(provider: &str) -> NewAccount<'_> {
        NewAccount { provider, label: None, runtime: Runtime::Native, wsl_distribution: None }
    }

    #[test]
    fn test_a01_two_providers_get_independent_ids_and_the_saved_text_is_metadata_only_in_the_old_apps_format() {
        let first = add_profile(&[], &new_account("claude"), FIRST_ID).unwrap();
        let both = add_profile(&first, &new_account("codex"), SECOND_ID).unwrap();
        assert_ne!(both[0].id, both[1].id);
        assert_eq!((both[0].label.as_str(), both[1].label.as_str()), ("Claude 1", "Codex 1"));
        let sample = vec![
            AccountProfile {
                id: FIRST_ID.into(),
                provider: "claude".into(),
                label: "Main".into(),
                runtime: Runtime::Native,
                wsl_distribution: None,
            },
            AccountProfile {
                id: SECOND_ID.into(),
                provider: "codex".into(),
                label: "Work \u{1f600} \"q\" \\ x".into(),
                runtime: Runtime::Wsl,
                wsl_distribution: Some("Ubuntu 22.04".into()),
            },
        ];
        assert_eq!(serialize_profile_list(&sample), SAMPLE);
        assert_eq!(parse_profile_list(SAMPLE).unwrap(), sample);
        assert_eq!(serialize_profile_list(&[]), "{\n  \"version\": 1,\n  \"profiles\": []\n}\n");
    }

    #[test]
    fn test_a02_invalid_providers_names_runtimes_and_identifiers_are_rejected() {
        let invalid_provider = |provider: &str| add_profile(&[], &new_account(provider), FIRST_ID).unwrap_err();
        assert_eq!(invalid_provider("__proto__"), ProfileError::InvalidProvider);
        assert_eq!(invalid_provider(""), ProfileError::InvalidProvider);
        for label in [" ", "A\nB", "A\u{0}B", "a\u{7f}", &"a".repeat(65), "\u{1f600}".repeat(33).as_str()] {
            let input = NewAccount { label: Some(label), ..new_account("claude") };
            assert_eq!(add_profile(&[], &input, FIRST_ID).unwrap_err(), ProfileError::InvalidLabel, "{label:?}");
        }
        assert!(checked_label(&"a".repeat(64)).is_ok());
        assert_eq!(checked_label(" \u{feff}Main\u{a0}").as_deref(), Ok("Main"));
        let remote = serde_json::json!({"id": FIRST_ID, "provider": "claude", "label": "A", "runtime": "remote"});
        assert_eq!(checked_profile_value(&remote).unwrap_err(), ProfileError::InvalidRuntime);
        let without_distribution =
            serde_json::json!({"id": FIRST_ID, "provider": "claude", "label": "A", "runtime": "wsl"});
        assert_eq!(checked_profile_value(&without_distribution).unwrap_err(), ProfileError::InvalidDistribution);
        let wsl_without_name = NewAccount { runtime: Runtime::Wsl, ..new_account("claude") };
        assert_eq!(add_profile(&[], &wsl_without_name, FIRST_ID).unwrap_err(), ProfileError::InvalidDistribution);
        for id in [
            "../.claude",
            "../../auth.json",
            "/tmp/anything",
            "",
            &FIRST_ID.to_uppercase(),
            "7f0c2a3e-4b1d-1c8e-9a5f-0123456789ab",
            "7f0c2a3e-4b1d-4c8e-ca5f-0123456789ab",
        ] {
            assert_eq!(checked_id(id), Err(ProfileError::InvalidId), "{id:?}");
            assert_eq!(rename_profile(&[], id, "Another").unwrap_err(), ProfileError::InvalidId);
            assert_eq!(remove_profile(&[], id).unwrap_err(), ProfileError::InvalidId);
        }
    }

    #[test]
    fn test_a03_default_names_count_up_and_a_duplicate_name_is_rejected_without_losing_entries() {
        let one = add_profile(&[], &new_account("claude"), FIRST_ID).unwrap();
        let two = add_profile(&one, &new_account("claude"), SECOND_ID).unwrap();
        assert_eq!((two[0].label.as_str(), two[1].label.as_str()), ("Claude 1", "Claude 2"));
        assert_eq!(rename_profile(&two, SECOND_ID, " claude 1 ").unwrap_err(), ProfileError::DuplicateLabel);
        let renamed = rename_profile(&two, SECOND_ID, "Extra").unwrap();
        assert_eq!((renamed[1].id.as_str(), renamed[1].label.as_str()), (SECOND_ID, "Extra"));
        assert_eq!(renamed.len(), 2);
        // 같은 이름이라도 도구가 다르면 허용하고, 자기 이름으로 바꾸는 것도 허용한다.
        let codex = NewAccount { label: Some("Claude 1"), ..new_account("codex") };
        assert_eq!(add_profile(&two, &codex, "11111111-1111-4111-8111-111111111111").unwrap().len(), 3);
        assert!(rename_profile(&two, FIRST_ID, "claude 1").is_ok());
    }

    #[test]
    fn test_a08_removing_an_account_drops_only_its_list_entry() {
        let both = parse_profile_list(SAMPLE).unwrap();
        let rest = remove_profile(&both, FIRST_ID).unwrap();
        assert_eq!(rest, vec![both[1].clone()]);
        assert_eq!(remove_profile(&rest, FIRST_ID).unwrap_err(), ProfileError::NotFound);
        assert_eq!(rename_profile(&rest, FIRST_ID, "Other").unwrap_err(), ProfileError::NotFound);
        assert_eq!(
            ProfileError::NotFound.to_string(),
            "This account is no longer available. Refresh the account list."
        );
    }

    #[test]
    fn test_a11_a_corrupt_or_mismatched_list_is_unreadable_instead_of_empty() {
        for text in [
            "{unfinished",
            "",
            "[]",
            r#"{"version":2,"profiles":[]}"#,
            r#"{"version":1}"#,
            r#"{"version":1,"profiles":[{"id":"x","provider":"claude","label":"A","runtime":"native"}]}"#,
            &SAMPLE.replace(SECOND_ID, FIRST_ID),
            &SAMPLE.replace("Work \u{1f600} \\\"q\\\" \\\\ x", "main").replace("codex", "claude"),
        ] {
            assert_eq!(parse_profile_list(text), Err(ProfileError::UnreadableList), "{text:?}");
        }
        // 빈 목록과 모르는 키가 든 항목은 읽힌다(모르는 키는 버려진다).
        assert_eq!(parse_profile_list(r#"{"version":1,"profiles":[]}"#), Ok(Vec::new()));
        let extra = SAMPLE.replace(r#""runtime": "native""#, r#""runtime": "native", "shareSkills": false"#);
        assert_eq!(serialize_profile_list(&parse_profile_list(&extra).unwrap()), SAMPLE);
        assert_eq!(
            ProfileError::UnreadableList.to_string(),
            "The saved account list cannot be read. Restore account-profiles.json before changing accounts."
        );
    }
}
