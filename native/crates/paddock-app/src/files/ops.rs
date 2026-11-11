//! 파일 목록이 디스크에 쓰는 일: 붙여넣기·끌어 놓기 복사(폴더는 재귀), 클립보드 그림 저장, 이름 바꾸기.
//! 모두 파일 시스템 일이라 창 스레드가 아닌 곳(`background_executor`)에서 부른다. 이름 규칙 검사와 복사 계획(덮어쓰지 않는 이름·자기 안 복사 거부)은
//! core(`entry_name`·`incoming`)가 정하고, 여기서는 정해진 계획을 실행해 결과를 문장으로 돌려준다. 실패는 그 일의 결과로 가둔다.
use std::io::Cursor;
use std::path::Path;

use paddock_core::entry_name::copy_entry_name;
use paddock_core::file_tree::{is_case_only_rename, renamed};
use paddock_core::incoming::CopyStep;
use paddock_core::paste_paths::{ImageKind, pasted_image_name};
use paddock_core::paths::{child_path, file_name_of, parent_of};
use paddock_core::search_model::{MAX_FILE_BYTES, SearchOptions, apply_edits, build_matcher, replace_plan};
use paddock_core::text_file::{decode_text, encode_for_save};

use super::open::{FileStamp, describe, file_stamp};

/// 복사 계획을 실행한 결과.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CopyOutcome {
    /// 새로 생긴 경로(최상위 항목 하나가 한 줄이다).
    pub copied: Vec<String>,
    /// 자기 안으로 복사하려다 거부한 항목 수.
    pub n_refused: usize,
    /// 이름이 없어(드라이브 뿌리 등) 복사하지 못한 항목 수.
    pub n_unnamed: usize,
    /// 첫 실패의 문장. 있으면 그 뒤 항목은 시도하지 않았다.
    pub failure: Option<String>,
}

/// 새 파일을 만든다: 같은 경로가 이미 있으면 `AlreadyExists`로 거부하고 덮어쓰지 않는다(이름 검사와 쓰기 사이에 다른 쓰기가 끼어들어도 안전하다).
fn create_new(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// 파일 하나를 `to`로 복사한다. `to`가 이미 있으면 `AlreadyExists`다. 권한과 수정 시각은 원본을 따른다(되도록).
fn copy_file_new(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    let mut source = std::fs::File::open(from)?;
    let mut target = create_new(to)?;
    std::io::copy(&mut source, &mut target)?;
    let metadata = source.metadata()?;
    // 권한·수정 시각은 못 옮겨도 복사 자체는 성공으로 본다(읽기 전용 파일 시스템 위의 일부 속성 등).
    let _ = target.set_permissions(metadata.permissions());
    if let Ok(modified) = metadata.modified() {
        let _ = target.set_modified(modified);
    }
    Ok(())
}

/// 폴더 `from`의 내용을 이미 만들어 둔 폴더 `to` 안으로 재귀 복사한다(링크는 따라가지 않고 건너뛴다).
fn copy_children(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let metadata = std::fs::symlink_metadata(&source)?;
        if metadata.is_dir() {
            std::fs::create_dir(&target)?;
            copy_children(&source, &target)?;
        } else if metadata.is_file() {
            copy_file_new(&source, &target)?;
        }
    }
    Ok(())
}

/// 최상위 항목 하나를 `to`로 복사한다. `to`를 만드는 순간 이미 있으면(다른 붙여넣기가 방금 그 이름을 썼다) `이름 copy`·`이름 copy 2`… 로 번호를 올려
/// 다시 시도한다. 새로 생긴 경로를 돌려준다.
fn copy_top(
    from: &str,
    to: &str,
) -> std::io::Result<String> {
    let name = file_name_of(from);
    let folder = parent_of(to);
    let is_dir = std::fs::symlink_metadata(from)?.is_dir();
    let mut target = to.to_string();
    let mut n_attempts = 0;
    let created = loop {
        let created =
            if is_dir { std::fs::create_dir(&target) } else { copy_file_new(Path::new(from), Path::new(&target)) };
        match created {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && n_attempts < MAX_NAME_ATTEMPTS => {
                n_attempts += 1;
                target = child_path(&folder, &copy_entry_name(&name, n_attempts));
            }
            other => break other,
        }
    };
    let finished =
        created.and_then(|()| if is_dir { copy_children(Path::new(from), Path::new(&target)) } else { Ok(()) });
    finished.map(|()| target)
}

/// 같은 이름 충돌 때 번호를 올려 다시 시도하는 최대 횟수.
const MAX_NAME_ATTEMPTS: usize = 1000;

/// 복사 계획을 순서대로 실행한다. `Refused`·`Unnamed`는 세어 두고 건너뛰며, 처음 실패한 항목에서 멈춘다.
pub fn run_copy_plan(plan: &[CopyStep]) -> CopyOutcome {
    let mut outcome = CopyOutcome::default();
    for step in plan {
        if outcome.failure.is_some() {
            break;
        }
        match step {
            CopyStep::Refused { .. } => outcome.n_refused += 1,
            CopyStep::Unnamed { .. } => outcome.n_unnamed += 1,
            CopyStep::Copy { from, to } => match copy_top(from, to) {
                Ok(created) => outcome.copied.push(created),
                Err(error) => {
                    outcome.failure = Some(format!("Could not copy \u{201c}{}\u{201d}: {error}", file_name_of(from)));
                }
            },
        }
    }
    outcome
}

/// 그림 바이트를 파일로 쓸 바이트로 바꾼다. BMP·모르는 형식은 PNG로 다시 인코딩하고, 나머지는 그대로다.
pub fn encoded_image(
    kind: ImageKind,
    bytes: &[u8],
) -> Result<Vec<u8>, String> {
    if kind.needs_png_encoding() {
        let decoded = image::load_from_memory(bytes).map_err(|error| format!("Could not read the image: {error}"))?;
        let mut png = Cursor::new(Vec::new());
        decoded
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| format!("Could not convert the image to PNG: {error}"))?;
        Ok(png.into_inner())
    } else {
        Ok(bytes.to_vec())
    }
}

/// 클립보드 그림을 `folder`에 `image.<확장자>`로 쓴다. 같은 이름이 있으면 `image copy.png`·`image copy 2.png`로 받아 덮어쓰지 않는다(쓰는 순간에도 `create_new`로 보장한다).
/// 쓴 경로를 돌려준다.
pub fn write_image(
    folder: &str,
    kind: ImageKind,
    bytes: &[u8],
) -> Result<String, String> {
    let data = encoded_image(kind, bytes)?;
    let name = format!("image.{}", kind.extension());
    let mut written = None;
    let mut failure = None;
    for n in 0..=MAX_NAME_ATTEMPTS {
        let target = child_path(folder, &copy_entry_name(&name, n));
        match create_new(Path::new(&target)).and_then(|mut file| std::io::Write::write_all(&mut file, &data)) {
            Ok(()) => {
                written = Some(target);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                failure = Some(format!("Could not save the image: {error}"));
                break;
            }
        }
    }
    written.ok_or_else(|| {
        failure.unwrap_or_else(|| "Could not save the image: too many images with the same name.".to_string())
    })
}

/// 터미널에 붙여 넣은 클립보드 그림을 `folder`에 `paste-<시각>-<난수>.<확장자>`로 쓰고 그 경로를 돌려준다. 폴더가 없으면 만든다. 같은 이름이 있으면 덮어쓰지 않고 실패한다.
pub fn save_pasted_image(
    folder: &Path,
    kind: ImageKind,
    bytes: &[u8],
    unix_seconds: u64,
    random: &str,
) -> Result<String, String> {
    let data = encoded_image(kind, bytes)?;
    std::fs::create_dir_all(folder).map_err(|error| format!("Could not save the image: {error}"))?;
    let target = folder.join(pasted_image_name(unix_seconds, random, kind.extension()));
    create_new(&target)
        .and_then(|mut file| std::io::Write::write_all(&mut file, &data))
        .map_err(|error| format!("Could not save the image: {error}"))?;
    Ok(target.to_string_lossy().to_string())
}

/// 두 경로가 같은 항목(같은 파일·폴더)을 가리키는지. Unix는 장치·inode 번호, Windows는 실제 철자로 푼 경로가 같은지로 가른다.
/// 읽지 못하면 다른 항목으로 본다.
fn is_same_entry(
    left: &Path,
    right: &Path,
) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(left), std::fs::symlink_metadata(right)) {
            (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
}

/// `from`을 `to`로 옮기되 `to`가 이미 있으면 `AlreadyExists`로 거부한다(덮어쓰지 않는다).
/// Linux는 `renameat2(RENAME_NOREPLACE)`로 확인과 이동을 한 번에 하고, Windows는 덮어쓰기 플래그 없는 `MoveFileExW`를 쓴다.
/// 그것을 쓸 수 없는 파일 시스템(커널·9P가 거부)과 그 밖의 운영체제는 확인한 뒤 옮긴다 — 확인과 이동 사이에 같은 이름이 생기는 경쟁이 남는다(드문 경우).
fn rename_no_replace(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let result = match rename_noreplace_linux(from, to) {
        Some(result) => result,
        None => rename_after_check(from, to),
    };
    #[cfg(windows)]
    let result = rename_noreplace_windows(from, to);
    #[cfg(not(any(target_os = "linux", windows)))]
    let result = rename_after_check(from, to);
    result
}

/// 확인한 뒤 옮긴다. 확인과 이동 사이에 같은 이름이 생기면 덮어쓰는 경쟁이 남는다.
#[cfg(not(windows))]
fn rename_after_check(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    if std::fs::symlink_metadata(to).is_ok() {
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    } else {
        std::fs::rename(from, to)
    }
}

/// `renameat2(RENAME_NOREPLACE)`로 옮긴다. 커널·파일 시스템이 그 호출을 받지 않으면 `None`(호출자가 확인 뒤 옮기기로 넘어간다).
#[cfg(target_os = "linux")]
fn rename_noreplace_linux(
    from: &Path,
    to: &Path,
) -> Option<std::io::Result<()>> {
    use std::os::unix::ffi::OsStrExt;
    let (source, target) =
        (std::ffi::CString::new(from.as_os_str().as_bytes()), std::ffi::CString::new(to.as_os_str().as_bytes()));
    match (source, target) {
        (Ok(source), Ok(target)) => {
            // SAFETY: 두 경로는 NUL로 끝나는 유효한 C 문자열이고 호출이 끝날 때까지 살아 있다.
            let status = unsafe {
                libc::syscall(
                    libc::SYS_renameat2,
                    libc::AT_FDCWD,
                    source.as_ptr(),
                    libc::AT_FDCWD,
                    target.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            };
            if status == 0 {
                Some(Ok(()))
            } else {
                let error = std::io::Error::last_os_error();
                let unsupported = matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP));
                if unsupported { None } else { Some(Err(error)) }
            }
        }
        (Err(error), _) | (_, Err(error)) => Some(Err(std::io::Error::other(error))),
    }
}

/// 덮어쓰기 플래그 없는 `MoveFileExW`로 옮긴다. 이미 있으면 `AlreadyExists`다.
#[cfg(windows)]
fn rename_noreplace_windows(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |path: &Path| path.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (source, target) = (wide(from), wide(to));
    // SAFETY: 두 버퍼는 NUL로 끝나는 UTF-16이고 호출이 끝날 때까지 살아 있다. 플래그 0은 덮어쓰지 않는다.
    let moved = unsafe { windows_sys::Win32::Storage::FileSystem::MoveFileExW(source.as_ptr(), target.as_ptr(), 0) };
    if moved != 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// 항목 `from`의 이름을 같은 폴더 안에서 `new_name`으로 바꾸고 새 경로를 돌려준다.
/// 다른 항목이 이미 그 이름이면 덮어쓰지 않고 안내 글을 돌려준다. 대소문자만 바꾸는 것(`readme.md` → `README.md`)은 `to`가 `from`과 **같은 항목**일 때만(대소문자를 가리지 않는
/// 파일 시스템에서 같은 항목의 철자만 바뀔 때만) 겹침으로 보지 않는다. 대소문자를 가리는 파일 시스템에서 `a.txt`와 `A.txt`가 따로 있으면 거부한다.
pub fn rename_entry(
    from: &str,
    new_name: &str,
) -> Result<String, String> {
    let to = renamed(from, new_name);
    let (source, target) = (Path::new(from), Path::new(&to));
    let spelling_only = is_case_only_rename(&file_name_of(from), new_name) && is_same_entry(source, target);
    let moved = if spelling_only { std::fs::rename(source, target) } else { rename_no_replace(source, target) };
    match moved {
        Ok(()) => Ok(to),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(format!("\u{201c}{new_name}\u{201d} already exists. Choose another name."))
        }
        Err(error) => Err(format!("Could not rename \u{201c}{}\u{201d}: {error}", file_name_of(from))),
    }
}

/// 파일 저장 시도의 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    /// 썼다. 값은 쓴 직후 파일의 쓴 시각·크기(읽지 못하면 None)다.
    Written(Option<FileStamp>),
    /// 열었을 때와 다른 내용이 디스크에 있어 쓰지 않았다(`force`가 아닐 때만).
    Conflict,
    /// 쓰지 못했다. 값은 안내 문장 조각(`permission denied.`)이다.
    Failed(String),
}

/// 열어 둔 파일의 편집 내용 `bytes`를 `path`에 쓴다. 디스크의 파일이 열었을 때(`expected`)와 달라졌으면 `force`가 아닌 한 쓰지 않고 `Conflict`다.
/// 파일이 그 사이 지워졌으면 충돌이 아니라 다시 만든다. 그 자리에 쓰므로(임시 파일로 바꿔치지 않음) 권한·심볼릭 링크·하드 링크가 그대로다.
///
/// Examples
/// --------
/// | 디스크의 시각·크기    | expected | force | 결과          |
/// |-----------------------|----------|-------|---------------|
/// | 같다                  | Some(A)  | false | `Written`     |
/// | 다르다                | Some(A)  | false | `Conflict`    |
/// | 다르다                | Some(A)  | true  | `Written`     |
/// | 파일이 없다           | Some(A)  | false | `Written`(다시 만듦) |
/// | 쓸 수 없다(권한 등)   | 무엇이든 | true  | `Failed`      |
pub fn save_file(
    path: &str,
    bytes: &[u8],
    expected: Option<FileStamp>,
    force: bool,
) -> SaveOutcome {
    let now = file_stamp(path);
    let conflict = !force && expected.is_some() && now.is_some() && now != expected;
    if conflict {
        SaveOutcome::Conflict
    } else {
        match write_whole_file(Path::new(path), bytes) {
            Ok(()) => SaveOutcome::Written(file_stamp(path)),
            Err(error) => SaveOutcome::Failed(describe(&error)),
        }
    }
}

/// 파일 전체를 새 내용으로 쓴다. 쓰다가 실패해도 원래 파일이 비거나 반쯤 쓰이지 않게, 일반 파일이면 같은 폴더의 임시 파일에 다 쓴 뒤 바꿔 넣는다
/// (Unix는 권한을 복사하고 `rename`, Windows는 `ReplaceFileW`가 속성·ACL을 이어받는다). 바꿔치면 안 되는 파일은 제자리에 쓴다:
/// 심볼릭 링크(바꿔치면 링크가 일반 파일이 된다), 하드 링크가 있는 파일(다른 이름이 옛 내용을 쥔다), 공유 폴더 경로(`\\wsl.localhost` 9P·UNC는 바꿔 넣기·권한 의미가 달라 실패하기 쉽다).
/// 제자리 쓰기로 넘어가는 다른 경우는 임시 파일을 **만들지** 못했을 때(폴더에 쓸 수 없음) 하나뿐이다. 임시 파일을 만든 뒤의 실패(디스크 가득·파일 크기 한도·바꿔 넣기 거부)는
/// 그 오류를 그대로 돌려준다 — 제자리에 다시 쓰면 같은 이유로 실패하면서 원본을 자르기 때문이다.
fn write_whole_file(
    path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    let attempt = if can_replace_in_folder(path) { write_by_replacing(path, bytes) } else { ReplaceAttempt::NoTemp };
    match attempt {
        ReplaceAttempt::Done => Ok(()),
        ReplaceAttempt::Failed(error) => Err(error),
        ReplaceAttempt::NoTemp => std::fs::write(path, bytes),
    }
}

/// 임시 파일로 바꿔 넣기를 시도한 결과.
enum ReplaceAttempt {
    /// 바꿔 넣었다.
    Done,
    /// 임시 파일을 만들지 못했다(원본은 건드리지 않았다). 호출자가 제자리에 쓴다.
    NoTemp,
    /// 임시 파일을 만든 뒤 쓰기·동기화·바꿔 넣기가 실패했다. 임시 파일은 지웠고 원본은 그대로다.
    Failed(std::io::Error),
}

/// 임시 파일 이름에 섞는 일련번호(같은 파일을 겹쳐 저장해도 서로의 임시 파일을 지우지 않게 한다).
static TEMP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 임시로 바꿔 넣어도 되는 파일인가(위 `write_whole_file`의 조건).
fn can_replace_in_folder(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let shared = text.starts_with("\\\\") || text.starts_with("//");
    let regular = std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file());
    !shared && regular && n_links(path) == Some(1)
}

/// 파일을 가리키는 이름(하드 링크)의 수. 알 수 없으면 None이다.
fn n_links(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).ok().map(|metadata| metadata.nlink())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let file = std::fs::File::open(path).ok()?;
        // SAFETY: 핸들은 방금 연 파일의 것이고 정보 구조체는 호출 동안 살아 있다.
        let mut information: windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION =
            unsafe { std::mem::zeroed() };
        let done = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
                file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
                &mut information,
            )
        };
        (done != 0).then_some(u64::from(information.nNumberOfLinks))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Some(1)
    }
}

/// 같은 폴더의 임시 파일에 쓴 뒤 `path`를 그것으로 바꾼다. 임시 파일은 자기가 만든 것만 지운다(`create_new`가 성공했을 때).
fn write_by_replacing(
    path: &Path,
    bytes: &[u8],
) -> ReplaceAttempt {
    let sequence = TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = path.with_file_name(format!(
        ".{}.paddock-save-{}-{sequence}",
        file_name_of(&path.to_string_lossy()),
        std::process::id()
    ));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
        Err(_) => ReplaceAttempt::NoTemp,
        Ok(mut file) => {
            use std::io::Write;
            let swapped = file.write_all(bytes).and_then(|()| file.sync_data()).and_then(|()| {
                drop(file);
                swap_in(&temp, path)
            });
            match swapped {
                Ok(()) => ReplaceAttempt::Done,
                Err(error) => {
                    let _ = std::fs::remove_file(&temp);
                    ReplaceAttempt::Failed(error)
                }
            }
        }
    }
}

/// `temp`를 `path` 자리로 옮긴다(권한·속성 유지).
fn swap_in(
    temp: &Path,
    path: &Path,
) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (replaced, replacement) = (wide(path), wide(temp));
        // SAFETY: 두 버퍼는 NUL로 끝나는 UTF-16이고 호출이 끝날 때까지 살아 있다. 백업 파일·예약 인자는 쓰지 않는다.
        let done = unsafe {
            windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                replaced.as_ptr(),
                replacement.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if done != 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
    #[cfg(not(windows))]
    {
        let permissions = std::fs::metadata(path)?.permissions();
        std::fs::set_permissions(temp, permissions)?;
        std::fs::rename(temp, path)
    }
}

/// 폴더 검색 결과의 파일들에 바꾸기를 하라는 일 하나.
#[derive(Clone, Debug)]
pub struct ReplaceJob {
    pub files: Vec<String>,
    pub query: String,
    pub options: SearchOptions,
    pub replacement: String,
}

/// 바꾸기를 마친 결과.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceOutcome {
    /// 실제로 바꿔 쓴 파일의 경로.
    pub replaced: Vec<String>,
    /// 바꾼 곳의 수(파일에서 일치마다 하나).
    pub n_matches: usize,
    /// 바꾸지 못한 파일 수(UTF-8이 아님·크기 초과·읽는 사이 바뀜·쓰기 실패).
    pub n_failed: usize,
    /// 질의를 해석하지 못했다면 그 문장(이때 파일은 하나도 건드리지 않는다).
    pub failure: Option<String>,
}

/// 파일 하나에서 같은 질의로 일치를 다시 찾아 바꾼다(검색 뒤 바뀐 파일은 지금 내용 기준으로 다시 계산한다). 바꾼 곳의 수를 돌려주고,
/// 바꿀 곳이 없으면 쓰지 않고 `Ok(0)`이다. UTF-8이 아닌 파일·너무 큰 파일은 `Err`이다. 저장은 `save_file`이라 BOM·줄바꿈이 그대로이고
/// 읽은 뒤 다른 프로그램이 고친 파일은 덮어쓰지 않는다(`Err`).
fn replace_in_file(
    path: &str,
    matcher: &grep_regex::RegexMatcher,
    replacement: &str,
    regex: bool,
) -> Result<usize, String> {
    let stamp = file_stamp(path);
    let bytes = std::fs::read(path).map_err(|error| describe(&error))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        Err("the file is too large.".to_string())
    } else {
        let (text, encoded) = decode_text(bytes);
        let edits = replace_plan(&text, matcher, replacement, regex);
        if encoded.lossy {
            Err("the file is not UTF-8.".to_string())
        } else if edits.is_empty() {
            Ok(0)
        } else {
            let written = encode_for_save(&apply_edits(&text, &edits), &encoded);
            match save_file(path, &written, stamp, false) {
                SaveOutcome::Written(_) => Ok(edits.len()),
                SaveOutcome::Conflict => Err("the file changed while replacing.".to_string()),
                SaveOutcome::Failed(reason) => Err(reason),
            }
        }
    }
}

/// `job`의 파일마다 바꾸기를 한다. 한 파일의 실패는 그 파일만 세고 나머지는 계속한다.
pub fn replace_in_files(job: &ReplaceJob) -> ReplaceOutcome {
    let mut outcome = ReplaceOutcome::default();
    match build_matcher(&job.query, job.options) {
        Err(message) => outcome.failure = Some(message),
        Ok(matcher) => {
            for path in &job.files {
                match replace_in_file(path, &matcher, &job.replacement, job.options.regex) {
                    Ok(0) => {}
                    Ok(n_matches) => {
                        outcome.n_matches += n_matches;
                        outcome.replaced.push(path.clone());
                    }
                    Err(_) => outcome.n_failed += 1,
                }
            }
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use paddock_core::incoming::copy_plan;

    use super::*;

    fn text(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn plan_for(
        sources: &[&Path],
        destination: &Path,
    ) -> Vec<CopyStep> {
        let sources: Vec<String> = sources.iter().map(|path| path.to_string_lossy().to_string()).collect();
        copy_plan(&sources, &destination.to_string_lossy(), |path| Path::new(path).exists())
    }

    #[test]
    fn test_c_d69_the_file_service_copies_a_folder_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir_all(source.join("deep")).unwrap();
        std::fs::write(source.join("a.txt"), "a").unwrap();
        std::fs::write(source.join("deep").join("b.txt"), "b").unwrap();
        let destination = dir.path().join("dest");
        std::fs::create_dir(&destination).unwrap();
        let outcome = run_copy_plan(&plan_for(&[&source], &destination));
        assert_eq!(outcome.copied, vec![child_path(&destination.to_string_lossy(), "src")]);
        assert_eq!(text(&destination.join("src").join("deep").join("b.txt")), "b");
        assert!(source.join("a.txt").exists(), "the original stays");
        // 같은 폴더를 다시 복사하면 `src copy`로 받고 앞 복사본은 그대로다.
        let again = run_copy_plan(&plan_for(&[&source], &destination));
        assert_eq!(again.copied, vec![child_path(&destination.to_string_lossy(), "src copy")]);
        assert_eq!(text(&destination.join("src").join("a.txt")), "a");
        assert_eq!(text(&destination.join("src copy").join("a.txt")), "a");
    }

    #[test]
    fn test_c_d69_a_folder_is_refused_into_itself_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir_all(source.join("deep")).unwrap();
        let outcome = run_copy_plan(&plan_for(&[&source], &source.join("deep")));
        assert_eq!((outcome.n_refused, outcome.copied.len()), (1, 0));
        assert!(!source.join("deep").join("src").exists());
    }

    #[test]
    fn test_c_d69_a_file_copy_never_overwrites_an_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.txt");
        std::fs::write(&source, "new").unwrap();
        let destination = dir.path().join("dest");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("a.txt"), "old").unwrap();
        let outcome = run_copy_plan(&plan_for(&[&source], &destination));
        assert_eq!(text(&destination.join("a.txt")), "old");
        assert_eq!(text(&destination.join("a copy.txt")), "new");
        assert_eq!(outcome.failure, None);
    }

    #[test]
    fn test_c_d69_a_pasted_bitmap_is_written_as_png() {
        let dir = tempfile::tempdir().unwrap();
        let mut bmp = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 2).write_to(&mut bmp, image::ImageFormat::Bmp).unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let first = write_image(&folder, ImageKind::Bmp, bmp.get_ref()).unwrap();
        assert!(first.ends_with("image.png"), "{first}");
        assert_eq!(&std::fs::read(&first).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        let second = write_image(&folder, ImageKind::Bmp, bmp.get_ref()).unwrap();
        assert!(second.ends_with("image copy.png"), "{second}");
        let third = write_image(&folder, ImageKind::Bmp, bmp.get_ref()).unwrap();
        assert!(third.ends_with("image copy 2.png"), "{third}");
        // PNG는 그대로 쓴다(다시 인코딩하지 않는다).
        let png = encoded_image(ImageKind::Bmp, bmp.get_ref()).unwrap();
        let kept = write_image(&folder, ImageKind::Png, &png).unwrap();
        assert_eq!(std::fs::read(kept).unwrap(), png);
        assert!(write_image(&folder, ImageKind::Bmp, b"not an image").is_err());
    }

    #[test]
    fn test_c_d69_a_terminal_pasted_image_is_saved_under_a_time_and_random_name() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("pasted-images");
        let path = save_pasted_image(&folder, ImageKind::Jpeg, b"jpeg bytes", 1_792_855_812, "a1b2").unwrap();
        assert!(path.ends_with("paste-20261024-153012-a1b2.jpg"), "{path}");
        assert_eq!(std::fs::read(path).unwrap(), b"jpeg bytes");
    }

    #[test]
    fn test_c_d70_renaming_never_overwrites_and_a_case_only_rename_skips_the_exists_check() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let from = child_path(&folder, "a.txt");
        let refused = rename_entry(&from, "b.txt").unwrap_err();
        assert!(refused.contains("already exists"), "{refused}");
        assert_eq!(text(&dir.path().join("b.txt")), "b");
        let moved = rename_entry(&from, "c.txt").unwrap();
        assert_eq!(moved, child_path(&folder, "c.txt"));
        assert!(!dir.path().join("a.txt").exists() && dir.path().join("c.txt").exists());
        // 대소문자만 바꾸면 겹침 검사 없이 바뀐다(대소문자를 가리지 않는 파일 시스템에서는 같은 항목이다).
        let upper = rename_entry(&moved, "C.txt").unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"C.txt".to_string()) && upper.ends_with("C.txt"), "{names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d70_a_case_only_rename_never_overwrites_a_different_file_on_a_case_sensitive_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        std::fs::write(dir.path().join("a.txt"), "lower").unwrap();
        std::fs::write(dir.path().join("A.txt"), "UPPER").unwrap();
        // macOS의 기본 파일 시스템은 대소문자를 가리지 않아 두 이름이 한 파일이다. 이 시험은 두 파일이 따로 있을 때만 성립한다.
        if std::fs::read_dir(dir.path()).unwrap().count() == 2 {
            let refused = rename_entry(&child_path(&folder, "a.txt"), "A.txt").unwrap_err();
            assert!(refused.contains("already exists"), "{refused}");
            assert_eq!(text(&dir.path().join("A.txt")), "UPPER", "the other file is untouched");
            assert_eq!(text(&dir.path().join("a.txt")), "lower");
        }
    }

    #[test]
    fn test_c_d70_a_single_file_changes_its_case_and_nothing_else_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        std::fs::write(dir.path().join("a.txt"), "only").unwrap();
        let to = rename_entry(&child_path(&folder, "a.txt"), "A.txt").unwrap();
        assert!(to.ends_with("A.txt"));
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["A.txt".to_string()]);
        assert_eq!(text(&dir.path().join("A.txt")), "only");
    }

    #[test]
    fn test_c_d69_two_overlapping_pastes_into_one_folder_never_overwrite_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.txt");
        std::fs::write(&source, "payload").unwrap();
        let destination = dir.path().join("dest");
        std::fs::create_dir(&destination).unwrap();
        // 두 붙여넣기가 같은 계획(같은 `to`)을 동시에 실행한다: 이름 검사에서는 둘 다 비어 있었다.
        let plan = vec![CopyStep::Copy {
            from: source.to_string_lossy().to_string(),
            to: child_path(&destination.to_string_lossy(), "a.txt"),
        }];
        let results: Vec<CopyOutcome> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4).map(|_| scope.spawn(|| run_copy_plan(&plan))).collect();
            workers.into_iter().map(|worker| worker.join().unwrap()).collect()
        });
        let mut made: Vec<String> = results.iter().flat_map(|outcome| outcome.copied.clone()).collect();
        made.sort();
        made.dedup();
        assert_eq!(made.len(), 4, "four distinct files: {made:?}");
        assert!(results.iter().all(|outcome| outcome.failure.is_none()));
        for path in made {
            assert_eq!(std::fs::read_to_string(path).unwrap(), "payload");
        }
    }

    #[test]
    fn test_c_d69_a_pasted_image_never_overwrites_an_existing_file_even_with_the_same_name_taken_late() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        std::fs::write(dir.path().join("image.png"), "keep").unwrap();
        let png = encoded_image(ImageKind::Bmp, &{
            let mut bmp = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 2).write_to(&mut bmp, image::ImageFormat::Bmp).unwrap();
            bmp.into_inner()
        })
        .unwrap();
        let written = write_image(&folder, ImageKind::Png, &png).unwrap();
        assert!(written.ends_with("image copy.png"), "{written}");
        assert_eq!(text(&dir.path().join("image.png")), "keep");
        // 터미널 붙여넣기 그림은 같은 이름이면 덮어쓰지 않고 실패한다.
        let pasted = dir.path().join("pasted");
        assert!(save_pasted_image(&pasted, ImageKind::Png, &png, 1, "same").is_ok());
        assert!(save_pasted_image(&pasted, ImageKind::Png, &png, 1, "same").is_err());
    }

    #[test]
    fn test_c_d70_renaming_a_folder_moves_what_is_inside() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join("src").join("x")).unwrap();
        std::fs::write(dir.path().join("src").join("x").join("f.txt"), "f").unwrap();
        let to = rename_entry(&child_path(&folder, "src"), "lib").unwrap();
        assert_eq!(text(&Path::new(&to).join("x").join("f.txt")), "f");
        assert!(rename_entry(&child_path(&folder, "gone"), "other").unwrap_err().starts_with("Could not rename"));
    }

    #[test]
    fn test_c_d67_save_writes_in_place_and_refuses_when_the_disk_changed_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let path = child_path(&dir.path().to_string_lossy(), "a.txt");
        std::fs::write(&path, "one").unwrap();
        let opened = file_stamp(&path);
        // 열었을 때와 같으면 쓴다.
        let SaveOutcome::Written(stamp) = save_file(&path, b"two", opened, false) else { panic!("expected a write") };
        assert_eq!(text(Path::new(&path)), "two");
        assert_eq!(stamp, file_stamp(&path));
        // 다른 프로그램이 고쳤으면(크기가 다르다) 쓰지 않고, 덮어쓰기를 고르면 쓴다.
        std::fs::write(&path, "changed elsewhere").unwrap();
        assert_eq!(save_file(&path, b"three", opened, false), SaveOutcome::Conflict);
        assert_eq!(text(Path::new(&path)), "changed elsewhere");
        assert!(matches!(save_file(&path, b"three", opened, true), SaveOutcome::Written(_)));
        assert_eq!(text(Path::new(&path)), "three");
        // 지워졌으면 충돌이 아니라 다시 만든다.
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(save_file(&path, b"four", opened, false), SaveOutcome::Written(_)));
        assert_eq!(text(Path::new(&path)), "four");
        // 쓸 수 없는 자리는 안내 문장 조각으로 실패한다.
        let missing = child_path(&dir.path().to_string_lossy(), "no-such-folder/b.txt");
        assert!(matches!(save_file(&missing, b"x", None, true), SaveOutcome::Failed(_)));
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d67_save_replaces_a_plain_file_keeping_its_permissions_and_writes_in_place_through_a_hard_link() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let plain = child_path(&folder, "plain.txt");
        std::fs::write(&plain, "old").unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(can_replace_in_folder(Path::new(&plain)));
        assert!(matches!(save_file(&plain, b"new", None, true), SaveOutcome::Written(_)));
        assert_eq!(text(Path::new(&plain)), "new");
        assert_eq!(std::fs::metadata(&plain).unwrap().permissions().mode() & 0o777, 0o640);
        // 임시 파일이 남지 않는다.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // 하드 링크가 있으면 제자리에 쓴다: 다른 이름도 새 내용을 본다.
        let linked = child_path(&folder, "linked.txt");
        std::fs::hard_link(&plain, &linked).unwrap();
        assert!(!can_replace_in_folder(Path::new(&plain)));
        assert!(matches!(save_file(&plain, b"third", None, true), SaveOutcome::Written(_)));
        assert_eq!(text(Path::new(&linked)), "third");
    }

    /// 파일 크기 한도(RLIMIT_FSIZE) 안에서 큰 내용을 저장하면 원본이 잘리지 않고 오류가 난다. 한도는 프로세스 전체에 걸리므로 시험 바이너리를 자식으로 다시 띄워 그 안에서만 건다.
    #[cfg(unix)]
    #[test]
    fn test_c_d67_a_failed_write_after_the_temp_file_exists_leaves_the_original_whole() {
        const CHILD: &str = "PADDOCK_TEST_FSIZE_CHILD";
        let dir = tempfile::tempdir().unwrap();
        let path = child_path(&dir.path().to_string_lossy(), "big.txt");
        if let Ok(target) = std::env::var(CHILD) {
            let limit = libc::rlimit { rlim_cur: 64, rlim_max: 64 };
            // SAFETY: 이 자식 프로세스에서만 파일 크기 한도를 걸고 그 초과 신호를 무시한다(쓰기는 EFBIG로 실패한다).
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                libc::setrlimit(libc::RLIMIT_FSIZE, &limit);
            }
            let outcome = save_file(&target, &vec![b'x'; 4096], None, true);
            assert!(matches!(outcome, SaveOutcome::Failed(_)), "{outcome:?}");
            let kept = std::fs::read(&target).unwrap();
            assert_eq!(kept, b"original content", "the original must not be truncated");
            let leftovers = std::fs::read_dir(std::path::Path::new(&target).parent().unwrap()).unwrap().count();
            assert_eq!(leftovers, 1, "the temp file is removed");
        } else {
            std::fs::write(&path, "original content").unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "files::ops::tests::test_c_d67_a_failed_write_after_the_temp_file_exists_leaves_the_original_whole",
                    "--nocapture",
                ])
                .env(CHILD, &path)
                .status()
                .unwrap();
            assert!(status.success());
            assert_eq!(std::fs::read(&path).unwrap(), b"original content");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d67_a_folder_that_cannot_hold_a_temp_file_falls_back_to_writing_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = child_path(&dir.path().to_string_lossy(), "a.txt");
        std::fs::write(&path, "old").unwrap();
        // 폴더에는 쓸 수 없지만 파일은 쓸 수 있다: 임시 파일을 만들지 못하므로 제자리에 쓴다(관리자 권한이면 바꿔 넣기가 되고 결과는 같다).
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let outcome = save_file(&path, b"new", None, true);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(outcome, SaveOutcome::Written(_)), "{outcome:?}");
        assert_eq!(text(Path::new(&path)), "new");
    }

    #[test]
    fn test_c_d67_shared_folder_paths_are_written_in_place() {
        for shared in ["\\\\wsl.localhost\\Ubuntu\\tmp\\a.txt", "//wsl.localhost/Ubuntu/tmp/a.txt"] {
            assert!(!can_replace_in_folder(Path::new(shared)), "{shared}");
        }
    }

    #[test]
    fn test_c_d67_saving_keeps_a_symlink_and_the_bytes_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let real = child_path(&folder, "real.txt");
        std::fs::write(&real, "x").unwrap();
        #[cfg(unix)]
        {
            let link = child_path(&folder, "link.txt");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(matches!(save_file(&link, b"\xef\xbb\xbfa\r\n", None, true), SaveOutcome::Written(_)));
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert_eq!(std::fs::read(&real).unwrap(), b"\xef\xbb\xbfa\r\n");
        }
        #[cfg(not(unix))]
        {
            assert!(matches!(save_file(&real, b"\xef\xbb\xbfa\r\n", None, true), SaveOutcome::Written(_)));
            assert_eq!(std::fs::read(&real).unwrap(), b"\xef\xbb\xbfa\r\n");
        }
    }

    #[test]
    fn test_c_d72_replace_rereads_files_and_keeps_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let crlf = child_path(&folder, "crlf.txt");
        let bom = child_path(&folder, "bom.txt");
        let latin = child_path(&folder, "latin.txt");
        let none = child_path(&folder, "none.txt");
        std::fs::write(&crlf, "a needle\r\nneedle\r\n").unwrap();
        std::fs::write(&bom, b"\xef\xbb\xbfneedle\nx").unwrap();
        std::fs::write(&latin, b"needle caf\xe9").unwrap();
        std::fs::write(&none, "nothing here").unwrap();
        let job = ReplaceJob {
            files: vec![crlf.clone(), bom.clone(), latin.clone(), none.clone()],
            query: "needle".to_string(),
            options: SearchOptions::default(),
            replacement: "thread".to_string(),
        };
        let outcome = replace_in_files(&job);
        assert_eq!((outcome.n_matches, outcome.n_failed, outcome.replaced.len()), (3, 1, 2));
        assert_eq!(std::fs::read(&crlf).unwrap(), b"a thread\r\nthread\r\n");
        assert_eq!(std::fs::read(&bom).unwrap(), b"\xef\xbb\xbfthread\nx");
        assert_eq!(std::fs::read(&latin).unwrap(), b"needle caf\xe9", "a non UTF-8 file is left alone");
        assert_eq!(text(Path::new(&none)), "nothing here");
        // 검색 뒤 바뀐 파일은 지금 내용으로 다시 계산한다: 이제 일치가 없으면 쓰지 않는다.
        let again = replace_in_files(&job);
        assert_eq!((again.n_matches, again.replaced.len()), (0, 0));
    }

    #[test]
    fn test_c_d72_an_invalid_pattern_touches_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = child_path(&dir.path().to_string_lossy(), "a.txt");
        std::fs::write(&path, "x(").unwrap();
        let job = ReplaceJob {
            files: vec![path.clone()],
            query: "(".to_string(),
            options: SearchOptions { regex: true, ..SearchOptions::default() },
            replacement: "y".to_string(),
        };
        let outcome = replace_in_files(&job);
        assert!(outcome.failure.is_some_and(|message| message.starts_with("Invalid search pattern")));
        assert_eq!(text(Path::new(&path)), "x(");
    }
}
