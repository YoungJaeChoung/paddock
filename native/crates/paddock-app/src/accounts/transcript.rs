//! 이어 갈 대화의 파일(JSONL)을 찾는다. 대화 본문은 읽지 않고, 파일이 있는지와 위치만 본다.
//!
//! Claude가 대화를 `<설정 폴더>/projects/<작업 폴더 이름>/<대화 ID>.jsonl`에 남긴다. 작업 폴더 위치(파일 세 개 확인)를 먼저 보고,
//! 없을 때만 `projects` 아래 모든 폴더를 훑는다 — 폴더가 수천 개면 WSL 경로(`\\wsl.localhost`)에서 수 초가 걸리기 때문이다.
//! 훑어서도 못 찾은 대화는 5분 동안 다시 훑지 않는다(그동안 생긴 파일은 위치 확인이 찾는다). 링크는 따라가지 않는다.
//! 파일 시스템을 읽으므로 앱의 작업 스레드에서만 부른다.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use paddock_core::account_login::WslMapping;
use paddock_core::account_session::{SessionIdentity, expected_transcript};

use crate::usage::files::assert_plain_path;

const REDIRECTED: &str =
    "A conversation path is redirected or has the wrong file type. Restore its folder before switching accounts.";
const AMBIGUOUS: &str = "The conversation was found in more than one project folder. Choose it in the official CLI before switching accounts.";

/// 설정 폴더 하나: 그 CLI가 보는 경로와, 그 경로를 이 앱이 읽는 경로로 바꾸는 방법.
#[derive(Clone, Debug)]
pub struct ConfigFolder {
    /// CLI가 보는 설정 폴더(WSL이면 Linux 경로).
    pub tool: String,
    pub separator: char,
    /// WSL 폴더면 `\\wsl.localhost\<배포판>\…`로 바꾸는 규칙.
    pub mapping: Option<WslMapping>,
}

impl ConfigFolder {
    /// 이 컴퓨터의 폴더.
    pub fn native(tool: &str) -> Self {
        Self { tool: tool.to_string(), separator: std::path::MAIN_SEPARATOR, mapping: None }
    }

    /// WSL 안의 폴더(Linux 경로).
    pub fn wsl(
        tool: &str,
        mapping: WslMapping,
    ) -> Self {
        Self { tool: tool.to_string(), separator: '/', mapping: Some(mapping) }
    }

    /// CLI가 보는 경로를 이 앱이 읽는 경로로 바꾼다.
    fn local(
        &self,
        tool_path: &str,
    ) -> Option<PathBuf> {
        match &self.mapping {
            Some(mapping) => mapping.to_local(tool_path).map(PathBuf::from),
            None => Some(PathBuf::from(tool_path)),
        }
    }
}

/// 찾은 대화 파일: CLI가 쓸 경로와 이 앱이 읽은 경로.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub tool_path: String,
    pub local: PathBuf,
}

/// 전체 탐색으로도 못 찾은 대화: `<설정 폴더>\0<대화 ID>` → 그 탐색 시각.
static MISSED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// `path`가 일반 파일이면 true, 없으면 false, 링크이거나 종류가 틀리면 오류다.
fn plain_file(path: &Path) -> Result<bool, String> {
    assert_plain_path(path, false).map_err(|_| REDIRECTED.to_string())?;
    Ok(path.is_file())
}

fn plain_directory(path: &Path) -> Result<bool, String> {
    assert_plain_path(path, true).map_err(|_| REDIRECTED.to_string())?;
    Ok(path.is_dir())
}

/// 대화가 기록될 것으로 기대하는 위치에 그 파일이 있으면 돌려준다(파일 세 개 확인).
fn expected_in(
    folder: &ConfigFolder,
    session_id: &str,
    cwd: &str,
) -> Result<Option<Found>, String> {
    let mut found = None;
    if let Some(tool_path) = expected_transcript(&folder.tool, session_id, cwd, folder.separator)
        && let Some(local) = folder.local(&tool_path)
    {
        let project = local.parent().map(Path::to_path_buf).unwrap_or_default();
        let projects = project.parent().map(Path::to_path_buf).unwrap_or_default();
        if plain_directory(&projects)? && plain_directory(&project)? && plain_file(&local)? {
            found = Some(Found { tool_path, local });
        }
    }
    Ok(found)
}

/// 대화 파일을 찾는다. 없으면 None이고, 링크·여러 곳에서 발견은 사용자에게 보일 문장의 오류다.
///
/// `origin`은 대화의 출처 계정 폴더, `current`는 CLI가 지금 쓰는 폴더다. 둘이 다를 수 있다(예: 되살린 터미널의 출처 정보는
/// 기본 폴더를 가리키는데 CLI는 계정 폴더에서 실행 중). 두 폴더의 기대 위치를 먼저 보고, 둘 다 없을 때만 출처 폴더를 훑는다.
pub fn locate_transcript(
    origin: &ConfigFolder,
    current: &ConfigFolder,
    session_id: &str,
    cwd: &str,
) -> Result<Option<Found>, String> {
    let mut found = expected_in(origin, session_id, cwd)?;
    if found.is_none() && current.tool != origin.tool {
        found = expected_in(current, session_id, cwd)?;
    }
    if found.is_none() {
        let key = format!("{}\0{session_id}", origin.tool);
        let retry = Duration::from_millis(SessionIdentity::SCAN_RETRY_MS);
        let skip = MISSED
            .lock()
            .ok()
            .and_then(|missed| missed.as_ref().and_then(|map| map.get(&key).copied()))
            .is_some_and(|at| at.elapsed() < retry);
        if !skip {
            found = scan_projects(origin, session_id)?;
            if let Ok(mut missed) = MISSED.lock() {
                let map = missed.get_or_insert_with(HashMap::new);
                if found.is_some() {
                    map.remove(&key);
                } else {
                    map.insert(key, Instant::now());
                }
            }
        }
    }
    Ok(found)
}

/// `projects` 아래 모든 폴더에서 `<대화 ID>.jsonl`을 찾는다. 두 곳 이상이면 고르지 않고 오류다.
fn scan_projects(
    folder: &ConfigFolder,
    session_id: &str,
) -> Result<Option<Found>, String> {
    let sep = folder.separator;
    let projects_tool = format!("{}{sep}projects", folder.tool.trim_end_matches(sep));
    let mut matches = Vec::new();
    if let Some(projects) = folder.local(&projects_tool)
        && plain_directory(&projects)?
    {
        let entries = std::fs::read_dir(&projects)
            .map_err(|error| format!("The projects folder cannot be read: {}", error.kind()))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_directory = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if is_directory {
                let tool_path = format!("{projects_tool}{sep}{name}{sep}{session_id}.jsonl");
                if let Some(local) = folder.local(&tool_path)
                    && plain_directory(local.parent().unwrap_or(&projects))?
                    && plain_file(&local)?
                {
                    matches.push(Found { tool_path, local });
                }
            }
        }
    }
    match matches.len() {
        0 | 1 => Ok(matches.pop()),
        _ => Err(AMBIGUOUS.to_string()),
    }
}

/// 이전에 찾았던 대화 파일이 아직 일반 파일로 있는지(링크이거나 없으면 false/오류).
pub fn still_there(found: &Found) -> Result<bool, String> {
    plain_file(&found.local)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";

    fn config(name: &str) -> (tempfile::TempDir, ConfigFolder) {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join(name).to_string_lossy().into_owned();
        std::fs::create_dir_all(&tool).unwrap();
        (dir, ConfigFolder::native(&tool))
    }

    fn write_transcript(
        folder: &ConfigFolder,
        project: &str,
    ) -> PathBuf {
        let directory = Path::new(&folder.tool).join("projects").join(project);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join(format!("{SESSION}.jsonl"));
        std::fs::write(&file, "{}\n").unwrap();
        file
    }

    #[test]
    fn test_c_d77_the_transcript_is_found_at_the_expected_place_first_then_in_the_cli_folder_then_by_scanning() {
        let (_a, origin) = config("origin");
        let (_b, current) = config("current");
        let expected = write_transcript(&origin, "-w-app");
        let found = locate_transcript(&origin, &current, SESSION, "/w/app").unwrap().unwrap();
        assert_eq!(found.local, expected);
        assert!(found.tool_path.ends_with(&format!("{SESSION}.jsonl")));
        // 출처 폴더에 없고 CLI가 쓰는 폴더에만 있으면 그쪽을 쓴다.
        let (_c, empty) = config("empty");
        let (_d, cli) = config("cli");
        let in_cli = write_transcript(&cli, "-w-app");
        assert_eq!(locate_transcript(&empty, &cli, SESSION, "/w/app").unwrap().unwrap().local, in_cli);
        // 작업 폴더 이름과 다른 프로젝트 폴더에 있으면 훑어서 찾는다(경로가 200자를 넘어 이름을 모르는 경우와 같다).
        let (_e, scanned) = config("scanned");
        let hidden = write_transcript(&scanned, "other-project");
        assert_eq!(locate_transcript(&scanned, &scanned, SESSION, "/w/app").unwrap().unwrap().local, hidden);
    }

    #[test]
    fn test_c_d77_a_missing_transcript_is_none_and_is_not_scanned_again_for_five_minutes_and_two_copies_are_refused() {
        let (_a, origin) = config("missing");
        std::fs::create_dir_all(Path::new(&origin.tool).join("projects").join("p")).unwrap();
        assert_eq!(locate_transcript(&origin, &origin, SESSION, "/x").unwrap(), None);
        // 훑은 뒤에 생긴 파일은 위치 확인이 찾지만, 위치가 다르면 5분 동안 훑지 않는다.
        write_transcript(&origin, "-p");
        assert_eq!(
            locate_transcript(&origin, &origin, SESSION, "/x").unwrap(),
            None,
            "the scan is skipped for a while"
        );
        assert!(
            locate_transcript(&origin, &origin, SESSION, "/p").unwrap().is_some(),
            "the expected place is always checked"
        );
        let (_b, twice) = config("twice");
        write_transcript(&twice, "one");
        write_transcript(&twice, "two");
        assert_eq!(locate_transcript(&twice, &twice, SESSION, "/zzz").unwrap_err(), AMBIGUOUS);
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d77_a_redirected_projects_folder_is_refused_and_a_removed_file_is_noticed() {
        let (dir, origin) = config("linked");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, Path::new(&origin.tool).join("projects")).unwrap();
        assert_eq!(locate_transcript(&origin, &origin, SESSION, "/x").unwrap_err(), REDIRECTED);
        let (_b, plain) = config("plain");
        let file = write_transcript(&plain, "-x");
        let found = locate_transcript(&plain, &plain, SESSION, "/x").unwrap().unwrap();
        assert_eq!(still_there(&found), Ok(true));
        std::fs::remove_file(file).unwrap();
        assert_eq!(still_there(&found), Ok(false));
    }
}
