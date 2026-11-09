//! 클립보드 항목(gpui)을 core가 아는 모양(`ClipboardPart`)으로 옮기는 어댑터. gpui 타입은 이 파일 밖으로 나가지 않는다.
use gpui_kit::{ClipboardEntry, ClipboardItem, ImageFormat};
use paddock_core::incoming::ClipboardPart;
use paddock_core::paste_paths::ImageKind;

/// gpui 그림 형식을 core의 형식으로.
fn kind_of(format: ImageFormat) -> ImageKind {
    match format {
        ImageFormat::Png => ImageKind::Png,
        ImageFormat::Jpeg => ImageKind::Jpeg,
        ImageFormat::Gif => ImageKind::Gif,
        ImageFormat::Webp => ImageKind::Webp,
        ImageFormat::Svg => ImageKind::Svg,
        ImageFormat::Bmp => ImageKind::Bmp,
        _ => ImageKind::Other,
    }
}

/// 클립보드 항목의 부분들(글자·파일 목록·그림).
pub fn parts_of(item: &ClipboardItem) -> Vec<ClipboardPart> {
    item.entries()
        .iter()
        .map(|entry| match entry {
            ClipboardEntry::String(_) => ClipboardPart::Text,
            ClipboardEntry::ExternalPaths(paths) => {
                ClipboardPart::Paths(paths.paths().iter().map(|path| path.to_string_lossy().to_string()).collect())
            }
            ClipboardEntry::Image(image) => {
                ClipboardPart::Image { kind: kind_of(image.format), bytes: image.bytes.clone() }
            }
        })
        .collect()
}

/// 클립보드의 글자 부분. 글자 항목이 없거나 비어 있으면 None이다(파일 목록의 경로를 글자로 합쳐 주지 않는다).
pub fn text_of(item: &ClipboardItem) -> Option<String> {
    let text: String = item
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::String(string) => Some(string.text.clone()),
            _ => None,
        })
        .collect();
    Some(text).filter(|text| !text.is_empty())
}
