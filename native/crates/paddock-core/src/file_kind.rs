//! 파일을 어떻게 보여 줄지 정하는 규칙: 파일 종류, 텍스트로 볼 수 없는 파일 판정, 크기 글자, 구문 강조 언어.
//!
//! 디스크를 읽는 일은 앱이 하고, 여기서는 이름과 앞부분 바이트만 받아 판정하는 순수 규칙만 둔다.
/// 텍스트 보기가 여는 파일 크기의 상한(1 MiB). 넘으면 안내 탭으로 보낸다.
pub const TEXT_LIMIT: u64 = 1024 * 1024;

/// 구문 강조를 켜는 텍스트 크기의 상한(512 KiB). 강조는 구문 트리와 글자별 스타일을 만들어 메모리를 크기에 거의 비례해 쓰므로(실측:
/// 1 MiB 파일이 강조 없이 +5 MB, 강조하면 +18 MB) 이보다 큰 파일은 강조 없이 보인다.
pub const HIGHLIGHT_LIMIT: u64 = 512 * 1024;

/// 확장자로 종류를 모르는 파일이 이진인지 볼 때 읽는 앞부분 길이(8 KB).
pub const HEAD_LEN: usize = 8 * 1024;

/// 텍스트로 읽을 수 없는 파일의 확장자(소문자, 점 없음). 이 파일을 텍스트로 열면 쓸모없는 글자만 보이므로
/// 텍스트 보기 대신 안내 탭을 연다. 이미지·오디오·영상·PDF는 각자의 종류가 있어 넣지 않는다.
const BINARY_EXTENSIONS: [&str; 41] = [
    "zip", "gz", "tgz", "bz2", "xz", "7z", "rar", "tar", "dmg", "iso", "jar", "war", "apk", "exe", "dll", "so",
    "dylib", "o", "a", "lib", "bin", "dat", "class", "pyc", "wasm", "ttf", "otf", "woff", "woff2", "eot", "db",
    "sqlite", "sqlite3", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "psd", "ai",
];

/// 별도 뷰어가 맡는 이미지 확장자(SVG 제외).
const IMAGE_EXTENSIONS: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff"];

/// 오디오·영상 확장자.
const MEDIA_EXTENSIONS: [&str; 13] =
    ["mp3", "wav", "ogg", "flac", "m4a", "aac", "mp4", "mov", "webm", "mkv", "avi", "m4v", "wmv"];

/// 파일 탭이 보여 주는 내용의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Text,
    Markdown,
    Image,
    Svg,
    Pdf,
    /// 오디오·영상. 앱 안에서 재생하지 않는다.
    Media,
    /// 텍스트로 읽을 수 없는 파일.
    Binary,
    /// Git 변경 비교. 파일 내용이 아니라 `git diff` 결과를 보여 준다. `staged`는 스테이징된 쪽(index와 HEAD)을 비교한다는 뜻이다.
    Diff {
        staged: bool,
    },
}

impl FileKind {
    /// 상태 글과 저장 값에 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Image => "image",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Media => "media",
            Self::Binary => "binary",
            Self::Diff { staged: false } => "diff",
            Self::Diff { staged: true } => "diff-staged",
        }
    }
}

/// 경로의 확장자(점 없음, 소문자). 폴더 이름의 점이나 이름이 점으로 시작하는 파일(`.gitignore`)은 확장자가 아니며, 없으면 빈 글자다.
///
/// Examples
/// --------
/// | 경로                   | 결과    |
/// |------------------------|---------|
/// | `/p/a.TXT`             | `txt`   |
/// | `/p/v1.2/readme`       | (빈 글자) |
/// | `/p/.gitignore`        | (빈 글자) |
/// | `/p/archive.tar.gz`    | `gz`    |
pub fn extension_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// 경로의 확장자로 파일 종류를 정한다. 확장자로 모르는 파일은 `Text`이고, 실제로 텍스트인지는 앞부분 바이트(`looks_binary`)가 가른다.
///
/// Examples
/// --------
/// | 경로             | 결과       |
/// |------------------|------------|
/// | `/p/a.rs`        | `Text`     |
/// | `/p/Setup.EXE`   | `Binary`   |
/// | `/p/b.JPG`       | `Image`    |
/// | `/p/logo.svg`    | `Svg`      |
/// | `/p/paper.pdf`   | `Pdf`      |
/// | `/p/README.md`   | `Markdown` |
/// | `/p/d.MOV`       | `Media`    |
pub fn kind_of(path: &str) -> FileKind {
    let extension = extension_of(path);
    let extension = extension.as_str();
    if BINARY_EXTENSIONS.contains(&extension) {
        FileKind::Binary
    } else if IMAGE_EXTENSIONS.contains(&extension) {
        FileKind::Image
    } else if MEDIA_EXTENSIONS.contains(&extension) {
        FileKind::Media
    } else {
        match extension {
            "svg" => FileKind::Svg,
            "pdf" => FileKind::Pdf,
            "md" | "markdown" => FileKind::Markdown,
            _ => FileKind::Text,
        }
    }
}

/// 파일 크기 글자: 1 KB 미만은 바이트, 그 위는 소수 첫째 자리까지 KB·MB·GB.
///
/// Examples
/// --------
/// | 바이트        | 결과       |
/// |---------------|------------|
/// | 1023          | `1023 B`   |
/// | 1536          | `1.5 KB`   |
/// | 1048576       | `1.0 MB`   |
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value >= KB * KB * KB {
        format!("{:.1} GB", value / (KB * KB * KB))
    } else if value >= KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 파일 앞부분 바이트에 NUL(0)이 있으면 텍스트가 아닌 파일로 본다. 앞 `HEAD_LEN` 바이트만 보며, 그 뒤의 NUL은 무시한다.
///
/// 확장자로 종류를 모르는 파일을 텍스트 보기에 열기 전에 거르는 데 쓴다.
pub fn looks_binary(head: &[u8]) -> bool {
    head.iter().take(HEAD_LEN).any(|byte| *byte == 0)
}

/// 확장자(점 없음, 대소문자 무관)에 맞는 구문 강조 언어 이름. 문법이 없는 확장자는 None이다.
///
/// Examples
/// --------
/// | 확장자        | 결과         |
/// |---------------|--------------|
/// | `ts`          | `typescript` |
/// | `tsx`         | `tsx`        |
/// | `yml`         | `yaml`       |
/// | `sh`          | `bash`       |
/// | `txt`         | None         |
pub fn language_of(extension: &str) -> Option<&'static str> {
    match extension.to_lowercase().as_str() {
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "py" => Some("python"),
        "json" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "md" | "markdown" => Some("markdown"),
        "sh" | "bash" => Some("bash"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "rs" => Some("rust"),
        "toml" => Some("toml"),
        _ => None,
    }
}

/// 텍스트 보기가 쓸 구문 강조 언어. `language_of`와 같되 `HIGHLIGHT_LIMIT`보다 큰 파일은 강조하지 않으므로 None이다.
///
/// Examples
/// --------
/// | 확장자 | 크기       | 결과         |
/// |--------|------------|--------------|
/// | `ts`   | 1 KB       | `typescript` |
/// | `ts`   | 512 KiB    | `typescript` |
/// | `ts`   | 512 KiB + 1 | None        |
/// | `txt`  | 1 KB       | None         |
pub fn highlight_language_of(
    extension: &str,
    size: u64,
) -> Option<&'static str> {
    if size > HIGHLIGHT_LIMIT { None } else { language_of(extension) }
}

/// 경로의 파일 이름(마지막 구성 요소).
pub fn file_name_of(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_kind_1_text_and_unknown_extensions_are_text() {
        for path in ["/p/a.rs", "/p/notes.txt", "/p/Makefile", "/p/.gitignore", "C:\\work\\main.go", "/p/v1.2/readme"] {
            assert_eq!(kind_of(path), FileKind::Text, "{path}");
        }
    }

    #[test]
    fn test_c_kind_2_binary_extensions_are_binary_ignoring_case() {
        for path in ["/p/a.zip", "/p/Setup.EXE", "/p/lib.so", "/p/f.woff2", "/p/d.sqlite3", "/p/r.docx", "/p/x.pyc"] {
            assert_eq!(kind_of(path), FileKind::Binary, "{path}");
        }
    }

    #[test]
    fn test_c_kind_3_image_extensions_are_images_and_svg_is_its_own_kind() {
        for path in ["/p/a.png", "/p/b.JPG", "/p/c.jpeg", "/p/d.gif", "/p/e.webp", "/p/f.bmp", "/p/g.ico"] {
            assert_eq!(kind_of(path), FileKind::Image, "{path}");
        }
        assert_eq!(kind_of("/p/logo.svg"), FileKind::Svg);
    }

    #[test]
    fn test_c_kind_4_pdf_and_markdown_have_their_own_kinds() {
        assert_eq!(kind_of("/p/paper.pdf"), FileKind::Pdf);
        assert_eq!(kind_of("/p/README.md"), FileKind::Markdown);
        assert_eq!(kind_of("/p/notes.markdown"), FileKind::Markdown);
    }

    #[test]
    fn test_c_kind_5_audio_and_video_extensions_are_media() {
        for path in ["/p/a.mp3", "/p/b.wav", "/p/c.mp4", "/p/d.MOV", "/p/e.webm", "/p/f.flac"] {
            assert_eq!(kind_of(path), FileKind::Media, "{path}");
        }
    }

    #[test]
    fn test_c_kind_6_sizes_use_one_decimal_from_kilobytes_up() {
        let table = [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1.0 KB"),
            (1536, "1.5 KB"),
            (1024 * 1024, "1.0 MB"),
            (5 * 1024 * 1024 + 512 * 1024, "5.5 MB"),
            (3 * 1024 * 1024 * 1024, "3.0 GB"),
        ];
        for (bytes, expected) in table {
            assert_eq!(format_size(bytes), expected, "{bytes}");
        }
    }

    #[test]
    fn test_c_kind_7_a_nul_byte_in_the_head_means_binary() {
        assert!(looks_binary(b"MZ\x90\x00\x03"));
        assert!(looks_binary(&[0]));
    }

    #[test]
    fn test_c_kind_8_text_without_nul_is_not_binary_and_neither_is_empty() {
        assert!(!looks_binary("한글 text\r\nline 2\n".as_bytes()));
        assert!(!looks_binary(b""));
    }

    #[test]
    fn test_c_kind_9_a_nul_after_the_first_8_kb_is_ignored() {
        let mut bytes = vec![b'a'; HEAD_LEN];
        bytes.push(0);
        assert!(!looks_binary(&bytes));
        bytes[HEAD_LEN - 1] = 0;
        assert!(looks_binary(&bytes));
    }

    #[test]
    fn test_c_kind_10_languages_follow_the_extension_for_the_twelve_grammars() {
        let table = [
            ("js", Some("javascript")),
            ("jsx", Some("javascript")),
            ("ts", Some("typescript")),
            ("tsx", Some("tsx")),
            ("py", Some("python")),
            ("json", Some("json")),
            ("yaml", Some("yaml")),
            ("yml", Some("yaml")),
            ("md", Some("markdown")),
            ("sh", Some("bash")),
            ("bash", Some("bash")),
            ("html", Some("html")),
            ("css", Some("css")),
            ("rs", Some("rust")),
            ("toml", Some("toml")),
            ("TS", Some("typescript")),
            ("txt", None),
            ("", None),
        ];
        for (extension, expected) in table {
            assert_eq!(language_of(extension), expected, "{extension}");
        }
    }

    #[test]
    fn test_c_kind_12_files_over_512_kib_are_shown_without_highlighting() {
        assert_eq!(highlight_language_of("ts", 1024), Some("typescript"));
        assert_eq!(highlight_language_of("ts", HIGHLIGHT_LIMIT), Some("typescript"));
        assert_eq!(highlight_language_of("ts", HIGHLIGHT_LIMIT + 1), None);
        assert_eq!(highlight_language_of("ts", TEXT_LIMIT), None);
        assert_eq!(highlight_language_of("txt", 1024), None);
    }

    #[test]
    fn test_c_kind_11_the_text_limit_is_one_mebibyte() {
        assert_eq!(TEXT_LIMIT, 1_048_576);
    }

    #[test]
    fn test_extension_of_ignores_dots_in_folder_names_and_lowercases() {
        assert_eq!(extension_of("/p/v1.2/readme"), "");
        assert_eq!(extension_of("C:\\a.b\\File.TXT"), "txt");
        assert_eq!(extension_of("/p/.gitignore"), "");
        assert_eq!(extension_of("/p/archive.tar.gz"), "gz");
    }
}
