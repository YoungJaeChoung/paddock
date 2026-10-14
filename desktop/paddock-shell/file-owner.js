// 파일 주소가 있는 화면이 어느 묶음에 속하는지 정한다. 작업 폴더가 품지 않는 파일은 지금 보는 위쪽 터미널 묶음에 속한다.
// 화면 열기 경로(Paddock 열기·Theia 열기·Git 변경 열기·확장 명령)와 무관하게 같은 기준을 쓰도록 순수 규칙만 둔다.
const { folderContaining } = require('./work-model');

/**
 * 새로 들어온 파일 화면이 속할 위쪽 터미널 묶음의 시작 터미널 id를 돌려준다.
 *
 * 작업 폴더가 파일을 품으면 그 폴더가 소속을 맡으므로 묶음이 필요 없다. 이미 묶음이 있는 화면은 그대로 둔다.
 * 파일 주소가 없거나 묶음으로 삼을 터미널이 없으면 null이다.
 * 위쪽 줄은 이 소속이 있어야 파일 화면을 탭으로 올리므로, 소속이 없으면 이름도 닫기도 없는 화면이 된다.
 *
 * Examples:
 *   folders [~/repo]
 *   uri 'file:///tmp/x.ts', 지금 화면의 묶음 'terminal-1'                → 'terminal-1'
 *   uri 'file:///tmp/x.ts', 지금 화면에 묶음 없음, 보이는 위쪽 터미널 't2' → 't2'
 *   uri 'file:///home/me/repo/a.ts' (작업 폴더 안)                        → null
 *   uri 'file:///tmp/x.ts', 이미 묶음 있음(hasRoot)                       → null
 *   uri null (파일이 아닌 화면)                                            → null
 */
function fileRootFor({
    state,
    uri,
    hasRoot,
    currentRoot,
    topTerminal,
}) {
    let root = null;
    if (typeof uri === 'string' && uri && !hasRoot && !folderContaining(state, uri)) {
        root = currentRoot || topTerminal || null;
    }
    return root;
}

module.exports = { fileRootFor };
