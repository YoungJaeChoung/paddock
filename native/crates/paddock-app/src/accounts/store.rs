//! 등록 계정 목록 파일(`~/.paddock/config/account-profiles.json`)의 읽기·쓰기.
//!
//! 이 파일은 기존 앱과 함께 쓰는 공유 계약이다. 형식과 검사는 core의 `account_profile`이 쥐고, 여기서는 파일을 다룬다:
//! 링크로 바뀐 파일·너무 큰 파일·깨진 파일은 덮어쓰지 않고 거부하고, 쓸 때는 임시 파일에 다 쓴 뒤 이름을 바꾸며(0600),
//! 계정을 더할 때는 계정 폴더를 비공개(0700)로 만든다. 계정 폴더 안(로그인·스킬 연결)은 읽지도 쓰지도 않는다.
use std::fmt;
use std::io::Write;
use std::path::Path;

use paddock_core::account_profile::{
    NewAccount, ProfileError, add_profile, parse_profile_list, remove_profile, rename_profile, serialize_profile_list,
};
use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_protocol::shared_config::{account_profiles_path, config_dir};
use rand::Rng;

/// 목록 파일의 최대 크기. 넘으면 읽지 않는다.
const MAX_LIST_BYTES: u64 = 1024 * 1024;

/// 목록을 바꾸지 못한 이유.
#[derive(Debug)]
pub enum StoreError {
    /// 입력이 규칙에 맞지 않거나 저장된 목록을 읽을 수 없다.
    Profile(ProfileError),
    /// 계정 폴더가 링크 등으로 바뀌어 있다.
    UnsafeFolder,
    /// 파일을 쓰지 못했다.
    Io(std::io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            Self::Profile(error) => error.fmt(formatter),
            Self::UnsafeFolder => formatter
                .write_str("An account folder is not a regular directory. Restore the folder before trying again."),
            Self::Io(_) => formatter.write_str("Could not save the account list. Check the folder permissions."),
        }
    }
}

impl From<ProfileError> for StoreError {
    fn from(error: ProfileError) -> Self {
        Self::Profile(error)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// 터미널을 열 때 쓰는 등록 계정. 목록을 엄격한 규칙(`read_profiles`)으로 읽고, 읽을 수 없으면 계정이 없는 것으로 본다.
///
/// 식별자·도구 이름이 계정 폴더 경로에 그대로 이어지므로, 느슨하게 읽은 목록을 쓰지 않는다.
pub fn registered_profiles() -> Vec<AccountProfile> {
    read_profiles().unwrap_or_default()
}

/// 저장된 계정 목록. 파일이 없으면 첫 실행이라 빈 목록이고, 깨졌거나 링크로 바뀌었으면 `UnreadableList`다.
pub fn read_profiles() -> Result<Vec<AccountProfile>, ProfileError> {
    let path = account_profiles_path();
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(ProfileError::UnreadableList),
        Ok(metadata) => {
            if metadata.is_file() && metadata.len() <= MAX_LIST_BYTES {
                std::fs::read_to_string(&path)
                    .map_err(|_| ProfileError::UnreadableList)
                    .and_then(|text| parse_profile_list(&text))
            } else {
                Err(ProfileError::UnreadableList)
            }
        }
    }
}

/// 새 UUID v4 식별자.
fn new_id() -> String {
    let mut bytes: [u8; 16] = rand::thread_rng().r#gen();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// 목록을 파일에 쓴다: 임시 파일(0600)에 다 쓴 뒤 이름을 바꿔, 쓰는 도중 멈춰도 이전 목록이 남는다.
fn write_profiles(profiles: &[AccountProfile]) -> std::io::Result<()> {
    let directory = config_dir();
    std::fs::create_dir_all(&directory)?;
    let temporary = directory.join(format!(".account-profiles.json.{}.tmp", new_id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(serialize_profile_list(profiles).as_bytes()))
        .and_then(|()| std::fs::rename(&temporary, account_profiles_path()));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// 폴더를 만든다. Unix는 만들 때부터 소유자 전용(0700)이다.
fn create_owner_only_directory(directory: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    let created = {
        let mut builder = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(directory)
    };
    #[cfg(windows)]
    let created = std::fs::create_dir(directory);
    created
}

/// `directory`가 링크가 아닌 실제 폴더이게 한다(없으면 비공개로 만든다).
fn ensure_private_directory(directory: &Path) -> Result<(), StoreError> {
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(StoreError::UnsafeFolder),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // 만들 때부터 소유자 전용이어야 만든 직후 다른 사용자가 열 틈이 없다.
            create_owner_only_directory(directory)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

/// 이 컴퓨터에서 쓰는 계정이면 계정 폴더(`agent-profiles/<id>/<도구>`)를 비공개로 만든다. WSL 계정의 폴더는 WSL 안에 있다.
fn ensure_account_folder(profile: &AccountProfile) -> Result<(), StoreError> {
    if profile.runtime == Runtime::Native {
        let profiles = config_dir().join("agent-profiles");
        let account = profiles.join(&profile.id);
        std::fs::create_dir_all(config_dir())?;
        [profiles.clone(), account.clone(), account.join(&profile.provider)]
            .iter()
            .try_for_each(|directory| ensure_private_directory(directory))
    } else {
        Ok(())
    }
}

/// 계정을 더한다. 계정 폴더를 만든 뒤에 목록에 올리므로, 폴더를 만들지 못하면 목록은 그대로다.
pub fn add(input: &NewAccount) -> Result<AccountProfile, StoreError> {
    let profiles = read_profiles()?;
    let next = add_profile(&profiles, input, &new_id())?;
    let added = next.last().cloned().ok_or(ProfileError::NotFound)?;
    ensure_account_folder(&added)?;
    write_profiles(&next)?;
    Ok(added)
}

/// 계정 이름을 바꾼다. 식별자·계정 폴더는 그대로다.
pub fn rename(
    id: &str,
    label: &str,
) -> Result<AccountProfile, StoreError> {
    let next = rename_profile(&read_profiles()?, id, label)?;
    write_profiles(&next)?;
    next.into_iter().find(|profile| profile.id == id).ok_or(StoreError::Profile(ProfileError::NotFound))
}

/// 계정을 목록에서만 뺀다. 계정 폴더·로그인 자료·실행 중인 터미널은 그대로 남는다.
pub fn remove(id: &str) -> Result<(), StoreError> {
    let next = remove_profile(&read_profiles()?, id)?;
    write_profiles(&next)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempHome;

    fn account(provider: &str) -> NewAccount<'_> {
        NewAccount { provider, label: None, runtime: Runtime::Native, wsl_distribution: None }
    }

    #[test]
    fn test_c_d39_a_list_with_an_id_that_could_leave_the_accounts_folder_opens_no_account() {
        let _home = TempHome::new();
        let good = add(&account("claude")).unwrap();
        assert_eq!(registered_profiles(), vec![good.clone()]);
        // 목록 파일에 경로 조각이 든 식별자가 있으면 엄격한 규칙으로 목록 전체를 읽을 수 없는 것으로 보고, 어떤 계정도 경로에 잇지 않는다.
        let bad = r#"{"version":1,"profiles":[{"id":"../../outside","provider":"claude","label":"Evil","runtime":"native"}]}"#;
        std::fs::write(account_profiles_path(), bad).unwrap();
        assert_eq!(registered_profiles(), Vec::new());
    }

    #[test]
    fn test_a01_added_accounts_get_private_folders_and_a_private_list_file_with_metadata_only() {
        let _home = TempHome::new();
        let first = add(&account("claude")).unwrap();
        let second = add(&account("codex")).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!((first.label.as_str(), second.label.as_str()), ("Claude 1", "Codex 1"));
        let folder = config_dir().join("agent-profiles").join(&first.id).join("claude");
        assert!(folder.is_dir());
        assert!(std::fs::read_dir(&folder).unwrap().next().is_none(), "the folder holds no credentials or links");
        assert_eq!(read_profiles().unwrap(), vec![first.clone(), second]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(mode(&account_profiles_path()), 0o600);
        }
        let text = std::fs::read_to_string(account_profiles_path()).unwrap();
        assert_eq!(text, serialize_profile_list(&read_profiles().unwrap()));
        assert!(text.ends_with("}\n") && text.contains("\n  \"profiles\": [\n    {\n      \"id\""));
        let leftovers: Vec<_> = std::fs::read_dir(config_dir()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert!(leftovers.iter().all(|name| !name.to_string_lossy().ends_with(".tmp")), "{leftovers:?}");
    }

    #[test]
    fn test_a03_rename_keeps_the_id_and_a_duplicate_name_changes_nothing() {
        let _home = TempHome::new();
        let first = add(&account("claude")).unwrap();
        let second = add(&account("claude")).unwrap();
        let before = std::fs::read_to_string(account_profiles_path()).unwrap();
        assert!(matches!(
            rename(&second.id, " claude 1 ").unwrap_err(),
            StoreError::Profile(ProfileError::DuplicateLabel)
        ));
        assert_eq!(std::fs::read_to_string(account_profiles_path()).unwrap(), before);
        let renamed = rename(&second.id, "Extra").unwrap();
        assert_eq!((renamed.id, renamed.label), (second.id, "Extra".to_string()));
        assert_eq!(read_profiles().unwrap().len(), 2);
        assert_eq!(read_profiles().unwrap()[0], first);
    }

    #[test]
    fn test_a08_removing_an_account_keeps_its_folder_and_the_other_entries() {
        let _home = TempHome::new();
        let first = add(&account("claude")).unwrap();
        let second = add(&account("codex")).unwrap();
        let folder = config_dir().join("agent-profiles").join(&first.id).join("claude");
        std::fs::write(folder.join("history.jsonl"), "fixture conversation\n").unwrap();
        remove(&first.id).unwrap();
        assert_eq!(read_profiles().unwrap(), vec![second]);
        assert_eq!(std::fs::read_to_string(folder.join("history.jsonl")).unwrap(), "fixture conversation\n");
        assert!(matches!(remove(&first.id).unwrap_err(), StoreError::Profile(ProfileError::NotFound)));
    }

    #[test]
    fn test_a11_a_corrupt_or_linked_list_file_is_refused_and_left_untouched() {
        let home = TempHome::new();
        std::fs::create_dir_all(config_dir()).unwrap();
        let path = account_profiles_path();
        std::fs::write(&path, "{unfinished").unwrap();
        assert!(matches!(add(&account("claude")).unwrap_err(), StoreError::Profile(ProfileError::UnreadableList)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{unfinished");
        std::fs::remove_file(&path).unwrap();
        let outside = home.dir.path().join("other-data.json");
        std::fs::write(&outside, r#"{"version":1,"profiles":[]}"#).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            assert_eq!(read_profiles().unwrap_err(), ProfileError::UnreadableList);
            assert!(matches!(add(&account("claude")).unwrap_err(), StoreError::Profile(ProfileError::UnreadableList)));
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), r#"{"version":1,"profiles":[]}"#);
        }
    }

    #[test]
    fn test_a09_a_redirected_account_root_saves_no_entry() {
        let home = TempHome::new();
        let outside = home.dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(config_dir()).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, config_dir().join("agent-profiles")).unwrap();
            assert!(matches!(add(&account("claude")).unwrap_err(), StoreError::UnsafeFolder));
            assert!(read_profiles().unwrap().is_empty());
            assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        }
    }

    #[test]
    fn test_a12_ids_are_version_4_uuids() {
        for _ in 0..20 {
            let id = new_id();
            assert!(paddock_core::account_profile::checked_id(&id).is_ok(), "{id}");
        }
    }

    #[test]
    fn test_a_wsl_account_creates_no_native_folder() {
        let _home = TempHome::new();
        let wsl = NewAccount { runtime: Runtime::Wsl, wsl_distribution: Some("Ubuntu"), ..account("codex") };
        let added = add(&wsl).unwrap();
        assert!(!config_dir().join("agent-profiles").join(&added.id).exists());
        assert_eq!(read_profiles().unwrap(), vec![added]);
    }
}
