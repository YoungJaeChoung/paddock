//! 사용량 범위(어느 계정·기본 CLI의 값을 상태 줄에 보일지)를 정하는 순수 규칙과, 상태 줄 묶음이 보일 모양.
//!
//! 범위는 `Scope`(등록 계정·기본 CLI·미등록 폴더)다. 도구(Claude·Codex)마다 마지막으로 고른 범위 하나를 상태 줄에 남기고,
//! 읽기 결과(`Snapshot`)는 범위마다 따로 쥔다. 어느 범위의 값도 다른 범위의 값으로 채우지 않는다 —
//! 기본 CLI의 값은 등록 계정 행을 채우지 않고, 지운 계정은 기본 값 대신 `Unavailable`로 남는다.
//! 읽기는 호출자(앱의 작업 스레드)가 하고, 이 모듈은 읽기 요청의 유효성(`Ticket`)과 결과 반영만 규칙으로 쥔다.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::agent_account::{AccountProfile, Linked, Scope, scope_name};
use crate::theme::{meter, usage_compact, usage_group};
use crate::usage_model::{
    ClaudeState, Reading, Reset, current_windows, describe, duration, reset_clock, short_age, window_name,
};

/// 사용량을 보이는 도구와 화면 문구.
pub struct UsageScopes;

impl UsageScopes {
    /// 도구 이름(상태 줄·목록 표기)과 출처 표지. 순서가 상태 줄 순서다.
    pub const PROVIDERS: [(&'static str, &'static str, &'static str); 2] =
        [("claude", "Claude", "\u{2731}"), ("codex", "Codex", "\u{25ce}")];
    /// 계정이 이 수 이상이면 목록에 검색칸이 생긴다.
    pub const N_SEARCH_THRESHOLD: usize = 6;
    /// 상태 줄 값을 다시 읽는 주기(초).
    pub const POLL_INTERVAL_SECONDS: u64 = 120;
    pub const CUSTOM_MESSAGE: &'static str = "This CLI runs with a configuration folder that is not a saved account. Usage from the default CLI is not shown for it.";
    pub const CURRENT_NAME: &'static str = "Default";
    pub const CURRENT_HINT: &'static str =
        "The default CLI (plain claude or codex in a terminal) is signed in to this account.";
    /// 이 시간(초)보다 오래된 기록은 지금 값처럼 보이지 않게 경과 시간을 함께 보인다.
    pub const STALE_SECONDS: i64 = 3600;
    pub const RECORDED_HINT: &'static str = "Live usage could not be read, so this is the last value recorded in a Codex session. Sign in again in this account if it continues.";
    pub const REMOVED_HINT: &'static str = "This account was removed from the list. Its existing terminal is kept.";
    pub const RETRY_HINT: &'static str = "Usage could not be read. Open Account usage and select Retry.";
    pub const WAITING_HINT: &'static str =
        "Send a message in this account\u{2019}s Claude terminal. Usage appears after Claude reports its limits.";
    pub const NOT_REPORTED_HINT: &'static str =
        "This CLI has not reported usage limits. No value is inferred from another account.";
}

/// 도구 이름의 화면 표기(`claude` → `Claude`). 모르는 도구는 빈 글자다.
pub fn provider_title(provider: &str) -> &'static str {
    UsageScopes::PROVIDERS.iter().find(|(name, _, _)| *name == provider).map_or("", |(_, title, _)| title)
}

fn is_known_provider(provider: &str) -> bool {
    UsageScopes::PROVIDERS.iter().any(|(name, _, _)| *name == provider)
}

/// 범위의 키. 등록 계정은 `<도구>:<id>`, 미등록 폴더는 `<도구>:custom`, 기본 CLI는 `<도구>:default`다.
///
/// Examples
/// --------
/// | 범위                | 키               |
/// |---------------------|------------------|
/// | 등록 계정 id `a`    | `claude:a`       |
/// | 미등록 폴더         | `codex:custom`   |
/// | 기본 CLI            | `codex:default`  |
pub fn scope_key(scope: &Scope) -> String {
    let tail = match (&scope.profile, scope.custom) {
        (Some(profile), _) => profile.id.as_str(),
        (None, true) => "custom",
        (None, false) => "default",
    };
    format!("{}:{tail}", scope.provider)
}

fn profile_id(scope: &Scope) -> Option<&str> {
    scope.profile.as_ref().map(|profile| profile.id.as_str())
}

/// 상태 줄에 마지막으로 고른 범위를 도구별로 모은 저장 값.
///
/// 미등록 폴더와 터미널 판정 연결(`linked`)은 그 터미널에서만 쓰는 값이라 담지 않는다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<Scope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<Scope>,
}

impl UsageSelection {
    pub fn get(
        &self,
        provider: &str,
    ) -> Option<&Scope> {
        match provider {
            "claude" => self.claude.as_ref(),
            "codex" => self.codex.as_ref(),
            _ => None,
        }
    }

    fn set(
        &mut self,
        provider: &str,
        scope: Scope,
    ) {
        match provider {
            "claude" => self.claude = Some(scope),
            "codex" => self.codex = Some(scope),
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.claude.is_none() && self.codex.is_none()
    }
}

/// 범위 하나의 읽기 상태.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Snapshot {
    Loading,
    Ready(Reading),
    /// 읽지 못했다. `message`는 사용자에게 보일 문장, `removed`는 목록에서 지워진 계정이다.
    Unavailable {
        message: Option<String>,
        removed: bool,
    },
}

impl Snapshot {
    fn ready(&self) -> Option<&Reading> {
        match self {
            Self::Ready(reading) => Some(reading),
            _ => None,
        }
    }
}

/// 읽기 요청의 표. 선택이 바뀌거나 설정이 바뀐 뒤 늦게 도착한 옛 응답을 가려내는 데 쓴다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub n_revision: u64,
    pub n_serial: u64,
}

/// `begin_read`의 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadStart {
    /// 같은 선택 아래 같은 범위를 이미 읽는 중이다. 새 요청을 만들지 않는다.
    Pending,
    /// 읽을 곳이 없거나(미등록 폴더) 이미 지워졌다. 요청 없이 끝난다.
    Nothing,
    /// 이 표로 읽기를 시작한다.
    Fetch(Ticket),
}

/// 선택·복원 같은 변경의 결과.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// 보이는 모양이 바뀌었다(다시 그린다).
    pub redraw: bool,
    /// 선택한 범위가 바뀌어 지금 값을 다시 읽어야 한다.
    pub reread: bool,
    /// 파일에 남길 새 저장 값(바뀌었을 때만).
    pub save: Option<UsageSelection>,
}

/// 한 번의 갱신이 읽을 범위.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefreshPlan {
    /// 지금 읽을 범위.
    pub reads: Vec<Scope>,
    /// 기본 Codex가 이미 같은 계정으로 확인되어 이번에는 읽지 않는 등록 계정 범위. 기본 응답에서 연결이 풀리면 이어서 읽는다.
    pub skipped: Option<Scope>,
    /// `skipped`를 건너뛴 근거가 된 기본 Codex 응답의 계정 id.
    pub linked_codex_id: Option<String>,
}

/// 범위마다의 사용량 읽기 상태와 상태 줄 선택.
#[derive(Debug, Default)]
pub struct UsageBook {
    profiles: Vec<AccountProfile>,
    profiles_known: bool,
    profiles_failed: bool,
    /// 도구별 마지막 선택.
    selected: BTreeMap<String, Scope>,
    /// 도구별로 미등록 폴더를 고르기 직전의 선택(없었으면 None). 미등록 폴더에서 벗어나면 이 값으로 돌아간다.
    before_custom: BTreeMap<String, Option<Scope>>,
    /// 지난 실행에서 되살린 등록 계정 선택의 도구. 계정 목록을 처음 읽을 때 지워진 계정이면 선택을 비운다.
    restored_providers: BTreeSet<String>,
    /// 저장 값을 되살리기 전에는 저장하지 않는다. 먼저 저장하면 아직 읽지 않은 다른 도구의 선택을 지운다.
    is_selection_restored: bool,
    saved: Option<UsageSelection>,
    /// 지금 보는 터미널의 범위.
    active: Option<Scope>,
    snapshots: BTreeMap<String, Snapshot>,
    pending: BTreeMap<String, Ticket>,
    n_revision: u64,
    n_serial: u64,
    /// 상태 줄에 보이는 값이 바뀔 수 있는 변경(선택·읽기·계정 목록·설정)마다 늘어난다. 그리는 쪽이 묶음 계산을 다시 할지 정하는 열쇠다.
    n_changes: u64,
}

impl UsageBook {
    pub fn profiles(&self) -> &[AccountProfile] {
        &self.profiles
    }

    pub fn profiles_known(&self) -> bool {
        self.profiles_known
    }

    pub fn profiles_failed(&self) -> bool {
        self.profiles_failed
    }

    pub fn n_changes(&self) -> u64 {
        self.n_changes
    }

    pub fn n_revision(&self) -> u64 {
        self.n_revision
    }

    pub fn active(&self) -> Option<&Scope> {
        self.active.as_ref()
    }

    pub fn selected(
        &self,
        provider: &str,
    ) -> Option<&Scope> {
        self.selected.get(provider)
    }

    /// 마지막으로 저장한 선택. 저장 값을 되살리기 전에는 빈 선택이다.
    pub fn saved_selection(&self) -> UsageSelection {
        self.saved.clone().unwrap_or_default()
    }

    /// 도구별 선택 전체(도구 이름 순).
    pub fn selected_all(&self) -> impl Iterator<Item = &Scope> {
        self.selected.values()
    }

    /// 지금 보는 터미널의 범위로 선택을 바꾼다. 터미널이 에이전트도 계정 터미널도 아니면 None이다.
    ///
    /// 미등록 폴더는 그 터미널에서 잠깐 쓰는 범위다. 다른 범위로 옮기면 그 전에 고른 범위로 되돌려 상태 줄에 남지 않게 한다.
    /// 터미널 판정의 연결(`linked`)은 그 터미널에서 기본 CLI가 실행 중인 동안만 믿는다. 판정이 다시 오지 않으면 낡기 때문이다 —
    /// 셸로 돌아오거나 다른 화면·도구로 옮기면 지운다. 그 뒤에는 계속 읽는 기본 CLI 응답의 계정만 연결 근거가 된다.
    pub fn select(
        &mut self,
        next: Option<Scope>,
    ) -> Change {
        self.n_changes += 1;
        let next = next.filter(|scope| is_known_provider(&scope.provider));
        let previous_key = self.active.as_ref().map(scope_key);
        let next_key = next.as_ref().map(scope_key);
        let providers: Vec<String> = self.selected.keys().cloned().collect();
        for provider in &providers {
            let leaves_custom = self.selected.get(provider).is_some_and(|selected| selected.custom)
                && !next.as_ref().is_some_and(|next| next.custom && &next.provider == provider);
            if leaves_custom {
                let back =
                    self.before_custom.get(provider).cloned().flatten().unwrap_or_else(|| Scope::provider(provider));
                self.selected.insert(provider.clone(), back);
            }
        }
        if let Some(next) = &next {
            let was_custom = self.selected.get(&next.provider).is_some_and(|selected| selected.custom);
            if next.custom && !was_custom {
                self.before_custom.insert(next.provider.clone(), self.selected.get(&next.provider).cloned());
            }
            self.selected.insert(next.provider.clone(), next.clone());
            self.restored_providers.remove(&next.provider);
        }
        let mut unlinked = false;
        for provider in &providers {
            let next_provider = next.as_ref().map(|scope| scope.provider.as_str());
            if let Some(selected) = self.selected.get_mut(provider)
                && selected.linked.is_some()
                && next_provider != Some(provider.as_str())
            {
                selected.linked = None;
                unlinked = true;
            }
        }
        self.active = next;
        let key_changed = previous_key != next_key;
        if key_changed {
            self.n_revision += 1;
        }
        Change { redraw: key_changed || unlinked, reread: key_changed, save: self.persist_selection() }
    }

    /// 지난 실행의 도구별 마지막 선택을 되살린다. 앱을 다시 켜도 상태 줄에 Claude·Codex가 각각 남게 하기 위해서다.
    ///
    /// 이번 실행에서 이미 고른 도구는 덮지 않는다. 다른 도구의 항목, 미등록 폴더, 터미널 판정 연결은 받지 않는다.
    /// 계정 목록을 이미 읽었으면 지워진 계정은 바로 버리고, 아직이면 처음 읽을 때 버린다.
    ///
    /// Examples
    /// --------
    /// | 저장 값                  | 이번 실행 선택 | 결과                          |
    /// |--------------------------|----------------|-------------------------------|
    /// | codex: 등록 계정 `c`     | (없음)         | codex = `c`                   |
    /// | claude: 기본 CLI         | claude = A     | claude = A (덮지 않음)        |
    /// | codex 칸에 claude 항목   | (없음)         | (비어 있음: 도구 불일치)      |
    pub fn restore_selection(
        &mut self,
        saved: &UsageSelection,
    ) -> Change {
        self.n_changes += 1;
        let mut redraw = false;
        for (provider, _, _) in UsageScopes::PROVIDERS {
            if let Some(entry) = saved.get(provider)
                && !self.selected.contains_key(provider)
                && entry.provider == provider
                && !entry.custom
            {
                let restored = Scope { linked: None, custom: false, ..entry.clone() };
                if restored.profile.is_some() {
                    self.restored_providers.insert(provider.to_string());
                }
                self.selected.insert(provider.to_string(), restored);
                redraw = true;
            }
        }
        self.is_selection_restored = true;
        let save = if self.profiles_known { self.drop_removed_restored() } else { self.persist_selection() };
        Change { redraw, reread: false, save }
    }

    /// 되살린 선택 중 계정 목록에 없는 등록 계정을 비운다. 지운 계정이 상태 줄에 "삭제됨"으로 남지 않게 한다. 한 번 확인하면 끝난다.
    fn drop_removed_restored(&mut self) -> Option<UsageSelection> {
        let providers: Vec<String> = self.restored_providers.iter().cloned().collect();
        for provider in providers {
            let selected_id = self.selected.get(&provider).and_then(profile_id).map(str::to_string);
            let present = selected_id.is_some_and(|id| self.profiles.iter().any(|profile| profile.id == id));
            if !present {
                self.selected.remove(&provider);
            }
        }
        self.restored_providers.clear();
        self.persist_selection()
    }

    /// 도구별 마지막 선택을 저장 값으로 돌려준다. 바뀌었을 때만 돌려주고, 저장 값을 되살리기 전에는 돌려주지 않는다.
    /// 미등록 폴더는 그 터미널에서만 쓰는 범위라 그 직전 선택을 담고, 터미널 판정 연결은 다시 켜면 낡으므로 뺀다.
    fn persist_selection(&mut self) -> Option<UsageSelection> {
        let mut now = UsageSelection::default();
        for (provider, _, _) in UsageScopes::PROVIDERS {
            let chosen = match self.selected.get(provider) {
                Some(selected) if selected.custom => self.before_custom.get(provider).cloned().flatten(),
                other => other.cloned(),
            };
            if let Some(chosen) = chosen {
                now.set(provider, Scope { linked: None, custom: false, ..chosen });
            }
        }
        let changed = self.saved.as_ref() != Some(&now);
        let mut result = None;
        if self.is_selection_restored && changed {
            self.saved = Some(now.clone());
            result = Some(now);
        }
        result
    }

    /// 범위가 지금 가리키는 계정. 등록 계정은 목록의 최신 항목이고, 기본 CLI는 로그인이 등록 계정 하나와 같다고 확인되면 그 계정이다.
    /// 상태 줄·목록이 같은 계정을 두 번(기본 CLI와 등록 계정) 보이지 않게 하려고 존재한다. 미등록 폴더는 그대로다.
    pub fn current(
        &self,
        scope: &Scope,
    ) -> Scope {
        if let Some(id) = profile_id(scope) {
            self.profiles.iter().find(|profile| profile.id == id).map_or_else(|| scope.clone(), Scope::of_profile)
        } else if !scope.custom {
            self.linked_profile(&scope.provider, Some(scope)).map_or_else(|| scope.clone(), Scope::of_profile)
        } else {
            scope.clone()
        }
    }

    /// 기본 CLI가 그 계정으로 로그인했다고 확인된 등록 계정. 없거나 목록에 없는 계정이면 None이다.
    ///
    /// 근거는 터미널 판정(`linked`, 같은 실행 환경에서 확인)이 먼저이고, 없으면 기본 CLI 사용량 응답의 `account`다.
    /// 터미널 판정은 그 터미널에서 기본 CLI가 실행 중인 동안만 남는다(`select`가 지운다).
    pub fn linked_profile(
        &self,
        provider: &str,
        scope: Option<&Scope>,
    ) -> Option<&AccountProfile> {
        let from_terminal =
            scope.filter(|scope| scope.profile.is_none() && !scope.custom).and_then(|scope| scope.linked.as_ref());
        let shared = self.snapshots.get(&format!("{provider}:default")).and_then(Snapshot::ready);
        let id = from_terminal
            .map(|linked| linked.id.as_str())
            .or_else(|| shared.and_then(|reading| reading.account.as_ref()).map(|account| account.id.as_str()));
        id.and_then(|id| self.profiles.iter().find(|profile| profile.id == id && profile.provider == provider))
    }

    /// 범위의 사용량 상태. 등록 계정은 기본 CLI 사용량이 같은 계정으로 확인되면 두 값 중 하나만 쓴다 —
    /// 자기 값이 아직 없거나, 기본 CLI 쪽이 한도를 가진 더 최근 값이면 기본 CLI 쪽이다. 같은 계정에 두 숫자를 보이지 않기 위해서다.
    /// 같은 계정인지는 기본 CLI 응답의 `account`만으로 정한다 — 터미널 판정은 쓰지 않는다. 기본 CLI 응답의 값은 이 컴퓨터와
    /// WSL 가운데 가장 최근 기록이라, `account`가 빠지면(두 환경의 로그인이 다름) 값은 다른 로그인의 것일 수 있기 때문이다.
    pub fn snapshot(
        &self,
        scope: &Scope,
    ) -> Snapshot {
        let removed = profile_id(scope)
            .is_some_and(|id| self.profiles_known && !self.profiles.iter().any(|profile| profile.id == id));
        let mut result = if removed {
            Snapshot::Unavailable { message: None, removed: true }
        } else {
            self.snapshots.get(&scope_key(scope)).cloned().unwrap_or(Snapshot::Loading)
        };
        if let (Some(id), false) = (profile_id(scope), removed)
            && let Some(shared) = self.snapshots.get(&format!("{}:default", scope.provider))
            && let Some(reading) = shared.ready()
            && reading.account.as_ref().is_some_and(|account| account.id == id)
        {
            let own_updated = result.ready().and_then(|own| own.updated_at).unwrap_or(0);
            let newer = !reading.windows.is_empty() && reading.updated_at.unwrap_or(0) > own_updated;
            if result.ready().is_none() || newer {
                result = shared.clone();
            }
        }
        result
    }

    /// 계정 목록을 읽은 결과를 반영한다. 실패하면 목록은 그대로 두고 실패만 표시한다.
    pub fn apply_profiles(
        &mut self,
        listed: Option<Vec<AccountProfile>>,
    ) -> Change {
        self.n_changes += 1;
        let mut save = None;
        match listed {
            Some(profiles) => {
                self.profiles = profiles;
                let providers: Vec<String> = self.selected.keys().cloned().collect();
                for provider in providers {
                    let current = self
                        .selected
                        .get(&provider)
                        .and_then(profile_id)
                        .and_then(|id| self.profiles.iter().find(|profile| profile.id == id))
                        .cloned();
                    if let Some(current) = current {
                        self.selected.insert(provider, Scope::of_profile(&current));
                    }
                }
                save = self.drop_removed_restored();
                self.profiles_known = true;
                self.profiles_failed = false;
            }
            None => self.profiles_failed = true,
        }
        Change { redraw: true, reread: false, save }
    }

    /// 설정이 바뀌었으니 진행 중인 읽기의 응답을 버린다(다시 읽는다).
    pub fn preferences_changed(&mut self) {
        self.n_changes += 1;
        self.n_revision += 1;
    }

    /// 한 번의 갱신이 읽을 범위를 정한다.
    ///
    /// 상태 줄에 남은 도구별 범위와, 기본 CLI가 등록 계정으로 보이는 동안의 기본 CLI 범위(로그인이 바뀌면 그 응답의
    /// `account`로 연결이 풀린다)를 읽는다. `all`이면(팝업이 열림) 두 도구의 기본 CLI와 모든 등록 계정도 읽는다.
    /// 기본 Codex 응답이 이미 같은 계정으로 확인됐고 이번에도 그 응답을 읽으면 그 계정 폴더는 따로 읽지 않는다 —
    /// 한도는 계정마다 하나라 같은 값을 `codex app-server` 두 번으로 읽게 되고, 두 읽기가 조금 달라 주기마다 숫자가 오갈 수 있기 때문이다.
    pub fn refresh_plan(
        &self,
        all: bool,
        enabled: &dyn Fn(&str) -> bool,
    ) -> RefreshPlan {
        let mut scopes: BTreeMap<String, Scope> = BTreeMap::new();
        for (provider, _, _) in UsageScopes::PROVIDERS {
            // 목록을 열지 않은 평소에는 상태 줄에 고른 도구만 읽는다. 고르지 않은 도구는 목록을 열 때 읽는다.
            let chosen = self.selected.get(provider).cloned().or_else(|| all.then(|| Scope::provider(provider)));
            if let Some(chosen) = chosen {
                let shown = self.current(&chosen);
                scopes.insert(scope_key(&shown), shown);
                if chosen.profile.is_none() && !chosen.custom {
                    scopes.insert(scope_key(&chosen), chosen);
                }
            }
        }
        if all {
            for (provider, _, _) in UsageScopes::PROVIDERS {
                scopes.insert(format!("{provider}:default"), Scope::provider(provider));
            }
            for profile in &self.profiles {
                let scope = Scope::of_profile(profile);
                scopes.insert(scope_key(&scope), scope);
            }
        }
        let shared_codex = self.snapshots.get("codex:default").and_then(Snapshot::ready);
        let linked_codex_id = if scopes.contains_key("codex:default") {
            shared_codex.and_then(|reading| reading.account.as_ref()).map(|account| account.id.clone())
        } else {
            None
        };
        let skipped = linked_codex_id.as_ref().and_then(|id| scopes.remove(&format!("codex:{id}")));
        let reads = scopes.into_values().filter(|scope| enabled(&scope.provider)).collect();
        RefreshPlan { reads, skipped, linked_codex_id }
    }

    /// 건너뛴 등록 계정을 이번 갱신에 이어서 읽어야 하는지: 기본 Codex 응답이 더는 그 계정으로 확인되지 않을 때다.
    /// 연결이 풀린 계정이 다음 주기까지 `Loading…`으로 남지 않게 한다.
    pub fn skipped_needs_read(
        &self,
        plan: &RefreshPlan,
    ) -> bool {
        let still_linked = self
            .snapshots
            .get("codex:default")
            .and_then(Snapshot::ready)
            .and_then(|reading| reading.account.as_ref())
            .is_some_and(|account| Some(&account.id) == plan.linked_codex_id.as_ref());
        plan.skipped.is_some() && !still_linked
    }

    /// 범위 하나의 읽기를 시작해도 되는지 정한다.
    pub fn begin_read(
        &mut self,
        scope: &Scope,
    ) -> ReadStart {
        self.n_changes += 1;
        let key = scope_key(scope);
        let revision = self.n_revision;
        let removed = matches!(self.snapshot(scope), Snapshot::Unavailable { removed: true, .. });
        if self.pending.get(&key).is_some_and(|ticket| ticket.n_revision == revision) {
            ReadStart::Pending
        } else if removed {
            ReadStart::Nothing
        } else if scope.custom {
            // 미등록 폴더의 사용량은 읽을 곳이 없다. 기본 CLI의 값이나 상태 줄 설정을 대신 쓰지 않는다.
            self.snapshots.insert(
                key,
                Snapshot::Unavailable { message: Some(UsageScopes::CUSTOM_MESSAGE.to_string()), removed: false },
            );
            ReadStart::Nothing
        } else {
            self.n_serial += 1;
            let ticket = Ticket { n_revision: revision, n_serial: self.n_serial };
            self.pending.insert(key, ticket);
            ReadStart::Fetch(ticket)
        }
    }

    /// 읽기 결과를 반영한다. 읽는 동안 선택이 바뀌었거나 더 새 요청이 있으면 버리고 false를 돌려준다.
    pub fn finish_read(
        &mut self,
        scope: &Scope,
        ticket: Ticket,
        snapshot: Snapshot,
    ) -> bool {
        self.n_changes += 1;
        let key = scope_key(scope);
        let mut applied = false;
        if self.n_revision == ticket.n_revision && self.pending.get(&key) == Some(&ticket) {
            self.snapshots.insert(key.clone(), snapshot);
            applied = true;
        }
        if self.pending.get(&key) == Some(&ticket) {
            self.pending.remove(&key);
        }
        applied
    }

    /// 읽는 중인 범위가 있는지.
    pub fn is_reading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// 범위의 읽기 결과를 바로 넣는다(시험·미등록 폴더 안내처럼 읽기 없이 정해지는 값).
    pub fn set_snapshot(
        &mut self,
        scope: &Scope,
        snapshot: Snapshot,
    ) {
        self.n_changes += 1;
        self.snapshots.insert(scope_key(scope), snapshot);
    }

    /// 기본 CLI 읽기 결과의 키(`<도구>:default`)로 넣어 둔 값.
    pub fn default_reading(
        &self,
        provider: &str,
    ) -> Option<&Reading> {
        self.snapshots.get(&format!("{provider}:default")).and_then(Snapshot::ready)
    }

    /// 다시 시도 단추가 필요한 읽기 실패가 있는지: 계정 목록을 못 읽었거나, 읽지 못한 등록·기본 범위가 있을 때다.
    /// 미등록 폴더는 다시 읽어도 값이 생기지 않으므로 대상이 아니다.
    pub fn retry_visible(
        &self,
        enabled: &dyn Fn(&str) -> bool,
    ) -> bool {
        let selected: Vec<Scope> = self.selected.values().map(|scope| self.current(scope)).collect();
        self.profiles_failed
            || self.profiles.iter().map(Scope::of_profile).chain(selected).any(|scope| {
                matches!(self.snapshot(&scope), Snapshot::Unavailable { removed: false, .. })
                    && !scope.custom
                    && enabled(&scope.provider)
            })
    }

    /// 상태 줄·목록에 쓰는 범위 이름. 기본 CLI는 사용량 응답이 같은 계정으로 확인한 등록 계정 이름을 쓰고, 아니면 범위 이름이다.
    pub fn display_name(
        &self,
        scope: &Scope,
    ) -> String {
        let matched = if scope.profile.is_none() {
            self.snapshot(scope)
                .ready()
                .and_then(|reading| reading.account.as_ref())
                .map(|account| account.label.clone())
        } else {
            None
        };
        matched.unwrap_or_else(|| scope_name(Some(scope)))
    }
}

/// 보고 있는 터미널의 범위: 계정 터미널이나 실행 중인 에이전트의 범위가 있으면 그것, 없고 앞쪽 프로그램이 도구이면 그 도구의
/// 기본 CLI, 아니면 None이다. 같은 터미널에서 도구나 계정이 바뀌면 다시 고르게 하는 키(`observe_key`)와 함께 쓴다.
///
/// Examples
/// --------
/// | 터미널 범위        | 프로그램 | 결과                |
/// |--------------------|----------|---------------------|
/// | 등록 계정 Work     | `claude` | Work                |
/// | 등록 계정 Work     | `bash`   | Work (계정 터미널)  |
/// | (없음)             | `codex`  | codex 기본 CLI      |
/// | (없음)             | `bash`   | None                |
pub fn observed_scope(
    account: Option<&Scope>,
    program: Option<&str>,
) -> Option<Scope> {
    account.cloned().or_else(|| program.filter(|name| is_known_provider(name)).map(Scope::provider))
}

/// 터미널이 바뀌거나 그 터미널의 도구·범위·연결이 바뀌었는지 가르는 키.
pub fn observe_key(
    terminal: Option<u32>,
    scope: Option<&Scope>,
    program: Option<&str>,
) -> String {
    format!(
        "{}:{}:{}:{}",
        terminal.map_or_else(String::new, |id| id.to_string()),
        program.unwrap_or(""),
        scope.map_or_else(String::new, scope_key),
        scope.and_then(|scope| scope.linked.as_ref()).map_or("", |linked| linked.id.as_str()),
    )
}

// -- 상태 줄에 보일 모양 --

/// 게이지 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeterView {
    pub id: String,
    pub label: String,
    pub used: i64,
    /// 말풍선 문장.
    pub title: String,
}

/// 오래된 값 옆의 경과 표시.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgeView {
    /// 상태 줄에 붙는 짧은 글자(`18h ago`, 시각을 모르면 `recorded`).
    pub short: String,
    /// 목록 행과 말풍선에 쓰는 문장.
    pub explanation: String,
}

/// 범위 하나의 값이 보일 모양.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueView {
    Meters {
        meters: Vec<MeterView>,
        /// 실시간 값이 아닌 세션 기록이거나 오래된 기록이라 흐리게 그린다.
        stale: bool,
        /// 창마다 초기화까지 남은 시간과 시각을 이은 문장(목록 행이 보인다).
        resets: Option<String>,
        age: Option<AgeView>,
    },
    /// 값이 없다: `Loading…`·`Unavailable`·`No usage yet`·`Not reported`와 그 설명.
    State { text: &'static str, title: String },
}

/// 범위의 읽기 상태를 보일 모양으로 바꾼다.
///
/// Claude는 사용자가 끈(`off`) 기록만 숨긴다 — `unset`이어도 값이 있으면 보인다. 다른 Paddock 설정 폴더의 상태 줄이
/// 같은 기록을 쓰고 있어 이 설정 폴더의 자동 켜기가 물러선 경우다.
///
/// Examples
/// --------
/// | 상태                                  | 결과                                         |
/// |---------------------------------------|----------------------------------------------|
/// | 값 있음, 1분 전                       | 게이지만                                     |
/// | 값 있음, 18시간 전                    | 흐린 게이지 + `18h ago`                      |
/// | 세션 기록(2분 전)                     | 흐린 게이지 + `2m ago` + 기록 안내           |
/// | Claude `on`인데 창 없음               | `No usage yet`                               |
/// | 읽기 실패                             | `Unavailable`                                |
pub fn value_view(
    snapshot: &Snapshot,
    scope: &Scope,
    display_name: &str,
    now: i64,
) -> ValueView {
    let key = scope_key(scope);
    let windows = match snapshot {
        Snapshot::Ready(reading) if scope.provider != "claude" || reading.state != ClaudeState::Off => {
            current_windows(&reading.windows, now)
        }
        _ => Vec::new(),
    };
    let reading = snapshot.ready();
    if let (false, Some(reading)) = (windows.is_empty(), reading) {
        let age = reading.updated_at.map_or(0, |updated| (now - updated).max(0));
        let stale = reading.recorded || age >= UsageScopes::STALE_SECONDS;
        let elapsed = duration(age).trim_start_matches("0h ").to_string();
        let updated =
            if reading.updated_at.is_some() { format!(" \u{b7} Usage recorded {elapsed} ago") } else { String::new() };
        let reason = if reading.recorded { format!(" \u{b7} {}", UsageScopes::RECORDED_HINT) } else { String::new() };
        let title = provider_title(&scope.provider);
        let meters = windows
            .iter()
            .map(|window| MeterView {
                id: format!("{key}-{}", window.label),
                label: window.label.clone(),
                used: window.used,
                title: format!(
                    "{title} \u{b7} {display_name} \u{b7} {}: {}{updated}{reason}",
                    window_name(&window.label),
                    describe(window, now)
                ),
            })
            .collect();
        let resets: Vec<String> = windows
            .iter()
            .filter_map(|window| match window.reset {
                Reset::At(at) => Some(format!(
                    "{} resets in {} ({})",
                    window_name(&window.label),
                    duration((at - now).max(0)),
                    reset_clock(window, now)
                )),
                _ => None,
            })
            .collect();
        let age_view = stale.then(|| {
            let when =
                if reading.updated_at.is_some() { format!("{elapsed} ago") } else { "at an unknown time".to_string() };
            let tail = if reading.recorded {
                format!(" \u{b7} {}", UsageScopes::RECORDED_HINT)
            } else {
                ". It may have changed since then.".to_string()
            };
            AgeView {
                short: if reading.updated_at.is_some() {
                    format!("{} ago", short_age(age))
                } else {
                    "recorded".to_string()
                },
                explanation: format!("Usage recorded {when}{tail}"),
            }
        });
        ValueView::Meters {
            meters,
            stale,
            resets: (!resets.is_empty()).then(|| resets.join(" \u{b7} ")),
            age: age_view,
        }
    } else {
        // 사용량 기록이 아직 없다는 뜻이다. 에이전트가 응답을 기다린다는 진행 상태로 읽히지 않게 사용량을 주어로 쓴다.
        let waiting_for_claude =
            scope.provider == "claude" && reading.is_some_and(|reading| reading.state == ClaudeState::On);
        let (text, title) = match snapshot {
            Snapshot::Unavailable { removed: true, .. } => ("Unavailable", UsageScopes::REMOVED_HINT.to_string()),
            Snapshot::Unavailable { message, removed: false } => {
                ("Unavailable", message.clone().unwrap_or_else(|| UsageScopes::RETRY_HINT.to_string()))
            }
            Snapshot::Loading => ("Loading\u{2026}", UsageScopes::NOT_REPORTED_HINT.to_string()),
            Snapshot::Ready(_) if waiting_for_claude => ("No usage yet", UsageScopes::WAITING_HINT.to_string()),
            Snapshot::Ready(_) => ("Not reported", UsageScopes::NOT_REPORTED_HINT.to_string()),
        };
        ValueView::State { text, title }
    }
}

/// 상태 줄의 묶음 하나(도구 한 개의 선택된 범위).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupView {
    pub provider: String,
    pub mark: &'static str,
    pub title: &'static str,
    pub name: String,
    pub key: String,
    pub tooltip: String,
    pub value: ValueView,
}

/// 상태 줄에 보일 묶음들: 도구별 마지막 선택(`current`로 다듬은 것)에서 켜진 도구만, Claude 다음 Codex 순서로 만든다.
pub fn bar_groups(
    book: &UsageBook,
    enabled: &dyn Fn(&str) -> bool,
    now: i64,
) -> Vec<GroupView> {
    UsageScopes::PROVIDERS
        .iter()
        .filter(|(provider, _, _)| enabled(provider))
        .filter_map(|(provider, title, mark)| {
            let scope = book.current(book.selected(provider)?);
            let name = book.display_name(&scope);
            let value = value_view(&book.snapshot(&scope), &scope, &name, now);
            Some(GroupView {
                provider: provider.to_string(),
                mark,
                title,
                key: scope_key(&scope),
                tooltip: format!("{title} \u{b7} {name}. Show usage for all accounts."),
                name,
                value,
            })
        })
        .collect()
}

// -- 사용량 목록(팝업) --

/// 목록 행 하나: 등록 계정·상태 줄에 고른 기본 CLI·미등록 폴더 가운데 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopupRow {
    pub scope: Scope,
    pub key: String,
    pub name: String,
    /// 상태 줄에 지금 이 범위가 보인다(`In status bar` 배지).
    pub in_bar: bool,
    /// 기본 CLI가 이 계정으로 로그인했다고 확인되었다(`Default` 배지).
    pub is_default_cli: bool,
    /// 이름 아래 환경 줄(`This device`·`WSL · Ubuntu`·`Folder set in the terminal`·`Signed in as …`·`Not linked to a saved account`).
    pub environment: String,
    /// 그 도구의 상태 줄 항목이 켜져 있다. 꺼져 있으면 값 대신 `Hidden in Quick settings`가 보인다.
    pub enabled: bool,
    /// `Open in new terminal`을 보인다(미등록 폴더는 Paddock이 다시 열 수 있는 환경이 아니다).
    pub can_open: bool,
    pub value: ValueView,
}

/// 도구 하나의 목록 절.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopupSection {
    pub provider: &'static str,
    pub title: &'static str,
    pub rows: Vec<PopupRow>,
}

/// 검색 결과가 없을 때의 안내.
pub const NO_MATCHING_ACCOUNTS: &str = "No matching accounts.";
/// 힌트 줄.
pub const POPUP_HINT: &str = "Percent of each limit used.";
/// 계정 목록을 읽지 못했을 때의 안내.
pub const ACCOUNTS_FAILED: &str = "Accounts could not be loaded. Select Retry.";

/// 목록에 검색칸을 보일지: 계정이 `N_SEARCH_THRESHOLD`개 이상이거나, 이미 질의가 입력돼 있을 때다(계정이 줄어도 입력 중인 질의는 지우지 않는다).
pub fn search_visible(
    n_profiles: usize,
    query: &str,
) -> bool {
    n_profiles >= UsageScopes::N_SEARCH_THRESHOLD || !query.is_empty()
}

fn environment_text(
    scope: &Scope,
    snapshot: &Snapshot,
) -> String {
    match &scope.profile {
        Some(profile) if profile.runtime == crate::agent_account::Runtime::Wsl => {
            format!("WSL \u{b7} {}", profile.wsl_distribution.as_deref().unwrap_or(""))
        }
        Some(_) => "This device".to_string(),
        None if scope.custom => "Folder set in the terminal".to_string(),
        None => match snapshot.ready().and_then(|reading| reading.account.as_ref()) {
            Some(account) => format!("Signed in as {}", account.label),
            None => "Not linked to a saved account".to_string(),
        },
    }
}

/// 목록 절들: 도구마다 한 절, 질의(`도구 이름 + 계정 이름`에 대소문자 무시로 포함)에 맞는 행만, 맞는 행이 없는 절은 빠진다.
///
/// 기본 CLI는 로그인이 등록 계정 하나와 같다고 확인되면 그 계정 행에 합쳐지고(`Default` 배지), 아니면 자기 행을 맨 위에 쥔다.
/// 상태 줄에 미등록 폴더가 남아 있으면 그 행도 맨 위에 쥔다.
pub fn popup_sections(
    book: &UsageBook,
    enabled: &dyn Fn(&str) -> bool,
    query: &str,
    now: i64,
) -> Vec<PopupSection> {
    let query = query.trim().to_lowercase();
    let mut scopes: Vec<Scope> = book
        .selected_all()
        .filter(|scope| scope.profile.is_none() && book.current(scope).profile.is_none())
        .cloned()
        .collect();
    scopes.extend(book.profiles().iter().map(Scope::of_profile));
    UsageScopes::PROVIDERS
        .iter()
        .filter_map(|(provider, title, _)| {
            let rows: Vec<PopupRow> = scopes
                .iter()
                .filter(|scope| scope.provider == *provider)
                .filter(|scope| format!("{title} {}", scope_name(Some(scope))).to_lowercase().contains(&query))
                .map(|scope| {
                    let snapshot = book.snapshot(scope);
                    let selected_key = book.selected(provider).map(|chosen| scope_key(&book.current(chosen)));
                    let name = book.display_name(scope);
                    let is_enabled = enabled(provider);
                    PopupRow {
                        key: scope_key(scope),
                        in_bar: selected_key.as_deref() == Some(scope_key(scope).as_str()),
                        is_default_cli: scope.profile.as_ref().is_some_and(|profile| {
                            book.linked_profile(provider, book.selected(provider))
                                .is_some_and(|linked| linked.id == profile.id)
                        }),
                        environment: environment_text(scope, &snapshot),
                        enabled: is_enabled,
                        can_open: !scope.custom,
                        value: if is_enabled {
                            value_view(&snapshot, scope, &name, now)
                        } else {
                            ValueView::State { text: "Hidden in Quick settings", title: String::new() }
                        },
                        name: scope_name(Some(scope)),
                        scope: scope.clone(),
                    }
                })
                .collect();
            (!rows.is_empty()).then_some(PopupSection { provider, title, rows })
        })
        .collect()
}

/// 상태 줄 오른쪽에 사용량 묶음을 펼칠지 `Usage` 단추 하나로 접을지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarMode {
    Full,
    Compact,
}

/// 묶음 하나가 차지할 폭의 추정(화면 크기 단위, 100% 배율).
///
/// 글자 폭은 평균값(`usage_compact::CHAR_WIDTH`·모노 `MONO_CHAR_WIDTH`)으로 센다. 정확한 측정이 아니라
/// 접을지 정하는 판단용 추정이라, 접는 경계가 몇 픽셀 어긋나도 괜찮도록 계정 이름은 최대 폭(120)을 넘지 않게만 센다.
pub fn group_width(group: &GroupView) -> f32 {
    let name_width =
        (group.name.chars().count() as f32 * usage_compact::CHAR_WIDTH).min(usage_group::ACCOUNT_NAME_MAX_WIDTH);
    let head = usage_group::PADDING_LEFT
        + usage_group::PADDING_RIGHT
        + usage_group::MARK_SIZE
        + usage_group::GAP
        + group.title.chars().count() as f32 * usage_compact::CHAR_WIDTH
        + usage_group::GAP
        + name_width;
    let body = match &group.value {
        ValueView::Meters { meters, age, .. } => {
            let meters_width: f32 = meters
                .iter()
                .map(|meter_view| {
                    usage_group::FIRST_METER_MARGIN
                        + (meter_view.label.chars().count() + 4) as f32 * usage_compact::MONO_CHAR_WIDTH
                        + meter::BAR_WIDTH
                        + 2.0 * meter::GAP
                })
                .sum();
            meters_width
                + age
                    .as_ref()
                    .map_or(0.0, |age| usage_group::GAP + age.short.chars().count() as f32 * usage_compact::CHAR_WIDTH)
        }
        ValueView::State { text, .. } => usage_group::GAP + text.chars().count() as f32 * usage_compact::CHAR_WIDTH,
    };
    head + usage_group::GAP + body
}

/// 사용량 묶음을 펼칠 자리가 있는지로 모드를 정한다.
///
/// 상태 줄 폭에서 양쪽 여백과 오른쪽의 다른 항목(검토 메모 수·저장 표시)을 빼고, 왼쪽 글자가 쥐는 폭
/// (`usage_compact::LEFT_MIN_WIDTH`까지)을 남긴 나머지가 묶음 폭의 합보다 작으면 `Compact`다.
/// 묶음이 없으면 접을 것도 없어 `Full`이다.
///
/// Examples
/// --------
/// | 창 폭 | 묶음 둘 합 | 오른쪽 다른 항목 | 결과      |
/// |-------|------------|------------------|-----------|
/// | 1100  | 560        | 0                | `Full`    |
/// | 720   | 560        | 0                | `Compact` |
pub fn bar_mode(
    bar_width: f32,
    left_text_chars: usize,
    other_right_width: f32,
    groups: &[GroupView],
    padding_x: f32,
) -> BarMode {
    let left = (left_text_chars as f32 * usage_compact::CHAR_WIDTH).min(usage_compact::LEFT_MIN_WIDTH);
    let available = bar_width - 2.0 * padding_x - left - other_right_width;
    let needed: f32 =
        groups.iter().map(group_width).sum::<f32>() + usage_group::GROUP_GAP * groups.len().saturating_sub(1) as f32;
    if !groups.is_empty() && needed > available { BarMode::Compact } else { BarMode::Full }
}

/// 누를 수 있는 `Usage` 단추가 필요한지: 접혔거나, 고른 범위가 없어 묶음이 하나도 없는데 켜진 도구가 있을 때다.
pub fn compact_button_visible(
    mode: BarMode,
    groups: &[GroupView],
    any_enabled: bool,
) -> bool {
    any_enabled && (mode == BarMode::Compact || groups.is_empty())
}

/// 말풍선이 없는 짧은 요약(자동 점검 상태 글): `Claude=Default:5h=23,week=61` 꼴.
pub fn group_summary(group: &GroupView) -> String {
    let value = match &group.value {
        ValueView::Meters { meters, stale, age, .. } => {
            let list = meters.iter().map(|view| format!("{}={}", view.label, view.used)).collect::<Vec<_>>().join(",");
            let mut text = list;
            if *stale {
                text.push_str(&format!(" stale={}", age.as_ref().map_or("", |age| age.short.as_str())));
            }
            text
        }
        ValueView::State { text, .. } => format!("({text})"),
    };
    format!("{}={}:{value}", group.provider, group.name)
}

/// 링크 연결 정보(`Linked`)를 `Scope`에 덧씌운 기본 CLI 범위.
pub fn with_link(
    scope: &Scope,
    linked: Option<Linked>,
) -> Scope {
    Scope { linked, ..scope.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_account::Runtime;
    use crate::usage_model::UsageWindow;

    fn profile(
        id: &str,
        provider: &str,
        label: &str,
    ) -> AccountProfile {
        AccountProfile {
            id: id.to_string(),
            provider: provider.to_string(),
            label: label.to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        }
    }

    fn reading(
        used: i64,
        updated_at: i64,
    ) -> Reading {
        Reading {
            windows: vec![UsageWindow { label: "5h".to_string(), used, reset: Reset::At(9_999_999_999) }],
            updated_at: Some(updated_at),
            state: ClaudeState::On,
            ..Reading::default()
        }
    }

    fn ready(
        used: i64,
        updated_at: i64,
    ) -> Snapshot {
        Snapshot::Ready(reading(used, updated_at))
    }

    fn with_account(
        mut reading: Reading,
        id: &str,
        label: &str,
    ) -> Snapshot {
        reading.account = Some(Linked { id: id.to_string(), label: label.to_string() });
        Snapshot::Ready(reading)
    }

    fn book_with(profiles: Vec<AccountProfile>) -> UsageBook {
        let mut book = UsageBook::default();
        book.apply_profiles(Some(profiles));
        book
    }

    fn all_enabled(_provider: &str) -> bool {
        true
    }

    #[test]
    fn test_c_account_usage_isolation_each_provider_keeps_its_last_selected_account_and_its_own_usage() {
        let claude = profile("c", "claude", "Main");
        let codex = profile("x", "codex", "Main");
        let mut book = book_with(vec![claude.clone(), codex.clone()]);
        book.select(Some(Scope::of_profile(&claude)));
        book.select(Some(Scope::of_profile(&codex)));
        book.set_snapshot(&Scope::of_profile(&claude), ready(12, 100));
        book.set_snapshot(&Scope::of_profile(&codex), ready(73, 100));
        book.set_snapshot(&Scope::provider("codex"), ready(99, 100));
        let used = |snapshot: Snapshot| snapshot.ready().map(|reading| reading.windows[0].used);
        assert_eq!(used(book.snapshot(&Scope::of_profile(&claude))), Some(12));
        assert_eq!(used(book.snapshot(&Scope::of_profile(&codex))), Some(73));
        assert_eq!(book.selected("claude").and_then(profile_id), Some("c"));
        assert_eq!(book.selected("codex").and_then(profile_id), Some("x"));
        assert_eq!(book.active().and_then(profile_id), Some("x"));
        assert_eq!(used(book.snapshot(&Scope::provider("codex"))), Some(99));
        assert_eq!(scope_name(Some(&Scope::provider("codex"))), "Default");
    }

    #[test]
    fn test_c_account_usage_active_a_shell_clears_the_active_account_and_an_unregistered_cli_never_inherits_its_identity()
     {
        let account = profile("c", "claude", "Main");
        let mut book = book_with(vec![account.clone()]);
        assert!(book.active().is_none());
        book.select(Some(Scope::of_profile(&account)));
        assert_eq!(book.active().and_then(profile_id), Some("c"));
        book.select(None);
        assert!(book.active().is_none());
        assert_eq!(
            book.selected("claude").and_then(profile_id),
            Some("c"),
            "the shell keeps the last selected account"
        );
        book.select(Some(Scope::provider("claude")));
        assert_eq!(book.active().and_then(profile_id), None);
        assert_eq!(scope_name(book.active()), "Default");
        let change = book.select(Some(Scope::of_profile(&account)));
        assert!(change.redraw && change.reread, "switching back to the account redraws and rereads");
    }

    #[test]
    fn test_c_account_usage_empty_initial_failure_and_empty_records_have_explicit_different_states() {
        let mut book = book_with(Vec::new());
        let claude = Scope::provider("claude");
        let failed = Snapshot::Unavailable { message: None, removed: false };
        book.set_snapshot(&claude, failed.clone());
        assert_eq!(book.snapshot(&claude), failed);
        book.set_snapshot(&claude, Snapshot::Ready(Reading::default()));
        assert!(matches!(book.snapshot(&claude), Snapshot::Ready(reading) if reading.windows.is_empty()));
    }

    #[test]
    fn test_c_account_usage_stale_a_to_b_to_a_rejects_the_first_a_response_even_after_the_second_a_completes() {
        let a = profile("a", "codex", "A");
        let b = profile("b", "codex", "B");
        let mut book = book_with(vec![a.clone(), b.clone()]);
        let (scope_a, scope_b) = (Scope::of_profile(&a), Scope::of_profile(&b));
        book.select(Some(scope_a.clone()));
        let ReadStart::Fetch(first_a) = book.begin_read(&scope_a) else { panic!("a read starts") };
        book.select(Some(scope_b.clone()));
        let ReadStart::Fetch(read_b) = book.begin_read(&scope_b) else { panic!("b read starts") };
        assert!(book.finish_read(&scope_b, read_b, ready(44, 1)));
        book.select(Some(scope_a.clone()));
        let ReadStart::Fetch(second_a) = book.begin_read(&scope_a) else { panic!("a read starts again") };
        assert!(book.finish_read(&scope_a, second_a, ready(22, 1)));
        assert!(!book.finish_read(&scope_a, first_a, ready(99, 1)), "the response of the first A request is dropped");
        assert_eq!(book.snapshot(&scope_a).ready().map(|reading| reading.windows[0].used), Some(22));
        assert_eq!(book.snapshot(&scope_b).ready().map(|reading| reading.windows[0].used), Some(44));
    }

    #[test]
    fn test_c_account_usage_isolation_renaming_updates_the_label_and_removing_makes_the_saved_selection_unavailable() {
        let account = profile("a", "codex", "Original");
        let mut book = book_with(vec![account.clone()]);
        book.select(Some(Scope::of_profile(&account)));
        book.apply_profiles(Some(vec![profile("a", "codex", "Renamed")]));
        let current = book.current(book.selected("codex").unwrap());
        assert_eq!(scope_name(Some(&current)), "Renamed");
        book.apply_profiles(Some(Vec::new()));
        assert_eq!(book.snapshot(&Scope::of_profile(&account)), Snapshot::Unavailable { message: None, removed: true });
        assert_eq!(
            book.selected("codex").and_then(profile_id),
            Some("a"),
            "the removed account stays selected as Unavailable"
        );
    }

    #[test]
    fn test_c_d74_removed_account_selection_is_unavailable_not_default() {
        let account = profile("a", "claude", "Main");
        let mut book = book_with(vec![account.clone()]);
        book.set_snapshot(&Scope::provider("claude"), ready(88, 1));
        book.apply_profiles(Some(Vec::new()));
        let snapshot = book.snapshot(&Scope::of_profile(&account));
        assert!(
            matches!(snapshot, Snapshot::Unavailable { removed: true, .. }),
            "the default value never fills a removed account"
        );
        let view = value_view(&snapshot, &Scope::of_profile(&account), "Main", 100);
        assert!(matches!(view, ValueView::State { text: "Unavailable", ref title } if title.contains("removed")));
    }

    #[test]
    fn test_c_d74_a_default_cli_reading_never_fills_a_saved_account() {
        let account = profile("m", "claude", "Main");
        let book = {
            let mut book = book_with(vec![account.clone()]);
            book.set_snapshot(&Scope::provider("claude"), ready(88, 5000));
            book
        };
        assert!(
            matches!(book.snapshot(&Scope::of_profile(&account)), Snapshot::Loading),
            "no account in the default reading, no sharing"
        );
        let mut linked = book;
        linked.set_snapshot(&Scope::provider("claude"), with_account(reading(88, 5000), "m", "Main"));
        assert_eq!(
            linked.snapshot(&Scope::of_profile(&account)).ready().map(|reading| reading.windows[0].used),
            Some(88),
            "a default reading that names the account is that account's value"
        );
    }

    #[test]
    fn test_c_account_usage_custom_an_unregistered_folder_never_reads_or_borrows_current_cli_usage() {
        let mut book = book_with(Vec::new());
        let custom = Scope::custom("codex");
        book.select(Some(custom.clone()));
        let plan = book.refresh_plan(false, &all_enabled);
        let keys: Vec<String> = plan.reads.iter().map(scope_key).collect();
        assert!(
            !keys.contains(&"codex:default".to_string()),
            "the status bar never reads the default CLI for an unregistered folder"
        );
        assert!(keys.contains(&"codex:custom".to_string()));
        assert_eq!(book.begin_read(&custom), ReadStart::Nothing);
        match book.snapshot(&custom) {
            Snapshot::Unavailable { message: Some(message), .. } => assert!(message.contains("not a saved account")),
            other => panic!("{other:?}"),
        }
        let all = book.refresh_plan(true, &all_enabled);
        assert!(
            all.reads.iter().any(|scope| scope_key(scope) == "codex:default"),
            "only the popup list reads the default CLI as its own row"
        );
        assert_eq!(scope_key(&custom), "codex:custom");
        assert_eq!(scope_name(Some(&custom)), "Unregistered folder");
    }

    #[test]
    fn test_c_account_usage_observe_a_different_cli_in_an_account_terminal_selects_that_cli_and_returns_to_the_account_afterwards()
     {
        let work = Scope::of_profile(&profile("c", "claude", "Work Claude"));
        let selections: Vec<Option<Scope>> = [
            (Some(&work), Some("claude")),
            (Some(&Scope::provider("codex")), Some("codex")),
            (Some(&work), Some("bash")),
            (None, Some("bash")),
        ]
        .into_iter()
        .map(|(account, program)| observed_scope(account, program))
        .collect();
        assert_eq!(selections, vec![Some(work.clone()), Some(Scope::provider("codex")), Some(work.clone()), None]);
        let key =
            |terminal: u32, scope: Option<&Scope>, program: &str| observe_key(Some(terminal), scope, Some(program));
        assert_eq!(
            key(1, Some(&work), "claude"),
            key(1, Some(&work), "claude"),
            "the same terminal and tool selects once"
        );
        assert_ne!(key(1, Some(&work), "claude"), key(1, Some(&Scope::provider("codex")), "codex"));
        assert_ne!(key(1, Some(&work), "claude"), key(2, Some(&work), "claude"));
        let linked =
            Scope { linked: Some(Linked { id: "x".to_string(), label: "X".to_string() }), ..Scope::provider("codex") };
        assert_ne!(
            key(1, Some(&Scope::provider("codex")), "codex"),
            key(1, Some(&linked), "codex"),
            "a link appearing selects again"
        );
    }

    #[test]
    fn test_c_account_usage_custom_leaving_an_unregistered_folder_restores_the_previous_selection_instead_of_keeping_it_in_the_bar()
     {
        let work = Scope::of_profile(&profile("x", "codex", "Work Codex"));
        let mut book = book_with(vec![profile("x", "codex", "Work Codex")]);
        book.select(Some(work.clone()));
        book.select(Some(Scope::custom("codex")));
        assert!(
            book.selected("codex").unwrap().custom,
            "the unregistered folder is shown while its terminal is selected"
        );
        book.select(None);
        assert_eq!(
            book.selected("codex"),
            Some(&work),
            "a shell brings back the account chosen before the unregistered folder"
        );
        book.select(Some(Scope::custom("codex")));
        book.select(Some(Scope::provider("claude")));
        assert_eq!(
            book.selected("codex"),
            Some(&work),
            "choosing another provider also clears the unregistered folder"
        );
        let mut fresh = book_with(Vec::new());
        fresh.select(Some(Scope::custom("codex")));
        fresh.select(None);
        assert_eq!(
            fresh.selected("codex"),
            Some(&Scope::provider("codex")),
            "without an earlier choice the bar returns to the default CLI"
        );
    }

    #[test]
    fn test_c_account_usage_linked_a_default_cli_verified_as_a_saved_account_is_shown_as_that_account_with_one_reading()
    {
        let work = profile("x", "codex", "Work Codex");
        let other = profile("y", "codex", "Other Codex");
        let mut book = book_with(vec![work.clone(), other.clone()]);
        book.select(Some(Scope::provider("codex")));
        let default = Scope::provider("codex");
        book.set_snapshot(&default, with_account(reading(30, 100), "x", "Work Codex"));
        book.set_snapshot(&Scope::of_profile(&work), ready(10, 50));
        book.set_snapshot(&Scope::of_profile(&other), ready(20, 50));
        assert_eq!(book.linked_profile("codex", None).map(|profile| profile.id.as_str()), Some("x"));
        assert_eq!(
            profile_id(&book.current(&default)),
            Some("x"),
            "the status bar shows the saved account instead of the default CLI"
        );
        assert!(book.current(&Scope::custom("codex")).custom);
        let used =
            |book: &UsageBook, scope: &Scope| book.snapshot(scope).ready().map(|reading| reading.windows[0].used);
        assert_eq!(
            used(&book, &Scope::of_profile(&work)),
            Some(30),
            "the newer default reading of the same account replaces the older one"
        );
        assert_eq!(used(&book, &Scope::of_profile(&other)), Some(20), "other accounts keep their own reading");
        book.set_snapshot(&Scope::of_profile(&work), ready(10, 200));
        assert_eq!(
            used(&book, &Scope::of_profile(&work)),
            Some(10),
            "the account keeps its own reading when it is newer"
        );
        book.set_snapshot(&default, ready(30, 100));
        assert!(
            book.linked_profile("codex", None).is_none(),
            "an ambiguous or different sign-in leaves the default CLI alone"
        );
        assert_eq!(profile_id(&book.current(&default)), None);
        let terminal_link =
            Scope { linked: Some(Linked { id: "x".to_string(), label: "Work Codex".to_string() }), ..default.clone() };
        assert_eq!(
            profile_id(&book.current(&terminal_link)),
            Some("x"),
            "a terminal link names the account before any reading arrives"
        );
        let gone = Scope { linked: Some(Linked { id: "gone".to_string(), label: "Gone".to_string() }), ..default };
        assert_eq!(profile_id(&book.current(&gone)), None, "an account not in the list is not linked");
    }

    #[test]
    fn test_c_account_usage_linked_the_bar_keeps_reading_the_default_cli_while_it_is_shown_as_the_saved_account() {
        let work = profile("x", "codex", "Work Codex");
        let mut book = book_with(vec![work.clone()]);
        book.select(Some(Scope::provider("codex")));
        book.set_snapshot(&Scope::provider("codex"), with_account(reading(5, 100), "x", "Work Codex"));
        let enabled = |provider: &str| provider == "codex";
        let first = book.refresh_plan(false, &enabled);
        let all = book.refresh_plan(true, &enabled);
        for plan in [&first, &all] {
            let keys: Vec<String> = plan.reads.iter().map(scope_key).collect();
            assert!(
                keys.contains(&"codex:default".to_string()),
                "the default reading is read each time so a changed sign-in unlinks it: {keys:?}"
            );
            assert!(
                !keys.contains(&"codex:x".to_string()),
                "once linked, the same account is not read again through its own folder: {keys:?}"
            );
        }
        assert_eq!(all.skipped.as_ref().map(scope_key), Some("codex:x".to_string()));
        assert_eq!(book.snapshot(&Scope::of_profile(&work)).ready().map(|reading| reading.windows[0].used), Some(5));
        assert!(!book.skipped_needs_read(&all), "still linked: the skipped account stays skipped");
        book.set_snapshot(&Scope::provider("codex"), ready(40, 200));
        assert!(
            book.skipped_needs_read(&all),
            "the link ended in this reading, so the skipped account is read in the same cycle"
        );
    }

    #[test]
    fn test_c_account_usage_linked_a_terminal_link_ends_when_the_agent_exits_so_signing_in_to_another_account_unlinks_it()
     {
        let work = profile("x", "codex", "Work Codex");
        let mut book = book_with(vec![work.clone()]);
        let linked = Scope {
            linked: Some(Linked { id: "x".to_string(), label: "Work Codex".to_string() }),
            ..Scope::provider("codex")
        };
        book.select(Some(linked));
        assert_eq!(profile_id(&book.current(book.selected("codex").unwrap())), Some("x"));
        let change = book.select(None);
        assert!(
            book.selected("codex").unwrap().linked.is_none(),
            "the terminal link is dropped once the agent is gone"
        );
        assert!(change.redraw, "the bar is redrawn even though the active scope key did not change");
        book.set_snapshot(&Scope::provider("codex"), with_account(reading(40, 100), "x", "Work Codex"));
        assert_eq!(
            profile_id(&book.current(book.selected("codex").unwrap())),
            Some("x"),
            "the default reading still confirms the same account until it changes"
        );
        book.set_snapshot(&Scope::provider("codex"), ready(40, 200));
        assert_eq!(
            profile_id(&book.current(book.selected("codex").unwrap())),
            None,
            "the new sign-in shows as the default CLI again"
        );
        let linked_again = Scope {
            linked: Some(Linked { id: "x".to_string(), label: "Work Codex".to_string() }),
            ..Scope::provider("codex")
        };
        book.select(Some(linked_again));
        book.select(Some(Scope::provider("claude")));
        assert!(
            book.selected("codex").unwrap().linked.is_none(),
            "moving to another tool's terminal also ends this tool's link"
        );
    }

    #[test]
    fn test_c_account_usage_linked_a_terminal_link_names_the_account_but_only_a_default_reading_of_that_account_replaces_its_value()
     {
        let main = profile("m", "claude", "Main");
        let mut book = book_with(vec![main.clone()]);
        let link = Scope {
            linked: Some(Linked { id: "m".to_string(), label: "Main".to_string() }),
            ..Scope::provider("claude")
        };
        book.select(Some(link));
        book.set_snapshot(&Scope::of_profile(&main), ready(84, 100));
        book.set_snapshot(&Scope::provider("claude"), ready(97, 5000));
        let shown = book.current(book.selected("claude").unwrap());
        assert_eq!(profile_id(&shown), Some("m"), "the bar names the saved account");
        assert_eq!(
            book.snapshot(&shown).ready().map(|reading| reading.windows[0].used),
            Some(84),
            "without a matching account in the default reading, the account keeps its own reading"
        );
        book.set_snapshot(&Scope::provider("claude"), with_account(reading(97, 5000), "m", "Main"));
        assert_eq!(book.snapshot(&shown).ready().map(|reading| reading.windows[0].used), Some(97));
    }

    #[test]
    fn test_c_account_usage_restore_the_last_account_per_tool_survives_a_restart_without_inventing_or_reviving_accounts()
     {
        let claude = profile("a", "claude", "Main");
        let codex = profile("c", "codex", "Work");
        let expected =
            UsageSelection { claude: Some(Scope::provider("claude")), codex: Some(Scope::of_profile(&codex)) };
        let mut saved = UsageBook::default();
        // 저장 값을 되살리기 전의 선택은 저장하지 않는다 — 아직 읽지 않은 다른 도구의 선택을 지우지 않기 위해서다.
        assert_eq!(saved.select(Some(Scope::of_profile(&claude))).save, None);
        saved.restore_selection(&UsageSelection::default());
        saved.select(Some(Scope::of_profile(&codex)));
        saved.select(Some(Scope::custom("codex")));
        // 미등록 폴더는 직전 계정으로, 터미널 판정 연결은 빼고 저장한다.
        let change = saved.select(Some(Scope {
            linked: Some(Linked { id: "a".to_string(), label: "Main".to_string() }),
            ..Scope::provider("claude")
        }));
        assert_eq!(change.save, Some(expected.clone()));

        let mut after = UsageBook::default();
        after.select(Some(Scope::of_profile(&claude)));
        after.restore_selection(&expected);
        // 이번 실행에서 이미 고른 Claude는 그대로, 아직 고르지 않은 Codex만 되살린다.
        assert_eq!(after.selected("claude"), Some(&Scope::of_profile(&claude)));
        assert_eq!(after.selected("codex"), Some(&Scope::of_profile(&codex)));

        let mut removed = UsageBook::default();
        removed.restore_selection(&UsageSelection {
            codex: Some(Scope::of_profile(&codex)),
            claude: Some(Scope::provider("codex")),
        });
        let change = removed.apply_profiles(Some(vec![claude]));
        // 지운 계정과 도구가 맞지 않는 항목은 되살리지 않는다.
        assert_eq!(removed.selected("codex"), None);
        assert_eq!(removed.selected("claude"), None);
        assert_eq!(change.save, Some(UsageSelection::default()));
    }

    #[test]
    fn test_c_account_usage_age_old_or_recorded_values_carry_their_age_next_to_the_value() {
        let now = 1_000_000;
        let scope = Scope::provider("codex");
        let windows = vec![UsageWindow { label: "week".to_string(), used: 16, reset: Reset::At(now + 600) }];
        let view = |updated_at: i64, recorded: bool| {
            let reading =
                Reading { windows: windows.clone(), updated_at: Some(updated_at), recorded, ..Reading::default() };
            value_view(&Snapshot::Ready(reading), &scope, "Default", now)
        };
        let ValueView::Meters { stale, age, .. } = view(now - 60, false) else { panic!("meters") };
        assert!(!stale && age.is_none());
        let ValueView::Meters { stale, age, .. } = view(now - 18 * 3600, false) else { panic!("meters") };
        assert!(stale);
        assert_eq!(age.unwrap().short, "18h ago");
        let ValueView::Meters { age, .. } = view(now - 120, true) else { panic!("meters") };
        let age = age.unwrap();
        assert_eq!(age.short, "2m ago", "a session record is labeled even when it is recent");
        assert!(age.explanation.contains("Live usage could not be read"));
        let claude = |state: ClaudeState| {
            let reading = Reading {
                windows: vec![UsageWindow { label: "5h".to_string(), used: 21, reset: Reset::At(now + 600) }],
                updated_at: Some(now - 60),
                state,
                ..Reading::default()
            };
            value_view(&Snapshot::Ready(reading), &Scope::provider("claude"), "Default", now)
        };
        assert!(
            matches!(claude(ClaudeState::Unset), ValueView::Meters { .. }),
            "an unset setting still shows a recorded value"
        );
        assert!(matches!(claude(ClaudeState::On), ValueView::Meters { .. }));
        assert!(
            matches!(claude(ClaudeState::Off), ValueView::State { text: "Not reported", .. }),
            "only a value the user turned off is hidden"
        );
    }

    #[test]
    fn test_c_d74_empty_states_say_what_is_missing() {
        let scope = Scope::provider("claude");
        let state = |snapshot: Snapshot| match value_view(&snapshot, &scope, "Default", 1) {
            ValueView::State { text, .. } => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(state(Snapshot::Loading), "Loading\u{2026}");
        assert_eq!(state(Snapshot::Unavailable { message: None, removed: false }), "Unavailable");
        assert_eq!(state(Snapshot::Ready(Reading { state: ClaudeState::On, ..Reading::default() })), "No usage yet");
        assert_eq!(state(Snapshot::Ready(Reading { state: ClaudeState::Unset, ..Reading::default() })), "Not reported");
    }

    #[test]
    fn test_c_d74_bar_groups_follow_the_enabled_tools_in_claude_then_codex_order() {
        let mut book = book_with(Vec::new());
        book.select(Some(Scope::provider("codex")));
        book.select(Some(Scope::provider("claude")));
        book.set_snapshot(&Scope::provider("claude"), ready(23, 100));
        let titles = |enabled: &dyn Fn(&str) -> bool| {
            bar_groups(&book, enabled, 200).iter().map(|group| group.title).collect::<Vec<_>>()
        };
        assert_eq!(titles(&all_enabled), vec!["Claude", "Codex"]);
        assert_eq!(titles(&|provider: &str| provider == "codex"), vec!["Codex"]);
        assert_eq!(titles(&|_: &str| false), Vec::<&str>::new());
    }

    #[test]
    fn test_c_d74_the_bar_folds_into_one_usage_button_when_the_groups_do_not_fit() {
        let mut book = book_with(Vec::new());
        book.select(Some(Scope::provider("codex")));
        book.select(Some(Scope::provider("claude")));
        let two_windows = |used: i64| {
            let mut reading = reading(used, 100);
            reading.windows.push(UsageWindow { label: "week".to_string(), used: 61, reset: Reset::At(9_999_999_999) });
            Snapshot::Ready(reading)
        };
        book.set_snapshot(&Scope::provider("claude"), two_windows(23));
        book.set_snapshot(&Scope::provider("codex"), two_windows(40));
        let groups = bar_groups(&book, &all_enabled, 200);
        assert_eq!(bar_mode(1500.0, 14, 0.0, &groups, 13.0), BarMode::Full);
        assert_eq!(bar_mode(720.0, 14, 0.0, &groups, 13.0), BarMode::Compact);
        assert_eq!(bar_mode(720.0, 14, 0.0, &[], 13.0), BarMode::Full, "nothing to fold");
        let width = group_width(&groups[0])
            + group_width(&groups[1])
            + usage_group::GROUP_GAP
            + 2.0 * 13.0
            + usage_compact::LEFT_MIN_WIDTH;
        assert_eq!(bar_mode(width + 1.0, 30, 0.0, &groups, 13.0), BarMode::Full);
        assert_eq!(bar_mode(width - 1.0, 30, 0.0, &groups, 13.0), BarMode::Compact);
        assert_eq!(
            bar_mode(width + 1.0, 30, 40.0, &groups, 13.0),
            BarMode::Compact,
            "Review notes and the save mark take their room first"
        );
        assert!(compact_button_visible(BarMode::Compact, &groups, true));
        assert!(!compact_button_visible(BarMode::Full, &groups, true));
        assert!(
            compact_button_visible(BarMode::Full, &[], true),
            "with nothing selected the button is the way to the list"
        );
        assert!(!compact_button_visible(BarMode::Full, &[], false));
    }

    #[test]
    fn test_c_d74_group_summary_lists_meters_for_the_automation_line() {
        let mut book = book_with(Vec::new());
        book.select(Some(Scope::provider("claude")));
        let reading = Reading {
            windows: vec![
                UsageWindow { label: "5h".to_string(), used: 23, reset: Reset::At(9_999_999_999) },
                UsageWindow { label: "week".to_string(), used: 61, reset: Reset::At(9_999_999_999) },
            ],
            updated_at: Some(1_999_999_990),
            state: ClaudeState::On,
            ..Reading::default()
        };
        book.set_snapshot(&Scope::provider("claude"), Snapshot::Ready(reading));
        let groups = bar_groups(&book, &all_enabled, 2_000_000_000);
        assert_eq!(group_summary(&groups[0]), "claude=Default:5h=23,week=61");
    }

    #[test]
    fn test_au20_popup_rows_list_the_selected_default_cli_first_then_every_saved_account_with_its_environment() {
        let wsl = AccountProfile {
            runtime: Runtime::Wsl,
            wsl_distribution: Some("Ubuntu".to_string()),
            ..profile("w", "claude", "WSL Work")
        };
        let mut book = book_with(vec![profile("m", "claude", "Main"), wsl, profile("c", "codex", "Codex One")]);
        book.select(Some(Scope::provider("claude")));
        book.select(Some(Scope::of_profile(&profile("c", "codex", "Codex One"))));
        let sections = popup_sections(&book, &all_enabled, "", 100);
        let names = |section: &PopupSection| section.rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>();
        assert_eq!(sections.iter().map(|section| section.title).collect::<Vec<_>>(), vec!["Claude", "Codex"]);
        assert_eq!(names(&sections[0]), vec!["Default", "Main", "WSL Work"]);
        assert_eq!(
            sections[0].rows.iter().map(|row| row.environment.as_str()).collect::<Vec<_>>(),
            vec!["Not linked to a saved account", "This device", "WSL \u{b7} Ubuntu"]
        );
        assert!(
            sections[0].rows[0].in_bar && !sections[0].rows[1].in_bar,
            "the selected default CLI is the status bar row"
        );
        assert!(sections[1].rows[0].in_bar, "the selected account is the status bar row");
    }

    #[test]
    fn test_au21_a_default_cli_confirmed_as_a_saved_account_merges_into_its_row_with_a_default_badge_and_two_logins_keep_it_apart()
     {
        let mut book = book_with(vec![profile("m", "claude", "Main"), profile("n", "claude", "Other")]);
        book.select(Some(Scope::provider("claude")));
        book.set_snapshot(&Scope::provider("claude"), with_account(reading(30, 100), "m", "Main"));
        let rows = popup_sections(&book, &all_enabled, "", 200).remove(0).rows;
        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            vec!["Main", "Other"],
            "no separate default row"
        );
        assert!(rows[0].is_default_cli && rows[0].in_bar && !rows[1].is_default_cli);
        book.set_snapshot(&Scope::provider("claude"), ready(30, 100));
        let rows = popup_sections(&book, &all_enabled, "", 200).remove(0).rows;
        assert_eq!(rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(), vec!["Default", "Main", "Other"]);
        assert!(
            rows.iter().all(|row| !row.is_default_cli),
            "an unconfirmed default CLI has no Default badge on any account"
        );
    }

    #[test]
    fn test_au22_search_matches_the_tool_and_account_name_without_case_and_empty_sections_disappear() {
        let book = book_with(
            (1..=7)
                .map(|n| profile(&format!("c{n}"), "claude", &format!("Work {n}")))
                .chain([profile("x", "codex", "Wolf")])
                .collect(),
        );
        let rows = |query: &str| {
            popup_sections(&book, &all_enabled, query, 0)
                .iter()
                .flat_map(|section| section.rows.iter().map(|row| row.name.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(rows("wo").len(), 8);
        assert_eq!(rows("WORK 3"), vec!["Work 3"]);
        assert_eq!(rows("codex"), vec!["Wolf"], "the tool name is searchable");
        assert_eq!(popup_sections(&book, &all_enabled, "wolf", 0).len(), 1);
        assert!(rows("nothing").is_empty());
        assert!(search_visible(6, "") && !search_visible(5, "") && search_visible(2, "wo"));
    }

    #[test]
    fn test_au23_a_tool_hidden_in_quick_settings_says_so_and_an_unregistered_folder_cannot_be_opened() {
        let mut book = book_with(vec![profile("m", "claude", "Main")]);
        book.select(Some(Scope::custom("claude")));
        let rows = popup_sections(&book, &|provider: &str| provider != "claude", "", 0).remove(0).rows;
        assert_eq!(rows[0].name, "Unregistered folder");
        assert!(!rows[0].can_open && rows[1].can_open);
        assert_eq!(rows[0].environment, "Folder set in the terminal");
        assert!(matches!(rows[1].value, ValueView::State { text: "Hidden in Quick settings", .. }));
    }

    #[test]
    fn test_c_d75_retry_shows_for_a_failed_registered_or_default_read_but_not_for_an_unregistered_folder_or_a_removed_account()
     {
        let mut book = book_with(vec![profile("m", "claude", "Main")]);
        assert!(!book.retry_visible(&all_enabled));
        book.set_snapshot(
            &Scope::of_profile(&profile("m", "claude", "Main")),
            Snapshot::Unavailable { message: None, removed: false },
        );
        assert!(book.retry_visible(&all_enabled));
        assert!(!book.retry_visible(&|provider: &str| provider != "claude"), "a hidden tool is not retried");
        let mut custom = book_with(Vec::new());
        custom.select(Some(Scope::custom("codex")));
        custom.set_snapshot(
            &Scope::custom("codex"),
            Snapshot::Unavailable { message: Some("x".to_string()), removed: false },
        );
        assert!(!custom.retry_visible(&all_enabled));
        book.apply_profiles(None);
        assert!(book.retry_visible(&all_enabled), "an account list that could not be read can be retried");
    }

    #[test]
    fn test_scope_keys_name_the_tool_and_the_account_default_or_custom_tail() {
        assert_eq!(scope_key(&Scope::of_profile(&profile("a", "claude", "Main"))), "claude:a");
        assert_eq!(scope_key(&Scope::custom("codex")), "codex:custom");
        assert_eq!(scope_key(&Scope::provider("codex")), "codex:default");
        assert_eq!(provider_title("codex"), "Codex");
        assert_eq!(provider_title("bash"), "");
    }
}
