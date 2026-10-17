//! 파일 탭이 파일을 여는 일: 작업 스레드에서 파일을 읽어 텍스트로 보일지 안내 탭으로 보낼지 정하고, 바깥 프로그램으로 여는 명령을 만든다.
//!
//! 읽기만 한다. 파일을 고치거나 만들거나 지우는 코드가 없다.
use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use paddock_core::file_kind::{FileKind, HEAD_LEN, TEXT_LIMIT, extension_of, looks_binary};

use super::image_load::{Limits, Pixels, RasterLoad, load_raster_with};
use super::pdf_render::{FILE_LIMIT as PDF_LIMIT, OpenError, PdfDocument};
use super::svg_load::{FILE_LIMIT as SVG_LIMIT, rasterize};

/// 파일이 읽힐 때의 쓴 시각과 크기. 둘 중 하나가 달라지면 디스크에서 바뀐 파일이다.
pub type FileStamp = (std::time::SystemTime, u64);

/// 파일의 쓴 시각·크기. 읽을 수 없으면(없거나 잠김) None이다.
pub fn file_stamp(path: &str) -> Option<FileStamp> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok().map(|modified| (modified, metadata.len())))
}

/// 안내 탭이 보이는 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeReason {
    /// 텍스트가 아닌 파일(알려진 이진 확장자이거나 앞부분에 NUL이 있음).
    Binary,
    /// 텍스트 보기의 크기 상한(1 MiB)을 넘는다.
    TooLarge,
    /// 오디오·영상: 이 앱 안의 재생기가 없어 바깥 프로그램에 맡긴다.
    NotPreviewable,
    /// 이미지가 열 수 있는 크기(5천만 픽셀)를 넘는다.
    ImageTooLarge,
    /// 이미지를 읽지 못했다(손상되었거나 지원하지 않는 형식).
    ImageUnreadable,
    /// PDF 파일이 열 수 있는 크기를 넘는다.
    PdfTooLarge,
    /// 암호가 걸린 PDF.
    PdfEncrypted,
    /// PDF를 읽지 못했다(손상되었거나 이 앱의 렌더러가 읽지 못하는 형식).
    PdfUnreadable,
}

/// 파일을 읽은 결과.
#[derive(Debug)]
pub enum Loaded {
    /// 텍스트 보기에 보일 내용. 잘못된 UTF-8 바이트는 대체 글자로 보인다.
    Text { text: String, size: u64 },
    /// 안내 탭으로 보낸다. `size`는 파일 크기(바이트)다.
    Notice { reason: NoticeReason, size: u64 },
    /// 화면에 올릴 이미지. `original`은 파일 속 가로세로, `size`는 파일 크기(바이트)다.
    Image { pixels: Pixels, original: (u32, u32), size: u64 },
    /// 열어 둔 PDF 문서.
    Pdf { document: Arc<PdfDocument>, size: u64 },
    /// 파일을 열지 못했다. 사용자에게 보일 문장이다.
    Failed(String),
}

/// 파일을 읽어 어떻게 보일지 정한다. 창 스레드가 아닌 곳에서 부른다(느린 파일 시스템에서도 창이 멈추지 않게).
///
/// `kind`는 탭이 보여 줄 방식이다. 이진·미리보기 없는 종류는 내용을 읽지 않고 크기만 잰다. 텍스트 종류는 1 MiB를 넘으면
/// 안내이고, 앞 8 KB에 NUL이 있으면 이진 안내이며, 그 밖에는 내용을 읽어 텍스트로 돌려준다. 사용자가 `Open as text anyway`로
/// 연 이진 파일은 호출자가 `kind`를 `Text`로 바꾸고 `ignore_nul`을 켜서 부른다 — 사용자가 직접 고른 것이라 NUL 검사를 건너뛰되
/// 1 MiB 상한은 그대로다.
pub fn load(
    path: &str,
    kind: FileKind,
    ignore_nul: bool,
) -> Loaded {
    load_fit(path, kind, ignore_nul, None)
}

/// `load`와 같되 이미지를 칸 크기 `fit_bound`(화면 픽셀) 안으로 줄여 올린다. 칸에 맞춰 보는 동안 원본 크기 그림을 쥐지 않으려는 것이다.
pub fn load_fit(
    path: &str,
    kind: FileKind,
    ignore_nul: bool,
    fit_bound: Option<(u32, u32)>,
) -> Loaded {
    match File::open(path).and_then(|file| file.metadata().map(|metadata| (file, metadata))) {
        Err(error) => Loaded::Failed(format!("Could not open this file: {}", describe(&error))),
        Ok((_, metadata)) if !metadata.is_file() => {
            Loaded::Failed("Could not open this file: it is not a file.".into())
        }
        Ok((file, metadata)) => {
            let size = metadata.len();
            match kind {
                FileKind::Binary => Loaded::Notice { reason: NoticeReason::Binary, size },
                FileKind::Media => Loaded::Notice { reason: NoticeReason::NotPreviewable, size },
                FileKind::Image => read_image(path, size, fit_bound),
                FileKind::Svg => read_svg(file, size),
                FileKind::Pdf => read_pdf(file, size),
                FileKind::Text | FileKind::Markdown => read_text(file, size, ignore_nul),
                // 변경 비교는 파일 내용이 아니라 git 결과라 창이 따로 읽는다. 여기로 오면 호출 실수이므로 안내만 한다.
                FileKind::Diff { .. } => Loaded::Failed("Could not open this file: it is a comparison.".into()),
            }
        }
    }
}

/// 래스터 이미지를 읽는다. 크기가 한도를 넘으면 디코드 없이 안내로 보낸다.
fn read_image(
    path: &str,
    size: u64,
    fit_bound: Option<(u32, u32)>,
) -> Loaded {
    match load_raster_with(path, Limits::viewer(fit_bound)) {
        RasterLoad::Ready { pixels, original } => Loaded::Image { pixels, original, size },
        RasterLoad::TooLarge { .. } => Loaded::Notice { reason: NoticeReason::ImageTooLarge, size },
        RasterLoad::Failed(_) => Loaded::Notice { reason: NoticeReason::ImageUnreadable, size },
    }
}

/// SVG 원문을 읽는다. 한도를 넘으면 안내로 보낸다.
fn read_svg(
    file: File,
    size: u64,
) -> Loaded {
    let mut bytes = Vec::new();
    match file.take(SVG_LIMIT + 1).read_to_end(&mut bytes) {
        Err(error) => Loaded::Failed(format!("Could not open this file: {}", describe(&error))),
        Ok(_) if bytes.len() as u64 > SVG_LIMIT => Loaded::Notice { reason: NoticeReason::ImageTooLarge, size },
        Ok(_) => match rasterize(&bytes) {
            Some((pixels, original)) => Loaded::Image { pixels, original, size },
            None => Loaded::Notice { reason: NoticeReason::ImageUnreadable, size },
        },
    }
}

/// PDF를 읽어 문서로 연다. 한도를 넘거나 암호가 걸렸거나 읽지 못하면 안내로 보낸다.
fn read_pdf(
    file: File,
    size: u64,
) -> Loaded {
    let mut bytes = Vec::new();
    match file.take(PDF_LIMIT + 1).read_to_end(&mut bytes) {
        Err(error) => Loaded::Failed(format!("Could not open this file: {}", describe(&error))),
        Ok(_) if bytes.len() as u64 > PDF_LIMIT => Loaded::Notice { reason: NoticeReason::PdfTooLarge, size },
        Ok(_) => match PdfDocument::open(bytes) {
            Ok(document) => Loaded::Pdf { document: Arc::new(document), size },
            Err(OpenError::Encrypted) => Loaded::Notice { reason: NoticeReason::PdfEncrypted, size },
            Err(OpenError::Unreadable) => Loaded::Notice { reason: NoticeReason::PdfUnreadable, size },
        },
    }
}

/// 텍스트 종류 파일의 내용을 읽는다. 상한보다 한 바이트 더 읽어 크기를 잴 때 사이에 늘어난 파일도 잡는다.
fn read_text(
    file: File,
    size: u64,
    ignore_nul: bool,
) -> Loaded {
    let mut bytes = Vec::new();
    let read = file.take(TEXT_LIMIT + 1).read_to_end(&mut bytes);
    match read {
        Err(error) => Loaded::Failed(format!("Could not open this file: {}", describe(&error))),
        Ok(_) if bytes.len() as u64 > TEXT_LIMIT => Loaded::Notice { reason: NoticeReason::TooLarge, size },
        Ok(_) if !ignore_nul && looks_binary(&bytes[..bytes.len().min(HEAD_LEN)]) => {
            Loaded::Notice { reason: NoticeReason::Binary, size }
        }
        Ok(_) => {
            // UTF-8 BOM은 보기에서만 뗀다(파일은 그대로다).
            let mut body = bytes;
            if body.starts_with(b"\xef\xbb\xbf") {
                body.drain(..3);
            }
            Loaded::Text { text: into_text(body), size }
        }
    }
}

/// 바이트를 글자로 만든다. UTF-8이면 복사 없이 쓰고, 아니면 잘못된 바이트만 대체 글자로 바꾼다.
fn into_text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

/// 입출력 오류를 사용자에게 보일 짧은 영어 문장 조각으로 바꾼다.
fn describe(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "the file no longer exists.".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied.".to_string(),
        _ => format!("{error}."),
    }
}

/// 안내 탭의 단추 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    ShowInFileManager,
    /// 텍스트로 억지로 연다(이진 파일을 사용자가 직접 고른 경우).
    OpenAsText,
    /// 바깥 프로그램에 맡긴다.
    OpenWithDefaultApp,
}

impl NoticeAction {
    /// 단추에 보이는 글자.
    pub fn label(self) -> &'static str {
        match self {
            Self::ShowInFileManager => "Show in file manager",
            Self::OpenAsText => "Open as text anyway",
            Self::OpenWithDefaultApp => "Open with default app",
        }
    }
}

/// 안내 탭이 이유별로 보이는 문구.
///
/// Examples
/// --------
/// | 이유             | 문구                                                                   |
/// |------------------|------------------------------------------------------------------------|
/// | `Binary`         | `This file can't be previewed here. It is a binary file, not text.`    |
/// | `TooLarge`       | `This file is larger than 1 MB.`                                       |
/// | `NotPreviewable` | `This file can't be previewed here. Open it with the default app.`     |
/// | `ImageTooLarge`  | `This image is too large to preview here.`                             |
/// | `PdfEncrypted`   | `This PDF is password protected. Open it with the default app.`        |
pub fn notice_message(reason: NoticeReason) -> &'static str {
    match reason {
        NoticeReason::ImageTooLarge => "This image is too large to preview here.",
        NoticeReason::ImageUnreadable => "Can't display this image here.",
        NoticeReason::PdfTooLarge => "This PDF is too large to preview here.",
        NoticeReason::PdfEncrypted => "This PDF is password protected. Open it with the default app.",
        NoticeReason::PdfUnreadable => "Can't display this PDF here.",
        NoticeReason::Binary => "This file can't be previewed here. It is a binary file, not text.",
        NoticeReason::TooLarge => "This file is larger than 1 MB.",
        NoticeReason::NotPreviewable => "This file can't be previewed here. Open it with the default app.",
    }
}

/// 안내 탭이 이유별로 두는 단추(왼쪽부터). 이진 파일만 텍스트로 억지로 열 수 있다 — 너무 큰 파일은 텍스트 보기가 받지 않는다.
pub fn notice_actions(reason: NoticeReason) -> Vec<NoticeAction> {
    match reason {
        NoticeReason::Binary => vec![NoticeAction::ShowInFileManager, NoticeAction::OpenAsText],
        NoticeReason::TooLarge
        | NoticeReason::NotPreviewable
        | NoticeReason::ImageTooLarge
        | NoticeReason::ImageUnreadable
        | NoticeReason::PdfTooLarge
        | NoticeReason::PdfEncrypted
        | NoticeReason::PdfUnreadable => {
            vec![NoticeAction::ShowInFileManager, NoticeAction::OpenWithDefaultApp]
        }
    }
}

/// 더블클릭해도 실행되지 않고 문서·편집기로 열리는 Windows 확장자(소문자). 편집기로 열 때(`as_text`) 이 확장자만 기본 앱에 맡긴다.
/// 코드·스크립트 확장자는 기본 앱이 실행기(`.py`·`.js`·`.sh`)이거나 엉뚱한 프로그램(`.ts`는 영상 재생기)일 수 있어 메모장으로 연다.
const DOCUMENT_EXTENSIONS: [&str; 16] = [
    "txt", "md", "markdown", "json", "yaml", "yml", "toml", "csv", "tsv", "log", "ini", "cfg", "conf", "xml", "rst",
    "css",
];

/// 파일을 바깥 프로그램으로 여는 프로그램과 인자. 셸을 거치지 않고 인자 배열로 띄우며, 경로는 절대 경로라 `--`가 필요 없다.
///
/// `as_text`는 편집기로 열고 싶은 경우(`Open in external editor`·너무 큰 텍스트)다. 이때는 파일을 프로그램으로 실행하지 않는다:
/// Windows는 문서 확장자(`DOCUMENT_EXTENSIONS`)만 기본 앱에 맡기고 나머지는 메모장으로 열며, macOS는 `open -t`로 텍스트 편집기에 보낸다.
/// `as_text`가 거짓이면 이미지·PDF·오디오·영상을 기본 앱에 맡긴다.
///
/// Examples
/// --------
/// | os        | 경로                | as_text | 프로그램       | 인자                    |
/// |-----------|---------------------|---------|----------------|-------------------------|
/// | `windows` | `C:/a/notes.md`     | 참      | `explorer.exe` | `C:\a\notes.md`         |
/// | `windows` | `C:/a/run.py`       | 참      | `notepad.exe`  | `C:\a\run.py`           |
/// | `macos`   | `/a/run.sh`         | 참      | `open`         | `-t`, `/a/run.sh`       |
/// | `macos`   | `/a/b.pdf`          | 거짓    | `open`         | `/a/b.pdf`              |
/// | `linux`   | `/a/run.sh`         | 참      | `xdg-open`     | `/a/run.sh`             |
pub fn open_command(
    os: &str,
    path: &str,
    as_text: bool,
) -> (String, Vec<String>) {
    match os {
        "windows" => {
            let native = path.replace('/', "\\");
            let is_document = DOCUMENT_EXTENSIONS.contains(&extension_of(path).as_str());
            let program = if as_text && !is_document { "notepad.exe" } else { "explorer.exe" };
            (program.to_string(), vec![native])
        }
        "macos" if as_text => ("open".to_string(), vec!["-t".to_string(), path.to_string()]),
        "macos" => ("open".to_string(), vec![path.to_string()]),
        _ => ("xdg-open".to_string(), vec![path.to_string()]),
    }
}

/// 웹·메일 주소를 기본 브라우저·메일 프로그램으로 연다. 호출자가 `markdown_view::external_link`로 허용 범위를 이미 걸렀다.
///
/// 셸을 거치지 않는다: Windows는 셸의 `open` 동작에 주소를 그대로 넘기고(기존 파일 열기와 같은 경로), 다른 운영체제는 인자 배열로 프로그램을
/// 띄운다. 기다리지 않는 별도 스레드에서 돈다. 실패는 진단 기록에만 남는다.
pub fn open_link_externally(target: &str) {
    let target = target.to_string();
    std::thread::spawn(move || {
        #[cfg(windows)]
        let outcome = shell_open_target(&target);
        #[cfg(not(windows))]
        let outcome = {
            let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
            std::process::Command::new(program).arg(&target).spawn().map(|_| ()).map_err(|error| error.to_string())
        };
        if let Err(reason) = outcome {
            crate::app_log::event(&format!("could not open a link: {reason}"));
        }
    });
}

/// Windows 셸의 `open` 동작으로 파일을 기본 앱에 맡긴다. 경로를 명령줄이 아니라 그대로 넘기므로 쉼표·공백·한글이 든 경로도 해석이 어긋나지 않는다.
///
/// 셸이 앱을 띄울 때까지 걸릴 수 있어 창 스레드가 아닌 곳에서 부른다. 실패하면 사용자에게 보일 이유 문장을 돌려준다.
#[cfg(windows)]
pub fn shell_open(path: &str) -> Result<(), String> {
    shell_open_target(&path.replace('/', "\\"))
}

/// `shell_open`의 본체: 이미 Windows 꼴로 맞춘 경로나 주소를 그대로 셸에 넘긴다.
#[cfg(windows)]
fn shell_open_target(target: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(target));
    // SAFETY: 두 글자열은 NUL로 끝나고 호출이 끝날 때까지 살아 있으며, 나머지 인자는 null 허용 자리에 null을 준다.
    // COM은 이 스레드에서 초기화에 성공했을 때만 해제한다.
    let code = unsafe {
        let com = CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32);
        let code = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        ) as usize;
        if com >= 0 {
            CoUninitialize();
        }
        code
    };
    match code {
        0..=32 => Err(match code {
            2 | 3 => "the file was not found.".to_string(),
            5 => "access was denied.".to_string(),
            31 => "no app is set up to open this kind of file.".to_string(),
            other => format!("the shell returned code {other}."),
        }),
        _ => Ok(()),
    }
}

#[cfg(windows)]
#[cfg(test)]
mod windows_tests {
    use super::*;

    #[test]
    fn test_shell_open_passes_commas_spaces_and_hangul_through_untouched_so_a_missing_file_is_just_an_error() {
        // 셸에 넘기는 경로는 문자열 그대로(쉼표·공백·한글 포함)라, 없는 파일은 인자 해석 오류가 아니라 "없음"으로만 실패한다.
        let error = shell_open("C:\\paddock-no-such-folder, a b\\한글 문서.txt").unwrap_err();
        assert!(error.contains("not found"), "{error}");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use paddock_core::file_kind::{FileKind, TEXT_LIMIT};

    use super::*;

    fn write(
        dir: &Path,
        name: &str,
        bytes: &[u8],
    ) -> String {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn test_c_d10_loading_a_file_for_the_text_view_leaves_it_untouched() {
        // 읽기 전용 파일을 열어도 내용·수정 시각이 그대로다(열기는 읽기뿐이다). 편집기에 키를 흘려도 불변인지는 `file-readonly` 대본이 확인한다.
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "a.rs", b"fn main() {}\n");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        let before = (fs::read(&path).unwrap(), fs::metadata(&path).unwrap().modified().unwrap());
        for _ in 0..2 {
            assert!(matches!(load(&path, FileKind::Text, false), Loaded::Text { .. }));
        }
        let after = (fs::read(&path).unwrap(), fs::metadata(&path).unwrap().modified().unwrap());
        assert_eq!(before, after);
    }

    #[test]
    fn test_a_small_text_file_loads_with_its_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "a.ts", "const 한글 = 1;\r\n".as_bytes());
        match load(&path, FileKind::Text, false) {
            Loaded::Text { text, size } => assert_eq!((text.as_str(), size), ("const 한글 = 1;\r\n", 19)),
            other => panic!("expected text, got {other:?}"),
        }
        let empty = write(dir.path(), "empty.txt", b"");
        assert!(matches!(load(&empty, FileKind::Text, false), Loaded::Text { size: 0, .. }));
    }

    #[test]
    fn test_a_file_over_one_mebibyte_becomes_a_notice_without_being_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let limit = usize::try_from(TEXT_LIMIT).unwrap();
        let exact = write(dir.path(), "exact.txt", &vec![b'a'; limit]);
        assert!(matches!(load(&exact, FileKind::Text, false), Loaded::Text { .. }), "exactly 1 MiB still opens");
        let big = write(dir.path(), "big.txt", &vec![b'a'; limit + 1]);
        assert!(matches!(
            load(&big, FileKind::Text, false),
            Loaded::Notice { reason: NoticeReason::TooLarge, size } if size == TEXT_LIMIT + 1
        ));
    }

    #[test]
    fn test_a_nul_in_the_first_8_kb_makes_even_an_unknown_extension_binary() {
        let dir = tempfile::tempdir().unwrap();
        let blob = write(dir.path(), "blob.xyz", b"ab\0cd");
        assert!(matches!(load(&blob, FileKind::Text, false), Loaded::Notice { reason: NoticeReason::Binary, size: 5 }));
        let mut late = vec![b'a'; 9000];
        late.push(0);
        let tail = write(dir.path(), "tail.xyz", &late);
        assert!(matches!(load(&tail, FileKind::Text, false), Loaded::Text { .. }), "a NUL after 8 KB is ignored");
    }

    #[test]
    fn test_binary_extension_and_media_become_notices_with_the_size() {
        let dir = tempfile::tempdir().unwrap();
        let zip = write(dir.path(), "a.zip", b"PK\x03\x04plain text inside");
        assert!(matches!(
            load(&zip, FileKind::Binary, false),
            Loaded::Notice { reason: NoticeReason::Binary, size: 21 }
        ));
        assert!(matches!(
            load(&zip, FileKind::Media, false),
            Loaded::Notice { reason: NoticeReason::NotPreviewable, size: 21 }
        ));
    }

    #[test]
    fn test_c_img_open_a_png_loads_as_pixels_and_a_broken_image_becomes_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("a.png");
        image::ImageBuffer::from_pixel(3, 2, image::Rgba([1u8, 2, 3, 255])).save(&png).unwrap();
        let path = png.to_string_lossy().into_owned();
        match load(&path, FileKind::Image, false) {
            Loaded::Image { pixels, original, size } => {
                assert_eq!((pixels.width, pixels.height, original), (3, 2, (3, 2)));
                assert!(size > 0);
            }
            other => panic!("expected an image, got {other:?}"),
        }
        let broken = write(dir.path(), "b.png", b"not a png");
        assert!(matches!(
            load(&broken, FileKind::Image, false),
            Loaded::Notice { reason: NoticeReason::ImageUnreadable, size: 9 }
        ));
    }

    #[test]
    fn test_c_img_open_svg_is_drawn_and_an_oversized_or_broken_one_is_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let svg = write(dir.path(), "a.svg", b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>");
        assert!(matches!(load(&svg, FileKind::Svg, false), Loaded::Image { original: (4, 4), size: 62, .. }));
        let big = write(dir.path(), "big.svg", &vec![b' '; usize::try_from(SVG_LIMIT).unwrap() + 1]);
        assert!(matches!(load(&big, FileKind::Svg, false), Loaded::Notice { reason: NoticeReason::ImageTooLarge, .. }));
    }

    #[test]
    fn test_c_pdf_open_a_pdf_loads_as_a_document_and_encrypted_or_broken_ones_become_notices() {
        let dir = tempfile::tempdir().unwrap();
        let sample = write(dir.path(), "s.pdf", include_bytes!("../../../../scripts/assets/sample-3p.pdf"));
        match load(&sample, FileKind::Pdf, false) {
            Loaded::Pdf { document, .. } => assert_eq!(document.n_pages(), 3),
            other => panic!("expected a pdf, got {other:?}"),
        }
        let locked = write(dir.path(), "e.pdf", include_bytes!("../../../../scripts/assets/encrypted.pdf"));
        assert!(matches!(
            load(&locked, FileKind::Pdf, false),
            Loaded::Notice { reason: NoticeReason::PdfEncrypted, .. }
        ));
        let broken = write(dir.path(), "x.pdf", b"%PDF-1.4 garbage");
        assert!(matches!(
            load(&broken, FileKind::Pdf, false),
            Loaded::Notice { reason: NoticeReason::PdfUnreadable, .. }
        ));
    }

    #[test]
    fn test_a_missing_or_unreadable_path_reports_a_sentence_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone.txt").to_string_lossy().into_owned();
        match load(&missing, FileKind::Text, false) {
            Loaded::Failed(message) => assert!(message.starts_with("Could not open this file"), "{message}"),
            other => panic!("expected a failure, got {other:?}"),
        }
        let folder = dir.path().to_string_lossy().into_owned();
        assert!(matches!(load(&folder, FileKind::Text, false), Loaded::Failed(_)), "a folder is not a file");
    }

    #[test]
    fn test_open_as_text_anyway_skips_the_nul_check_but_not_the_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let blob = write(dir.path(), "blob.xyz", b"ab\0cd");
        assert!(matches!(load(&blob, FileKind::Text, true), Loaded::Text { size: 5, .. }));
        let big = write(dir.path(), "big.bin", &vec![0; usize::try_from(TEXT_LIMIT).unwrap() + 1]);
        assert!(matches!(load(&big, FileKind::Text, true), Loaded::Notice { reason: NoticeReason::TooLarge, .. }));
    }

    #[test]
    fn test_notice_text_and_buttons_follow_the_reason() {
        assert_eq!(
            notice_message(NoticeReason::Binary),
            "This file can't be previewed here. It is a binary file, not text."
        );
        assert_eq!(notice_message(NoticeReason::TooLarge), "This file is larger than 1 MB.");
        let labels = |reason| notice_actions(reason).into_iter().map(NoticeAction::label).collect::<Vec<_>>();
        assert_eq!(labels(NoticeReason::Binary), ["Show in file manager", "Open as text anyway"]);
        assert_eq!(labels(NoticeReason::TooLarge), ["Show in file manager", "Open with default app"]);
        assert_eq!(labels(NoticeReason::NotPreviewable), ["Show in file manager", "Open with default app"]);
        for reason in [
            NoticeReason::ImageTooLarge,
            NoticeReason::ImageUnreadable,
            NoticeReason::PdfTooLarge,
            NoticeReason::PdfEncrypted,
            NoticeReason::PdfUnreadable,
        ] {
            assert_eq!(labels(reason), ["Show in file manager", "Open with default app"], "{reason:?}");
        }
        assert_eq!(notice_message(NoticeReason::ImageTooLarge), "This image is too large to preview here.");
        assert_eq!(notice_message(NoticeReason::PdfUnreadable), "Can't display this PDF here.");
    }

    #[test]
    fn test_a_utf8_bom_is_dropped_from_the_view_but_the_size_still_counts_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "bom.txt", b"\xef\xbb\xbfhello");
        match load(&path, FileKind::Text, false) {
            Loaded::Text { text, size } => assert_eq!((text.as_str(), size), ("hello", 8)),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn test_utf16_text_has_nul_bytes_so_it_is_a_binary_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "wide.txt", b"\xff\xfeh\0i\0");
        assert!(matches!(load(&path, FileKind::Text, false), Loaded::Notice { reason: NoticeReason::Binary, .. }));
    }

    #[test]
    fn test_invalid_utf8_is_shown_with_replacement_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "latin.txt", b"caf\xe9");
        match load(&path, FileKind::Text, false) {
            Loaded::Text { text, .. } => assert_eq!(text, "caf\u{fffd}"),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn test_the_open_command_never_runs_a_script_as_a_program() {
        let editor = |os: &str, path: &str| open_command(os, path, true);
        assert_eq!(editor("windows", "C:/a/notes.md"), ("explorer.exe".into(), vec![r"C:\a\notes.md".into()]));
        // 더블클릭하면 실행되거나 엉뚱한 프로그램이 열릴 수 있는 확장자·이름은 메모장으로 연다.
        for path in ["C:/a/run.js", "C:/a/x.BAT", "C:/a/y.ps1", "C:/a/z.vbs", "C:/a/w.py", "C:/a/q.ts", "C:/a/Makefile"]
        {
            assert_eq!(editor("windows", path).0, "notepad.exe", "{path}");
        }
        for path in ["C:/a/n.TXT", "C:/a/d.json", "C:/a/s.css"] {
            assert_eq!(editor("windows", path).0, "explorer.exe", "{path}");
        }
        assert_eq!(editor("macos", "/a/run.sh"), ("open".into(), vec!["-t".into(), "/a/run.sh".into()]));
        assert_eq!(editor("linux", "/a/run.sh"), ("xdg-open".into(), vec!["/a/run.sh".into()]));
        assert_eq!(open_command("macos", "/a/b.pdf", false), ("open".into(), vec!["/a/b.pdf".into()]));
        assert_eq!(open_command("windows", "C:/a/b.mp3", false).0, "explorer.exe");
    }
}
