//! 등록 계정·기본 CLI의 로그인 식별과, WSL 배포판 안 파일을 이 앱이 읽는 경로로 바꾸는 규칙.
//!
//! 로그인 정보 파일은 "같은 로그인인가"만 대조하려고 읽는다. 결과에는 계정 번호 같은 비밀이 아닌 식별자만 담고
//! 토큰은 담지 않는다. 파일 읽기는 호출자가 하므로 이 모듈은 파일 내용을 받아 판정만 한다.
use std::collections::BTreeSet;

use serde_json::Value;

use crate::agent_account::{AccountProfile, Runtime};

/// Claude 로그인 계정의 고유 번호(`oauthAccount.accountUuid`). 인증 토큰이 아니라 Claude Code가 남기는 계정 정보만 읽는다.
/// 파일 내용이 없거나 쓰는 도중이라 읽지 못하면 None이다.
pub fn claude_login(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text)
        .ok()?
        .get("oauthAccount")?
        .get("accountUuid")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// base64url(패딩 없음) 글자를 바이트로 푼다. 모양이 틀리면 None이다. JWT 본문을 읽는 데만 쓴다(서명 검증 없음).
fn decode_base64_url(text: &str) -> Option<Vec<u8>> {
    let values: Option<Vec<u32>> = text
        .trim_end_matches('=')
        .chars()
        .map(|character| match character {
            'A'..='Z' => Some(character as u32 - 'A' as u32),
            'a'..='z' => Some(character as u32 - 'a' as u32 + 26),
            '0'..='9' => Some(character as u32 - '0' as u32 + 52),
            '-' | '+' => Some(62),
            '_' | '/' => Some(63),
            _ => None,
        })
        .collect();
    values.map(|values| {
        let mut bytes = Vec::with_capacity(values.len() * 3 / 4);
        let (mut buffer, mut n_bits) = (0u32, 0u32);
        for value in values {
            buffer = (buffer << 6) | value;
            n_bits += 6;
            if n_bits >= 8 {
                n_bits -= 8;
                bytes.push(((buffer >> n_bits) & 0xff) as u8);
            }
        }
        bytes
    })
}

/// Codex 로그인 계정을 가리키는 비밀이 아닌 식별자: `ChatGPT 계정 id[/사용자 id]`. `auth.json`을 읽은 값에서 꺼낸다.
///
/// 기본 CLI 폴더와 등록 계정 폴더가 같은 로그인인지 대조하려고 존재한다. 토큰은 결과에 들어가지 않는다.
/// 계정 id는 `tokens.account_id`, 없으면 id_token 본문의 `chatgpt_account_id`다. 팀 작업 공간은 사용자마다 계정 id가 같으므로
/// id_token 본문에 사용자 id(`chatgpt_user_id`·`user_id`·`sub`)가 있으면 함께 붙인다 — 한쪽에만 있으면 서로 다른 값이 되어 연결되지 않는다.
/// API 키 로그인처럼 계정 id가 없거나 형식이 다르면 None이다.
///
/// Examples
/// --------
/// | auth.json                                                  | 결과          |
/// |------------------------------------------------------------|---------------|
/// | `{tokens: {account_id: "acc-1"}}`                          | `acc-1`       |
/// | `{tokens: {account_id: "acc-1", id_token: <sub "u-1">}}`   | `acc-1/u-1`   |
/// | `{tokens: {id_token: <chatgpt_account_id "acc-2">}}`       | `acc-2`       |
/// | `{OPENAI_API_KEY: "sk-…"}`                                 | None          |
pub fn codex_login_id(auth: &Value) -> Option<String> {
    let tokens = auth.get("tokens");
    let claims = tokens
        .and_then(|tokens| tokens.get("id_token"))
        .and_then(Value::as_str)
        .and_then(|token| token.split('.').nth(1))
        .and_then(decode_base64_url)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let namespace = claims.as_ref().and_then(|claims| claims.get("https://api.openai.com/auth"));
    let text =
        |value: Option<&Value>| value.and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_string);
    let account_id = text(tokens.and_then(|tokens| tokens.get("account_id")))
        .or_else(|| text(namespace.and_then(|ns| ns.get("chatgpt_account_id"))));
    let user_id = text(namespace.and_then(|ns| ns.get("chatgpt_user_id")))
        .or_else(|| text(namespace.and_then(|ns| ns.get("user_id"))))
        .or_else(|| text(claims.as_ref().and_then(|claims| claims.get("sub"))));
    account_id.map(|account| match user_id {
        Some(user) => format!("{account}/{user}"),
        None => account,
    })
}

/// `auth.json` 글자에서 읽은 로그인 식별자. 파일이 없거나 쓰는 도중이라 읽지 못하면 None이다.
pub fn codex_login(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text).ok().and_then(|auth| codex_login_id(&auth))
}

/// 도구별 로그인 식별자를 읽는다. 모르는 도구면 None이다.
pub fn read_login(
    provider: &str,
    text: &str,
) -> Option<String> {
    match provider {
        "claude" => claude_login(text),
        "codex" => codex_login(text),
        _ => None,
    }
}

/// 등록 계정과 그 폴더의 로그인 식별자(읽지 못했으면 None).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedLogin {
    pub profile: AccountProfile,
    pub login: Option<String>,
}

/// 기본 CLI의 로그인과 같은 로그인을 쓰는 등록 계정. 정확히 하나일 때만 그 계정이고, 없거나 둘 이상(어느 쪽인지 모호)이면 None이다.
/// 기본 CLI를 그 계정으로 취급해도 되는지(이름·사용량 행을 합칠지) 정하는 마지막 판정이다.
///
/// Examples
/// --------
/// | login | 후보의 login      | 결과 |
/// |-------|-------------------|------|
/// | `x`   | A=`x`, B=`y`      | A    |
/// | `x`   | A=`x`, B=`x`      | None |
/// | `x`   | A=`y`, B=없음     | None |
/// | 없음  | A=없음            | None |
pub fn unique_login_match<'a>(
    login: Option<&str>,
    candidates: &'a [SavedLogin],
) -> Option<&'a AccountProfile> {
    let login = login.filter(|login| !login.is_empty())?;
    let mut matches = candidates.iter().filter(|candidate| candidate.login.as_deref() == Some(login));
    match (matches.next(), matches.next()) {
        (Some(only), None) => Some(&only.profile),
        _ => None,
    }
}

/// 실행 환경의 계정 변수로 기본 CLI의 설정 폴더를 정한다(그 실행 환경 기준 경로, `/`로 잇는다).
/// Claude는 `CLAUDE_CONFIG_DIR`가 있으면 그 폴더, 없으면 홈 폴더 자체이고(로그인 정보 파일 `.claude.json`이 홈에 놓인다),
/// Codex는 `CODEX_HOME`, 없으면 `$HOME/.codex`다. 정할 수 없으면 None이다.
///
/// Examples
/// --------
/// | 도구   | 환경                                | 결과              |
/// |--------|-------------------------------------|-------------------|
/// | claude | HOME=/home/me                       | `/home/me`        |
/// | claude | HOME=/home/me, CLAUDE_CONFIG_DIR=/c | `/c`              |
/// | codex  | HOME=/home/me                       | `/home/me/.codex` |
/// | codex  | CODEX_HOME=/x                       | `/x`              |
/// | codex  | (없음)                              | None              |
pub fn default_config_dir(
    provider: &str,
    config_dir: Option<&str>,
    home: Option<&str>,
) -> Option<String> {
    fn non_empty(value: Option<&str>) -> Option<&str> {
        value.filter(|value| !value.is_empty())
    }
    match provider {
        "claude" => non_empty(config_dir).or(non_empty(home)).map(str::to_string),
        "codex" => match (non_empty(config_dir), non_empty(home)) {
            (Some(dir), _) => Some(dir.to_string()),
            (None, Some(home)) => Some(format!("{}/.codex", home.trim_end_matches('/'))),
            (None, None) => None,
        },
        _ => None,
    }
}

/// 기본 CLI의 로그인 정보 파일 경로: 설정 폴더(`default_config_dir`) 안의 `.claude.json`(Claude)·`auth.json`(Codex).
///
/// Examples
/// --------
/// | 도구   | 환경                                | 결과                        |
/// |--------|-------------------------------------|-----------------------------|
/// | claude | HOME=/home/me                       | `/home/me/.claude.json`     |
/// | codex  | HOME=/home/me                       | `/home/me/.codex/auth.json` |
/// | codex  | CODEX_HOME=/x                       | `/x/auth.json`              |
pub fn default_login_file(
    provider: &str,
    config_dir: Option<&str>,
    home: Option<&str>,
) -> Option<String> {
    let name = if provider == "claude" { ".claude.json" } else { "auth.json" };
    default_config_dir(provider, config_dir, home).map(|dir| format!("{}/{name}", dir.trim_end_matches('/')))
}

/// 기본 환경(이 컴퓨터·WSL) 하나의 로그인 식별자와 실행 환경.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultLogin {
    pub login: Option<String>,
    pub runtime: Runtime,
    pub wsl_distribution: Option<String>,
}

/// 환경(`runtime`·배포판)이 같은 등록 계정들의 로그인과 `login`을 대조한다. 정확히 하나일 때만 그 계정이다.
fn match_in_environment<'a>(
    provider: &str,
    login: Option<&str>,
    runtime: Runtime,
    wsl_distribution: Option<&str>,
    saved: &'a [SavedLogin],
) -> Option<&'a AccountProfile> {
    let candidates: Vec<SavedLogin> = saved
        .iter()
        .filter(|candidate| {
            candidate.profile.provider == provider
                && candidate.profile.runtime == runtime
                && (runtime != Runtime::Wsl
                    || wsl_distribution.is_none_or(|name| {
                        candidate
                            .profile
                            .wsl_distribution
                            .as_deref()
                            .is_some_and(|saved| saved.eq_ignore_ascii_case(name))
                    }))
        })
        .cloned()
        .collect();
    unique_login_match(login, &candidates)
        .and_then(|profile| saved.iter().find(|candidate| candidate.profile.id == profile.id))
        .map(|candidate| &candidate.profile)
}

/// 기본 CLI 사용량 행이 가리키는 등록 계정. 로그인한 기본 환경이 모두 같은 등록 계정 하나와 맞을 때만 그 계정이다.
///
/// 기본 사용량은 두 환경 중 한쪽(Codex는 값이 있는 쪽, Claude는 더 최근 기록)을 보이므로, 한 환경이라도 다른 로그인이면 어느 계정의 값인지,
/// 그리고 그냥 `codex`·`claude`가 어느 계정을 쓰는지 단정할 수 없어 None이다. 로그인하지 않은 환경(로그인 정보가 없음)은 대조에서 빠진다.
/// 등록 계정과 대조되지 않은 환경도 로그인 식별자가 대조된 환경과 같으면 같은 계정으로 센다 — 등록 계정이 WSL에만 있고
/// 이 컴퓨터에도 같은 로그인이 남아 있으면, 이 컴퓨터 쪽은 대조할 등록 계정이 없을 뿐 다른 계정이 아니기 때문이다.
///
/// Examples
/// --------
/// | 이 컴퓨터                              | WSL         | 결과 |
/// |----------------------------------------|-------------|------|
/// | A로 로그인                             | 로그인 없음 | A    |
/// | A로 로그인                             | A로 로그인  | A    |
/// | A의 로그인(이 컴퓨터에 등록 계정 없음) | A로 로그인  | A    |
/// | 미등록 B                               | A로 로그인  | None |
/// | 로그인 없음                            | 로그인 없음 | None |
pub fn link_default<'a>(
    provider: &str,
    sources: &[DefaultLogin],
    saved: &'a [SavedLogin],
) -> Option<&'a AccountProfile> {
    let logins: Vec<&DefaultLogin> = sources.iter().filter(|source| source.login.is_some()).collect();
    let matches: Vec<Option<&AccountProfile>> = logins
        .iter()
        .map(|source| {
            match_in_environment(
                provider,
                source.login.as_deref(),
                source.runtime,
                source.wsl_distribution.as_deref(),
                saved,
            )
        })
        .collect();
    let matched = matches.iter().flatten().next().copied();
    let matched_logins: BTreeSet<&str> = logins
        .iter()
        .zip(&matches)
        .filter(|(_, found)| found.is_some())
        .filter_map(|(source, _)| source.login.as_deref())
        .collect();
    let consistent = matched.is_some_and(|matched| {
        matches.iter().zip(&logins).all(|(found, source)| match found {
            Some(profile) => profile.id == matched.id,
            None => source.login.as_deref().is_some_and(|login| matched_logins.contains(login)),
        })
    });
    if consistent { matched } else { None }
}

/// WSL 배포판 안 Linux 경로를 Windows 앱이 읽는 `\\wsl.localhost\<배포판>\…` 경로로 바꾸는 규칙.
///
/// 저장한 배포판의 홈 폴더로만 만든다. 다른 배포판의 홈이거나 홈이 Linux 절대 경로가 아니면 만들지 않는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslMapping {
    /// 배포판 안 홈 폴더(Linux 절대 경로).
    pub linux_home: String,
    /// `\\wsl.localhost\<배포판>`.
    root: String,
}

/// 경로에 쓸 수 없는 글자(제어 문자)가 있는지.
fn has_control(text: &str) -> bool {
    text.chars().any(|character| character < ' ')
}

/// 배포판 이름이 경로 한 칸으로 쓸 수 있는지: 비어 있지 않고 구분자·제어 문자가 없다.
fn is_path_component(text: &str) -> bool {
    !text.is_empty() && !text.contains(['/', '\\']) && !has_control(text)
}

/// `distribution`의 홈 폴더(`linux_home`)로 경로 바꾸는 규칙을 만든다. 홈이 Linux 절대 경로가 아니거나 제어 문자가 있거나
/// 배포판 이름이 경로 한 칸으로 쓸 수 없으면 None이다.
///
/// Examples
/// --------
/// | 홈                        | 배포판         | 결과                                         |
/// |---------------------------|----------------|----------------------------------------------|
/// | `/home/person with space` | `Debian Saved` | `\\wsl.localhost\Debian Saved\home\person with space` 아래 |
/// | `home/user`               | `Ubuntu`       | None                                         |
/// | `/home/user`              | `a\b`          | None                                         |
pub fn wsl_file_mapping(
    linux_home: &str,
    distribution: &str,
) -> Option<WslMapping> {
    let valid = linux_home.starts_with('/') && !has_control(linux_home) && is_path_component(distribution);
    valid.then(|| WslMapping { linux_home: linux_home.to_string(), root: format!("\\\\wsl.localhost\\{distribution}") })
}

impl WslMapping {
    /// 배포판 안 절대 경로를 이 앱이 읽는 경로로 바꾼다. 절대 경로가 아니거나 제어 문자가 있으면 None이다. `..`·`.`는 정리한다.
    ///
    /// Examples
    /// --------
    /// | Linux 경로                          | 결과                                                    |
    /// |-------------------------------------|---------------------------------------------------------|
    /// | `/home/u/.paddock/a`                | `\\wsl.localhost\Debian\home\u\.paddock\a`              |
    /// | `/home/u/../v`                      | `\\wsl.localhost\Debian\home\v`                         |
    /// | `relative`                          | None                                                    |
    pub fn to_local(
        &self,
        file: &str,
    ) -> Option<String> {
        let normalized = crate::agent_account::normalize_path(file);
        (!normalized.is_empty() && !has_control(file))
            .then(|| format!("{}{}", self.root, normalized.replace('/', "\\")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_account::Runtime;
    use serde_json::json;

    fn profile(
        id: &str,
        provider: &str,
        runtime: Runtime,
        distribution: Option<&str>,
    ) -> AccountProfile {
        AccountProfile {
            id: id.to_string(),
            provider: provider.to_string(),
            label: id.to_string(),
            runtime,
            wsl_distribution: distribution.map(str::to_string),
        }
    }

    fn saved(
        profile: AccountProfile,
        login: Option<&str>,
    ) -> SavedLogin {
        SavedLogin { profile, login: login.map(str::to_string) }
    }

    /// 서명 없는 가짜 id_token. 본문(두 번째 조각)만 의미가 있다.
    fn fake_id_token(claims: &Value) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let bytes = claims.to_string().into_bytes();
        let mut text = String::new();
        for chunk in bytes.chunks(3) {
            let value =
                chunk.iter().enumerate().fold(0u32, |sum, (index, byte)| sum | (u32::from(*byte) << (16 - 8 * index)));
            for index in 0..=chunk.len() {
                text.push(ALPHABET[((value >> (18 - 6 * index)) & 63) as usize] as char);
            }
        }
        format!("e30.{text}.signature")
    }

    #[test]
    fn test_au05_wsl_lookup_uses_the_saved_distribution_and_maps_spaced_linux_homes() {
        let mapping = wsl_file_mapping("/home/person with space", "Debian Saved").unwrap();
        assert_eq!(
            mapping.to_local("/home/person with space/.paddock/agent-profiles/x/codex/sessions").as_deref(),
            Some(
                "\\\\wsl.localhost\\Debian Saved\\home\\person with space\\.paddock\\agent-profiles\\x\\codex\\sessions"
            )
        );
        assert_eq!(mapping.to_local("/home/u/../v").as_deref(), Some("\\\\wsl.localhost\\Debian Saved\\home\\v"));
        assert_eq!(mapping.to_local("relative"), None);
        assert_eq!(mapping.to_local("/home/a\nb"), None);
    }

    #[test]
    fn test_au05_a_home_or_distribution_that_cannot_be_a_path_gives_no_mapping() {
        for (home, distribution) in [
            ("home/user", "Ubuntu"),
            ("/home/a\u{1}b", "Ubuntu"),
            ("/home/user", ""),
            ("/home/user", "a\\b"),
            ("/home/user", "a/b"),
        ] {
            assert_eq!(wsl_file_mapping(home, distribution), None, "{home:?} {distribution:?}");
        }
    }

    #[test]
    fn test_au20_the_claude_login_is_the_account_number_only_and_a_half_written_file_is_none() {
        let text = r#"{"oauthAccount":{"accountUuid":"main-login","emailAddress":"main@example.com"},"x":1}"#;
        assert_eq!(claude_login(text).as_deref(), Some("main-login"));
        assert_eq!(claude_login("{\"oauthAccount\":"), None);
        assert_eq!(claude_login(""), None);
        assert_eq!(claude_login(r#"{"oauthAccount":{"accountUuid":""}}"#), None);
    }

    #[test]
    fn test_au21_the_codex_login_id_is_the_account_id_and_user_id_only_and_never_a_token() {
        let id = |value: Value| codex_login_id(&value);
        assert_eq!(id(json!({"tokens": {"account_id": "acc-1", "access_token": "secret-access", "refresh_token": "secret-refresh"}})).as_deref(), Some("acc-1"));
        assert_eq!(
            id(json!({"tokens": {"account_id": "acc-1", "id_token": fake_id_token(&json!({"sub": "u-1"}))}}))
                .as_deref(),
            Some("acc-1/u-1")
        );
        let auth = |claims: Value| json!({"tokens": {"id_token": fake_id_token(&json!({"https://api.openai.com/auth": claims}))}});
        assert_eq!(
            id(auth(json!({"chatgpt_account_id": "acc-2", "chatgpt_user_id": "u-2"}))).as_deref(),
            Some("acc-2/u-2")
        );
        assert_eq!(id(auth(json!({"chatgpt_account_id": "acc-3"}))).as_deref(), Some("acc-3"));
        assert_eq!(id(json!({"tokens": {"account_id": "acc-4", "id_token": "not-a-jwt"}})).as_deref(), Some("acc-4"));
        assert_eq!(id(json!({"OPENAI_API_KEY": "sk-secret"})), None);
        assert_eq!(id(json!({"tokens": {"account_id": 42}})), None);
        assert_eq!(id(Value::Null), None);
    }

    #[test]
    fn test_c_d75_login_results_carry_no_token_text() {
        let secrets = ["secret-access", "secret-refresh", "sk-secret", "id-token-secret-body"];
        let claude = format!(r#"{{"oauthAccount":{{"accountUuid":"u-1"}},"primaryApiKey":"{}"}}"#, secrets[2]);
        let codex = json!({"tokens": {"account_id": "acc-1", "access_token": secrets[0], "refresh_token": secrets[1],
            "id_token": fake_id_token(&json!({"sub": "u-1", "note": secrets[3]}))}, "OPENAI_API_KEY": secrets[2]})
        .to_string();
        let results =
            [claude_login(&claude), codex_login(&codex), read_login("claude", &claude), read_login("codex", &codex)];
        let shown = format!("{results:?}");
        for secret in secrets {
            assert!(!shown.contains(secret), "{secret} leaked into {shown}");
        }
        assert_eq!(results[1].as_deref(), Some("acc-1/u-1"));
    }

    #[test]
    fn test_au22_a_login_links_only_to_exactly_one_saved_account() {
        let (a, b) = (profile("a", "codex", Runtime::Native, None), profile("b", "codex", Runtime::Native, None));
        let pick =
            |login: Option<&str>, list: &[SavedLogin]| unique_login_match(login, list).map(|found| found.id.clone());
        assert_eq!(pick(Some("x"), &[saved(a.clone(), Some("x")), saved(b.clone(), Some("y"))]).as_deref(), Some("a"));
        assert_eq!(pick(Some("x"), &[saved(a.clone(), Some("x")), saved(b.clone(), Some("x"))]), None);
        assert_eq!(pick(Some("x"), &[saved(a.clone(), Some("y")), saved(b, None)]), None);
        assert_eq!(pick(None, &[saved(a, None)]), None);
        assert_eq!(pick(Some(""), &[]), None);
    }

    #[test]
    fn test_au23_the_default_login_file_follows_the_environment_variables() {
        assert_eq!(default_login_file("claude", None, Some("/home/me")).as_deref(), Some("/home/me/.claude.json"));
        assert_eq!(default_login_file("claude", Some("/c"), Some("/home/me")).as_deref(), Some("/c/.claude.json"));
        assert_eq!(default_login_file("codex", None, Some("/home/me")).as_deref(), Some("/home/me/.codex/auth.json"));
        assert_eq!(default_login_file("codex", Some("/x"), Some("/home/me")).as_deref(), Some("/x/auth.json"));
        assert_eq!(default_login_file("codex", None, None), None);
        assert_eq!(default_login_file("bash", None, Some("/home/me")), None);
    }

    #[test]
    fn test_au24_the_default_codex_login_links_to_one_saved_account_of_the_same_environment_and_two_are_ambiguous() {
        let main = profile("main", "codex", Runtime::Native, None);
        let other = profile("other", "codex", Runtime::Native, None);
        let claude = profile("claude", "claude", Runtime::Native, None);
        let list =
            [saved(main, Some("acc-main")), saved(other.clone(), Some("acc-other")), saved(claude, Some("acc-main"))];
        let native = |login: Option<&str>| DefaultLogin {
            login: login.map(str::to_string),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let linked = |sources: &[DefaultLogin], list: &[SavedLogin]| {
            link_default("codex", sources, list).map(|found| found.id.clone())
        };
        assert_eq!(
            linked(&[native(Some("acc-main"))], &list).as_deref(),
            Some("main"),
            "a same string id of another tool is not compared"
        );
        let wsl = DefaultLogin {
            login: Some("acc-main".to_string()),
            runtime: Runtime::Wsl,
            wsl_distribution: Some("Ubuntu".to_string()),
        };
        assert_eq!(linked(&[wsl], &list), None, "an account of another environment is not compared");
        assert_eq!(linked(&[native(Some("acc-unknown"))], &list), None);
        assert_eq!(linked(&[native(None)], &list), None);
        let twice = [list[0].clone(), saved(other, Some("acc-main")), list[2].clone()];
        assert_eq!(
            linked(&[native(Some("acc-main"))], &twice),
            None,
            "two saved accounts with one login are ambiguous"
        );
    }

    #[test]
    fn test_au25_two_saved_claude_accounts_with_one_login_leave_the_default_claude_unlinked() {
        let list = [
            saved(profile("first", "claude", Runtime::Native, None), Some("same-login")),
            saved(profile("second", "claude", Runtime::Native, None), Some("same-login")),
        ];
        let source =
            DefaultLogin { login: Some("same-login".to_string()), runtime: Runtime::Native, wsl_distribution: None };
        assert_eq!(link_default("claude", &[source], &list), None);
    }

    #[test]
    fn test_ur07_the_default_cli_links_only_when_every_signed_in_default_environment_matches_that_one_account() {
        let on_pc = profile("pc", "claude", Runtime::Native, None);
        let in_wsl = profile("wsl", "claude", Runtime::Wsl, Some("Ubuntu"));
        let list = [saved(on_pc.clone(), Some("a")), saved(in_wsl, Some("b"))];
        let source = |login: Option<&str>, runtime: Runtime| DefaultLogin {
            login: login.map(str::to_string),
            runtime,
            wsl_distribution: (runtime == Runtime::Wsl).then(|| "Ubuntu".to_string()),
        };
        let linked = |sources: &[DefaultLogin]| link_default("claude", sources, &list).map(|found| found.id.clone());
        assert_eq!(linked(&[source(Some("a"), Runtime::Native), source(None, Runtime::Wsl)]).as_deref(), Some("pc"));
        assert_eq!(
            linked(&[source(Some("a"), Runtime::Native), source(Some("a"), Runtime::Wsl)]).as_deref(),
            Some("pc"),
            "an unmatched environment with the same login does not block"
        );
        assert_eq!(linked(&[source(Some("zzz"), Runtime::Native), source(Some("b"), Runtime::Wsl)]).as_deref(), None);
        assert_eq!(
            linked(&[source(Some("a"), Runtime::Native), source(Some("b"), Runtime::Wsl)]),
            None,
            "different accounts in the two environments"
        );
        assert_eq!(linked(&[source(None, Runtime::Native), source(None, Runtime::Wsl)]), None);
    }

    #[test]
    fn test_ur08_an_environment_with_the_same_sign_in_but_no_saved_account_in_its_runtime_does_not_block_the_link() {
        let in_wsl = profile("wsl", "codex", Runtime::Wsl, Some("Ubuntu"));
        let list = [saved(in_wsl, Some("acc"))];
        let sources = [
            DefaultLogin { login: Some("acc".to_string()), runtime: Runtime::Native, wsl_distribution: None },
            DefaultLogin {
                login: Some("acc".to_string()),
                runtime: Runtime::Wsl,
                wsl_distribution: Some("Ubuntu".to_string()),
            },
        ];
        assert_eq!(link_default("codex", &sources, &list).map(|found| found.id.as_str()), Some("wsl"));
    }

    #[test]
    fn test_ur03_a_registered_wsl_account_matches_only_in_the_same_distribution() {
        let in_debian = profile("d", "claude", Runtime::Wsl, Some("Debian"));
        let list = [saved(in_debian, Some("x"))];
        let source = |distribution: &str| DefaultLogin {
            login: Some("x".to_string()),
            runtime: Runtime::Wsl,
            wsl_distribution: Some(distribution.to_string()),
        };
        assert!(
            link_default("claude", &[source("debian")], &list).is_some(),
            "distribution names compare without case"
        );
        assert_eq!(link_default("claude", &[source("Ubuntu")], &list), None);
    }
}
