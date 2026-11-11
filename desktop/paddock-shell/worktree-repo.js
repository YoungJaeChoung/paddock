/**
 * 작업 폴더가 어느 git 저장소의 워크트리인지 `.git` 한 항목만 보고 정하는 순수 계산.
 *
 * Work 목록이 같은 저장소의 워크트리 폴더를 한 묶음으로 보이려면 폴더마다 "같은 저장소"를 가리키는 키가 필요하다.
 * git 프로세스를 띄우지 않고 `.git`이 디렉터리인지, 파일이면 `gitdir:` 한 줄이 무엇을 가리키는지로 판정한다.
 * 폴더 키는 폴더의 file URI 문자열이고(`work-model.js`), 결과 키도 같은 형식이라 주 워크트리 폴더의 키와 그대로 같아진다.
 */

// `<저장소 루트>/.git/worktrees/<이름>`. 루트 쪽을 뽑는다. 서브모듈(`.git/modules/…`)과 그 밖의 모양은 맞지 않는다.
const LINKED_WORKTREE = /^(.*)\/\.git\/worktrees\/[^/]+\/?$/;

// 드라이브 문자로 시작하는 Windows 경로.
const DRIVE_PATH = /^[A-Za-z]:\//;

// WSL 폴더 키의 서버 이름. 그 뒤 첫 경로 조각이 배포판 이름이다.
const WSL_HOSTS = ['wsl.localhost', 'wsl$'];

function encodePath(
    path,
) {
    // file URI 경로 인코딩. encodeURIComponent가 남기는 !'()*도 인코딩해 폴더 키(`URI.toString()`)와 같은 모양으로 맞춘다.
    return path.split('/').map(part => encodeURIComponent(part).replace(/[!'()*]/g, char => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)).join('/');
}

/** 폴더 키를 `{ host, path }`로 나눈다. path는 디코딩한 `/`로 시작하는 경로이고 file URI가 아니면 null이다. */
function splitKey(
    folderKey,
) {
    const match = /^file:\/\/([^/]*)(\/.*)$/.exec(folderKey);
    let result = null;
    if (match) {
        try {
            result = { host: match[1], path: match[2].split('/').map(part => decodeURIComponent(part)).join('/') };
        } catch {
            result = null;
        }
    }
    return result;
}

/** 기준 경로(`/`로 시작)에서 상대 경로를 풀어 `.`·`..`을 정리한 절대 경로로 만든다. */
function resolveRelative(
    base,
    relative,
) {
    const parts = [];
    for (const part of `${base}/${relative}`.split('/')) {
        if (part === '..') parts.pop();
        else if (part !== '' && part !== '.') parts.push(part);
    }
    return `/${parts.join('/')}`;
}

/**
 * 폴더의 `.git` 읽기 결과로 저장소 키를 정한다. 같은 저장소의 폴더는 같은 키를 얻어 한 묶음이 된다.
 * 묶지 않는 경우(서브모듈·`.git` 없음·읽기 실패·알 수 없는 모양)는 null이다.
 *
 * `dotGit`은 `{ directory: true }`(디렉터리, 이 폴더가 주 워크트리), `{ content }`(파일의 앞부분 글),
 * `{ none: true }`(`.git`이 없음), null(읽지 못함)이다. WSL 폴더(서버 `wsl.localhost`)의 gitdir는 Linux 경로라
 * 폴더 키의 `wsl.localhost/<배포판>` 접두를 붙여 같은 형식으로 맞춘다.
 *
 * Examples:
 *   ('file:///r/main', { directory: true })                                   → 'file:///r/main'
 *   ('file:///r/wt', { content: 'gitdir: /r/main/.git/worktrees/wt' })         → 'file:///r/main'
 *   ('file:///r/wt', { content: 'gitdir: ../main/.git/worktrees/wt' })         → 'file:///r/main'
 *   ('file://wsl.localhost/Ubuntu/h/wt', { content: 'gitdir: /h/main/.git/worktrees/wt' }) → 'file://wsl.localhost/Ubuntu/h/main'
 *   ('file:///r/sub', { content: 'gitdir: ../.git/modules/sub' })              → null
 *   ('file:///r/x', null)                                                      → null
 */
function repoKeyOf(
    folderKey,
    dotGit,
) {
    const folder = folderKey.length > 1 ? folderKey.replace(/\/+$/, '') : folderKey;
    let key = null;
    if (dotGit?.directory === true) {
        key = folder;
    } else if (typeof dotGit?.content === 'string') {
        const parts = splitKey(folder);
        const line = /^gitdir:[ \t]*(.+?)[ \t]*$/m.exec(dotGit.content.replace(/\r/g, ''));
        const target = line ? line[1].replace(/\\/g, '/') : null;
        const root = target ? LINKED_WORKTREE.exec(target)?.[1] : null;
        if (parts && root) {
            // Theia는 `\\wsl$`의 `$`를 `%24`로 인코딩해 서버 이름이 `wsl%24`로 온다.
            const isWsl = WSL_HOSTS.includes(decodeURIComponent(parts.host).toLowerCase());
            // Windows 드라이브 폴더(`/c:/…`)에서 WSL의 git이 만든 워크트리는 `/mnt/<문자>/…`로 적혀 있어 드라이브 경로로 되돌린다.
            const mount = parts.host === '' && /^\/[A-Za-z]:\//.test(parts.path) ? /^\/mnt\/([A-Za-z])(\/.*)$/.exec(root) : null;
            if (DRIVE_PATH.test(`${root}/`)) {
                // Windows 경로. 드라이브 문자는 폴더 키처럼 소문자로 쓴다.
                key = `file:///${encodePath(`${root[0].toLowerCase()}${root.slice(1)}`)}`;
            } else if (mount) {
                key = `file:///${encodePath(`${mount[1].toLowerCase()}:${mount[2]}`)}`;
            } else if (root.startsWith('/')) {
                // Linux 경로. WSL이면 폴더 키의 `<서버>/<배포판>` 접두를 붙인다.
                const distribution = isWsl ? parts.path.split('/')[1] : null;
                key = isWsl ? (distribution ? `file://${parts.host}/${encodePath(distribution)}${encodePath(root)}` : null) : `file://${parts.host}${encodePath(root)}`;
            } else {
                // 상대 경로. 폴더 경로(WSL이면 배포판 이름 포함) 안에서 푼다.
                key = `file://${parts.host}${encodePath(resolveRelative(parts.path, root))}`;
            }
        }
    }
    return key;
}

module.exports = {
    repoKeyOf,
};
