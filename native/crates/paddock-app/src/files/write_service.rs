//! 앱이 디스크에 쓰는 모든 일의 한 길: 명령(`WriteCommand`)을 받아 작업 스레드에서 실행하고 결과(`WriteResult`)만 창으로 돌려준다.
//!
//! 붙여넣기·끌어 놓기 복사, 클립보드 그림 저장, 이름 바꾸기, 열어 둔 파일 저장이 이 길을 쓰고, Replace in Files도 같은 길로 들어온다(명령 하나를 더하면 된다).
//! 명령마다 따로 스레드를 띄워 오래 걸리는 복사가 다음 일을 막지 않고, 어느 명령이 오류(패닉)로 죽어도 그 명령의 실패 결과가 되어 앱은 계속 돈다.
//! 창 스레드는 파일 시스템에 쓰지 않는다. 쓰는 규칙(덮어쓰지 않음·이름 번호)은 `ops`가 쥔다.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use paddock_core::incoming::copy_plan;
use paddock_core::paste_paths::ImageKind;

use super::open::FileStamp;
use super::ops::{
    CopyOutcome, ReplaceJob, ReplaceOutcome, SaveOutcome, rename_entry, replace_in_files, run_copy_plan, save_file,
    save_pasted_image, write_image,
};

/// 디스크에 쓰라는 명령.
#[derive(Clone, Debug)]
pub enum WriteCommand {
    /// `sources`(파일·폴더 경로)를 `destination` 폴더에 복사한다. 같은 이름은 `copy`로 받는다.
    Copy { sources: Vec<String>, destination: String },
    /// 클립보드 그림을 `folder`에 `image.<확장자>`로 쓴다.
    Image { folder: String, kind: ImageKind, bytes: Vec<u8> },
    /// 터미널에 붙여 넣을 클립보드 그림을 `folder`(`pasted-images`)에 쓴다. 결과는 그 터미널로 돌아온다.
    PastedImage { terminal: u32, folder: PathBuf, kind: ImageKind, bytes: Vec<u8>, unix_seconds: u64, random: String },
    /// `from` 항목의 이름을 같은 폴더 안에서 `new_name`으로 바꾼다. `watch`는 바꾼 뒤의 열린 파일 경로들이고, 바뀐 뒤 그 파일들의 쓴 시각·크기를 쓰기 스레드가 재어 결과에 싣는다.
    Rename { from: String, new_name: String, watch: Vec<String> },
    /// 열어 둔 파일의 편집 내용 `bytes`를 `path`에 쓴다. 디스크가 열었을 때(`expected`)와 달라졌으면 `force`가 아닌 한 쓰지 않는다.
    /// `text`는 쓴 글자로, 결과에 실려 돌아와 Markdown 미리보기를 저장본으로 바꾸는 데 쓴다.
    Save { path: String, bytes: Vec<u8>, text: String, expected: Option<FileStamp>, force: bool },
    /// 폴더 검색 결과의 파일들에서 같은 질의로 일치를 다시 찾아 `replacement`로 바꿔 쓴다(Replace in Files).
    Replace(ReplaceJob),
}

/// 명령의 결과. 어느 명령의 것인지 알도록 명령이 알려 준 값(받는 폴더·원래 경로·터미널)을 그대로 싣는다.
#[derive(Debug)]
pub enum WriteResult {
    Copied { destination: String, outcome: CopyOutcome },
    Image { folder: String, result: Result<String, String> },
    PastedImage { terminal: u32, result: Result<String, String> },
    Renamed { from: String, result: Result<String, String>, stamps: Vec<(String, Option<FileStamp>)> },
    Saved { path: String, text: String, outcome: SaveOutcome },
    Replaced { outcome: ReplaceOutcome },
}

/// 명령 하나를 실행해 결과로 만든다(스레드 위에서 부른다).
fn run(command: WriteCommand) -> WriteResult {
    match command {
        WriteCommand::Copy { sources, destination } => {
            let plan = copy_plan(&sources, &destination, |path| std::path::Path::new(path).exists());
            WriteResult::Copied { outcome: run_copy_plan(&plan), destination }
        }
        WriteCommand::Image { folder, kind, bytes } => {
            WriteResult::Image { result: write_image(&folder, kind, &bytes), folder }
        }
        WriteCommand::PastedImage { terminal, folder, kind, bytes, unix_seconds, random } => WriteResult::PastedImage {
            terminal,
            result: save_pasted_image(&folder, kind, &bytes, unix_seconds, &random),
        },
        WriteCommand::Rename { from, new_name, watch } => {
            let result = rename_entry(&from, &new_name);
            let stamps = if result.is_ok() {
                watch.into_iter().map(|path| (path.clone(), crate::files::open::file_stamp(&path))).collect()
            } else {
                Vec::new()
            };
            WriteResult::Renamed { result, from, stamps }
        }
        WriteCommand::Save { path, bytes, text, expected, force } => {
            WriteResult::Saved { outcome: save_file(&path, &bytes, expected, force), path, text }
        }
        WriteCommand::Replace(job) => WriteResult::Replaced { outcome: replace_in_files(&job) },
    }
}

/// 명령이 오류로 죽었을 때 그 명령의 실패 결과. 안내 문장은 일마다 다르다.
fn failure_of(command: &WriteCommand) -> WriteResult {
    let stopped = "the writer stopped unexpectedly".to_string();
    match command {
        WriteCommand::Copy { destination, .. } => WriteResult::Copied {
            destination: destination.clone(),
            outcome: CopyOutcome { failure: Some(format!("Could not copy: {stopped}.")), ..CopyOutcome::default() },
        },
        WriteCommand::Image { folder, .. } => {
            WriteResult::Image { folder: folder.clone(), result: Err(format!("Could not save the image: {stopped}.")) }
        }
        WriteCommand::PastedImage { terminal, .. } => WriteResult::PastedImage {
            terminal: *terminal,
            result: Err(format!("Could not save the image: {stopped}.")),
        },
        WriteCommand::Rename { from, .. } => WriteResult::Renamed {
            from: from.clone(),
            result: Err(format!("Could not rename: {stopped}.")),
            stamps: Vec::new(),
        },
        WriteCommand::Replace(job) => {
            WriteResult::Replaced { outcome: ReplaceOutcome { n_failed: job.files.len(), ..ReplaceOutcome::default() } }
        }
        WriteCommand::Save { path, text, .. } => WriteResult::Saved {
            path: path.clone(),
            text: text.clone(),
            outcome: SaveOutcome::Failed(format!("{stopped}.")),
        },
    }
}

/// 쓰기 명령을 받고 결과를 돌려주는 창구. 창이 하나 쥔다.
pub struct WriteService {
    results_in: Sender<WriteResult>,
    results: Receiver<WriteResult>,
}

impl WriteService {
    pub fn new() -> Self {
        let (results_in, results) = channel();
        Self { results_in, results }
    }

    /// 명령을 작업 스레드에 맡긴다. 기다리지 않고 돌아온다.
    pub fn submit(
        &self,
        command: WriteCommand,
    ) {
        let results = self.results_in.clone();
        let spawned = std::thread::Builder::new().name("paddock-write".to_string()).spawn({
            let command = command.clone();
            move || {
                let result = catch_unwind(AssertUnwindSafe(|| run(command.clone()))).unwrap_or_else(|_| {
                    crate::app_log::event("files: a write command stopped unexpectedly");
                    failure_of(&command)
                });
                let _ = results.send(result);
            }
        });
        if spawned.is_err() {
            // 스레드를 만들지 못하면 그 명령만 실패로 돌려준다.
            let _ = self.results_in.send(failure_of(&command));
        }
    }

    /// 도착한 결과 하나. 아직 없으면 None이다.
    pub fn try_recv(&self) -> Option<WriteResult> {
        self.results.try_recv().ok()
    }
}

impl Default for WriteService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn wait_one(service: &WriteService) -> WriteResult {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = service.try_recv() {
                break result;
            }
            assert!(Instant::now() < deadline, "no result within ten seconds");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn test_c_d65_every_disk_write_goes_through_one_command_path_and_comes_back_as_a_result() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let service = WriteService::new();
        service.submit(WriteCommand::Copy { sources: vec![format!("{folder}/a.txt")], destination: folder.clone() });
        match wait_one(&service) {
            WriteResult::Copied { destination, outcome } => {
                assert_eq!(destination, folder);
                assert_eq!(outcome.copied.len(), 1);
                assert!(outcome.copied[0].ends_with("a copy.txt"), "{:?}", outcome.copied);
            }
            other => panic!("unexpected {other:?}"),
        }
        service.submit(WriteCommand::Rename {
            from: format!("{folder}/a.txt"),
            new_name: "b.txt".to_string(),
            watch: vec![format!("{folder}/b.txt")],
        });
        match wait_one(&service) {
            WriteResult::Renamed { from, result, stamps } => {
                assert_eq!((from, result.is_ok()), (format!("{folder}/a.txt"), true));
                assert!(stamps.len() == 1 && stamps[0].1.is_some(), "the renamed file was stat'ed by the writer");
            }
            other => panic!("unexpected {other:?}"),
        }
        service.submit(WriteCommand::Image { folder: folder.clone(), kind: ImageKind::Png, bytes: b"png".to_vec() });
        match wait_one(&service) {
            WriteResult::Image { result, .. } => assert!(result.unwrap().ends_with("image.png")),
            other => panic!("unexpected {other:?}"),
        }
        let saved = format!("{folder}/b.txt");
        service.submit(WriteCommand::Save {
            path: saved.clone(),
            bytes: b"\xef\xbb\xbfhi\r\n".to_vec(),
            text: "hi\r\n".to_string(),
            expected: None,
            force: false,
        });
        match wait_one(&service) {
            WriteResult::Saved { path, text, outcome } => {
                assert_eq!((path.as_str(), text.as_str()), (saved.as_str(), "hi\r\n"));
                assert!(matches!(outcome, SaveOutcome::Written(_)));
                assert_eq!(std::fs::read(&saved).unwrap(), b"\xef\xbb\xbfhi\r\n");
            }
            other => panic!("unexpected {other:?}"),
        }
        service.submit(WriteCommand::PastedImage {
            terminal: 7,
            folder: dir.path().join("pasted"),
            kind: ImageKind::Png,
            bytes: b"png".to_vec(),
            unix_seconds: 1,
            random: "r".to_string(),
        });
        match wait_one(&service) {
            WriteResult::PastedImage { terminal, result } => assert_eq!((terminal, result.is_ok()), (7, true)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn test_c_d65_a_failing_command_comes_back_as_its_own_failure_and_the_next_command_still_runs() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let service = WriteService::new();
        service.submit(WriteCommand::Rename {
            from: format!("{folder}/missing.txt"),
            new_name: "x.txt".to_string(),
            watch: Vec::new(),
        });
        match wait_one(&service) {
            WriteResult::Renamed { result, .. } => assert!(result.unwrap_err().starts_with("Could not rename")),
            other => panic!("unexpected {other:?}"),
        }
        // 오류로 죽은 명령의 실패 결과는 그 명령의 종류에 맞는 문장이다.
        let failure = failure_of(&WriteCommand::Copy { sources: Vec::new(), destination: folder.clone() });
        assert!(
            matches!(failure, WriteResult::Copied { outcome, .. } if outcome.failure.as_ref().is_some_and(|text| text.contains("stopped unexpectedly")))
        );
        service.submit(WriteCommand::Image { folder, kind: ImageKind::Png, bytes: b"x".to_vec() });
        assert!(matches!(wait_one(&service), WriteResult::Image { result: Ok(_), .. }));
    }
}
