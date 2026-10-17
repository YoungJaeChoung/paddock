//! Work 목록의 규칙: 작업 폴더와 Unassigned 묶음의 소속, 터미널 이름 번호, 탭 이동, 닫은 뒤 다음 선택.
//! 화면과 저장소를 모르는 순수 계산이다.
//!
//! 터미널은 작업 폴더 하나 아니면 Unassigned 묶음 하나에 속한다. 작업 폴더는 사용자가 등록하지 않고, 터미널에서
//! 폴더로 이동한 뒤 에이전트를 실행하면 그 폴더가 목록에 생긴다(`WorkState::promote_agent_terminals`).
use std::collections::HashSet;

/// 같은 탭 줄에서 옮겨 갈 위치.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabTarget {
    /// 앞 탭. 첫 탭에서는 마지막 탭으로 돈다.
    Previous,
    /// 뒤 탭. 마지막 탭에서는 첫 탭으로 돈다.
    Next,
    First,
    Last,
}

/// 사이드바 순서의 터미널 묶음. `ids`에는 지금 열 수 있는 터미널만 든다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalGroup<T> {
    pub key: String,
    pub ids: Vec<T>,
}

/// 같은 탭 줄에서 옮겨 갈 터미널. 단축키(Ctrl+방향키)로 탭을 오갈 때 쓴다.
///
/// 현재 터미널이 줄에 없거나 줄이 비면 None이다.
///
/// Examples
/// --------
/// | ids       | current | target     | 결과      |
/// |-----------|---------|------------|-----------|
/// | a, b, c   | a       | Previous   | `Some(c)` |
/// | a, b, c   | b       | Last       | `Some(c)` |
/// | a, b      | x       | Next       | `None`    |
pub fn tab_target<'a, T: PartialEq>(
    ids: &'a [T],
    current: &T,
    target: TabTarget,
) -> Option<&'a T> {
    let n_tabs = ids.len();
    let index = ids.iter().position(|id| id == current);
    let mut result = None;
    if let Some(index) = index {
        let position = match target {
            TabTarget::Previous => (index + n_tabs - 1) % n_tabs,
            TabTarget::Next => (index + 1) % n_tabs,
            TabTarget::First => 0,
            TabTarget::Last => n_tabs - 1,
        };
        result = ids.get(position);
    }
    result
}

/// 화면을 닫은 뒤 대신 보여 줄 터미널. 본문이 비지 않게 사이드바 순서에서 가장 가까운 묶음의 터미널을 고른다.
///
/// 닫은 화면의 묶음(`closed_key`)에 남은 터미널이 먼저이고, 없으면 바로 위 묶음부터 위로, 그다음 아래 묶음을 본다.
/// `closed_key`가 목록에 없으면 첫 묶음부터 본다. 남은 터미널이 하나도 없으면 None이다.
///
/// Examples
/// --------
/// | groups                                   | closed_key | 결과      |
/// | ---------------------------------------- | ---------- | --------- |
/// | a: [a1, a2], b: [b1]                     | a          | `Some(a1)`|
/// | a: [a1], b: [], c: [c1]                  | b          | `Some(a1)`|
/// | a: [], b: [], c: [c1]                    | a          | `Some(c1)`|
/// | a: [], b: []                             | a          | `None`    |
pub fn nearest_terminal<'a, T>(
    groups: &'a [TerminalGroup<T>],
    closed_key: &str,
) -> Option<&'a T> {
    let index = groups.iter().position(|group| group.key == closed_key);
    let order: Vec<&TerminalGroup<T>> = match index {
        Some(index) => std::iter::once(&groups[index])
            .chain(groups[..index].iter().rev())
            .chain(groups[index + 1..].iter())
            .collect(),
        None => groups.iter().collect(),
    };
    order.iter().find_map(|group| group.ids.first())
}

/// 새 터미널의 기본 이름(`terminal N`)에 붙일 번호. 지금 쓰고 있는 번호를 피한 가장 작은 양의 정수다.
/// 번호는 만들 때 한 번 정하므로, 앞 터미널이 닫혀도 남은 터미널의 이름은 바뀌지 않는다.
///
/// Examples
/// --------
/// | used       | 결과 |
/// | ---------- | ---- |
/// | {}         | 1    |
/// | {1, 2}     | 3    |
/// | {2, 3}     | 1    |
pub fn free_terminal_number(used: &HashSet<u32>) -> u32 {
    let mut number = 1;
    while used.contains(&number) {
        number += 1;
    }
    number
}

/// 터미널 이름들 중 기본 이름(`terminal N`)이 쓰고 있는 번호. 사용자가 바꾼 이름은 번호로 보지 않는다.
pub fn used_terminal_numbers<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> HashSet<u32> {
    names
        .into_iter()
        .filter_map(|name| {
            let digits = name.as_ref().strip_prefix("terminal ")?;
            let is_number = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
            if is_number { digits.parse::<u32>().ok() } else { None }
        })
        .collect()
}

/// 새 터미널의 기본 이름(`terminal N`). 번호는 Work 목록 전체에서 겹치지 않게 고른다.
/// 다른 묶음에 같은 `terminal 1`이 있으면 이름만으로 어느 터미널인지 가릴 수 없기 때문이다.
///
/// `names`에는 Work 목록의 모든 묶음의 터미널 이름을 합쳐 넘기고, `reserved_numbers`는 이름 목록에 없지만 쓰고 있는 번호다.
///
/// Examples
/// --------
/// | names                          | reserved_numbers | 결과         |
/// | ------------------------------ | ---------------- | ------------ |
/// | (없음)                         | {}               | `terminal 1` |
/// | `terminal 1`, `claude`         | {}               | `terminal 2` |
/// | `terminal 2`                   | {1}              | `terminal 3` |
pub fn default_terminal_name<S: AsRef<str>>(
    names: impl IntoIterator<Item = S>,
    reserved_numbers: &HashSet<u32>,
) -> String {
    let mut used = used_terminal_numbers(names);
    used.extend(reserved_numbers.iter().copied());
    format!("terminal {}", free_terminal_number(&used))
}

/// 한 폴더를 펼쳤을 때 한 번에 보이는 터미널 줄 수. 넘으면 "Show N more" 줄로 묶는다.
pub const N_VISIBLE_TERMINALS: usize = 8;

/// 경로가 Windows 모양(드라이브 글자 또는 `\\서버\공유`)인지.
fn is_windows_style(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("\\\\") || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

/// 작업 폴더의 key. 폴더를 가리키는 경로를 끝 구분자만 다른 같은 폴더가 한 줄이 되도록 맞춘 글자다.
///
/// 드라이브 경로는 구분자를 `/`로, 드라이브 글자를 대문자로 맞춘다(OSC 7과 탐색기 경로가 섞여 와도 같은 폴더가 되게).
/// `\\wsl.localhost\…` 같은 공유 경로는 역슬래시를 그대로 둔다 — `wsl.exe`가 이 모양으로 받는다.
/// 루트(`/`·`C:/`)의 구분자는 남긴다.
///
/// Examples
/// --------
/// | 경로                            | key                           |
/// |---------------------------------|-------------------------------|
/// | `/home/me/`                     | `/home/me`                    |
/// | `c:\Users\me\`                  | `C:/Users/me`                 |
/// | `\\wsl.localhost\Ubuntu\home\`  | `\\wsl.localhost\Ubuntu\home` |
/// | `/`                             | `/`                           |
pub fn folder_key(path: &str) -> String {
    let trimmed = path.trim();
    let bytes = trimmed.as_bytes();
    let mut key = trimmed.to_string();
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        // 같은 공유 경로가 OSC 7에서는 슬래시로, WSL 조사에서는 역슬래시로 온다. 역슬래시 한 가지 철자로 모은다.
        key = trimmed.replace('/', "\\");
        if let Some(host_end) = key[2..].find('\\').map(|at| at + 2) {
            let host = key[2..host_end].to_ascii_lowercase();
            if host == "wsl.localhost" || host == "wsl$" {
                key.replace_range(2..host_end, &host);
            }
        }
        while key.len() > 2 && (key.ends_with('\\') || key.ends_with('/')) {
            key.pop();
        }
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        key = trimmed.replace('\\', "/");
        key.replace_range(..1, &trimmed[..1].to_ascii_uppercase());
        while key.len() > 3 && key.ends_with('/') {
            key.pop();
        }
        if key.len() == 2 {
            key.push('/');
        }
    } else {
        while key.len() > 1 && (key.ends_with('/') || key.ends_with('\\')) {
            key.pop();
        }
    }
    key
}

/// 두 폴더 key를 같은 폴더인지 가르는 비교용 글자. 폴더를 "같다"고 보는 기준이 이 한 곳에 있다.
///
/// 드라이브 경로(`C:/…`)와 일반 공유 경로(`\\서버\\…`)는 윈도 파일 시스템이라 대소문자를 가리지 않으므로 모두 소문자로 맞춘다.
/// `\\wsl.localhost\\…`·`\\wsl$\\…`는 배포판 이름까지만 소문자로 맞추고 그 아래는 리눅스 파일 시스템이라 그대로 둔다.
/// 리눅스 경로도 그대로다. 표시는 이 글자가 아니라 원래 key로 한다.
///
/// Examples
/// --------
/// | key                           | 비교용 글자                  |
/// |-------------------------------|------------------------------|
/// | `C:/Users/Me`                 | `c:/users/me`                |
/// | `\\srv\\Share\\Dir`           | `\\srv\\share\\dir`          |
/// | `\\wsl.localhost\\Ubuntu\\App` | `\\wsl.localhost\\ubuntu\\App` |
/// | `/App`                        | `/App`                       |
pub fn folder_identity(key: &str) -> String {
    let key = folder_key(key);
    let is_wsl_share = key.starts_with("\\\\wsl.localhost\\") || key.starts_with("\\\\wsl$\\");
    if is_wsl_share {
        let distribution_end = key[2..].match_indices('\\').nth(1).map_or(key.len(), |(at, _)| at + 2);
        let (head, tail) = key.split_at(distribution_end);
        format!("{}{tail}", head.to_ascii_lowercase())
    } else if is_windows_style(&key) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

/// 두 경로가 같은 작업 폴더를 가리키는지. 끝 구분자와 윈도 경로의 대소문자 차이는 같은 폴더다.
pub fn same_folder(
    left: &str,
    right: &str,
) -> bool {
    folder_identity(left) == folder_identity(right)
}

/// `path`가 `key` 폴더 안(또는 그 폴더 자체)에 있는지.
pub fn is_inside(
    key: &str,
    path: &str,
) -> bool {
    let (key, path) = (folder_identity(key), folder_identity(path));
    let is_root = key.ends_with('/') || key.ends_with('\\');
    path == key
        || (is_root && path.starts_with(&key))
        || path.strip_prefix(&key).is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// 행에 보일 폴더 이름. 사용자가 바꾼 이름이 먼저이고, 홈 폴더는 `~`, 나머지는 마지막 폴더 이름이다.
///
/// Examples
/// --------
/// | key                        | 바꾼 이름 | 홈          | 결과    |
/// |----------------------------|-----------|-------------|---------|
/// | `/home/a`                  | (없음)    | `/home/me`  | `a`     |
/// | `/home/a`                  | `mind`    | `/home/me`  | `mind`  |
/// | `/home/me`                 | (없음)    | `/home/me`  | `~`     |
/// | `\\wsl.localhost\U\home\a` | (없음)    | (모름)      | `a`     |
/// | `/`                        | (없음)    | (모름)      | `/`     |
pub fn folder_name(
    key: &str,
    label: Option<&str>,
    home: Option<&str>,
) -> String {
    let is_home = home.is_some_and(|home| same_folder(home, key));
    let base = key.rsplit(['/', '\\']).find(|part| !part.is_empty() && !part.ends_with(':'));
    match (label.filter(|label| !label.is_empty()), is_home, base) {
        (Some(label), _, _) => label.to_string(),
        (None, true, _) => "~".to_string(),
        (None, false, Some(base)) => base.to_string(),
        (None, false, None) => key.to_string(),
    }
}

/// 작업 폴더 하나. `label`은 사용자가 목록에서만 바꾼 이름이고 디스크의 폴더 이름은 그대로다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub key: String,
    pub expanded: bool,
    pub label: Option<String>,
}

/// 작업 폴더에 속한 터미널. 폴더 안 목록 순서는 `WorkState::terminals`의 순서다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: u32,
    pub folder: String,
    pub name: String,
}

/// Unassigned 묶음 안의 터미널.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMember {
    pub id: u32,
    pub name: String,
}

/// 작업 폴더에 속하지 않은 터미널의 묶음. 같은 묶음의 터미널은 한 탭 줄에 나란히 보이고 에이전트를 실행하면 함께 폴더로 옮겨진다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// 묶음을 시작한 터미널. 행 이름이 이 터미널의 이름이다.
    pub root: u32,
    /// 탭 순서의 터미널. 첫 항목이 `root`다.
    pub members: Vec<GroupMember>,
    /// 묶음에서 마지막으로 쓴 터미널. 행을 누르면 이 터미널로 돌아간다.
    pub last_used: u32,
}

/// 탭 줄과 본문이 한 번에 보여 주는 터미널 묶음의 이름표.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GroupKey {
    Folder(String),
    /// Unassigned 묶음. 값은 `Group::root`.
    Group(u32),
}

/// 터미널 행 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRow {
    pub id: u32,
    pub folder: String,
    pub name: String,
    /// 같은 폴더에 같은 이름이 있으면 두 번째부터 `·2`, `·3`. 없으면 빈 글자.
    pub suffix: String,
}

/// Work 목록의 작업 폴더 구획 행.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Folder {
        key: String,
        expanded: bool,
        n_terminals: usize,
        current: bool,
    },
    Terminal(TerminalRow),
    /// 한도(8줄) 때문에 가려진 터미널 수.
    More {
        folder: String,
        n_hidden: usize,
    },
}

/// 새 터미널이 열릴 자리.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewPlace {
    Folder(String),
    Group(u32),
    NewGroup,
}

/// 에이전트를 올릴지 판정할 터미널 하나의 관찰 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: u32,
    /// 터미널의 현재 폴더. 아직 모르면 None.
    pub cwd: Option<String>,
    pub program: Option<String>,
    pub is_agent: bool,
    /// 지금 목록에 보이는 이름.
    pub name: String,
}

/// 에이전트 승격의 결과: 소속이 바뀐 터미널과 새로 생긴 폴더 key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Promotion {
    pub promoted: Vec<u32>,
    pub added: Vec<String>,
}

/// 폴더를 뺀 결과: 닫을 터미널과 그 뒤 옮겨 갈 터미널.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    pub closed: Vec<u32>,
    pub next: Option<u32>,
}

/// 새 터미널(탭 줄 ＋·단축키)이 열릴 자리. 보이는 작업 폴더가 있으면 그 폴더, 없고 Unassigned 묶음이 보이면 그 묶음, 둘 다 없으면 새 묶음이다.
///
/// 터미널은 폴더나 묶음 중 하나에 속하므로 지금 보는 터미널의 소속이 곧 입력이다. 소속을 잃은 터미널은 둘 다 None이라 새 묶음이 된다.
///
/// Examples
/// --------
/// | 보이는 폴더 | 보이는 묶음 | 결과        |
/// |-------------|-------------|-------------|
/// | `/a`        | 1           | `Folder(/a)`|
/// | 없음        | 1           | `Group(1)`  |
/// | 없음        | 없음        | `NewGroup`  |
pub fn new_terminal_place(
    folder: Option<&str>,
    group: Option<u32>,
) -> NewPlace {
    match (folder, group) {
        (Some(folder), _) => NewPlace::Folder(folder.to_string()),
        (None, Some(root)) => NewPlace::Group(root),
        (None, None) => NewPlace::NewGroup,
    }
}

/// Work 목록의 상태: 작업 폴더, 폴더 안 터미널, Unassigned 묶음.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkState {
    pub folders: Vec<Folder>,
    /// 폴더에 배정된 터미널. 폴더 안 목록 순서이기도 하다.
    pub terminals: Vec<Member>,
    pub groups: Vec<Group>,
}

impl WorkState {
    /// 폴더가 없으면 목록 끝에 더한다. 끝 구분자만 다른 같은 폴더는 한 줄이다.
    pub fn ensure_folder(
        &mut self,
        key: &str,
    ) {
        let key = self.resolve_key(key);
        if !self.folders.iter().any(|folder| folder.key == key) {
            self.folders.push(Folder { key, expanded: true, label: None });
        }
    }

    /// 경로가 가리키는 폴더의 key. 이미 같은 폴더(`same_folder`)가 목록에 있으면 그 폴더의 key이고, 없으면 정규화한 새 key다.
    fn resolve_key(
        &self,
        path: &str,
    ) -> String {
        self.folders
            .iter()
            .find(|folder| same_folder(&folder.key, path))
            .map_or_else(|| folder_key(path), |folder| folder.key.clone())
    }

    /// 터미널을 한 폴더에 소속시킨다. 이미 다른 폴더나 묶음에 있었으면 그곳에서 빠진다. 폴더 안 목록 위치는 처음 배정된 자리를 지킨다.
    pub fn assign_terminal(
        &mut self,
        id: u32,
        key: &str,
        name: &str,
    ) {
        self.ensure_folder(key);
        let key = self.resolve_key(key);
        self.leave_group(id);
        match self.terminals.iter_mut().find(|member| member.id == id) {
            Some(member) => {
                member.folder = key;
                member.name = name.to_string();
            }
            None => self.terminals.push(Member { id, folder: key, name: name.to_string() }),
        }
    }

    pub fn rename_terminal(
        &mut self,
        id: u32,
        name: &str,
    ) {
        if let Some(member) = self.terminals.iter_mut().find(|member| member.id == id) {
            member.name = name.to_string();
        }
        for group in &mut self.groups {
            for member in group.members.iter_mut().filter(|member| member.id == id) {
                member.name = name.to_string();
            }
        }
    }

    /// 닫힌 터미널의 소속만 지운다. 폴더는 사용자가 뺄 때까지 남고, 묶음은 마지막 터미널이 닫히면 사라진다.
    pub fn forget_terminal(
        &mut self,
        id: u32,
    ) {
        self.terminals.retain(|member| member.id != id);
        self.leave_group(id);
    }

    /// 터미널을 묶음에서 뺀다. 묶음의 시작 터미널이 빠지면 다음 터미널이 시작 터미널이 되고, 비면 묶음이 사라진다.
    fn leave_group(
        &mut self,
        id: u32,
    ) {
        for group in &mut self.groups {
            group.members.retain(|member| member.id != id);
            if let Some(first) = group.members.first() {
                if group.root == id {
                    group.root = first.id;
                }
                if group.last_used == id {
                    group.last_used = group.root;
                }
            }
        }
        self.groups.retain(|group| !group.members.is_empty());
    }

    pub fn folder_of(
        &self,
        id: u32,
    ) -> Option<&str> {
        self.terminals.iter().find(|member| member.id == id).map(|member| member.folder.as_str())
    }

    /// 터미널이 속한 Unassigned 묶음의 시작 터미널.
    pub fn group_of(
        &self,
        id: u32,
    ) -> Option<u32> {
        self.groups.iter().find(|group| group.members.iter().any(|member| member.id == id)).map(|group| group.root)
    }

    /// 지금 목록에 보이는 터미널 이름.
    pub fn name_of(
        &self,
        id: u32,
    ) -> Option<&str> {
        let in_folder = self.terminals.iter().find(|member| member.id == id).map(|member| member.name.as_str());
        in_folder.or_else(|| {
            self.groups
                .iter()
                .flat_map(|group| group.members.iter())
                .find(|member| member.id == id)
                .map(|member| member.name.as_str())
        })
    }

    /// 파일 경로가 속한 작업 폴더. 폴더가 겹치면(홈과 그 아래 저장소) 가장 가까운(긴) 폴더다. 없으면 None.
    ///
    /// Examples
    /// --------
    /// | 폴더                  | 경로                       | 결과        |
    /// |-----------------------|----------------------------|-------------|
    /// | `~`, `~/repo`         | `~/repo/README.md`         | `~/repo`    |
    /// | `~/repo`              | `/tmp/x`                   | None        |
    pub fn folder_containing(
        &self,
        path: &str,
    ) -> Option<&str> {
        self.folders
            .iter()
            .filter(|folder| is_inside(&folder.key, path))
            .max_by_key(|folder| folder.key.len())
            .map(|folder| folder.key.as_str())
    }

    /// 폴더 안 터미널 id. 목록 순서다.
    pub fn terminals_of(
        &self,
        key: &str,
    ) -> Vec<u32> {
        let key = self.resolve_key(key);
        self.terminals.iter().filter(|member| member.folder == key).map(|member| member.id).collect()
    }

    pub fn set_expanded(
        &mut self,
        key: &str,
        expanded: bool,
    ) {
        let key = self.resolve_key(key);
        if let Some(folder) = self.folders.iter_mut().find(|folder| folder.key == key) {
            folder.expanded = expanded;
        }
    }

    /// 목록에 보일 이름을 바꾼다. 공백뿐이면 이름을 지워 폴더 이름으로 돌아간다.
    pub fn set_folder_label(
        &mut self,
        key: &str,
        label: &str,
    ) {
        let key = self.resolve_key(key);
        let text = label.trim();
        if let Some(folder) = self.folders.iter_mut().find(|folder| folder.key == key) {
            folder.label = (!text.is_empty()).then(|| text.to_string());
        }
    }

    pub fn folder_label(
        &self,
        key: &str,
    ) -> Option<&str> {
        let key = self.resolve_key(key);
        self.folders.iter().find(|folder| folder.key == key).and_then(|folder| folder.label.as_deref())
    }

    /// 새 Unassigned 묶음을 목록 끝에 만든다. `root`가 첫 터미널이다.
    pub fn add_group(
        &mut self,
        root: u32,
        name: &str,
    ) {
        self.forget_terminal(root);
        self.groups.push(Group {
            root,
            members: vec![GroupMember { id: root, name: name.to_string() }],
            last_used: root,
        });
    }

    /// 터미널을 `root` 묶음의 맨 뒤에 더한다. 그런 묶음이 없으면 이 터미널로 새 묶음을 시작한다.
    pub fn join_group(
        &mut self,
        root: u32,
        id: u32,
        name: &str,
    ) {
        self.forget_terminal(id);
        match self.groups.iter_mut().find(|group| group.root == root) {
            Some(group) => {
                group.members.push(GroupMember { id, name: name.to_string() });
                group.last_used = id;
            }
            None => self.add_group(id, name),
        }
    }

    /// 묶음 안 터미널 id. 탭 순서다.
    pub fn group_members(
        &self,
        root: u32,
    ) -> Vec<u32> {
        self.groups
            .iter()
            .find(|group| group.root == root)
            .map(|group| group.members.iter().map(|member| member.id).collect())
            .unwrap_or_default()
    }

    /// 터미널을 쓴 것으로 기록한다. 묶음 행을 누르면 이 터미널로 돌아간다.
    pub fn touch(
        &mut self,
        id: u32,
    ) {
        if let Some(group) = self.groups.iter_mut().find(|group| group.members.iter().any(|member| member.id == id)) {
            group.last_used = id;
        }
    }

    /// 사이드바 순서의 묶음: 작업 폴더(목록 순서) 다음에 Unassigned 묶음.
    pub fn group_order(&self) -> Vec<GroupKey> {
        self.folders
            .iter()
            .map(|folder| GroupKey::Folder(folder.key.clone()))
            .chain(self.groups.iter().map(|group| GroupKey::Group(group.root)))
            .collect()
    }

    /// 터미널이 속한 묶음.
    pub fn group_key_of(
        &self,
        id: u32,
    ) -> Option<GroupKey> {
        self.folder_of(id)
            .map(|key| GroupKey::Folder(key.to_string()))
            .or_else(|| self.group_of(id).map(GroupKey::Group))
    }

    /// 묶음 안 터미널 id.
    pub fn ids_in(
        &self,
        key: &GroupKey,
    ) -> Vec<u32> {
        match key {
            GroupKey::Folder(folder) => self.terminals_of(folder),
            GroupKey::Group(root) => self.group_members(*root),
        }
    }

    /// `closed_id` 터미널을 닫은 뒤 대신 보여 줄 터미널. 닫은 묶음의 남은 터미널이 먼저이고, 없으면 사이드바 순서에서 가까운 묶음이다.
    /// Unassigned 묶음은 마지막으로 쓴 터미널로 돌아간다.
    pub fn nearest_after_close(
        &self,
        closed_key: &GroupKey,
        closed_id: u32,
    ) -> Option<u32> {
        let label = |key: &GroupKey| match key {
            GroupKey::Folder(folder) => format!("f:{folder}"),
            GroupKey::Group(root) => format!("g:{root}"),
        };
        let groups: Vec<TerminalGroup<u32>> = self
            .group_order()
            .iter()
            .map(|key| TerminalGroup {
                key: label(key),
                ids: self.ids_in(key).into_iter().filter(|id| *id != closed_id).collect(),
            })
            .collect();
        let found = nearest_terminal(&groups, &label(closed_key)).copied()?;
        let last_used = match self.group_key_of(found) {
            Some(GroupKey::Group(root)) => self
                .groups
                .iter()
                .find(|group| group.root == root)
                .map(|group| group.last_used)
                .filter(|id| *id != closed_id),
            _ => None,
        };
        last_used.or(Some(found))
    }

    /// 에이전트가 실행 중인 터미널을 그 터미널의 현재 폴더의 작업 터미널로 올린다.
    ///
    /// 에이전트(claude·codex 등)를 띄운 곳이 곧 작업할 곳이라 사용자가 폴더를 등록하지 않아도 목록에 선다.
    /// 작업 폴더는 이 함수로만 새로 생긴다. Unassigned 묶음은 어느 터미널이 에이전트를 실행하든 묶음 전체가 함께 옮겨지고,
    /// 이미 폴더에 있던 터미널이 다른 폴더에서 에이전트를 실행하면 그 터미널만 해당 폴더로 옮겨진다.
    /// 에이전트가 끝나도 목록에는 남는다. 현재 폴더를 아직 모르는 터미널은 올리지 않는다(호출자가 다음 주기에 다시 준다).
    ///
    /// 이름은 에이전트를 실행한 터미널이 프로그램 이름이 되고, 함께 옮겨진 터미널은 지금 이름을 지킨다.
    ///
    /// Examples
    /// --------
    /// | 상태                       | 후보                              | 결과                            |
    /// |----------------------------|-----------------------------------|---------------------------------|
    /// | 묶음 [1]                   | 1: `/r/`, claude, 에이전트        | 1이 `/r`에 `claude`로, 폴더 생성 |
    /// | `/a`의 0                   | 0: `/b`, claude, 에이전트         | 0이 `/b`로 이동                 |
    /// | 묶음 [1]                   | 1: `/a`, bash, 에이전트 아님      | 변화 없음                       |
    pub fn promote_agent_terminals(
        &mut self,
        candidates: &[Candidate],
    ) -> Promotion {
        let mut promotion = Promotion::default();
        let mut moved: HashSet<u32> = HashSet::new();
        for candidate in candidates {
            let Some(cwd) = candidate.cwd.as_deref().filter(|_| candidate.is_agent) else { continue };
            let key = self.resolve_key(cwd);
            if moved.contains(&candidate.id) || self.folder_of(candidate.id) == Some(key.as_str()) {
                continue;
            }
            if !self.folders.iter().any(|folder| folder.key == key) && !promotion.added.contains(&key) {
                promotion.added.push(key.clone());
            }
            let members = match self.group_of(candidate.id) {
                Some(root) => self.group_members(root),
                None => vec![candidate.id],
            };
            for member in members {
                let name = if member == candidate.id {
                    candidate.program.clone().unwrap_or_else(|| candidate.name.clone())
                } else {
                    self.name_of(member).unwrap_or("terminal").to_string()
                };
                self.assign_terminal(member, &key, &name);
                promotion.promoted.push(member);
                moved.insert(member);
            }
        }
        promotion
    }

    /// 같은 폴더 안에서 터미널 한 개의 목록 순서를 바꾼다. `after`가 false면 `target` 앞에, true면 뒤에 놓는다.
    /// 두 터미널이 같은 폴더에 있지 않거나 같은 터미널이면 바꾸지 않는다.
    pub fn move_terminal(
        &mut self,
        id: u32,
        target: u32,
        after: bool,
    ) {
        let same_folder = match (self.folder_of(id), self.folder_of(target)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        };
        if id != target && same_folder {
            let index = self.terminals.iter().position(|member| member.id == id).expect("member of a folder");
            let member = self.terminals.remove(index);
            let at = self.terminals.iter().position(|other| other.id == target).expect("member of a folder");
            self.terminals.insert(at + usize::from(after), member);
        }
    }

    /// 한 폴더의 터미널 행. 목록 순서대로 이름을 쓰고, 같은 이름은 두 번째부터 `·2`·`·3`을 붙인다.
    /// 사이드바와 폴더의 탭 줄이 같은 이름표를 쓰도록 둘 다 이 행을 쓴다.
    ///
    /// Examples
    /// --------
    /// | 이름                       | suffix                |
    /// |----------------------------|-----------------------|
    /// | `claude`, `terminal`, `claude` | ``, ``, `·2`      |
    pub fn terminal_rows(
        &self,
        key: &str,
    ) -> Vec<TerminalRow> {
        let key = self.resolve_key(key);
        let mut seen: Vec<(&str, usize)> = Vec::new();
        let mut rows = Vec::new();
        for member in self.terminals.iter().filter(|member| member.folder == key) {
            let n_same = match seen.iter_mut().find(|(name, _)| *name == member.name) {
                Some((_, count)) => {
                    *count += 1;
                    *count
                }
                None => {
                    seen.push((member.name.as_str(), 1));
                    1
                }
            };
            let suffix = if n_same > 1 { format!("·{n_same}") } else { String::new() };
            rows.push(TerminalRow { id: member.id, folder: key.clone(), name: member.name.clone(), suffix });
        }
        rows
    }

    /// 폴더를 목록에서 뺀다. 디스크의 폴더는 건드리지 않는다.
    ///
    /// `closed`는 호출자가 닫을 터미널이고, `next`는 현재 터미널(`current`)이 뺀 폴더에 있었을 때 옮겨 갈 터미널이다.
    /// 다음 선택은 바로 위 폴더의 첫 터미널, 없으면 아래 폴더의 첫 터미널, 그것도 없으면 None이다.
    /// 현재 터미널이 다른 폴더에 있으면 `next`는 그 터미널 그대로다.
    pub fn remove_folder(
        &mut self,
        key: &str,
        current: Option<u32>,
    ) -> Removal {
        let key = self.resolve_key(key);
        let index = self.folders.iter().position(|folder| folder.key == key);
        let closed = self.terminals_of(&key);
        let mut next = current;
        if current.is_some_and(|id| closed.contains(&id)) {
            let at = index.unwrap_or(0);
            let above = self.folders[..at].iter().rev();
            let below = self.folders.get(at + 1..).unwrap_or_default().iter();
            next = above.chain(below).find_map(|folder| self.terminals_of(&folder.key).first().copied());
        }
        self.folders.retain(|folder| folder.key != key);
        for id in &closed {
            self.forget_terminal(*id);
        }
        Removal { closed, next }
    }

    /// 사이드바 Work 목록의 작업 폴더 구획 행.
    ///
    /// 현재 터미널(`current`)이 속한 폴더는 접혀 있어도 펼치고, 8개 한도 밖이어도 현재 터미널은 보인다.
    /// `show_all`에 든 폴더는 한도 없이 모두 보인다.
    ///
    /// Examples
    /// --------
    /// | 폴더 터미널 | current | 결과                                   |
    /// |-------------|---------|----------------------------------------|
    /// | 20개        | 없음    | 터미널 8줄 + `More` 12                  |
    /// | 20개        | 15번째  | 터미널 9줄(15번째 포함) + `More` 11     |
    pub fn visible_rows(
        &self,
        current: Option<u32>,
        show_all: &HashSet<String>,
    ) -> Vec<Row> {
        let mut rows = Vec::new();
        let current_folder = current.and_then(|id| self.folder_of(id));
        for folder in &self.folders {
            let n_terminals = self.terminals_of(&folder.key).len();
            let is_current = current_folder == Some(folder.key.as_str());
            let expanded = folder.expanded || is_current;
            rows.push(Row::Folder { key: folder.key.clone(), expanded, n_terminals, current: is_current });
            if expanded {
                let named = self.terminal_rows(&folder.key);
                let n_all = named.len();
                let shown: Vec<TerminalRow> = named
                    .into_iter()
                    .enumerate()
                    .filter(|(index, row)| {
                        show_all.contains(&folder.key) || *index < N_VISIBLE_TERMINALS || Some(row.id) == current
                    })
                    .map(|(_, row)| row)
                    .collect();
                let n_shown = shown.len();
                rows.extend(shown.into_iter().map(Row::Terminal));
                if n_shown < n_all {
                    rows.push(Row::More { folder: folder.key.clone(), n_hidden: n_all - n_shown });
                }
            }
        }
        rows
    }

    /// 새 터미널의 기본 이름(`terminal N`). 번호는 작업 폴더와 Unassigned 묶음 전체에서 겹치지 않게 고른다.
    pub fn next_terminal_name(&self) -> String {
        let folder_names = self.terminals.iter().map(|member| member.name.as_str());
        let group_names = self.groups.iter().flat_map(|group| group.members.iter()).map(|member| member.name.as_str());
        default_terminal_name(folder_names.chain(group_names), &HashSet::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(values: &[u32]) -> HashSet<u32> {
        values.iter().copied().collect()
    }

    #[test]
    fn test_c6_terminal_number_skips_used() {
        assert_eq!(free_terminal_number(&numbers(&[])), 1);
        assert_eq!(free_terminal_number(&numbers(&[1, 2])), 3);
        assert_eq!(free_terminal_number(&numbers(&[2, 3])), 1);
        assert_eq!(default_terminal_name(["terminal 1", "claude", "terminal 3"], &numbers(&[])), "terminal 2");
    }

    #[test]
    fn test_c6_terminal_number_unique_across_groups() {
        // 모든 묶음의 이름을 함께 넘기므로 다른 묶음에서 쓰는 번호도 건너뛴다.
        let group_a = ["terminal 1", "claude", "terminal 3"];
        let group_b = ["terminal 2"];
        let all: Vec<&str> = group_a.iter().chain(group_b.iter()).copied().collect();
        assert_eq!(default_terminal_name(all, &numbers(&[])), "terminal 4");
        // 넘긴 이름에 없는 터미널이 잡아 둔 번호도 건너뛴다.
        assert_eq!(default_terminal_name(["terminal 2"], &numbers(&[1])), "terminal 3");
        assert_eq!(default_terminal_name(Vec::<&str>::new(), &numbers(&[])), "terminal 1");
    }

    #[test]
    fn test_c6_renamed_terminal_does_not_hold_a_number() {
        assert_eq!(
            used_terminal_numbers(["terminal 1", "terminal  2", "terminal 2x", "Terminal 3", "terminal 07"]),
            numbers(&[1, 7])
        );
    }

    // -- 작업 폴더와 Unassigned 묶음 --

    fn with_terminals(
        key: &str,
        names: &[&str],
    ) -> WorkState {
        let mut state = WorkState::default();
        state.ensure_folder(key);
        for (index, name) in names.iter().enumerate() {
            state.assign_terminal(index as u32, key, name);
        }
        state
    }

    fn folder_keys(state: &WorkState) -> Vec<&str> {
        state.folders.iter().map(|folder| folder.key.as_str()).collect()
    }

    fn candidate(
        id: u32,
        cwd: Option<&str>,
        program: &str,
        is_agent: bool,
    ) -> Candidate {
        Candidate {
            id,
            cwd: cwd.map(str::to_string),
            program: Some(program.to_string()),
            is_agent,
            name: format!("terminal {id}"),
        }
    }

    #[test]
    fn test_c_work_f1_1_new_folder_goes_to_the_end_of_the_list() {
        let mut state = WorkState::default();
        state.ensure_folder("/home/a");
        state.ensure_folder("/home/b");
        assert_eq!(folder_keys(&state), vec!["/home/a", "/home/b"]);
    }

    #[test]
    fn test_c_work_f1_2_existing_folder_is_not_duplicated_when_only_the_trailing_separator_differs() {
        let mut state = WorkState::default();
        state.ensure_folder("/home/a");
        state.ensure_folder("/home/a/");
        state.ensure_folder("C:\\work\\x");
        state.ensure_folder("C:/work/x/");
        assert_eq!(folder_keys(&state), vec!["/home/a", "C:/work/x"]);
    }

    #[test]
    fn test_c_work_f1_3_assigning_to_another_folder_leaves_the_previous_one() {
        let mut state = with_terminals("/a", &["claude"]);
        state.ensure_folder("/b");
        state.assign_terminal(0, "/b", "claude");
        assert_eq!(state.folder_of(0), Some("/b"));
        assert!(state.terminals_of("/a").is_empty());
    }

    #[test]
    fn test_c_work_f1_4_file_in_nested_work_folders_belongs_to_the_nearest_one() {
        let mut state = WorkState::default();
        state.ensure_folder("/home/me");
        state.ensure_folder("/home/me/repo");
        assert_eq!(state.folder_containing("/home/me/repo/README.md"), Some("/home/me/repo"));
        assert_eq!(state.folder_containing("/home/me/notes.md"), Some("/home/me"));
        assert_eq!(state.folder_containing("/home/me/repository/a.md"), Some("/home/me"));
        assert_eq!(state.folder_containing("/tmp/x"), None);
        // Windows 경로와 WSL 공유 경로도 같은 규칙이다.
        let mut windows = WorkState::default();
        windows.ensure_folder("C:/Users/me");
        windows.ensure_folder("\\\\wsl.localhost\\Ubuntu\\home\\me");
        assert_eq!(windows.folder_containing("C:/Users/me/app/a.md"), Some("C:/Users/me"));
        assert_eq!(
            windows.folder_containing("\\\\wsl.localhost\\Ubuntu\\home\\me\\app"),
            Some("\\\\wsl.localhost\\Ubuntu\\home\\me")
        );
    }

    #[test]
    fn test_c_work_f2_1_removing_a_folder_returns_only_its_terminals_to_close() {
        let mut state = with_terminals("/a", &["x", "y"]);
        state.ensure_folder("/b");
        state.assign_terminal(10, "/b", "z");
        let removal = state.remove_folder("/a", Some(10));
        let mut closed = removal.closed.clone();
        closed.sort_unstable();
        assert_eq!(closed, vec![0, 1]);
        assert_eq!(folder_keys(&state), vec!["/b"]);
        assert_eq!(state.folder_of(10), Some("/b"));
    }

    #[test]
    fn test_c_work_f2_2_removing_the_current_folder_selects_above_then_below_then_nothing() {
        let mut state = WorkState::default();
        for (key, id) in [("/a", 1), ("/b", 2), ("/c", 3)] {
            state.ensure_folder(key);
            state.assign_terminal(id, key, "t");
        }
        assert_eq!(state.clone().remove_folder("/b", Some(2)).next, Some(1));
        assert_eq!(state.clone().remove_folder("/a", Some(1)).next, Some(2));
        assert_eq!(state.clone().remove_folder("/c", Some(2)).next, Some(2));
        let mut single = WorkState::default();
        single.assign_terminal(9, "/x", "x");
        assert_eq!(single.remove_folder("/x", Some(9)).next, None);
    }

    #[test]
    fn test_c_work_f2_3_folder_stays_when_its_terminal_closes() {
        let mut state = with_terminals("/a", &["x"]);
        state.forget_terminal(0);
        assert_eq!(state.folders.len(), 1);
        assert!(state.terminals_of("/a").is_empty());
    }

    #[test]
    fn test_c_work_f3_1_more_than_eight_terminals_show_eight_rows_and_a_more_row() {
        let names: Vec<String> = (0..20).map(|index| format!("n{index}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let state = with_terminals("/a", &names);
        let rows = state.visible_rows(None, &HashSet::new());
        assert_eq!(rows.iter().filter(|row| matches!(row, Row::Terminal(_))).count(), 8);
        assert!(rows.contains(&Row::More { folder: "/a".to_string(), n_hidden: 12 }));
    }

    #[test]
    fn test_c_work_f3_2_current_terminal_is_visible_even_past_the_ninth() {
        let names: Vec<String> = (0..20).map(|index| format!("n{index}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let state = with_terminals("/a", &names);
        let rows = state.visible_rows(Some(15), &HashSet::new());
        assert!(rows.iter().any(|row| matches!(row, Row::Terminal(terminal) if terminal.id == 15)));
        assert!(rows.contains(&Row::More { folder: "/a".to_string(), n_hidden: 11 }));
        // Show N more를 누른 폴더는 한도 없이 모두 보인다.
        let all = state.visible_rows(None, &["/a".to_string()].into_iter().collect());
        assert_eq!(all.iter().filter(|row| matches!(row, Row::Terminal(_))).count(), 20);
    }

    #[test]
    fn test_c_work_f3_3_duplicate_names_get_a_numbered_suffix_from_the_second() {
        let state = with_terminals("/a", &["mind", "git", "mind", "mind"]);
        let suffixes: Vec<String> = state
            .visible_rows(None, &HashSet::new())
            .into_iter()
            .filter_map(|row| if let Row::Terminal(terminal) = row { Some(terminal.suffix) } else { None })
            .collect();
        assert_eq!(suffixes, vec!["", "", "·2", "·3"]);
    }

    #[test]
    fn test_c_work_f3_4_folder_of_the_current_terminal_opens_even_when_collapsed() {
        let mut state = with_terminals("/a", &["x"]);
        state.set_expanded("/a", false);
        let has_terminal =
            |current| state.visible_rows(current, &HashSet::new()).iter().any(|row| matches!(row, Row::Terminal(_)));
        assert!(has_terminal(Some(0)));
        assert!(!has_terminal(None));
    }

    #[test]
    fn test_c_work_f3_5_folder_label_replaces_the_folder_name_and_empty_label_restores_it() {
        let mut state = WorkState::default();
        state.ensure_folder("/home/a");
        state.set_folder_label("/home/a/", "mind");
        assert_eq!(state.folder_label("/home/a"), Some("mind"));
        state.set_folder_label("/home/a", "  ");
        assert_eq!(state.folder_label("/home/a"), None);
        assert_eq!(folder_name("/home/a", None, Some("/home/me")), "a");
        assert_eq!(folder_name("/home/a", Some("mind"), Some("/home/me")), "mind");
        assert_eq!(folder_name("/home/me", None, Some("/home/me")), "~");
        assert_eq!(folder_name("C:/Users/me/app", None, None), "app");
        assert_eq!(folder_name("\\\\wsl.localhost\\Ubuntu\\home\\me\\app", None, None), "app");
        assert_eq!(folder_name("/", None, None), "/");
    }

    #[test]
    fn test_c_work_f8_1_agent_terminal_becomes_a_work_terminal_named_after_the_agent() {
        let mut state = WorkState::default();
        state.add_group(1, "terminal 1");
        let promotion = state.promote_agent_terminals(&[candidate(1, Some("/home/me/repo/"), "claude", true)]);
        assert_eq!(promotion.promoted, vec![1]);
        assert_eq!(promotion.added, vec!["/home/me/repo"]);
        assert_eq!(state.folder_of(1), Some("/home/me/repo"));
        assert_eq!(state.name_of(1), Some("claude"));
        assert!(state.groups.is_empty());
    }

    #[test]
    fn test_c_work_f8_2_existing_folder_is_reused_when_an_agent_starts_in_it() {
        let mut state = with_terminals("/repo", &["terminal"]);
        state.add_group(5, "terminal 5");
        let promotion = state.promote_agent_terminals(&[candidate(5, Some("/repo"), "codex", true)]);
        assert_eq!(promotion.promoted, vec![5]);
        assert!(promotion.added.is_empty());
        assert_eq!(state.folders.len(), 1);
        assert_eq!(state.terminals_of("/repo"), vec![0, 5]);
    }

    #[test]
    fn test_c_work_f8_3_agent_in_another_folder_moves_the_terminal_there() {
        let mut state = with_terminals("/a", &["terminal"]);
        let promotion = state.promote_agent_terminals(&[candidate(0, Some("/b"), "claude", true)]);
        assert_eq!(promotion.promoted, vec![0]);
        assert_eq!(promotion.added, vec!["/b"]);
        assert_eq!(state.folder_of(0), Some("/b"));
        assert_eq!(state.name_of(0), Some("claude"));
        assert!(state.terminals_of("/a").is_empty());
    }

    #[test]
    fn test_c_work_f8_4_non_agent_or_unknown_folder_is_not_promoted() {
        let mut state = WorkState::default();
        state.add_group(1, "shell");
        state.add_group(2, "unknown");
        let promotion = state
            .promote_agent_terminals(&[candidate(1, Some("/a"), "bash", false), candidate(2, None, "claude", true)]);
        assert!(promotion.promoted.is_empty());
        assert!(state.folders.is_empty());
        assert_eq!(state.groups.len(), 2);
    }

    #[test]
    fn test_c_work_f8_5_agent_in_any_group_member_promotes_the_whole_group_in_tab_order() {
        for agent_id in [10, 11, 12] {
            let mut state = WorkState::default();
            state.add_group(10, "terminal 1");
            state.join_group(10, 11, "terminal 2");
            state.join_group(10, 12, "terminal 3");
            state.add_group(20, "terminal 4");
            let candidates: Vec<Candidate> = [10, 11, 12, 20]
                .into_iter()
                .map(|id| candidate(id, Some("/repo"), if id == agent_id { "claude" } else { "bash" }, id == agent_id))
                .collect();
            let promotion = state.promote_agent_terminals(&candidates);
            assert_eq!(state.terminals_of("/repo"), vec![10, 11, 12]);
            assert_eq!(promotion.promoted, vec![10, 11, 12]);
            assert_eq!(state.folder_of(20), None);
            assert_eq!(state.name_of(agent_id), Some("claude"));
            assert_eq!(promotion.added, vec!["/repo"]);
            assert_eq!(state.groups.len(), 1);
        }
    }

    #[test]
    fn test_c_work_f8_6_group_promotion_changes_membership_without_touching_each_cwd() {
        let mut state = WorkState::default();
        state.add_group(1, "terminal 1");
        state.join_group(1, 2, "terminal 2");
        let candidates = vec![candidate(1, Some("/home"), "bash", false), candidate(2, Some("/repo"), "codex", true)];
        let promotion = state.promote_agent_terminals(&candidates);
        assert_eq!(state.terminals_of("/repo"), vec![1, 2]);
        assert_eq!(promotion.promoted, vec![1, 2]);
        // 후보는 읽기 전용이라 각 터미널의 폴더 정보가 그대로다.
        assert_eq!(candidates[0].cwd.as_deref(), Some("/home"));
        assert_eq!(candidates[1].cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn test_c_work_f9_1_folder_rows_hold_only_that_folders_terminals_with_the_same_suffixes_as_the_sidebar() {
        let mut state = with_terminals("/a", &["claude", "terminal", "claude"]);
        state.assign_terminal(50, "/b", "claude");
        let rows = state.terminal_rows("/a/");
        let shown: Vec<(u32, &str, &str)> =
            rows.iter().map(|row| (row.id, row.name.as_str(), row.suffix.as_str())).collect();
        assert_eq!(shown, vec![(0, "claude", ""), (1, "terminal", ""), (2, "claude", "·2")]);
    }

    #[test]
    fn test_c_work_f14_1_new_terminal_prefers_the_shown_folder_over_an_unassigned_group() {
        assert_eq!(new_terminal_place(Some("/a"), Some(1)), NewPlace::Folder("/a".to_string()));
        assert_eq!(new_terminal_place(Some("/a"), None), NewPlace::Folder("/a".to_string()));
    }

    #[test]
    fn test_c_work_f14_2_without_a_folder_a_shown_unassigned_group_receives_the_terminal() {
        assert_eq!(new_terminal_place(None, Some(1)), NewPlace::Group(1));
    }

    #[test]
    fn test_c_work_f14_3_nothing_shown_opens_a_new_unassigned_group() {
        assert_eq!(new_terminal_place(None, None), NewPlace::NewGroup);
    }

    #[test]
    fn test_c_work_f14_4_a_terminal_that_lost_its_group_still_falls_back_to_a_new_group() {
        // 묶음을 잃은 터미널은 어느 폴더·묶음에도 속하지 않으므로 호출자가 둘 다 None으로 넘긴다.
        let state = WorkState::default();
        assert_eq!(new_terminal_place(state.folder_of(7), state.group_of(7)), NewPlace::NewGroup);
    }

    #[test]
    fn test_c_work_u1_terminal_order_changes_only_inside_one_folder() {
        let mut state = WorkState::default();
        for (id, folder) in [(1, "/p"), (2, "/p"), (3, "/p"), (4, "/q")] {
            state.assign_terminal(id, folder, "t");
        }
        let mut moved = state.clone();
        moved.move_terminal(3, 1, false);
        assert_eq!(moved.terminals_of("/p"), vec![3, 1, 2]);
        let mut after = state.clone();
        after.move_terminal(1, 2, true);
        assert_eq!(after.terminals_of("/p"), vec![2, 1, 3]);
        let mut other_folder = state.clone();
        other_folder.move_terminal(4, 1, false);
        assert_eq!(other_folder, state, "a terminal from another folder is not moved");
        let mut same = state.clone();
        same.move_terminal(1, 1, true);
        assert_eq!(same, state);
    }

    #[test]
    fn test_folder_key_normalizes_separators_drive_letters_and_trailing_separators() {
        assert_eq!(folder_key("/home/me/"), "/home/me");
        assert_eq!(folder_key("/"), "/");
        assert_eq!(folder_key("c:\\Users\\me\\"), "C:/Users/me");
        assert_eq!(folder_key("C:\\"), "C:/");
        assert_eq!(folder_key("C:/"), "C:/");
        // WSL 공유 경로는 역슬래시를 그대로 둔다(wsl.exe가 이 모양으로 받는다).
        assert_eq!(folder_key("\\\\wsl.localhost\\Ubuntu\\home\\me\\"), "\\\\wsl.localhost\\Ubuntu\\home\\me");
        assert_eq!(folder_key("  /tmp/x  "), "/tmp/x");
    }

    #[test]
    fn test_folder_key_gives_one_spelling_to_wsl_paths_from_the_osc_report_and_the_wsl_probe() {
        // OSC 7은 슬래시, WSL 조사는 역슬래시로 같은 폴더를 알려 온다. 어느 쪽이든 같은 key가 된다.
        let probe = "\\\\wsl.localhost\\Ubuntu\\home\\me\\app";
        assert_eq!(folder_key("//wsl.localhost/Ubuntu/home/me/app"), probe);
        assert_eq!(folder_key("//wsl.localhost/Ubuntu/home/me/app/"), probe);
        assert_eq!(folder_key("//WSL.LOCALHOST/Ubuntu/home/me/app"), probe);
        assert_eq!(folder_key(probe), probe);
        assert_eq!(folder_key("//wsl$/Debian/srv"), "\\\\wsl$\\Debian\\srv");
        assert_eq!(folder_key("\\\\wsl$\\Debian\\srv\\"), "\\\\wsl$\\Debian\\srv");
        let mut state = WorkState::default();
        state.ensure_folder("//wsl.localhost/Ubuntu/home/me/app");
        state.ensure_folder(probe);
        assert_eq!(state.folders.len(), 1);
    }

    #[test]
    fn test_group_follows_membership_and_closing_the_root_hands_it_to_the_next_member() {
        let mut state = WorkState::default();
        state.add_group(1, "terminal 1");
        state.join_group(1, 2, "terminal 2");
        assert_eq!(state.group_of(2), Some(1));
        assert_eq!(state.group_members(1), vec![1, 2]);
        state.touch(2);
        assert_eq!(state.groups[0].last_used, 2);
        state.forget_terminal(1);
        assert_eq!(state.group_of(2), Some(2));
        assert_eq!(state.groups[0].last_used, 2);
        state.forget_terminal(2);
        assert!(state.groups.is_empty());
    }

    #[test]
    fn test_default_name_avoids_numbers_in_folders_and_groups() {
        let mut state = with_terminals("/a", &["terminal 1", "claude"]);
        state.add_group(7, "terminal 2");
        assert_eq!(state.next_terminal_name(), "terminal 3");
    }

    #[test]
    fn test_remove_folder_next_terminal_comes_from_nearest_group_in_sidebar_order() {
        let mut state = WorkState::default();
        state.ensure_folder("/a");
        state.assign_terminal(1, "/a", "x");
        state.add_group(9, "terminal 9");
        let order = state.group_order();
        assert_eq!(order, vec![GroupKey::Folder("/a".to_string()), GroupKey::Group(9)]);
        assert_eq!(state.nearest_after_close(&GroupKey::Folder("/a".to_string()), 1), Some(9));
    }

    #[test]
    fn test_tab_target_moves_within_row() {
        let ids = ["a", "b", "c"];
        assert_eq!(tab_target(&ids, &"b", TabTarget::Previous), Some(&"a"));
        assert_eq!(tab_target(&ids, &"a", TabTarget::Previous), Some(&"c"));
        assert_eq!(tab_target(&ids, &"c", TabTarget::Next), Some(&"a"));
        assert_eq!(tab_target(&ids, &"b", TabTarget::First), Some(&"a"));
        assert_eq!(tab_target(&ids, &"b", TabTarget::Last), Some(&"c"));
    }

    #[test]
    fn test_tab_target_none_when_current_missing_or_row_empty() {
        assert_eq!(tab_target(&["a", "b"], &"x", TabTarget::Next), None);
        assert_eq!(tab_target(&[] as &[&str], &"x", TabTarget::First), None);
    }

    fn group(
        key: &str,
        ids: &[&'static str],
    ) -> TerminalGroup<&'static str> {
        TerminalGroup { key: key.to_string(), ids: ids.to_vec() }
    }

    #[test]
    fn test_nearest_terminal_prefers_closed_group_then_above_then_below() {
        assert_eq!(nearest_terminal(&[group("a", &["a1", "a2"]), group("b", &["b1"])], "a"), Some(&"a1"));
        assert_eq!(nearest_terminal(&[group("a", &["a1"]), group("b", &[]), group("c", &["c1"])], "b"), Some(&"a1"));
        assert_eq!(nearest_terminal(&[group("a", &[]), group("b", &[]), group("c", &["c1"])], "a"), Some(&"c1"));
    }

    #[test]
    fn test_nearest_terminal_none_when_nothing_left_and_unknown_key_starts_at_first() {
        assert_eq!(nearest_terminal(&[group("a", &[])], "a"), None);
        assert_eq!(nearest_terminal(&[] as &[TerminalGroup<&str>], "a"), None);
        assert_eq!(nearest_terminal(&[group("a", &[]), group("b", &["b1"])], "gone"), Some(&"b1"));
    }

    #[test]
    fn test_c_folder_same_folder_rule_is_one_function_ignoring_case_only_for_windows_drive_and_share_paths() {
        assert!(same_folder("c:\\Users\\me", "C:\\users\\me\\"));
        assert!(same_folder("\\\\server\\Share\\Dir", "\\\\SERVER\\share\\dir"));
        // 리눅스 파일 시스템은 대소문자를 가린다: 리눅스 경로와 WSL 공유 경로의 배포판 이하.
        assert!(!same_folder("/App", "/app"));
        assert!(same_folder("\\\\wsl.localhost\\Ubuntu\\home\\me", "//WSL.localhost/ubuntu/home/me/"));
        assert!(!same_folder("\\\\wsl.localhost\\Ubuntu\\home\\App", "\\\\wsl.localhost\\Ubuntu\\home\\app"));
    }

    #[test]
    fn test_c_folder_spellings_of_one_windows_folder_share_one_row_and_keep_the_first_spelling() {
        let mut work = WorkState::default();
        work.assign_terminal(1, "C:\\Users\\Me", "a");
        work.assign_terminal(2, "c:\\users\\me\\", "b");
        assert_eq!(work.folders.len(), 1);
        assert_eq!(work.folders[0].key, "C:/Users/Me");
        assert_eq!(work.terminals_of("C:/USERS/ME"), vec![1, 2]);
        work.set_folder_label("c:/users/me", "mine");
        assert_eq!(work.folder_label("C:\\Users\\Me"), Some("mine"));
        assert_eq!(work.folder_containing("c:\\USERS\\me\\src"), Some("C:/Users/Me"));
    }

    #[test]
    fn test_c_folder_linux_and_wsl_paths_that_differ_only_by_case_are_different_folders() {
        let mut work = WorkState::default();
        work.ensure_folder("/App");
        work.ensure_folder("/app");
        work.ensure_folder("\\\\wsl.localhost\\Ubuntu\\home\\App");
        work.ensure_folder("\\\\wsl.localhost\\Ubuntu\\home\\app");
        assert_eq!(work.folders.len(), 4);
        assert_eq!(work.folder_containing("/app/src"), Some("/app"));
        assert_eq!(work.folder_containing("/App/src"), Some("/App"));
    }
}
