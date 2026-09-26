/**
 * 작업 폴더와 그 아래 작업 터미널의 소속을 계산하는 순수 모델.
 *
 * 사이드바 Work 보기가 이 모델로 행을 그리고, 제어 층이 결과대로 터미널을 열고 닫는다.
 * DOM·Theia에 기대지 않아 Node 테스트로 바로 검증한다. 모든 함수는 새 상태를 돌려주며
 * 받은 상태를 바꾸지 않는다.
 *
 * 상태 모양: `{ folders: [{ key, expanded, label? }], terminals: { [id]: { folder, name } } }`.
 * `label`은 사용자가 목록에서 바꾼 표시 이름이며 디스크의 폴더 이름은 바꾸지 않는다.
 * `key`는 폴더의 file URI 문자열이며 끝 슬래시를 뺀 형태로 맞춘다.
 */

// 한 폴더를 펼쳤을 때 한 번에 보이는 터미널 줄 수. 넘으면 "Show N more" 줄로 묶는다.
const N_VISIBLE_TERMINALS = 8;

function empty() {
    return { folders: [], terminals: {} };
}

function normalizeKey(
    key,
) {
    return key.length > 1 ? key.replace(/\/+$/, '') : key;
}

/** 폴더가 없으면 목록 끝에 더한다. 끝 슬래시만 다른 같은 폴더는 한 줄로 본다. */
function ensureFolder(
    state,
    key,
) {
    const folderKey = normalizeKey(key);
    const exists = state.folders.some(folder => folder.key === folderKey);
    const folders = exists ? state.folders : [...state.folders, { key: folderKey, expanded: true }];
    return { ...state, folders };
}

/** 터미널을 한 폴더에 소속시킨다. 이미 다른 폴더에 있었으면 그곳에서 빠진다. */
function assignTerminal(
    state,
    id,
    key,
    name,
) {
    const withFolder = ensureFolder(state, key);
    return { ...withFolder, terminals: { ...withFolder.terminals, [id]: { folder: normalizeKey(key), name } } };
}

function renameTerminal(
    state,
    id,
    name,
) {
    const entry = state.terminals[id];
    return entry ? { ...state, terminals: { ...state.terminals, [id]: { ...entry, name } } } : state;
}

/** 닫힌 터미널의 소속만 지운다. 폴더는 사용자가 뺄 때까지 남는다. */
function forgetTerminal(
    state,
    id,
) {
    const terminals = { ...state.terminals };
    delete terminals[id];
    return { ...state, terminals };
}

function folderOf(
    state,
    id,
) {
    return state.terminals[id]?.folder ?? null;
}

/**
 * 파일 주소가 속한 작업 폴더. 여러 폴더가 겹치면(홈과 그 아래 레포) 가장 가까운(긴) 폴더다. 없으면 null.
 *
 * Examples:
 *   folders [~, ~/repo], 'file:///home/me/repo/README.md' → '~/repo'의 key
 *   folders [~/repo], 'file:///tmp/x' → null
 */
function folderContaining(
    state,
    uri,
) {
    let found = null;
    for (const { key } of state.folders) {
        if (uri.startsWith(`${key}/`) && (!found || key.length > found.length)) found = key;
    }
    return found;
}

function terminalsOf(
    state,
    key,
) {
    const folderKey = normalizeKey(key);
    return Object.keys(state.terminals).filter(id => state.terminals[id].folder === folderKey);
}

function setExpanded(
    state,
    key,
    expanded,
) {
    const folderKey = normalizeKey(key);
    return { ...state, folders: state.folders.map(folder => (folder.key === folderKey ? { ...folder, expanded } : folder)) };
}

/** 목록에 보일 이름을 바꾼다. 공백뿐이면 이름을 지워 폴더 이름으로 돌아간다. */
function setFolderLabel(
    state,
    key,
    label,
) {
    const folderKey = normalizeKey(key);
    const text = String(label ?? '').trim();
    return {
        ...state,
        folders: state.folders.map((folder) => {
            let next = folder;
            if (folder.key === folderKey) {
                next = { ...folder };
                if (text) next.label = text;
                else delete next.label;
            }
            return next;
        }),
    };
}

function folderLabel(
    state,
    key,
) {
    const folderKey = normalizeKey(key);
    return state.folders.find(folder => folder.key === folderKey)?.label ?? null;
}

/**
 * 에이전트가 실행 중인 추가 터미널을 그 터미널 현재 폴더의 작업 터미널로 올린다.
 *
 * 에이전트(claude·codex 등)를 띄운 곳이 곧 작업할 곳이라, 사용자가 폴더를 따로 등록하지 않아도 목록에 선다.
 * 작업 폴더는 이 함수로만 새로 생긴다. 이미 작업 폴더에 속한 터미널은 다른 폴더에서 에이전트를 실행해도
 * 옮기지 않는다 — 한 번 든 터미널은 사용자가 닫거나 폴더를 뺄 때까지 제자리에 있고, 에이전트가 끝나도 남는다.
 * 현재 폴더를 아직 모르는 터미널은 올리지 않는다(호출자가 다음 주기에 다시 준다).
 *
 * 입력 `terminals`: `[{ id, cwd, program, isAgent }]`. `cwd`는 폴더 file URI, `program`은 앞쪽 프로그램 이름.
 * 출력: `{ state, promoted, added }` — 올린 터미널 id, 새로 생긴 폴더 key. 올린 것이 없으면 받은 상태 그대로다.
 *
 * Examples:
 *   빈 상태, [{ id: 't1', cwd: 'file:///r/', program: 'claude', isAgent: true }]
 *     → t1이 'file:///r'에 이름 'claude'로, added ['file:///r']
 *   'file:///a'에 속한 t0, [{ id: 't0', cwd: 'file:///b', isAgent: true, … }] → promoted []
 *   [{ id: 's', cwd: 'file:///a', program: 'bash', isAgent: false }]           → promoted []
 */
function promoteAgentTerminals(
    state,
    terminals,
) {
    let result = state;
    const promoted = [];
    const added = [];
    for (const { id, cwd, program, isAgent } of terminals) {
        if (isAgent && cwd && !folderOf(result, id)) {
            const key = normalizeKey(cwd);
            if (!result.folders.some(folder => folder.key === key)) added.push(key);
            result = assignTerminal(result, id, key, program);
            promoted.push(id);
        }
    }
    return { state: result, promoted, added };
}

/**
 * 한 폴더의 터미널 행. 목록 순서대로 이름을 쓰고, 같은 이름은 두 번째부터 `suffix`에 ·2·3을 붙인다.
 * 사이드바 Work 보기와 작업 폴더 탭 줄이 같은 이름표를 쓰도록 둘 다 이 행을 쓴다.
 *
 * Examples:
 *   이름 [claude, terminal, claude] → suffix ['', '', '·2']
 */
function terminalRows(
    state,
    key,
) {
    const folderKey = normalizeKey(key);
    const n_seen = new Map();
    return terminalsOf(state, folderKey).map((id) => {
        const name = state.terminals[id].name;
        const n_same = (n_seen.get(name) ?? 0) + 1;
        n_seen.set(name, n_same);
        return { kind: 'terminal', id, folder: folderKey, name, suffix: n_same > 1 ? `·${n_same}` : '' };
    });
}

/**
 * 같은 탭 줄에서 옮겨 갈 터미널. 단축키(Ctrl+방향키)로 탭을 오갈 때 쓴다.
 *
 * `target`은 `previous`·`next`(끝에서 반대쪽 끝으로 돈다), `first`·`last`다.
 * 현재 터미널이 줄에 없거나 줄이 비면 null이다.
 *
 * Examples:
 *   ['a', 'b', 'c'], 'a', 'previous' → 'c'
 *   ['a', 'b', 'c'], 'b', 'last'     → 'c'
 *   ['a', 'b'],      'x', 'next'     → null
 */
function tabTarget(
    ids,
    currentId,
    target,
) {
    const index = ids.indexOf(currentId);
    const n_tabs = ids.length;
    const positions = { previous: (index - 1 + n_tabs) % n_tabs, next: (index + 1) % n_tabs, first: 0, last: n_tabs - 1 };
    return index >= 0 ? ids[positions[target]] ?? null : null;
}

/**
 * 폴더를 목록에서 뺀다. 디스크의 폴더는 건드리지 않는다.
 *
 * `closed`는 호출자가 닫을 터미널 id, `next`는 현재 터미널이 뺀 폴더에 있었을 때 옮겨 갈 터미널이다.
 * 다음 선택은 바로 위 폴더의 첫 터미널, 없으면 아래 폴더의 첫 터미널, 그것도 없으면 `null`이다.
 * 현재 터미널이 다른 폴더에 있으면 `next`는 그 터미널 그대로다.
 */
function removeFolder(
    state,
    key,
    currentId,
) {
    const folderKey = normalizeKey(key);
    const index = state.folders.findIndex(folder => folder.key === folderKey);
    const closed = terminalsOf(state, folderKey);
    let next = currentId ?? null;
    if (closed.includes(currentId)) {
        const above = state.folders.slice(0, Math.max(index, 0)).reverse();
        const below = state.folders.slice(index + 1);
        next = null;
        for (const folder of [...above, ...below]) {
            const first = terminalsOf(state, folder.key)[0];
            if (next === null && first) {
                next = first;
            }
        }
    }
    let result = { ...state, folders: state.folders.filter(folder => folder.key !== folderKey) };
    for (const id of closed) {
        result = forgetTerminal(result, id);
    }
    return { state: result, closed, next };
}

/**
 * 사이드바 Work 보기의 행 목록을 만든다.
 *
 * 행 종류: `folder`(폴더 줄), `terminal`(작업 터미널 줄, 같은 이름이면 `suffix`에 ·2·3), `more`(가려진 수).
 * 현재 터미널이 속한 폴더는 접혀 있어도 펼치고, 8개 한도 밖이어도 현재 터미널은 보인다.
 * `showAll`에 든 폴더는 한도 없이 모두 보인다.
 *
 * Examples:
 *   20개 폴더, 현재 없음 → terminal 8줄 + { kind: 'more', n_hidden: 12 }
 *   20개 폴더, 현재 't15' → terminal 9줄(t15 포함) + n_hidden 11
 */
function visibleRows(
    state,
    currentId,
    showAll,
) {
    const rows = [];
    const currentFolder = currentId ? folderOf(state, currentId) : null;
    for (const folder of state.folders) {
        const ids = terminalsOf(state, folder.key);
        const expanded = folder.expanded || folder.key === currentFolder;
        rows.push({ kind: 'folder', key: folder.key, expanded, n_terminals: ids.length, current: folder.key === currentFolder });
        if (expanded) {
            const named = terminalRows(state, folder.key);
            const limited = showAll.has(folder.key) ? named : named.filter((row, index) => index < N_VISIBLE_TERMINALS || row.id === currentId);
            rows.push(...limited);
            if (limited.length < named.length) {
                rows.push({ kind: 'more', folder: folder.key, n_hidden: named.length - limited.length });
            }
        }
    }
    return rows;
}

function serialize(
    state,
) {
    return JSON.stringify(state);
}

/**
 * 저장 문자열에서 상태를 되살린다. 지금 살아 있는 터미널(`liveIds`)만 소속을 되살리고,
 * 폴더 목록은 그대로 남긴다. 형식이 깨졌으면 빈 상태를 돌려준다.
 */
function restore(
    text,
    liveIds,
) {
    let parsed = null;
    try {
        parsed = JSON.parse(text);
    } catch {
        parsed = null;
    }
    let state = empty();
    const valid = parsed && Array.isArray(parsed.folders) && parsed.terminals && typeof parsed.terminals === 'object';
    if (valid) {
        for (const folder of parsed.folders) {
            if (typeof folder?.key === 'string') {
                state = setExpanded(ensureFolder(state, folder.key), folder.key, folder.expanded !== false);
                if (typeof folder.label === 'string') state = setFolderLabel(state, folder.key, folder.label);
            }
        }
        for (const [id, entry] of Object.entries(parsed.terminals)) {
            if (liveIds.has(id) && typeof entry?.folder === 'string') {
                state = assignTerminal(state, id, entry.folder, String(entry.name ?? ''));
            }
        }
    }
    return state;
}

/** 시스템 메모리 사용률(%)을 정수로 돌려준다. 전체 용량을 모르면 `null`. */
function memoryPercent(
    total,
    free,
) {
    return total > 0 ? Math.round(((total - free) / total) * 100) : null;
}

module.exports = {
    N_VISIBLE_TERMINALS,
    empty,
    ensureFolder,
    assignTerminal,
    renameTerminal,
    forgetTerminal,
    folderOf,
    folderContaining,
    terminalsOf,
    setExpanded,
    setFolderLabel,
    folderLabel,
    promoteAgentTerminals,
    terminalRows,
    tabTarget,
    removeFolder,
    visibleRows,
    serialize,
    restore,
    memoryPercent,
};
