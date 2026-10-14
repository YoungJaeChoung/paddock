// 파일 검색이 훑을 폴더를 정한다. Paddock은 Theia의 작업 공간을 열지 않고 쓰므로(작업 공간 없음), 검색 엔진이 받는 폴더 목록을 Paddock이 직접 댄다.
// 판정 규칙만 두어 Theia 없이 시험한다.

/**
 * 검색할 폴더 주소 목록을 돌려준다.
 *
 * Theia 작업 공간이 열려 있으면 그 루트를 그대로 쓴다. 아니면 사이드바 파일 영역이 보이는 폴더, 그것도 없으면 선택한 터미널의 현재 폴더 하나다.
 * 어느 것도 없으면 빈 목록이다(검색 명령을 쓸 수 없다).
 *
 * Examples:
 *   workspaceRoots  filesFolder  topTerminalCwd  → 폴더 목록
 *   [a, b]          f            c               → [a, b]
 *   []              f            c               → [f]
 *   []              null         c               → [c]
 *   []              null         null            → []
 */
function searchRoots({
    workspaceRoots,
    filesFolder,
    topTerminalCwd,
}) {
    let roots = [];
    if (workspaceRoots.length > 0) roots = [...workspaceRoots];
    else if (filesFolder) roots = [filesFolder];
    else if (topTerminalCwd) roots = [topTerminalCwd];
    return roots;
}

module.exports = { searchRoots };
