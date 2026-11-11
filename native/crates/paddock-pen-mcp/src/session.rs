//! 한 MCP 프로그램이 서비스하는 Claude의 환경과, 그 환경이 준 경로를 Pen 대상으로 바꾸는 일, 그리고 이 프로그램이 연 파일의 목록.
use std::sync::Mutex;

use paddock_core::pen_paths::{PenHost, PenTarget, is_pen_file, normalize_document, resolve_target};

/// 두 대상이 같은 파일인지. 표기를 맞춘 경로(`normalize_document`)가 같거나, 디스크에서 읽은 실제 경로가 같으면 같은 파일이다.
///
/// 실제 경로는 표기만으로 가를 수 없는 별칭(짧은 이름(8.3), 연결 드라이브와 그 공유 경로, 비ASCII 대소문자, 링크)을 푼다. 파일이 없어 실제
/// 경로를 못 읽으면 표기만 비교한다.
fn is_same_file(
    one: &PenTarget,
    other: &PenTarget,
) -> bool {
    normalize_document(&one.windows) == normalize_document(&other.windows)
        || matches!(
            (std::fs::canonicalize(&one.local), std::fs::canonicalize(&other.local)),
            (Ok(one), Ok(other)) if one == other
        )
}

/// 이 프로그램을 띄운 Claude의 환경. 프로그램 자체는 항상 이 운영체제의 프로세스다.
pub struct Session {
    host: PenHost,
    wsl_distribution: Option<String>,
    /// `pen_open`이 연 파일. 저장 뒤 이 파일들의 수정 시각을 비교해, 저장이 다른 파일에 떨어지지 않았는지 확인한다.
    opened: Mutex<Vec<PenTarget>>,
}

impl Session {
    /// 실행 인자(`--wsl-distribution <이름>`)로 환경을 정한다. 배포판 인자가 있으면 WSL 안의 Claude이고, 없으면 Windows Claude다.
    /// 환경 변수는 WSL interop으로 넘어가지 않으므로 배포판 이름은 인자로 받는다.
    pub fn from_arguments(arguments: impl IntoIterator<Item = String>) -> Self {
        let mut arguments = arguments.into_iter();
        let mut wsl_distribution = None;
        while let Some(argument) = arguments.next() {
            if argument == "--wsl-distribution" {
                wsl_distribution = arguments.next().filter(|name| !name.is_empty());
            }
        }
        let host = if wsl_distribution.is_some() { PenHost::Wsl } else { PenHost::Windows };
        Self { host, wsl_distribution, opened: Mutex::new(Vec::new()) }
    }

    /// 연 파일 목록에 더한다. 같은 파일(대소문자·경로 표기·별칭만 다른 것 포함, `is_same_file`)은 한 번만 남는다.
    pub fn remember_opened(
        &self,
        target: &PenTarget,
    ) {
        let mut opened = self.opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !opened.iter().any(|known| is_same_file(known, target)) {
            opened.push(target.clone());
        }
    }

    /// 연 파일 목록에서 뺀다(창을 닫은 파일). 목록에 없으면 아무 일도 없다.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn forget_opened(
        &self,
        target: &PenTarget,
    ) {
        let mut opened = self.opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        opened.retain(|known| !is_same_file(known, target));
    }

    /// 연 파일 가운데 `target`과 같은 파일(`is_same_file`)이 아닌 것.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn opened_besides(
        &self,
        target: &PenTarget,
    ) -> Vec<PenTarget> {
        let opened = self.opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        opened.iter().filter(|known| !is_same_file(known, target)).cloned().collect()
    }

    /// 입력 경로를 Pen 대상으로 바꾼다. 경로가 비었거나 변환할 수 없으면 에이전트에게 보일 문장을 오류로 돌려준다.
    ///
    /// Windows가 아닌 운영체제에서는 변환 없이 절대 경로만 받는다(Pen을 시스템 열기로 넘기고, 파일은 같은 경로로 읽는다).
    pub fn resolve(
        &self,
        input: Option<&str>,
    ) -> Result<PenTarget, String> {
        match input.filter(|path| !path.is_empty()) {
            None => Err("path is required.".to_string()),
            Some(path) if cfg!(windows) => {
                resolve_target(path, self.host, self.wsl_distribution.as_deref()).map_err(|error| error.to_string())
            }
            Some(path) if path.starts_with('/') => Ok(PenTarget { local: path.to_string(), windows: path.to_string() }),
            Some(_) => Err("Use an absolute path (a relative path has no known folder).".to_string()),
        }
    }

    /// `resolve`에 더해 `.pen` 파일이고 디스크에 있는지까지 확인한다.
    pub fn resolve_existing_pen_file(
        &self,
        input: Option<&str>,
    ) -> Result<PenTarget, String> {
        let target = self.resolve(input)?;
        if !is_pen_file(&target.local) {
            Err(format!("Only .pen files can be opened: {}", target.local))
        } else if !std::path::Path::new(&target.local).exists() {
            Err(format!("File not found: {}", target.local))
        } else {
            Ok(target)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(arguments: &[&str]) -> Session {
        Session::from_arguments(arguments.iter().map(ToString::to_string))
    }

    #[test]
    fn test_session_1_a_wsl_distribution_argument_makes_the_host_wsl() {
        let wsl = session(&["--wsl-distribution", "Ubuntu"]);
        assert_eq!(wsl.host, PenHost::Wsl);
        assert_eq!(wsl.wsl_distribution.as_deref(), Some("Ubuntu"));
        let plain = session(&[]);
        assert_eq!(plain.host, PenHost::Windows);
        assert_eq!(plain.wsl_distribution, None);
        // 값이 없거나 비면 인자가 없는 것과 같다.
        assert_eq!(session(&["--wsl-distribution"]).host, PenHost::Windows);
        assert_eq!(session(&["--wsl-distribution", ""]).host, PenHost::Windows);
    }

    #[test]
    fn test_session_2_a_missing_path_is_reported() {
        let session = session(&[]);
        assert_eq!(session.resolve(None), Err("path is required.".to_string()));
        assert_eq!(session.resolve(Some("")), Err("path is required.".to_string()));
    }

    #[test]
    fn test_session_5_opened_files_are_kept_once_and_listed_besides_the_target() {
        let session = session(&[]);
        let target = |windows: &str| PenTarget { local: windows.to_string(), windows: windows.to_string() };
        let (a, b) = (target("C:\\w\\a.pen"), target("C:\\w\\b.pen"));
        session.remember_opened(&a);
        session.remember_opened(&target("c:\\W\\A.PEN"));
        session.remember_opened(&b);
        assert_eq!(session.opened_besides(&a), vec![b.clone()]);
        assert_eq!(session.opened_besides(&target("C:\\w\\c.pen")).len(), 2);
        session.forget_opened(&target("C:\\W\\B.pen"));
        assert_eq!(session.opened_besides(&a), Vec::<PenTarget>::new());
        assert_eq!(session.opened_besides(&b), vec![a]);
    }

    #[cfg(unix)]
    #[test]
    fn test_session_6_another_spelling_of_the_same_file_on_disk_is_the_same_opened_file() {
        // 표기로는 다르지만(`..` 조각, 링크) 디스크에서는 같은 파일인 철자는 다른 연 파일로 남지 않는다.
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("a.pen"), "x").unwrap();
        std::os::unix::fs::symlink(root.join("a.pen"), root.join("link.pen")).unwrap();
        let target = |path: std::path::PathBuf| {
            let path = path.to_string_lossy().to_string();
            PenTarget { local: path.clone(), windows: path }
        };
        let session = session(&[]);
        let plain = target(root.join("a.pen"));
        session.remember_opened(&target(root.join("sub").join("..").join("a.pen")));
        session.remember_opened(&target(root.join("link.pen")));
        session.remember_opened(&plain);
        assert_eq!(session.opened_besides(&target(root.join("missing.pen"))).len(), 1);
        assert_eq!(session.opened_besides(&plain), Vec::<PenTarget>::new());
        session.forget_opened(&target(root.join("link.pen")));
        assert_eq!(session.opened_besides(&target(root.join("missing.pen"))), Vec::<PenTarget>::new());
    }

    #[cfg(not(windows))]
    #[test]
    fn test_session_3_outside_windows_only_absolute_paths_pass_through() {
        let session = session(&[]);
        let target = session.resolve(Some("/w/a.pen")).unwrap();
        assert_eq!((target.local.as_str(), target.windows.as_str()), ("/w/a.pen", "/w/a.pen"));
        assert!(session.resolve(Some("a.pen")).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn test_session_4_the_existing_pen_file_check_names_the_reason() {
        let directory = tempfile::tempdir().unwrap();
        let session = session(&[]);
        let text = directory.path().join("note.txt");
        std::fs::write(&text, "x").unwrap();
        let error = session.resolve_existing_pen_file(text.to_str()).unwrap_err();
        assert!(error.starts_with("Only .pen files can be opened"), "{error}");
        let missing = directory.path().join("gone.pen");
        let error = session.resolve_existing_pen_file(missing.to_str()).unwrap_err();
        assert!(error.starts_with("File not found"), "{error}");
        let found = directory.path().join("a.pen");
        std::fs::write(&found, "x").unwrap();
        assert!(session.resolve_existing_pen_file(found.to_str()).is_ok());
    }
}
