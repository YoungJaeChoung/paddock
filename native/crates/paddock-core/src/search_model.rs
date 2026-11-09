//! 폴더 검색의 규칙과 결과 모델: 질의 옵션, 결과 상한, 미리보기 줄 자르기, 파일별 묶기, 제외 이름.
//! 파일을 읽는 일(훑기·일치 찾기)은 앱의 검색 엔진이 하고, 이 모듈은 그 결과를 받아 화면에 보일 모양으로 만든다.
use std::collections::HashSet;
use std::ops::Range;

use grep_matcher::{Captures as _, Matcher as _};
use grep_regex::{RegexCaptures, RegexMatcher, RegexMatcherBuilder};

/// 한 번의 검색이 모으는 일치 줄의 상한. 넘으면 더 모으지 않고 상한 안내를 보인다.
pub const MAX_RESULTS: usize = 2000;

/// 미리보기 줄에서 일치 앞뒤로 남기는 글자 수.
pub const PREVIEW_CONTEXT_CHARS: usize = 40;

/// 미리보기에 남기는 일치 글자의 최대 길이. 줄 전체와 맞는 정규식 같은 긴 일치가 결과 하나에 큰 글자를 싣지 않게 한다.
pub const PREVIEW_MATCH_CHARS: usize = 200;

/// 검색에서 읽는 파일 크기 상한(바이트). 더 큰 파일(로그 덩어리 등)은 건너뛴다.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// 결과가 상한에 닿았을 때 결과 아래에 보이는 안내.
pub const LIMIT_NOTICE: &str =
    "This is only a subset of all results. Use a more specific search term to narrow down the result list.";

/// 검색할 폴더가 없을 때(홈 폴더·파일 시스템 루트의 터미널) 입력칸 자리에 보이는 안내.
pub const NO_FOLDER_NOTICE: &str = "No folder to search.";

/// 검색 작업이 끝났다는 소식 없이 멈췄을 때(작업 스레드의 오류) 결과 개수 자리에 보이는 안내.
pub const STOPPED_NOTICE: &str = "Search stopped unexpectedly.";

/// 질의를 해석하는 방식 세 가지(입력칸 오른쪽 단추).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// 대소문자를 구별한다.
    pub case_sensitive: bool,
    /// 단어 전체가 맞는 것만 찾는다.
    pub whole_word: bool,
    /// 질의를 정규식으로 읽는다. 꺼져 있으면 글자 그대로 찾는다.
    pub regex: bool,
}

/// 정규식에서 기호로 읽히는 글자(글자 그대로 찾을 때 앞에 `\`를 붙인다).
const REGEX_SYNTAX: &str = "\\.+*?()|[]{}^$#&-~";

/// 검색 엔진에 줄 정규식 글. 정규식 옵션이 꺼져 있으면 질의의 기호를 글자 그대로 뜻하게 막는다.
///
/// Examples
/// --------
/// | 질의      | 정규식 옵션 | 결과       |
/// |-----------|-------------|------------|
/// | `a.b(c)`  | 꺼짐        | `a\.b\(c\)` |
/// | `a.b(c)`  | 켜짐        | `a.b(c)`   |
/// | `한글`    | 꺼짐        | `한글`     |
pub fn pattern_of(
    query: &str,
    options: SearchOptions,
) -> String {
    if options.regex {
        query.to_string()
    } else {
        let mut escaped = String::with_capacity(query.len());
        for c in query.chars() {
            if REGEX_SYNTAX.contains(c) {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        escaped
    }
}

/// 질의와 옵션으로 일치 규칙을 만든다. 폴더 검색과 바꾸기가 같은 규칙으로 일치를 찾도록 둘 다 이 함수를 쓴다.
/// 해석하지 못하는 질의(정규식 오류 등)는 사용자에게 보일 문장을 돌려준다.
pub fn build_matcher(
    query: &str,
    options: SearchOptions,
) -> Result<RegexMatcher, String> {
    RegexMatcherBuilder::new()
        .case_insensitive(!options.case_sensitive)
        .word(options.whole_word)
        .build(&pattern_of(query, options))
        .map_err(|error| format!("Invalid search pattern: {error}"))
}

/// 바꿀 자리 하나: 줄이 아니라 파일 글 전체 기준 바이트 범위와 그 자리에 들어갈 글.
pub type Edit = (Range<usize>, String);

/// `replacement`에서 `$1`·`${1}`·`${이름}`·`$$`를 그 일치의 묶음 글로 펼친다(정규식 옵션이 켜졌을 때만). 없는 묶음은 빈 글자다.
fn expand_replacement(
    replacement: &str,
    matcher: &RegexMatcher,
    captures: &RegexCaptures,
    line: &str,
) -> String {
    let group = |index: usize| captures.get(index).map(|found| &line[found.start()..found.end()]).unwrap_or("");
    let mut out = String::with_capacity(replacement.len());
    let chars: Vec<char> = replacement.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] != '$' {
            out.push(chars[at]);
            at += 1;
        } else if chars.get(at + 1) == Some(&'$') {
            out.push('$');
            at += 2;
        } else if chars.get(at + 1) == Some(&'{')
            && let Some(close) = chars[at + 2..].iter().position(|c| *c == '}')
        {
            let name: String = chars[at + 2..at + 2 + close].iter().collect();
            let index = name.parse::<usize>().ok().or_else(|| matcher.capture_index(&name));
            out.push_str(index.map_or("", group));
            at += close + 3;
        } else {
            let digits: String = chars[at + 1..].iter().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                out.push('$');
                at += 1;
            } else {
                out.push_str(group(digits.parse().unwrap_or(usize::MAX)));
                at += 1 + digits.len();
            }
        }
    }
    out
}

/// 파일 글(`text`)에서 `matcher`가 찾는 모든 일치를 `replacement`로 바꾸는 계획을 만든다. 폴더 검색처럼 줄마다(줄 끝 `\n` 앞까지) 찾고, 빈 일치는 건너뛰며,
/// 겹치지 않는 순서대로 돌려준다. `regex`(정규식 옵션)가 켜져 있으면 `replacement`의 `$1`·`${이름}`을 묶음 글로 펼치고 꺼져 있으면 글자 그대로 쓴다.
///
/// Examples
/// --------
/// | text            | 질의(정규식) | replacement | 결과                           |
/// |-----------------|--------------|-------------|--------------------------------|
/// | `a needle\nneedle` | `needle`  | `thread`    | 두 곳 → `a thread\nthread`     |
/// | `a-1 b-2`       | `(\\w)-(\\d)` (켬) | `$2$1`      | `1a 2b`                        |
/// | `a-1`           | `(\\w)-(\\d)` (끔) | `$2$1`      | 일치 없음(글자 그대로 찾는다)  |
pub fn replace_plan(
    text: &str,
    matcher: &RegexMatcher,
    replacement: &str,
    regex: bool,
) -> Vec<Edit> {
    let mut edits: Vec<Edit> = Vec::new();
    let mut base = 0;
    let mut captures = matcher.new_captures().ok();
    for raw in text.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        if let Some(captures) = captures.as_mut() {
            let _ = matcher.captures_iter(line.as_bytes(), captures, |found| {
                if let Some(whole) = found.get(0).filter(|whole| whole.end() > whole.start()) {
                    let new_text = if regex {
                        expand_replacement(replacement, matcher, found, line)
                    } else {
                        replacement.to_string()
                    };
                    edits.push((base + whole.start()..base + whole.end(), new_text));
                }
                true
            });
        }
        base += raw.len();
    }
    edits
}

/// `replace_plan`이 만든 바꿀 자리들을 글에 적용한다. 자리는 겹치지 않고 앞에서부터 있어야 한다.
///
/// Examples
/// --------
/// | text     | edits                  | 결과     |
/// |----------|------------------------|----------|
/// | `abcabc` | (0..1, `X`), (3..4, `Y`) | `XbcYbc` |
pub fn apply_edits(
    text: &str,
    edits: &[Edit],
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, new_text) in edits {
        out.push_str(&text[at..range.start]);
        out.push_str(new_text);
        at = range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// 바꾼 뒤 알리는 문장.
///
/// Examples
/// --------
/// | n_matches | n_files | 결과                              |
/// |-----------|---------|-----------------------------------|
/// | 12        | 3       | `Replaced 12 matches in 3 files`  |
/// | 1         | 1       | `Replaced 1 match in 1 file`      |
pub fn replace_summary(
    n_files: usize,
    n_matches: usize,
) -> String {
    let matches = if n_matches == 1 { "match" } else { "matches" };
    let files = if n_files == 1 { "file" } else { "files" };
    format!("Replaced {n_matches} {matches} in {n_files} {files}")
}

/// 바꾸기를 마친 뒤 상태 줄에 알릴 문장: 요약, 이어서 저장하지 않아 건너뛴 파일마다 안내, 바꾸지 못한 파일 수.
///
/// Examples
/// --------
/// | 바꾼 파일·일치 | 건너뛴 파일          | 못 바꾼 파일 | 결과                                                            |
/// |----------------|----------------------|--------------|-----------------------------------------------------------------|
/// | 3·12           | (없음)               | 0            | `Replaced 12 matches in 3 files`                                |
/// | 1·2            | `a.js`               | 0            | `Replaced 2 matches in 1 file. a.js has unsaved changes. Save it first.` |
/// | 0·0            | `a.js`               | 1            | `a.js has unsaved changes. Save it first. 1 file could not be replaced.` |
pub fn replace_report(
    n_files: usize,
    n_matches: usize,
    unsaved_names: &[String],
    n_failed: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if n_files > 0 {
        parts.push(format!("{}.", replace_summary(n_files, n_matches)));
    }
    parts.extend(unsaved_names.iter().map(|name| unsaved_skip_text(name)));
    if n_failed > 0 {
        let files = if n_failed == 1 { "file" } else { "files" };
        parts.push(format!("{n_failed} {files} could not be replaced."));
    }
    if parts.is_empty() {
        parts.push("Nothing to replace.".to_string());
    }
    let joined = parts.join(" ");
    // 맨 끝의 마침표는 요약 하나만 있을 때 빼서 `replace_summary`와 같은 모양으로 둔다.
    if parts.len() == 1 { joined.trim_end_matches('.').to_string() } else { joined }
}

/// 저장하지 않은 변경이 있는 열린 탭의 파일을 건너뛸 때 알리는 문장.
pub fn unsaved_skip_text(name: &str) -> String {
    format!("{name} has unsaved changes. Save it first.")
}

/// 모두 바꾸기 확인 창의 제목 문장(`Replace N matches in M files?`). 개수는 검색 결과의 일치 줄 수와 파일 수다.
pub fn replace_all_prompt(
    n_matches: usize,
    n_files: usize,
) -> String {
    let matches = if n_matches == 1 { "match" } else { "matches" };
    let files = if n_files == 1 { "file" } else { "files" };
    format!("Replace {n_matches} {matches} in {n_files} {files}?")
}

/// 결과 한 줄의 미리보기: 보여 줄 글자와 그 안에서 일치한 부분(바이트 범위).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub highlight: Range<usize>,
}

/// 미리보기에서 잘린 쪽 끝에 붙이는 표지.
const ELLIPSIS: char = '\u{2026}';

/// 제어 문자(탭·줄 끝 등)를 공백으로 바꾼 글. 한 글자가 한 글자로 바뀌어 글자 수는 같다.
fn printable(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// 일치한 줄(`line`, 줄 끝 문자 없음)에서 `matched`(바이트 범위) 앞뒤 `PREVIEW_CONTEXT_CHARS`글자만 남긴 미리보기를 만든다.
/// 줄 앞의 들여쓰기는 뗀다. 잘린 쪽 끝에는 `…`를 붙이고, 제어 문자는 공백으로 바꾼다. 글자 경계를 지켜 자른다.
///
/// Examples
/// --------
/// | 줄                      | 일치   | 미리보기 글자                         |
/// |-------------------------|--------|---------------------------------------|
/// | `    return needle;`    | needle | `return needle;`                      |
/// | 앞 50글자 + needle + 뒤 50글자 | needle | `…` + 앞 40글자 + needle + 뒤 40글자 + `…` |
pub fn preview_of(
    line: &str,
    matched: Range<usize>,
) -> Preview {
    let start = matched.start.min(line.len());
    let end = matched.end.clamp(start, line.len());
    // 줄 앞의 들여쓰기는 일치 앞에 있을 때만 뗀다.
    let indent = line.len() - line.trim_start().len();
    let before = &line[indent.min(start)..start];
    let (before, cut_before) = match before.char_indices().rev().nth(PREVIEW_CONTEXT_CHARS - 1) {
        Some((offset, _)) if before.chars().count() > PREVIEW_CONTEXT_CHARS => (&before[offset..], true),
        _ => (before, false),
    };
    let found = &line[start..end];
    let (found, cut_match) = match found.char_indices().nth(PREVIEW_MATCH_CHARS) {
        Some((offset, _)) => (&found[..offset], true),
        None => (found, false),
    };
    // 일치를 줄여 실었으면 뒤 글자는 싣지 않는다(일치 뒤가 이미 잘렸다).
    let after = if cut_match { "" } else { &line[end..] };
    let (after, cut_after) = match after.char_indices().nth(PREVIEW_CONTEXT_CHARS) {
        Some((offset, _)) => (&after[..offset], true),
        None => (after, false),
    };
    let mut text = String::new();
    if cut_before {
        text.push(ELLIPSIS);
    }
    text.push_str(&printable(before));
    let highlight_start = text.len();
    text.push_str(&printable(found));
    let highlight = highlight_start..text.len();
    text.push_str(printable(after).trim_end());
    if cut_after || cut_match {
        text.push(ELLIPSIS);
    }
    Preview { text, highlight }
}

/// 일치한 줄 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// 줄 번호(1부터).
    pub line: u64,
    pub preview: Preview,
}

/// 일치가 있는 파일 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileResult {
    /// 파일의 전체 경로.
    pub path: String,
    /// 검색 폴더 기준 상대 경로(구분자는 `/`).
    pub relative: String,
    pub matches: Vec<LineMatch>,
}

/// 한 번의 검색이 모은 결과. 일치가 `MAX_RESULTS`를 넘으면 넘는 것은 담지 않고 `capped`를 켠다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Results {
    files: Vec<FileResult>,
    n_matches: usize,
    capped: bool,
}

impl Results {
    /// 파일 하나의 일치를 더한다. 상한을 넘는 일치는 버리고 `capped`를 켠다. 하나도 담지 못한 파일은 더하지 않는다.
    pub fn add_file(
        &mut self,
        path: String,
        relative: String,
        matches: Vec<LineMatch>,
    ) {
        let mut matches = matches;
        let n_room = MAX_RESULTS - self.n_matches;
        if matches.len() > n_room {
            matches.truncate(n_room);
            self.capped = true;
        }
        if !matches.is_empty() {
            self.n_matches += matches.len();
            self.files.push(FileResult { path, relative, matches });
        }
    }

    pub fn files(&self) -> &[FileResult] {
        &self.files
    }

    /// 담은 일치 줄 수.
    pub fn n_matches(&self) -> usize {
        self.n_matches
    }

    /// 일치가 상한을 넘어 더 모으지 않았다.
    pub fn capped(&self) -> bool {
        self.capped
    }

    /// 전체 일치 줄 중 `n`번째(0부터)와 그 파일. 결과 순서는 파일 순서 안의 줄 순서다.
    pub fn match_at(
        &self,
        n: usize,
    ) -> Option<(&FileResult, &LineMatch)> {
        self.files.iter().flat_map(|file| file.matches.iter().map(move |found| (file, found))).nth(n)
    }
}

/// 결과 트리의 한 행.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultRow {
    /// 파일 행: 이름, 검색 폴더 기준 폴더 경로(없으면 빈 글), 일치 수, 접힘.
    File { file: usize, name: String, dir: String, n_matches: usize, collapsed: bool },
    /// 일치 줄 행.
    Match { file: usize, index: usize, line: u64, preview: Preview },
}

/// 결과를 파일 행과 그 아래 일치 줄 행으로 펼친다. `collapsed`에 든 파일은 일치 줄 행을 내지 않는다.
///
/// Examples
/// --------
/// | 결과                    | 접힌 파일 | 행                                          |
/// |-------------------------|-----------|---------------------------------------------|
/// | a.js(2줄), b/c.js(1줄)  | (없음)    | File a, Match, Match, File c, Match         |
/// | 같음                    | a         | File a(접힘), File c, Match                 |
pub fn rows(
    results: &Results,
    collapsed: &HashSet<usize>,
) -> Vec<ResultRow> {
    let mut rows = Vec::new();
    for (file, result) in results.files().iter().enumerate() {
        let (dir, name) = result.relative.rsplit_once('/').unwrap_or(("", result.relative.as_str()));
        let is_collapsed = collapsed.contains(&file);
        rows.push(ResultRow::File {
            file,
            name: name.to_string(),
            dir: dir.to_string(),
            n_matches: result.matches.len(),
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            rows.extend(result.matches.iter().enumerate().map(|(index, found)| ResultRow::Match {
                file,
                index,
                line: found.line,
                preview: found.preview.clone(),
            }));
        }
    }
    rows
}

/// 결과 개수 글.
///
/// Examples
/// --------
/// | 일치 수 | 파일 수 | 결과                   |
/// |---------|---------|------------------------|
/// | 0       | 0       | `No results found.`    |
/// | 1       | 1       | `1 result in 1 file`   |
/// | 7       | 5       | `7 results in 5 files` |
pub fn count_text(
    n_matches: usize,
    n_files: usize,
) -> String {
    if n_matches == 0 {
        "No results found.".to_string()
    } else {
        let results = if n_matches == 1 { "result" } else { "results" };
        let files = if n_files == 1 { "file" } else { "files" };
        format!("{n_matches} {results} in {n_files} {files}")
    }
}

/// 검색 범위 안내 줄: `Searching in <폴더 이름>`.
pub fn scope_text(folder_name: &str) -> String {
    format!("Searching in {folder_name}")
}

/// 검색이 들어가지 않는 이름인지. `files.exclude`에서 켜진 이름(파일과 폴더 모두)이면 훑지 않는다.
/// `.gitignore`는 이 규칙이 아니라 검색 엔진이 폴더마다 읽어 따로 지킨다.
pub fn is_excluded_name(
    name: &str,
    hidden: &HashSet<String>,
) -> bool {
    hidden.contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(
        line: &str,
        word: &str,
    ) -> Preview {
        let start = line.find(word).expect("the word is in the line");
        preview_of(line, start..start + word.len())
    }

    fn line_match(
        line: u64,
        word: &str,
    ) -> LineMatch {
        LineMatch { line, preview: matched(word, word) }
    }

    #[test]
    fn test_c_search_1_the_pattern_is_literal_unless_the_regex_option_is_on() {
        let off = SearchOptions::default();
        let on = SearchOptions { regex: true, ..SearchOptions::default() };
        assert_eq!(pattern_of("a.b(c)", off), "a\\.b\\(c\\)");
        assert_eq!(pattern_of("a.b(c)", on), "a.b(c)");
        assert_eq!(pattern_of("\u{d55c}\u{ae00}", off), "\u{d55c}\u{ae00}");
        assert_eq!(pattern_of("[x]+*?|^$\\{}", off), "\\[x\\]\\+\\*\\?\\|\\^\\$\\\\\\{\\}");
    }

    #[test]
    fn test_c_search_2_the_preview_keeps_forty_characters_around_the_match_on_character_boundaries() {
        let plain = matched("    return needle;", "needle");
        assert_eq!(plain.text, "return needle;");
        assert_eq!(&plain.text[plain.highlight.clone()], "needle");
        let long = format!("{}needle{}", "a".repeat(50), "b".repeat(50));
        let cut = matched(&long, "needle");
        assert_eq!(cut.text, format!("\u{2026}{}needle{}\u{2026}", "a".repeat(40), "b".repeat(40)));
        assert_eq!(&cut.text[cut.highlight.clone()], "needle");
        // 한글은 한 글자가 3바이트이므로 글자 수로 자른다.
        let korean = format!("{}needle{}", "\u{d55c}".repeat(45), "\u{ae00}".repeat(45));
        let cut = matched(&korean, "needle");
        assert_eq!(cut.text.chars().filter(|c| *c == '\u{d55c}').count(), 40);
        assert_eq!(cut.text.chars().filter(|c| *c == '\u{ae00}').count(), 40);
        assert_eq!(&cut.text[cut.highlight.clone()], "needle");
        // 제어 문자는 공백이 되고 일치 범위는 어긋나지 않는다.
        let tabbed = matched("a\tb\u{85}needle", "needle");
        assert_eq!(tabbed.text, "a b needle");
        assert_eq!(&tabbed.text[tabbed.highlight.clone()], "needle");
        // 너무 긴 일치는 줄여서 싣는다.
        let wide = "x".repeat(500);
        let cut = preview_of(&wide, 0..500);
        assert!(cut.text.chars().count() <= PREVIEW_MATCH_CHARS + 2);
    }

    #[test]
    fn test_c_search_3_matches_past_the_limit_are_dropped_and_only_then_the_result_is_capped() {
        let mut results = Results::default();
        let first: Vec<LineMatch> = (1..=1500).map(|n| line_match(n, "x")).collect();
        results.add_file("/p/a".into(), "a".into(), first);
        assert_eq!((results.n_matches(), results.capped()), (1500, false));
        let second: Vec<LineMatch> = (1..=500).map(|n| line_match(n, "x")).collect();
        results.add_file("/p/b".into(), "b".into(), second);
        assert_eq!((results.n_matches(), results.capped()), (MAX_RESULTS, false), "exactly the limit is not capped");
        results.add_file("/p/c".into(), "c".into(), vec![line_match(1, "x")]);
        assert_eq!((results.n_matches(), results.capped(), results.files().len()), (MAX_RESULTS, true, 2));
        let mut partial = Results::default();
        partial.add_file("/p/a".into(), "a".into(), (1..=2001).map(|n| line_match(n, "x")).collect());
        assert_eq!((partial.n_matches(), partial.capped()), (MAX_RESULTS, true));
        assert_eq!(partial.files()[0].matches.len(), MAX_RESULTS);
    }

    #[test]
    fn test_c_search_4_rows_group_matches_under_their_file_and_collapsed_files_show_no_lines() {
        let mut results = Results::default();
        results.add_file("/p/a.js".into(), "a.js".into(), vec![line_match(3, "x"), line_match(9, "y")]);
        results.add_file("/p/b/c.js".into(), "b/c.js".into(), vec![line_match(12, "z")]);
        let shape = |rows: &[ResultRow]| -> Vec<String> {
            rows.iter()
                .map(|row| match row {
                    ResultRow::File { name, dir, n_matches, collapsed, .. } => {
                        format!("file {name} [{dir}] {n_matches}{}", if *collapsed { " collapsed" } else { "" })
                    }
                    ResultRow::Match { line, preview, .. } => format!("match {line} {}", preview.text),
                })
                .collect()
        };
        assert_eq!(
            shape(&rows(&results, &HashSet::new())),
            ["file a.js [] 2", "match 3 x", "match 9 y", "file c.js [b] 1", "match 12 z"]
        );
        let collapsed: HashSet<usize> = [0].into_iter().collect();
        assert_eq!(shape(&rows(&results, &collapsed)), ["file a.js [] 2 collapsed", "file c.js [b] 1", "match 12 z"]);
        let (file, found) = results.match_at(2).expect("the third match");
        assert_eq!((file.path.as_str(), found.line), ("/p/b/c.js", 12));
        assert!(results.match_at(3).is_none());
    }

    #[test]
    fn test_c_search_5_the_count_text_and_notices_are_the_fixed_sentences() {
        assert_eq!(count_text(0, 0), "No results found.");
        assert_eq!(count_text(1, 1), "1 result in 1 file");
        assert_eq!(count_text(7, 5), "7 results in 5 files");
        assert_eq!(count_text(2, 1), "2 results in 1 file");
        assert_eq!(scope_text("shop-api"), "Searching in shop-api");
        assert!(LIMIT_NOTICE.starts_with("This is only a subset of all results."));
    }

    #[test]
    fn test_c_search_6_a_name_turned_on_in_files_exclude_is_never_entered() {
        let hidden: HashSet<String> = [".git", "node_modules"].iter().map(|name| name.to_string()).collect();
        assert!(is_excluded_name(".git", &hidden));
        assert!(is_excluded_name("node_modules", &hidden));
        assert!(!is_excluded_name("src", &hidden));
        assert!(!is_excluded_name("Node_Modules", &hidden), "names compare exactly");
        assert!(!is_excluded_name(".gitignore", &hidden));
    }

    fn plan(
        text: &str,
        query: &str,
        options: SearchOptions,
        replacement: &str,
    ) -> String {
        let matcher = build_matcher(query, options).unwrap();
        apply_edits(text, &replace_plan(text, &matcher, replacement, options.regex))
    }

    #[test]
    fn test_c_replace_1_every_match_on_every_line_is_replaced_in_order() {
        let off = SearchOptions::default();
        assert_eq!(plan("a needle\nNeedle needle\nnone", "needle", off, "thread"), "a thread\nthread thread\nnone");
        let case = SearchOptions { case_sensitive: true, ..off };
        assert_eq!(plan("a needle\nNeedle", "needle", case, "x"), "a x\nNeedle");
        let word = SearchOptions { whole_word: true, ..off };
        assert_eq!(plan("needle needles", "needle", word, "x"), "x needles");
    }

    #[test]
    fn test_c_replace_2_groups_expand_only_when_the_regex_option_is_on() {
        let regex = SearchOptions { regex: true, ..SearchOptions::default() };
        assert_eq!(plan("a-1 b-2", "(\\w)-(\\d)", regex, "$2$1"), "1a 2b");
        assert_eq!(plan("a-1", "(?P<x>\\w)-(\\d)", regex, "${x}+${2}$$"), "a+1$");
        assert_eq!(plan("a-1", "(\\w)-", regex, "[$9]"), "[]1");
        // 정규식이 꺼지면 질의도 글자 그대로이고 바꿀 글의 `$1`도 그대로다.
        let off = SearchOptions::default();
        assert_eq!(plan("a.b a-b", "a.b", off, "$1"), "$1 a-b");
    }

    #[test]
    fn test_c_replace_3_empty_matches_crlf_and_multibyte_text_keep_their_byte_ranges() {
        let regex = SearchOptions { regex: true, ..SearchOptions::default() };
        assert_eq!(plan("abc", "x*", regex, "-"), "abc", "empty matches are skipped");
        assert_eq!(
            plan("한글 needle\r\n한글 needle\r\n", "needle", SearchOptions::default(), "n"),
            "한글 n\r\n한글 n\r\n"
        );
        let matcher = build_matcher("b", SearchOptions::default()).unwrap();
        let edits = replace_plan("a\nb\nb", &matcher, "X", false);
        assert_eq!(edits, vec![(2..3, "X".to_string()), (4..5, "X".to_string())]);
    }

    #[test]
    fn test_c_replace_4_sentences() {
        assert_eq!(replace_summary(3, 12), "Replaced 12 matches in 3 files");
        assert_eq!(replace_summary(1, 1), "Replaced 1 match in 1 file");
        assert_eq!(replace_all_prompt(12, 3), "Replace 12 matches in 3 files?");
        assert_eq!(unsaved_skip_text("a.js"), "a.js has unsaved changes. Save it first.");
    }

    #[test]
    fn test_c_replace_6_the_report_adds_skipped_and_failed_files_after_the_summary() {
        assert_eq!(replace_report(3, 12, &[], 0), "Replaced 12 matches in 3 files");
        assert_eq!(
            replace_report(1, 2, &["a.js".to_string()], 0),
            "Replaced 2 matches in 1 file. a.js has unsaved changes. Save it first."
        );
        assert_eq!(
            replace_report(0, 0, &["a.js".to_string()], 1),
            "a.js has unsaved changes. Save it first. 1 file could not be replaced."
        );
        assert_eq!(replace_report(2, 2, &[], 2), "Replaced 2 matches in 2 files. 2 files could not be replaced.");
        assert_eq!(replace_report(0, 0, &[], 0), "Nothing to replace");
    }

    #[test]
    fn test_c_replace_5_an_invalid_pattern_gives_the_search_sentence() {
        let regex = SearchOptions { regex: true, ..SearchOptions::default() };
        assert!(build_matcher("(", regex).unwrap_err().starts_with("Invalid search pattern"));
    }
}
