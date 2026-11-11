//! 기본 CLI의 로그인이 등록 계정 하나와 같은지 알아내는 읽기: 로그인 정보 파일(`.claude.json`·`auth.json`)에서 비밀이 아닌 식별자만 꺼내 대조한다.
//!
//! 읽기만 하고, 토큰은 결과·기록·상태 글 어디에도 두지 않는다(식별자를 꺼내자마자 파일 글자를 버린다). 계정 목록은 매번 새로 읽고
//! 계정 폴더의 식별자만 10초 동안 다시 쓴다 — 터미널 상태를 몇 초마다 묻는 요청이 계정 폴더를 매번 읽지 않게 한다.
//! 계정을 추가·삭제하면 다음 대조에 바로 반영된다. 로그인을 확인할 수 없는 계정(WSL이 꺼짐·폴더가 바뀜)은 로그인이 없는 것으로 본다.
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use paddock_core::account_login::{DefaultLogin, SavedLogin, default_login_file, link_default, read_login};
use paddock_core::agent_account::{AccountProfile, Linked, Runtime};
use paddock_core::codex_usage::CodexEnv;

use super::files::{ClaudeSource, ReadContext, assert_plain_path, resolve_profile};
use crate::accounts::store::read_profiles;

/// 등록 계정의 로그인 식별자를 다시 읽지 않고 쓰는 시간.
const CACHE: Duration = Duration::from_secs(10);

/// 로그인 정보 파일의 최대 크기. 이보다 크면 읽지 않는다(Claude의 `.claude.json`은 대화 기록 요약으로 커질 수 있다).
const MAX_BYTES: u64 = 32 * 1024 * 1024;

static CACHE_BY_ACCOUNT: Mutex<BTreeMap<String, (Instant, Option<String>)>> = Mutex::new(BTreeMap::new());

/// 로그인 정보 파일 하나에서 도구별 식별자를 읽는다. 링크이거나 없거나 너무 크거나 쓰는 도중이면 None이다.
pub fn login_in_file(
    provider: &str,
    file: &Path,
) -> Option<String> {
    assert_plain_path(file, false).ok()?;
    let metadata = std::fs::metadata(file).ok().filter(|metadata| metadata.len() <= MAX_BYTES)?;
    let text = std::fs::read_to_string(file).ok().filter(|_| metadata.is_file())?;
    read_login(provider, &text)
}

/// 도구가 같은 등록 계정들과 그 폴더의 로그인 식별자.
pub fn saved_logins(
    provider: &str,
    context: &ReadContext,
) -> Vec<SavedLogin> {
    let profiles: Vec<AccountProfile> =
        read_profiles().unwrap_or_default().into_iter().filter(|profile| profile.provider == provider).collect();
    if let Ok(mut cache) = CACHE_BY_ACCOUNT.lock() {
        // 목록에서 빠진 계정의 캐시는 버린다.
        cache.retain(|id, _| profiles.iter().any(|profile| &profile.id == id));
    }
    profiles
        .into_iter()
        .map(|profile| {
            let cached = CACHE_BY_ACCOUNT.lock().ok().and_then(|cache| {
                cache.get(&profile.id).filter(|(at, _)| at.elapsed() < CACHE).map(|(_, login)| login.clone())
            });
            let login = cached.unwrap_or_else(|| {
                let login =
                    resolve_profile(&profile, context).ok().and_then(|dirs| login_in_file(provider, &dirs.login_file));
                if let Ok(mut cache) = CACHE_BY_ACCOUNT.lock() {
                    cache.insert(profile.id.clone(), (Instant::now(), login.clone()));
                }
                login
            });
            SavedLogin { profile, login }
        })
        .collect()
}

/// 기본 Claude 기록 위치마다의 로그인(그 환경의 로그인 정보 파일).
pub fn claude_default_logins(sources: &[ClaudeSource]) -> Vec<DefaultLogin> {
    sources
        .iter()
        .map(|source| DefaultLogin {
            login: login_in_file("claude", &source.login_file),
            runtime: source.runtime,
            wsl_distribution: source.wsl_distribution.clone(),
        })
        .collect()
}

/// 기본 Codex 환경마다의 로그인(그 환경 설정 폴더의 `auth.json`). 세션 기록 폴더(`…/sessions`)의 부모가 설정 폴더다.
pub fn codex_default_logins(envs: &[CodexEnv]) -> Vec<DefaultLogin> {
    envs.iter()
        .map(|env| DefaultLogin {
            login: Path::new(&env.sessions_dir)
                .parent()
                .and_then(|config| login_in_file("codex", &config.join("auth.json"))),
            runtime: env.runtime,
            wsl_distribution: env.wsl_distribution.clone(),
        })
        .collect()
}

/// 기본 CLI의 로그인이 이 도구의 등록 계정 하나와 같다고 확인되면 그 계정. 한 환경이라도 다른 로그인이면 None이다.
pub fn link_of(
    provider: &str,
    sources: &[DefaultLogin],
    context: &ReadContext,
) -> Option<Linked> {
    let saved = saved_logins(provider, context);
    link_default(provider, sources, &saved)
        .map(|profile| Linked { id: profile.id.clone(), label: profile.label.clone() })
}

/// 한 터미널의 기본 CLI가 등록 계정 하나와 같은 로그인인지(같은 실행 환경의 등록 계정끼리만 대조한다). 확인할 수 없으면 None이다.
///
/// 이 컴퓨터의 터미널은 앱 프로세스의 계정 변수(`CLAUDE_CONFIG_DIR`·`CODEX_HOME`)와 홈 폴더로, WSL 터미널은 그 배포판 사용자의 홈 폴더로 정한다.
pub fn link_of_terminal(
    provider: &str,
    runtime: Runtime,
    wsl_distribution: Option<&str>,
    context: &ReadContext,
) -> Option<Linked> {
    let mut distribution = None;
    let file = if runtime == Runtime::Wsl {
        let (name, mapping) = (context.wsl)(wsl_distribution)?;
        let linux = default_login_file(provider, None, Some(&mapping.linux_home))?;
        distribution = Some(name);
        std::path::PathBuf::from(mapping.to_local(&linux)?)
    } else {
        super::files::native_default_login_file(provider)
    };
    let source = DefaultLogin { login: login_in_file(provider, &file), runtime, wsl_distribution: distribution };
    link_of(provider, &[source], context)
}

#[cfg(test)]
mod tests {
    use paddock_protocol::shared_config::config_dir;

    use super::*;
    use crate::test_support::TempHome;
    use crate::usage::codex::CodexReader;

    const MAIN: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";

    fn context() -> ReadContext {
        ReadContext { wsl: Box::new(|_| None), codex: std::sync::Arc::new(CodexReader::default()) }
    }

    fn write(
        path: &Path,
        text: &str,
    ) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn register(profiles: &[(&str, &str, &str)]) {
        let items: Vec<String> = profiles
            .iter()
            .map(|(id, provider, label)| {
                format!(r#"{{"id":"{id}","provider":"{provider}","label":"{label}","runtime":"native"}}"#)
            })
            .collect();
        write(
            &config_dir().join("account-profiles.json"),
            &format!(r#"{{"version":1,"profiles":[{}]}}"#, items.join(",")),
        );
        CACHE_BY_ACCOUNT.lock().unwrap().clear();
    }

    fn account_file(
        id: &str,
        provider: &str,
        name: &str,
    ) -> std::path::PathBuf {
        config_dir().join("agent-profiles").join(id).join(provider).join(name)
    }

    #[test]
    fn test_c_d75_a_default_cli_is_named_after_the_only_matching_saved_account() {
        let home = TempHome::new();
        register(&[(MAIN, "claude", "Main"), (OTHER, "claude", "Other")]);
        write(&account_file(MAIN, "claude", ".claude.json"), r#"{"oauthAccount":{"accountUuid":"main-login"}}"#);
        write(&account_file(OTHER, "claude", ".claude.json"), r#"{"oauthAccount":{"accountUuid":"other-login"}}"#);
        write(&home.dir.path().join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"main-login"}}"#);
        let linked = link_of_terminal("claude", Runtime::Native, None, &context());
        assert_eq!(linked, Some(Linked { id: MAIN.to_string(), label: "Main".to_string() }));
        write(&home.dir.path().join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"unknown-login"}}"#);
        CACHE_BY_ACCOUNT.lock().unwrap().clear();
        assert_eq!(link_of_terminal("claude", Runtime::Native, None, &context()), None);
    }

    #[test]
    fn test_c_d75_two_saved_accounts_with_the_same_login_keep_default_unlinked() {
        let home = TempHome::new();
        register(&[(MAIN, "codex", "Main"), (OTHER, "codex", "Second")]);
        for id in [MAIN, OTHER] {
            write(&account_file(id, "codex", "auth.json"), r#"{"tokens":{"account_id":"acc-main"}}"#);
        }
        write(&home.dir.path().join(".codex/auth.json"), r#"{"tokens":{"account_id":"acc-main"}}"#);
        assert_eq!(link_of_terminal("codex", Runtime::Native, None, &context()), None);
        // 둘째 계정을 지우면(캐시가 남아 있어도) 바로 하나로 연결된다.
        register(&[(MAIN, "codex", "Main")]);
        assert_eq!(
            link_of_terminal("codex", Runtime::Native, None, &context()).map(|linked| linked.id),
            Some(MAIN.to_string())
        );
    }

    #[test]
    fn test_au26_a_cached_login_does_not_outlive_the_account_it_belongs_to() {
        let _home = TempHome::new();
        register(&[(MAIN, "codex", "Main")]);
        write(&account_file(MAIN, "codex", "auth.json"), r#"{"tokens":{"account_id":"acc-main"}}"#);
        assert_eq!(saved_logins("codex", &context())[0].login.as_deref(), Some("acc-main"));
        assert!(CACHE_BY_ACCOUNT.lock().unwrap().contains_key(MAIN));
        register(&[]);
        saved_logins("codex", &context());
        assert!(!CACHE_BY_ACCOUNT.lock().unwrap().contains_key(MAIN), "a removed account's cache is dropped");
    }

    #[cfg(unix)]
    #[test]
    fn test_au10_a_redirected_login_file_is_not_read() {
        let home = TempHome::new();
        let target = home.dir.path().join("elsewhere.json");
        write(&target, r#"{"oauthAccount":{"accountUuid":"x"}}"#);
        let link = home.dir.path().join(".claude.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(login_in_file("claude", &link), None);
        assert_eq!(login_in_file("claude", &target).as_deref(), Some("x"));
    }

    #[test]
    fn test_c_d75_login_results_carry_no_token_text() {
        let home = TempHome::new();
        let secrets = ["secret-access-token", "secret-refresh-token", "sk-secret-api-key"];
        register(&[(MAIN, "codex", "Main")]);
        let auth = format!(
            r#"{{"tokens":{{"account_id":"acc-1","access_token":"{}","refresh_token":"{}"}},"OPENAI_API_KEY":"{}"}}"#,
            secrets[0], secrets[1], secrets[2]
        );
        write(&account_file(MAIN, "codex", "auth.json"), &auth);
        write(&home.dir.path().join(".codex/auth.json"), &auth);
        let saved = saved_logins("codex", &context());
        let linked = link_of_terminal("codex", Runtime::Native, None, &context());
        let snapshot = crate::usage::files::read_scope(
            &paddock_core::agent_account::Scope::provider("codex"),
            &context(),
            false,
            0,
        );
        let shown = format!("{saved:?} {linked:?} {snapshot:?}");
        let log = std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs/app.log"))
            .unwrap_or_default();
        for secret in secrets {
            assert!(!shown.contains(secret), "{secret} reached a result: {shown}");
            assert!(!log.contains(secret), "{secret} reached the log");
        }
        assert_eq!(saved[0].login.as_deref(), Some("acc-1"));
    }
}
