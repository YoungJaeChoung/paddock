//! 클립보드·끌어 놓기로 들어온 항목을 어떻게 받을지 정하는 규칙: 무엇이 들어왔는지(파일 목록 → 그림 → 글자만이면 없음), 어느 폴더에 받을지,
//! 같은 이름이 있을 때 어떤 이름으로 복사할지. 클립보드 타입은 이 모듈이 모르고 앱이 어댑터(`ClipboardPart`)로 바꿔 넘긴다.
//! 화면과 파일 시스템을 모르는 순수 계산이다(이름이 이미 있는지는 호출자가 `exists`로 알려 준다).
use std::collections::HashSet;

use crate::entry_name::copy_entry_name;
use crate::paste_paths::ImageKind;
use crate::paths::{child_path, file_name_of, is_inside, parent_of};

/// 클립보드의 한 부분. 앱이 gpui의 클립보드 항목을 이 모양으로 옮겨 넘긴다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardPart {
    Text,
    /// 탐색기에서 복사한 파일·폴더의 경로.
    Paths(Vec<String>),
    Image {
        kind: ImageKind,
        bytes: Vec<u8>,
    },
}

/// 받을 것.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    Paths(Vec<String>),
    Image { kind: ImageKind, bytes: Vec<u8> },
}

/// 클립보드에서 받을 것을 고른다: 파일 목록이 있으면 그것, 없으면 그림. 글자만 있거나 비어 있으면 None이다(호출자가 안내한다).
///
/// Examples
/// --------
/// | 클립보드                  | 결과             |
/// |---------------------------|------------------|
/// | 글자 + 파일 둘            | `Paths` 둘       |
/// | 글자 + 그림               | `Image`          |
/// | 파일 + 그림               | `Paths`          |
/// | 글자만·빈 파일 목록·없음  | None             |
pub fn incoming_of(parts: &[ClipboardPart]) -> Option<Incoming> {
    let paths = parts.iter().find_map(|part| match part {
        ClipboardPart::Paths(paths) if !paths.is_empty() => Some(Incoming::Paths(paths.clone())),
        _ => None,
    });
    paths.or_else(|| {
        parts.iter().find_map(|part| match part {
            ClipboardPart::Image { kind, bytes } if !bytes.is_empty() => {
                Some(Incoming::Image { kind: *kind, bytes: bytes.clone() })
            }
            _ => None,
        })
    })
}

/// 붙여넣기·끌어 놓기가 받을 폴더. 폴더 행이면 그 폴더, 파일 행이면 그 파일이 있는 폴더이고, 행이 없으면 머리줄 새 파일이 만드는 폴더(`creation_folder`)다.
///
/// Examples
/// --------
/// | 행                    | 머리줄 폴더 | 결과        |
/// |-----------------------|-------------|-------------|
/// | 폴더 `/p/src`         | `/p`        | `/p/src`    |
/// | 파일 `/p/src/a.js`    | `/p`        | `/p/src`    |
/// | 없음                  | `/p`        | `/p`        |
/// | 없음                  | 없음        | None        |
pub fn paste_destination(
    row: Option<(&str, bool)>,
    creation_folder: Option<&str>,
) -> Option<String> {
    match row {
        Some((path, true)) => Some(path.to_string()),
        Some((path, false)) => Some(parent_of(path)).filter(|parent| !parent.is_empty()),
        None => creation_folder.map(str::to_string),
    }
}

/// 받은 결과를 상태 줄에 알릴 문장. `folder`는 받은 폴더의 이름이다.
///
/// Examples
/// --------
/// | 복사 | 거부 | 이름 없음 | 실패 | 결과                                        |
/// |------|------|-----------|------|---------------------------------------------|
/// | 3    | 0    | 0         | 없음 | `Copied 3 items into src`                   |
/// | 1    | 0    | 0         | 없음 | `Copied 1 item into src`                    |
/// | 0    | 1    | 0         | 없음 | `A folder can\u{2019}t be copied into itself.` |
/// | 0    | 0    | 1         | 없음 | 드라이브 뿌리는 이름이 없다는 안내          |
/// | 2    | 1    | 0         | 없음 | `Copied 2 items into src`                   |
/// | 1    | 0    | 0         | `…`  | 실패 문장                                   |
pub fn copied_message(
    n_copied: usize,
    n_refused: usize,
    n_unnamed: usize,
    failure: Option<&str>,
    folder: &str,
) -> String {
    match failure {
        Some(failure) => failure.to_string(),
        None if n_copied == 0 && n_refused > 0 => "A folder can\u{2019}t be copied into itself.".to_string(),
        None if n_copied == 0 && n_unnamed > 0 => {
            "A drive root has no name to copy. Open the drive and copy the folders inside instead.".to_string()
        }
        None => format!("Copied {n_copied} item{} into {folder}", if n_copied == 1 { "" } else { "s" }),
    }
}

/// 복사 한 건.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyStep {
    /// `from`을 `to`로 복사한다(폴더면 안까지). `to`는 아직 없는 경로다.
    Copy { from: String, to: String },
    /// 폴더를 그 폴더 자신이나 그 안에 복사하려 해서 거부한다.
    Refused { from: String },
    /// 이름이 없는 항목(드라이브 뿌리 `C:\` 등)이라 복사할 이름을 정할 수 없다.
    Unnamed { from: String },
}

/// 원본들을 `destination` 폴더에 복사할 계획. 덮어쓰지 않는다: 같은 이름이 있으면(`exists`) 최상위 이름만 `이름 copy`·`이름 copy 2`로 바꾼다.
/// 같은 묶음 안에서 같은 이름이 둘 오면 둘째도 겹치지 않게 번호를 올린다. 폴더를 자기 자신이나 그 안에 복사하는 항목은 `Refused`다.
pub fn copy_plan(
    sources: &[String],
    destination: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<CopyStep> {
    let mut taken: HashSet<String> = HashSet::new();
    sources
        .iter()
        .map(|from| {
            let name = file_name_of(from);
            if name.is_empty() {
                CopyStep::Unnamed { from: from.clone() }
            } else if is_inside(from, destination) {
                CopyStep::Refused { from: from.clone() }
            } else {
                let to = (0..)
                    .map(|n| child_path(destination, &copy_entry_name(&name, n)))
                    .find(|candidate| !exists(candidate) && !taken.contains(&crate::paths::folder_identity(candidate)))
                    .unwrap_or_else(|| child_path(destination, &name));
                taken.insert(crate::paths::folder_identity(&to));
                CopyStep::Copy { from: from.clone(), to }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn test_c_incoming_1_files_come_before_images_and_text_alone_is_nothing() {
        let image = ClipboardPart::Image { kind: ImageKind::Png, bytes: vec![1, 2] };
        let files = ClipboardPart::Paths(paths(&["/a/x.txt", "/a/y.txt"]));
        assert_eq!(
            incoming_of(&[ClipboardPart::Text, files.clone(), image.clone()]),
            Some(Incoming::Paths(paths(&["/a/x.txt", "/a/y.txt"])))
        );
        assert_eq!(
            incoming_of(&[ClipboardPart::Text, image]),
            Some(Incoming::Image { kind: ImageKind::Png, bytes: vec![1, 2] })
        );
        assert_eq!(incoming_of(&[ClipboardPart::Text]), None);
        assert_eq!(incoming_of(&[]), None);
        assert_eq!(incoming_of(&[ClipboardPart::Paths(Vec::new())]), None);
        assert_eq!(incoming_of(&[ClipboardPart::Image { kind: ImageKind::Png, bytes: Vec::new() }]), None);
    }

    #[test]
    fn test_c_incoming_2_a_folder_row_receives_into_itself_a_file_row_into_its_folder() {
        assert_eq!(paste_destination(Some(("/p/src", true)), Some("/p")).as_deref(), Some("/p/src"));
        assert_eq!(paste_destination(Some(("/p/src/a.js", false)), Some("/p")).as_deref(), Some("/p/src"));
        assert_eq!(paste_destination(None, Some("/p")).as_deref(), Some("/p"));
        assert_eq!(paste_destination(None, None), None);
        assert_eq!(
            paste_destination(Some(("a.js", false)), Some("/p")),
            None,
            "a path without a folder has no destination"
        );
    }

    #[test]
    fn test_c_d69_copy_plan_never_overwrites_and_refuses_copying_a_folder_into_itself() {
        let taken: HashSet<&str> = ["/d/a.txt", "/d/a copy.txt"].into_iter().collect();
        let exists = |path: &str| taken.contains(path);
        let plan = copy_plan(&paths(&["/x/a.txt", "/x/b.txt", "/y/b.txt"]), "/d", exists);
        assert_eq!(
            plan,
            vec![
                CopyStep::Copy { from: "/x/a.txt".to_string(), to: "/d/a copy 2.txt".to_string() },
                CopyStep::Copy { from: "/x/b.txt".to_string(), to: "/d/b.txt".to_string() },
                CopyStep::Copy { from: "/y/b.txt".to_string(), to: "/d/b copy.txt".to_string() },
            ],
            "existing names and names planned earlier in the same batch are skipped"
        );
        // 폴더를 자기 자신이나 그 안에 복사하면 거부한다. 형제 폴더로는 복사한다.
        let refused = copy_plan(&paths(&["/p/src", "/p/lib"]), "/p/src/deep", |_| false);
        assert_eq!(refused[0], CopyStep::Refused { from: "/p/src".to_string() });
        assert_eq!(refused[1], CopyStep::Copy { from: "/p/lib".to_string(), to: "/p/src/deep/lib".to_string() });
        assert_eq!(
            copy_plan(&paths(&["/p/src"]), "/p/src", |_| false),
            vec![CopyStep::Refused { from: "/p/src".to_string() }]
        );
        // 같은 폴더에 다시 붙이면 `copy`가 붙는다.
        let same = copy_plan(&paths(&["/p/a.txt"]), "/p", |path| path == "/p/a.txt");
        assert_eq!(same, vec![CopyStep::Copy { from: "/p/a.txt".to_string(), to: "/p/a copy.txt".to_string() }]);
    }

    #[test]
    fn test_c_incoming_5_a_drive_root_has_no_name_to_copy_under() {
        let plan = copy_plan(&paths(&["C:\\", "/", "C:/x/a.txt"]), "D:/dest", |_| false);
        assert_eq!(plan[0], CopyStep::Unnamed { from: "C:\\".to_string() });
        assert_eq!(plan[1], CopyStep::Unnamed { from: "/".to_string() });
        assert_eq!(plan[2], CopyStep::Copy { from: "C:/x/a.txt".to_string(), to: "D:/dest/a.txt".to_string() });
    }

    #[test]
    fn test_c_incoming_3_a_windows_destination_compares_names_ignoring_case() {
        let plan = copy_plan(&paths(&["C:/x/A.txt", "C:/y/a.txt"]), "C:/d", |_| false);
        assert_eq!(
            plan,
            vec![
                CopyStep::Copy { from: "C:/x/A.txt".to_string(), to: "C:/d/A.txt".to_string() },
                CopyStep::Copy { from: "C:/y/a.txt".to_string(), to: "C:/d/a copy.txt".to_string() },
            ]
        );
    }

    #[test]
    fn test_c_incoming_4_the_status_line_counts_what_was_copied_and_names_a_refusal() {
        assert_eq!(copied_message(3, 0, 0, None, "src"), "Copied 3 items into src");
        assert_eq!(copied_message(1, 0, 0, None, "src"), "Copied 1 item into src");
        assert_eq!(copied_message(0, 1, 0, None, "src"), "A folder can\u{2019}t be copied into itself.");
        assert_eq!(copied_message(2, 1, 0, None, "src"), "Copied 2 items into src");
        assert!(copied_message(0, 0, 1, None, "src").starts_with("A drive root has no name"));
        assert_eq!(
            copied_message(1, 0, 0, Some("Could not copy \u{201c}a\u{201d}: denied"), "src"),
            "Could not copy \u{201c}a\u{201d}: denied"
        );
    }
}
