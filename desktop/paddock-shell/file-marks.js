/**
 * 사이드바 파일 목록의 Git 변경 표시 — 바뀐 파일에 상태 글자(M·U·A 등)와 색을, 바뀐 파일을 품은 폴더에 색을 붙인다.
 * Source control 보기로 가지 않아도 무엇을 바꿨는지 파일 목록에서 바로 보이게 한다.
 */

/** 폴더 하나에 여러 변경이 섞이면 이 순서로 앞선 색을 쓴다(충돌이 가장 급하고, 새 파일이 가장 덜 급하다). */
const FOLDER_COLOR_RANK = Object.freeze([
    'gitDecoration.conflictingResourceForeground',
    'gitDecoration.modifiedResourceForeground',
    'gitDecoration.deletedResourceForeground',
    'gitDecoration.renamedResourceForeground',
    'gitDecoration.addedResourceForeground',
    'gitDecoration.untrackedResourceForeground',
]);

/**
 * 상태 글자별 색 이름. Git 확장이 넘기는 변경 정보에는 글자만 있고 색이 없어서 글자로 색을 정한다.
 * 글자 뜻: M 수정, U 추적 안 함(새 파일), A 추가, D 삭제, R 이름 바꿈, C·! 충돌.
 */
const LETTER_COLORS = Object.freeze({
    M: 'gitDecoration.modifiedResourceForeground',
    U: 'gitDecoration.untrackedResourceForeground',
    A: 'gitDecoration.addedResourceForeground',
    D: 'gitDecoration.deletedResourceForeground',
    R: 'gitDecoration.renamedResourceForeground',
    C: 'gitDecoration.conflictingResourceForeground',
    '!': 'gitDecoration.conflictingResourceForeground',
});

/** 색 변수가 없을 때(Git 확장이 아직 색을 등록하기 전) 쓸 색. 어두운 테마 기본값이다. */
const FALLBACK_COLORS = Object.freeze({
    'gitDecoration.modifiedResourceForeground': '#e2c08d',
    'gitDecoration.untrackedResourceForeground': '#73c991',
    'gitDecoration.addedResourceForeground': '#81b88b',
    'gitDecoration.deletedResourceForeground': '#c74e39',
    'gitDecoration.renamedResourceForeground': '#73c991',
    'gitDecoration.conflictingResourceForeground': '#e4676b',
});

/**
 * 변경 정보의 색을 색 이름으로 맞춘다. Theia는 색을 이미 화면 값(`var(--theia-gitDecoration-…)`)으로 넘기므로 이름으로 되돌린다.
 *
 * Examples
 * --------
 * | 입력                                                       | 결과                                         |
 * | ---------------------------------------------------------- | -------------------------------------------- |
 * | `var(--theia-gitDecoration-modifiedResourceForeground)`    | `gitDecoration.modifiedResourceForeground`   |
 * | `gitDecoration.modifiedResourceForeground`                 | 그대로                                       |
 * | `''`, 알 수 없는 값                                        | `''`                                         |
 */
function colorId(
    color,
) {
    const variable = /^var\(--theia-gitDecoration-(\w+)\)$/.exec(color || '');
    let id = '';
    if (variable) id = `gitDecoration.${variable[1]}`;
    else if (/^gitDecoration\.\w+$/.test(color || '')) id = color;
    return id;
}

/**
 * 변경 목록에서 파일·폴더 표시를 찾는 함수 둘을 만든다.
 *
 * 입력 `changes`: `[{ uri, letter, color, tooltip? }]`. `uri`는 끝에 `/`가 없는 파일 URI 문자열, `color`는 색 이름(예: `gitDecoration.modifiedResourceForeground`)이다.
 * 같은 파일이 두 번 오면(스테이징 전후 등) 먼저 온 것을 쓴다. 색은 이름이나 화면 값 어느 쪽이든 받고(`colorId`), 없으면 상태 글자로 정한다(`LETTER_COLORS`).
 * 반환: `{ file(uri) → { letter, color, tooltip } | null, folder(uri) → color | null }`. 폴더는 그 아래(하위 폴더 포함)에 변경이 있을 때만 색을 돌려준다.
 *
 * Examples
 * --------
 * | changes                                   | 질문                   | 결과                         |
 * | ----------------------------------------- | ---------------------- | ---------------------------- |
 * | `[{uri: 'file:///r/src/a.js', letter: 'M', color: M}]` | `file('file:///r/src/a.js')` | `{ letter: 'M', color: M, tooltip: '' }` |
 * | 위와 같음                                 | `folder('file:///r/src')` | `M`                        |
 * | 위와 같음                                 | `folder('file:///r/s')` | `null` (이름 앞부분만 같은 폴더) |
 * | `[]`                                      | `file(…)`, `folder(…)` | `null`                       |
 */
function fileMarks(
    changes,
) {
    const files = new Map();
    for (const change of changes) {
        if (!files.has(change.uri)) files.set(change.uri, { letter: change.letter || '', color: colorId(change.color) || LETTER_COLORS[change.letter] || '', tooltip: change.tooltip || '' });
    }
    const rank = color => {
        const index = FOLDER_COLOR_RANK.indexOf(color);
        return index < 0 ? FOLDER_COLOR_RANK.length : index;
    };
    const folderColors = new Map();
    for (const [uri, mark] of files) {
        // 파일의 모든 조상 폴더에 색을 올린다. 더 급한 색이 이미 있으면 그대로 둔다.
        let end = uri.lastIndexOf('/');
        while (end > 'file://'.length) {
            const folder = uri.slice(0, end);
            const previous = folderColors.get(folder);
            if (mark.color && (previous === undefined || rank(mark.color) < rank(previous))) folderColors.set(folder, mark.color);
            end = folder.lastIndexOf('/');
        }
    }
    const trim = uri => uri.replace(/\/+$/, '');
    return {
        file: uri => files.get(trim(uri)) ?? null,
        folder: uri => folderColors.get(trim(uri)) ?? null,
    };
}

/** 색 이름을 화면 색 변수로 바꾼다. 예: `gitDecoration.modifiedResourceForeground` → `var(--theia-gitDecoration-modifiedResourceForeground, #e2c08d)`. */
function colorVariable(
    color,
) {
    const fallback = FALLBACK_COLORS[color];
    return color ? `var(--theia-${color.replace(/\./g, '-')}${fallback ? `, ${fallback}` : ''})` : '';
}

module.exports = { FOLDER_COLOR_RANK, LETTER_COLORS, colorId, fileMarks, colorVariable };
