// 본문(main area)에 열리는 화면 종류를 전수로 열어, Paddock의 위쪽 탭 줄·이름 줄 규칙에 맞는지 점검한다.
// 한 칸 상태와 오른쪽에 터미널 칸을 더 나눈 두 칸 상태에서 화면마다 영역·탭·이름 줄·× 닫기·칸 이동·로그·캡처를 확인한다.
//
// 단독 실행: node scripts/screen-sweep.cjs [--port=9411] [--workspace]
// 임시 HOME·임시 사용자 데이터 폴더로 앱을 새로 띄우고, 끝나면 이 스크립트가 띄운 프로세스만 종료한다.
// 기본은 작업 공간(Theia workspace)을 열지 않고 띄운다(사용자의 일상 실행). `--workspace`를 주면 임시 폴더를 작업 공간으로 열어 띄운다.
// 어느 모드든 점검용 파일은 임시 폴더에 있고 OpenerService로 그 폴더의 파일을 직접 연다.
// 검색은 본문 화면이 아니라 사이드바 파일 영역 안의 위젯이라 따로 점검한다(`sweepSearch`): 버튼 → 검색어 → 결과 → 결과 클릭 → × → Ctrl+Shift+F.
// 결과: captures/screen-sweep/<날짜시각>/report.md + 화면별 PNG
// 회귀 확인 순서: npm test → npm run build → PADDOCK_CDP_PORT=<포트> npm run smoke:shell → npm run sweep:screens (기본, 작업 공간 없음) → npm run sweep:screens -- --workspace
// 두 모드 모두 '실패 칸 0'(설정의 두 칸 칸 이동은 설계상 예외)이어야 한다. 사용자가 쓰는 앱이 떠 있으면 다른 --port를 쓴다.
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const desktopDirectory = path.join(__dirname, '..');
// Pencil 확장(웹뷰 기반 확장 페이지 점검용)과 마크다운 확장. 읽기만 하고 임시 HOME으로 복사한다.
const PLUGIN_SOURCES = [
    '/mnt/c/Users/young/.paddock/config/deployedPlugins/highagency.pencildev@0.6.75',
    // 마크다운 미리보기 점검용(Paddock의 미리보기 버튼이 이 확장의 명령을 쓴다).
    '/mnt/c/Users/young/.paddock/config/deployedPlugins/shd101wyy.markdown-preview-enhanced@0.8.40',
];
const PENCIL_EXTENSION_ID = 'highagency.pencildev';
// 화면을 연 뒤 탭 줄이 다시 그려지길 기다리는 시간(ms).
const SETTLE_MS = 1500;
const OPEN_TIMEOUT_MS = 12000;

/** 화면 점검 상태: 한 칸, 오른쪽에 터미널 칸을 더 나눈 두 칸. */
const STATE = { SINGLE: 'single', SPLIT: 'split' };

/**
 * 점검할 화면 목록. `match`는 위젯 w를 받아 이 화면이면 참인 식(페이지 안에서 실행),
 * `open`은 화면을 여는 식(페이지 안에서 실행, 확장 페이지처럼 끝나지 않는 명령은 기다리지 않는다).
 * `skip`이 있으면 열지 않고 그 이유를 표에 남긴다. `needsWorkspace`는 열린 작업 공간이 있어야 열리는 화면이다.
 */
const SCREENS = [
    {
        name: '파일 편집기(.ts)',
        slug: 'file-ts',
        match: "w.getResourceUri?.()?.path.base === 'b.ts' && w.getResourceUri().scheme === 'file'",
        open: "openFile('b.ts')",
    },
    {
        name: '마크다운 미리보기',
        slug: 'markdown-preview',
        match: "w.title?.label === 'Preview a.md'",
        open: "(async () => { await openFile('a.md'); await svc('PaddockWorkspace').markdownPreview.setMarkdownView('preview'); })()",
        cleanup: "closeByMatch(\"w.getResourceUri?.()?.path.base === 'a.md'\")",
    },
    {
        name: 'git diff 편집기',
        slug: 'git-diff',
        match: "w.id?.startsWith('code-editor-opener:diff:')",
        // Git 패널에서 변경 항목을 누르는 것과 같은 열기(resource.open). 끝나지 않는 promise라 기다리지 않는다.
        open: "(() => { svc('ScmService').selectedRepository.provider.groups.find((group) => group.resources.length).resources[0].open(); return true; })()",
        setup: 'modifyTracked',
    },
    {
        name: '설정(preferences)',
        slug: 'preferences',
        match: "w.id === 'settings_widget'",
        open: "svc('CommandRegistry').executeCommand('preferences:open')",
    },
    {
        name: '키보드 단축키',
        slug: 'keyboard-shortcuts',
        skip: '제공 안 함: 키 설정 화면을 앱에 넣지 않기로 함(keymaps 확장이 앱에 없음, 2026-10-14 결정)',
    },
    {
        name: '확장 페이지(extension.open)',
        slug: 'extension-page',
        match: "w.id?.startsWith('vsx-extension-editor')",
        // 업데이트 알림을 기다리느라 promise가 끝나지 않으므로 기다리지 않는다.
        open: `(() => { svc('CommandRegistry').executeCommand('extension.open', ${JSON.stringify(PENCIL_EXTENSION_ID)}); return true; })()`,
    },
    {
        name: '확장 목록(Extensions)',
        slug: 'extensions-list',
        match: "w.id === 'vsx-extensions-view-container'",
        open: "svc('CommandRegistry').executeCommand('vsxExtensions.toggle')",
    },
    {
        name: '출력(Output)',
        slug: 'output',
        match: "w.id === 'outputView'",
        open: "svc('CommandRegistry').executeCommand('output:toggle')",
    },
    {
        name: '문제(Problems)',
        slug: 'problems',
        match: "w.id === 'problems'",
        open: "svc('CommandRegistry').executeCommand('problemsView:toggle')",
    },
    {
        name: '이미지 미리보기(.png)',
        slug: 'image-png',
        match: "w.getResourceUri?.()?.path.base === 'dot.png'",
        open: "openFile('dot.png')",
    },
    {
        name: 'PDF 보기(.pdf)',
        slug: 'pdf',
        match: "w.getResourceUri?.()?.path.base === 'doc.pdf' || w.title?.label === 'doc.pdf'",
        open: "openFile('doc.pdf')",
    },
    {
        name: 'Pencil 편집기(.pen)',
        slug: 'pen-file',
        match: "w.getResourceUri?.()?.path.base === 'design.pen' || w.title?.label === 'design.pen'",
        open: "openFile('design.pen')",
    },
    {
        name: '노트북(.ipynb)',
        slug: 'notebook',
        match: "w.getResourceUri?.()?.path.base === 'n.ipynb' || w.title?.label === 'n.ipynb'",
        open: "openFile('n.ipynb')",
    },
    {
        name: 'Merge editor',
        slug: 'merge-editor',
        skip: '시도 안 함: 충돌 중인 저장소와 3방향 인자(_open.mergeEditor)가 필요해 쉬운 열기 방법이 없음',
    },
    {
        name: '테스트 결과(test-result-widget)',
        slug: 'test-result',
        skip: '시도 안 함: 테스트 제공자(확장)가 없어 열 결과가 없음',
    },
];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** `--port=9411` 형태 인자에서 포트를 읽는다. 없으면 9411. */
function parsePort(
    argv,
) {
    const found = argv.find((arg) => arg.startsWith('--port='));
    return found ? Number(found.slice('--port='.length)) : 9411;
}

/** `--workspace` 인자가 있으면 임시 폴더를 작업 공간으로 열어 띄우는 모드다. 없으면 작업 공간 없이 띄운다. */
function parseWorkspaceMode(
    argv,
) {
    return argv.includes('--workspace');
}

/** 지금 시각을 폴더 이름용 `YYYYMMDD-HHMMSS`로 만든다. */
function timestamp() {
    const now = new Date();
    const pad = (value) => String(value).padStart(2, '0');
    return `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
}

/** 임시 HOME·작업 폴더·사용자 데이터 폴더를 만들고 점검용 파일을 채운다. */
async function prepareSandbox() {
    const root = await fsp.mkdtemp(path.join(os.tmpdir(), 'paddock-sweep-'));
    const home = path.join(root, 'home');
    const work = path.join(root, 'work');
    const userData = path.join(root, 'user-data');
    const plugins = path.join(home, '.paddock', 'config', 'deployedPlugins');
    await fsp.mkdir(plugins, { recursive: true });
    await fsp.mkdir(work, { recursive: true });
    await fsp.mkdir(userData, { recursive: true });
    for (const source of PLUGIN_SOURCES) await fsp.cp(source, path.join(plugins, path.basename(source)), { recursive: true });
    await fsp.writeFile(path.join(work, 'a.md'), '# Sweep\n\n본문 화면 점검용 문서.\n');
    await fsp.writeFile(path.join(work, 'b.ts'), 'export const value: number = 1;\n');
    // 1x1 PNG, 최소 PDF, 빈 .pen, 빈 노트북.
    await fsp.writeFile(path.join(work, 'dot.png'), Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64'));
    await fsp.writeFile(path.join(work, 'doc.pdf'), '%PDF-1.1\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n');
    await fsp.writeFile(path.join(work, 'design.pen'), JSON.stringify({ version: '2.6', children: [] }));
    await fsp.writeFile(path.join(work, 'n.ipynb'), JSON.stringify({ cells: [], metadata: {}, nbformat: 4, nbformat_minor: 5 }));
    const git = (...args) => new Promise((resolve, reject) => {
        const child = spawn('git', ['-c', 'user.email=sweep@example.com', '-c', 'user.name=sweep', ...args], { cwd: work, stdio: 'ignore' });
        child.on('exit', (code) => (code === 0 ? resolve() : reject(new Error(`git ${args.join(' ')} 실패(${code})`))));
    });
    await git('init', '-q');
    await git('add', '.');
    await git('commit', '-q', '-m', 'init');
    return { root, home, work, userData };
}

/** 앱을 자기 프로세스 그룹으로 띄운다. 그룹 번호로만 종료하므로 다른 Paddock에 영향이 없다. */
function launchApp(
    sandbox,
    port,
    withWorkspace,
) {
    const stdoutPath = path.join(sandbox.root, 'stdout.log');
    const logFd = fs.openSync(stdoutPath, 'a');
    const child = spawn(process.execPath, [
        path.join(__dirname, 'start.cjs'),
        ...(withWorkspace ? [sandbox.work] : []),
        `--remote-debugging-port=${port}`,
        `--user-data-dir=${sandbox.userData}`,
    ], {
        cwd: desktopDirectory,
        detached: true,
        stdio: ['ignore', logFd, logFd],
        env: { ...process.env, HOME: sandbox.home },
    });
    fs.closeSync(logFd);
    return { child, stdoutPath };
}

/** 이 스크립트가 띄운 프로세스만 끝낸다: 앱 그룹, 앱과 분리돼 남는 백엔드, 임시 HOME을 환경에 가진 남은 프로세스. */
async function stopApp(
    child,
    sandbox,
) {
    const alive = (pid) => {
        let result = true;
        try {
            process.kill(pid, 0);
        } catch {
            result = false;
        }
        return result;
    };
    const send = (pid, signal) => {
        try {
            process.kill(pid, signal);
        } catch {
            // 이미 끝난 프로세스다.
        }
    };
    if (child?.pid) send(-child.pid, 'SIGTERM');
    // 앱은 백엔드를 분리해 띄우므로 접속 정보에 적힌 pid를 따로 끝낸다(임시 설정 폴더의 것이라 이 실행이 띄운 백엔드다).
    let backendPid = null;
    try {
        backendPid = JSON.parse(fs.readFileSync(path.join(sandbox.home, '.paddock', 'config', 'backend.json'), 'utf8')).pid;
    } catch {
        // 접속 정보가 없으면 백엔드가 뜨지 않은 것이다.
    }
    if (backendPid) send(backendPid, 'SIGTERM');
    await sleep(2500);
    if (child?.pid && alive(child.pid)) send(-child.pid, 'SIGKILL');
    if (backendPid && alive(backendPid)) send(backendPid, 'SIGKILL');
    // 임시 HOME을 환경에 가진 프로세스(플러그인 호스트 등)가 남았으면 그 pid만 끝낸다.
    for (const entry of fs.readdirSync('/proc').filter((name) => /^\d+$/.test(name))) {
        const pid = Number(entry);
        let environ = '';
        try {
            environ = fs.readFileSync(`/proc/${entry}/environ`, 'utf8');
        } catch {
            environ = '';
        }
        if (pid !== process.pid && environ.includes(`HOME=${sandbox.home}\0`)) send(pid, 'SIGKILL');
    }
}

/** 페이지 안에서 쓰는 도우미. 한 번 주입하면 window.__sweep 에서 서비스·위젯을 다룬다. */
const PAGE_HELPERS = `(() => {
    const container = window.theia.container;
    const svc = (name) => container.get([...container._bindingDictionary._map.keys()].find((key) => key && (key.name === name || key.description === name)));
    const shell = () => svc('ApplicationShell');
    // 점검용 파일이 있는 임시 폴더의 주소. 작업 공간 없는 모드에서는 열린 작업 공간이 없으므로 앱 안의 주소 객체에서 주소 클래스를 찾아 직접 만든다.
    const findUriClass = () => {
        const seen = new Set();
        let found = null;
        const walk = (value, depth) => {
            if (!found && value && typeof value === 'object' && !seen.has(value) && depth <= 3) {
                seen.add(value);
                if ('codeUri' in value && typeof value.resolve === 'function') found = value.constructor;
                else for (const key of Object.keys(value).slice(0, 60)) { try { walk(value[key], depth + 1); } catch {} }
            }
        };
        // 서비스를 새로 만들지 않도록 이미 만들어진 것(싱글톤 캐시)만 훑는다.
        for (const bindings of container._bindingDictionary._map.values()) {
            for (const binding of bindings) { if (binding.activated) walk(binding.cache, 0); }
            if (found) break;
        }
        return found;
    };
    const root = () => {
        const roots = svc('WorkspaceService').tryGetRoots();
        return roots.length ? roots[0].resource : new (findUriClass())('file://' + window.__sweepWorkPath);
    };
    const openFile = async (name) => {
        const uri = root().resolve(name);
        const opener = await svc('OpenerService').getOpener(uri);
        return opener.open(uri);
    };
    const find = (expression) => {
        const matcher = new Function('w', 'return (' + expression + ')');
        return [...shell().widgets].find((w) => { try { return matcher(w); } catch { return false; } });
    };
    const closeByMatch = async (expression) => {
        const widget = find(expression);
        if (widget) await shell().closeWidget(widget.id);
        return true;
    };
    const isGone = (widget) => widget.isDisposed || !widget.isAttached;
    // 화면의 탭은 그 화면을 품은 쪽(검색은 보기 컨테이너)의 탭이므로, 화면 노드를 품은 소유자의 탭을 찾는다.
    const realTabOf = (widget) => {
        const owner = shell().getTabBarFor(widget)?.titles.find((title) => title.owner === widget || title.owner.node.contains(widget.node))?.owner;
        return owner ? document.getElementById('shell-tab-' + owner.id) : null;
    };
    const rectOf = (node) => { const r = node.getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height }; };
    /** 찾은 위젯의 영역·보임·위쪽 탭·이름 줄을 읽는다. */
    const inspect = (expression) => {
        const widget = find(expression);
        let result = { found: false };
        if (widget) {
            const bar = shell().getTabBarFor(widget);
            const rect = rectOf(widget.node);
            const tab = document.querySelector('.folder-tabs [role="tab"][data-widget-id="' + widget.id + '"]');
            const pathBar = bar?.node.querySelector(':scope > .path-bar');
            // 칸을 나눈 뒤 위쪽 줄에 없는 화면은 그 칸이 실제 탭 줄(shows-tabs)을 보여 이름과 ×를 맡는다.
            const realTab = bar?.node.classList.contains('shows-tabs') ? realTabOf(widget) : null;
            result = {
                found: true,
                id: widget.id,
                label: widget.title.label,
                kind: widget.constructor.name,
                area: shell().getAreaFor(widget) || null,
                inSidebar: Boolean(widget.node.closest('[data-host]')),
                visible: Boolean(widget.isVisible && rect.width > 0 && rect.height > 0),
                rect,
                tab: tab ? { selected: tab.getAttribute('aria-selected') === 'true', label: tab.textContent } : null,
                realTab: realTab ? { close: Boolean(realTab.querySelector('[class*="tabCloseIcon"]')), text: realTab.textContent } : null,
                pathBar: pathBar ? { height: Math.round(pathBar.getBoundingClientRect().height), close: Boolean(pathBar.querySelector('.path-action')), text: pathBar.textContent } : null,
            };
        }
        return result;
    };
    /** 위쪽 탭의 ×를 누른다. 없으면 이름 줄의 닫기를 누른다. 누른 곳을 돌려준다. */
    const clickClose = (expression) => {
        const widget = find(expression);
        let via = 'none';
        if (widget) {
            const tab = document.querySelector('.folder-tabs [role="tab"][data-widget-id="' + widget.id + '"]');
            const tabClose = tab?.closest('.tab')?.querySelector('.tab-close');
            const pathClose = shell().getTabBarFor(widget)?.node.querySelector(':scope > .path-bar .path-action');
            const realTab = shell().getTabBarFor(widget)?.node.classList.contains('shows-tabs') ? realTabOf(widget) : null;
            const realClose = realTab?.querySelector('[class*="tabCloseIcon"]');
            if (tabClose) { tabClose.click(); via = 'tab'; }
            else if (realClose) {
                // Lumino 탭 줄은 실제 마우스로 눌러야 닫으므로, 누를 좌표만 알려 주고 누르기는 호출자가 CDP로 한다.
                const r = realClose.getBoundingClientRect();
                window.__sweepPoint = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
                via = 'real-tab';
            }
            else if (pathClose) { pathClose.click(); via = 'path-bar'; }
        }
        return via;
    };
    /** 다른 칸의 위쪽 줄 빈 곳(kind=row)이나 본문 가운데(kind=body)를 가리키는 좌표를 만든다. */
    const dockPoint = (expression, kind) => {
        const widget = find(expression);
        const own = widget && shell().getTabBarFor(widget);
        const dest = [...shell().mainPanel.tabBars()].find((bar) => bar !== own && bar.currentTitle?.owner?.isVisible);
        let point = null;
        if (dest) {
            const body = dest.currentTitle.owner.node.getBoundingClientRect();
            if (kind === 'body') {
                point = { x: body.left + body.width / 2, y: body.top + body.height / 2 };
            } else {
                const centerX = body.left + body.width / 2;
                const group = [...document.querySelectorAll('.folder-tabs .folder-tab-group')].find((node) => { const r = node.getBoundingClientRect(); return centerX >= r.left && centerX < r.right; });
                const strip = group || document.querySelector('.folder-tabs');
                const r = strip.getBoundingClientRect();
                point = { x: r.right - 6, y: r.top + r.height / 2, grouped: Boolean(group) };
            }
        }
        return point;
    };
    /** 좌표로 tabDockTarget을 부르고, 실제 dockTab으로 옮겨 도착 칸이 바뀌었는지 돌려준다. */
    const dockTry = async (expression, kind) => {
        const widget = find(expression);
        const point = dockPoint(expression, kind);
        let result = { point: Boolean(point), target: null, moved: false };
        if (widget && point) {
            const workspace = svc('PaddockWorkspace');
            const target = workspace.folderTabs.tabDockTarget(widget.id, point.x, point.y);
            result.target = target ? { mode: target.mode, isInsert: Boolean(target.isInsert) } : null;
            const before = shell().getTabBarFor(widget);
            await workspace.folderTabs.dockTab(widget.id, point.x, point.y);
            result.moved = shell().getTabBarFor(widget) !== before;
            result.grouped = point.grouped;
        }
        return result;
    };
    window.__sweep = { svc, root, openFile, find, closeByMatch, inspect, clickClose, dockTry, isGone };
    return true;
})()`;

async function main() {
    const port = parsePort(process.argv.slice(2));
    const withWorkspace = parseWorkspaceMode(process.argv.slice(2));
    const busy = await fetch(`http://127.0.0.1:${port}/json/list`).then(() => true, () => false);
    if (busy) {
        throw new Error(`포트 ${port}에서 이미 응답하는 프로세스가 있습니다. 다른 포트로 --port=를 지정하세요(건드리지 않고 중단).`);
    }
    const runDirectory = path.join(desktopDirectory, 'captures', 'screen-sweep', timestamp());
    await fsp.mkdir(runDirectory, { recursive: true });
    const sandbox = await prepareSandbox();
    const { child, stdoutPath } = launchApp(sandbox, port, withWorkspace);
    const backendLogPath = path.join(sandbox.home, '.paddock', 'config', 'backend.log');
    let socket = null;
    let rows = [];
    let backgroundNote = '';
    try {
        // CDP 접속
        let target = null;
        for (let n_tries = 0; n_tries < 120 && !target; n_tries += 1) {
            const list = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json(), () => []);
            target = list.find((item) => item.type === 'page') || null;
            if (!target) await sleep(1000);
        }
        if (!target) throw new Error('앱의 디버그 포트가 열리지 않았습니다. stdout: ' + stdoutPath);
        socket = new WebSocket(target.webSocketDebuggerUrl);
        await new Promise((resolve) => socket.addEventListener('open', resolve, { once: true }));
        let n_messages = 0;
        const pending = new Map();
        const consoleErrors = [];
        socket.addEventListener('message', ({ data }) => {
            const message = JSON.parse(data);
            const entry = pending.get(message.id);
            if (entry) {
                clearTimeout(entry.timeout);
                pending.delete(message.id);
                if (message.error) entry.reject(new Error(message.error.message));
                else entry.resolve(message.result);
            } else if (message.method === 'Runtime.consoleAPICalled' && message.params.type === 'error') {
                consoleErrors.push(message.params.args.map((arg) => arg.value ?? arg.description ?? '').join(' '));
            } else if (message.method === 'Runtime.exceptionThrown') {
                const details = message.params.exceptionDetails;
                consoleErrors.push(`예외: ${details.exception?.description ?? details.text}`);
            } else if (message.method === 'Log.entryAdded' && message.params.entry.level === 'error') {
                consoleErrors.push(`${message.params.entry.source}: ${message.params.entry.text} ${message.params.entry.url ?? ''}`.trim());
            }
        });
        const send = (method, params = {}) => {
            n_messages += 1;
            const id = n_messages;
            return new Promise((resolve, reject) => {
                const timeout = setTimeout(() => {
                    pending.delete(id);
                    reject(new Error(`응답 시간 초과: ${method}`));
                }, 30000);
                pending.set(id, { resolve, reject, timeout });
                socket.send(JSON.stringify({ id, method, params }));
            });
        };
        const evaluate = async (expression) => {
            const response = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
            if (response.exceptionDetails) throw new Error(response.exceptionDetails.exception?.description ?? response.exceptionDetails.text);
            return response.result.value;
        };
        const until = async (expression, timeoutMs = 30000) => {
            let value = false;
            const startedAt = Date.now();
            while (!value && Date.now() - startedAt < timeoutMs) {
                value = await evaluate(expression).catch(() => false);
                if (!value) await sleep(200);
            }
            return value;
        };
        await send('Runtime.enable');
        await send('Log.enable');
        await send('Page.enable');
        // 다른 창이 OS 포커스를 가져가도 포커스 이벤트가 오게 한다.
        await send('Emulation.setFocusEmulationEnabled', { enabled: true });
        // 작업 공간 신뢰 대화상자는 작업 공간을 연 모드에서만 뜬다. "Yes, I trust"를 눌러 닫는다.
        const trusted = !withWorkspace || await until(`(() => { const button = [...document.querySelectorAll('.workspace-trust-dialog .theia-button')].find((node) => /Yes, I trust/.test(node.textContent)); if (button) { button.click(); return true; } return false; })()`, 90000);
        if (!trusted) throw new Error('작업 공간 신뢰 대화상자가 뜨지 않았습니다.');
        const ready = await until("Boolean(window.theia?.container) && document.querySelectorAll('.folder-tabs [role=tab]').length >= 1 && !document.querySelector('.workspace-trust-dialog')", 90000);
        if (!ready) throw new Error('첫 터미널 탭이 나타나지 않았습니다.');
        await evaluate(`window.__sweepWorkPath = ${JSON.stringify(sandbox.work)}`);
        await evaluate(PAGE_HELPERS);
        const sweep = (call) => evaluate(`window.__sweep.${call}`);
        // Pencil 확장은 시작하면 pencil-welcome.pen을 스스로 연다. 점검 대상이 아니므로 나타난 뒤 닫아 첫 화면을 비운다.
        const welcome = "w.id?.startsWith('plugin-custom-editor') && String(w.title?.label).includes('pencil-welcome')";
        const hasWelcome = await until(`Boolean(window.__sweep.find(${JSON.stringify(welcome)}))`, 20000);
        if (hasWelcome) await evaluate(`window.__sweep.closeByMatch(${JSON.stringify(welcome)})`);
        await sleep(3000);
        // 배경 오류: 화면을 열지 않은 동안 나오는 오류는 어느 화면 탓도 아니므로 점검에서 뺀다.
        const readLogTail = (file, offset) => {
            let text = '';
            try {
                text = fs.readFileSync(file).subarray(offset).toString('utf8');
            } catch {
                text = '';
            }
            return text;
        };
        const logSize = (file) => {
            let size = 0;
            try {
                size = fs.statSync(file).size;
            } catch {
                size = 0;
            }
            return size;
        };
        const errorLines = (text) => text.split('\n').filter((line) => /\bERROR\b/.test(line)).map((line) => line.trim().slice(0, 200));
        const backgroundStart = { console: consoleErrors.length, backend: logSize(backendLogPath), stdout: logSize(stdoutPath) };
        await sleep(5000);
        const backgroundConsole = new Set(consoleErrors.slice(backgroundStart.console));
        const backgroundBackend = new Set([
            ...errorLines(readLogTail(backendLogPath, backgroundStart.backend)),
            ...errorLines(readLogTail(stdoutPath, backgroundStart.stdout)),
        ]);
        backgroundNote = `배경 오류(화면을 열지 않은 5초 동안 나온 것)는 점검에서 제외: 프런트 ${backgroundConsole.size}종, 백엔드 ${backgroundBackend.size}종`;

        async function capture(
            fileName,
        ) {
            const screenshot = await send('Page.captureScreenshot', { format: 'png' });
            await fsp.writeFile(path.join(runDirectory, fileName), Buffer.from(screenshot.data, 'base64'));
            return fileName;
        }

        /** 화면 하나를 한 상태에서 점검해 검사 칸(1~7)을 채운 행을 돌려준다. */
        async function sweepScreen(
            screen,
            state,
            context,
        ) {
            const row = { screen: screen.name, state, cells: {} };
            const cell = (n, ok, note) => { row.cells[n] = { ok, note: String(note).replace(/\s+/g, ' ') }; };
            const consoleStart = consoleErrors.length;
            const backendStart = logSize(backendLogPath);
            const stdoutStart = logSize(stdoutPath);
            try {
                if (screen.setup === 'modifyTracked') {
                    await fsp.writeFile(path.join(sandbox.work, 'b.ts'), `export const value: number = ${Date.now()};\n`);
                    // Git 확장이 변경을 인식할 때까지 기다린다.
                    const seen = await until("Boolean(window.__sweep.svc('ScmService').selectedRepository?.provider.groups.some((group) => group.resources.length))", 15000);
                    if (!seen) row.openError = 'Git 변경이 인식되지 않음';
                }
                // 이전 화면이 남긴 알림 토스트를 걷어 캡처에 섞이지 않게 한다.
                await evaluate("window.__sweep.svc('CommandRegistry').executeCommand('notifications.commands.hide')").catch(() => {});
                evaluate(`(function () { with (window.__sweep) { return (${screen.open}); } })()`).catch((error) => { row.openError = error.message; });
                const matchJson = JSON.stringify(screen.match);
                const opened = await until(`window.__sweep.inspect(${matchJson}).found`, OPEN_TIMEOUT_MS);
                await sleep(SETTLE_MS);
                const info = opened ? await sweep(`inspect(${matchJson})`) : { found: false };
                if (!info.found) {
                    const isDisabledWithoutWorkspace = screen.needsWorkspace && !withWorkspace && /cannot be executed/.test(row.openError ?? '');
                    cell(1, isDisabledWithoutWorkspace ? 'info' : false, `열리지 않음${row.openError ? `: ${row.openError.slice(0, 80)}` : ''}${isDisabledWithoutWorkspace ? ' — 작업 공간 없이는 명령이 꺼짐(Theia 설계)' : ''}`);
                    for (const n of [2, 3, 4, 5]) cell(n, null, '-');
                } else {
                    row.info = info;
                    const isMain = info.area === 'main';
                    // 사이드바가 품는 보기(Source control·Extensions)는 사이드바 호스트 안에 보이면 통과한다. 본문 규칙(탭·이름 줄)은 적용하지 않는다.
                    const place = info.inSidebar ? 'sidebar' : (info.area ?? '영역 없음');
                    cell(1, (isMain || info.inSidebar) && info.visible, `${place}·${info.visible ? '보임' : '안 보임'}(${info.kind})`);
                    if (!isMain) {
                        for (const n of [2, 3, 4, 5]) cell(n, null, `- (${place}이라 본문 규칙 밖)`);
                    } else {
                        // 위쪽 줄에 탭이 없어도 펼친 이름 줄이 이름과 닫기를 맡으면 일관된 화면(△, 실패로 세지 않음)이다. 설정 같은 전역 화면이 이 경우다.
                        const namedByBar = Boolean(info.pathBar && info.pathBar.height > 1 && info.pathBar.close);
                        const namedByRealTab = Boolean(info.realTab && info.realTab.close);
                        if (info.tab) cell(2, info.tab.selected, info.tab.selected ? '탭 있음·선택' : '탭 있음·선택 안 됨');
                        else if (namedByBar) cell(2, 'info', '위쪽 줄에 탭 없음(펼친 이름 줄이 이름·닫기를 맡음)');
                        else if (namedByRealTab) cell(2, 'info', '위쪽 줄에 탭 없음(칸의 실제 탭 줄이 이름·닫기를 맡음)');
                        else cell(2, false, '위쪽 줄에 탭 없음');
                        const bar = info.pathBar;
                        const barText = bar ? `h=${bar.height}${bar.close ? '·×있음' : '·×없음'}` : '이름 줄 없음';
                        // 위쪽 줄에 탭이 있는 한 칸 화면은 이름 줄이 접혀야 하고, 탭이 없으면 이름 줄이 펼쳐져 닫기를 맡아야 한다.
                        let barOk = true;
                        let barNote = barText;
                        if (!info.tab) {
                            barOk = namedByBar || namedByRealTab;
                            if (namedByRealTab && !namedByBar) barNote = `${barText} — 실제 탭 줄의 탭·×가 이름과 닫기를 맡음`;
                            if (!barOk) barNote = `${barText} — 탭도 없고 펼쳐진 이름 줄·닫기도 없음`;
                        } else if (state === STATE.SINGLE && bar && bar.height > 1) {
                            barOk = false;
                            barNote = `${barText} — 위쪽 탭이 있는데 이름 줄이 펼쳐져 이름이 두 줄`;
                        }
                        cell(3, barOk, barNote);
                    }
                    row.capture = await capture(`${screen.slug}-${context.slug}-${state}.png`);
                    if (isMain && state === STATE.SPLIT) {
                        // 5: 다른 칸의 위쪽 줄 빈 곳 → 본문 가운데 순으로 옮겨 본다(두 번째는 첫 번째 도착 칸의 반대편).
                        const notes = [];
                        let dockOk = true;
                        for (const kind of ['row', 'body']) {
                            const tried = await sweep(`dockTry(${matchJson}, ${JSON.stringify(kind)})`);
                            const label = kind === 'row' ? '위쪽 줄 빈 곳' : '본문 가운데';
                            if (!tried.point) {
                                dockOk = false;
                                notes.push(`${label}: 다른 보이는 칸 없음`);
                            } else if (!tried.target) {
                                dockOk = false;
                                notes.push(`${label}: tabDockTarget이 null${tried.grouped === false ? '(칸별 묶음 없음)' : ''}`);
                            } else if (!tried.moved) {
                                dockOk = false;
                                notes.push(`${label}: 대상(${tried.target.mode})은 나왔으나 dockTab 뒤 칸이 그대로`);
                            } else {
                                notes.push(`${label}: 이동됨(${tried.target.mode})`);
                            }
                            await sleep(600);
                        }
                        cell(5, dockOk, notes.join(' / '));
                    } else if (isMain) {
                        cell(5, null, '- (한 칸)');
                    }
                    // 4: 위쪽 탭 ×(없으면 이름 줄 ×)로 닫는다.
                    if (isMain) {
                        const via = await sweep(`clickClose(${matchJson})`);
                        if (via === 'real-tab') {
                            const point = await evaluate('window.__sweepPoint');
                            for (const type of ['mousePressed', 'mouseReleased']) {
                                await send('Input.dispatchMouseEvent', { type, x: point.x, y: point.y, button: 'left', clickCount: 1 });
                            }
                        }
                        const gone = via !== 'none' && await until(`(() => { const w = window.__sweep.find(${matchJson}); return !w || window.__sweep.isGone(w); })()`, 5000);
                        const viaName = { tab: '위쪽 탭 ×', 'path-bar': '이름 줄 ×(위쪽 탭 ×는 없음)', 'real-tab': '칸의 실제 탭 ×(위쪽 탭 ×는 없음)' }[via];
                        cell(4, via === 'tab' ? Boolean(gone) : (via !== 'none' && gone ? 'info' : false), via === 'none' ? '누를 ×가 없음' : `${viaName}로 ${gone ? '닫힘' : '안 닫힘'}`);
                    }
                }
                // 정리: 남은 화면과 영역을 닫는다.
                if (screen.cleanup) await evaluate(`(function () { with (window.__sweep) { return (${screen.cleanup}); } })()`);
                await evaluate(`(async () => { const w = window.__sweep.find(${JSON.stringify(screen.match)}); if (w) { const area = window.__sweep.svc('ApplicationShell').getAreaFor(w); await window.__sweep.svc('ApplicationShell').closeWidget(w.id); if (area && area !== 'main') window.__sweep.svc('ApplicationShell').collapsePanel(area); } return true; })()`);
                // 사이드바 보기(Extensions 등)가 열려 있으면 Work 목록으로 되돌린다. 목록은 Work 보기에서만 다시 그려져 다음 점검(작업 폴더 만들기)이 막힌다.
                await evaluate("window.__sweep.svc('PaddockWorkspace').workSidebar.showView('work')");
                if (screen.setup === 'modifyTracked') {
                    await fsp.writeFile(path.join(sandbox.work, 'b.ts'), 'export const value: number = 1;\n');
                }
                await sleep(600);
            } catch (error) {
                cell(1, false, `점검 오류: ${error.message.slice(0, 120)}`);
            }
            // 6: 연 동안 새로 생긴 프런트 오류·백엔드 ERROR
            await sleep(500);
            const newConsole = [...new Set(consoleErrors.slice(consoleStart))].filter((text) => !backgroundConsole.has(text));
            const newBackend = [...new Set([
                ...errorLines(readLogTail(backendLogPath, backendStart)),
                ...errorLines(readLogTail(stdoutPath, stdoutStart)),
            ])].filter((line) => !backgroundBackend.has(line));
            const logNote = [
                newConsole.length ? `프런트 error ${newConsole.length}건: ${newConsole[0].replace(/\s+/g, ' ').slice(0, 100)}` : '',
                newBackend.length ? `백엔드 ERROR ${newBackend.length}건: ${newBackend[0].slice(0, 100)}` : '',
            ].filter(Boolean).join(' / ');
            cell(6, !logNote, logNote || '새 오류 없음');
            row.cells[7] = { ok: null, note: row.capture ?? '-' };
            return row;
        }

        /**
         * 검색 행 하나를 한 상태에서 점검한다. 검사 칸의 뜻은 본문 화면 행과 다르다:
         * 1 파일 영역 제목 줄 버튼 → 파일 목록 자리에 검색 위젯, 2 검색어 → 결과 1건 이상, 3 결과 클릭 → 본문에 파일(줄 선택)·결과 유지,
         * 4 × → 파일 목록 복귀, 5 Ctrl+Shift+F → 같은 자리, 6 로그, 7 캡처.
         */
        async function sweepSearch(
            state,
            context,
        ) {
            const row = { screen: '검색(search-in-workspace)', state, cells: {}, info: { search: true } };
            const cell = (n, ok, note) => { row.cells[n] = { ok, note: String(note).replace(/\s+/g, ' ') }; };
            const consoleStart = consoleErrors.length;
            const backendStart = logSize(backendLogPath);
            const stdoutStart = logSize(stdoutPath);
            const searchState = () => evaluate(`(() => {
                const workspace = window.__sweep.svc('PaddockWorkspace');
                const host = document.querySelector('.files-search');
                const widget = window.__sweep.svc('WidgetManager').tryGetWidget('search-in-workspace');
                const tree = widget?.resultTreeWidget;
                const list = document.querySelector('.file-list');
                const rect = host.getBoundingClientRect();
                return {
                    isOpen: !host.hidden && rect.width > 0 && rect.height > 0 && Boolean(widget && host.contains(widget.node) && widget.isVisible),
                    filesFolder: workspace.workSidebar.filesFolder(),
                    isListShown: Boolean(list && list.getBoundingClientRect().height > 0),
                    n_results: tree ? [...tree.resultTree.values()].reduce((sum, file) => sum + [...file.children.values()].reduce((n, group) => n + (group.children?.length ?? 0), 0), 0) : 0,
                    notice: document.querySelector('.paddock-search-scope')?.textContent ?? null,
                };
            })()`);
            const pointOf = (selector) => evaluate(`(() => { const node = document.querySelector(${JSON.stringify(selector)}); if (!node) return null; const r = node.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
            const clickAt = async (point) => {
                for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) await send('Input.dispatchMouseEvent', { type, x: point.x, y: point.y, button: 'left', clickCount: 1 });
            };
            try {
                if (context.slug === 'unassigned') {
                    // 미배정 터미널은 임시 HOME에 있어 점검용 파일이 없다. 점검용 폴더로 옮긴 뒤 그 현재 폴더가 검색 범위가 된다.
                    await evaluate(`window.__sweep.svc('PaddockWorkspace').terminals.all[0].sendText(${JSON.stringify(`cd ${sandbox.work}\n`)})`);
                    await until(`window.__sweep.svc('PaddockWorkspace').workSidebar.filesFolder() === ${JSON.stringify(`file://${sandbox.work}`)}`, 20000);
                    await sleep(1200);
                }
                // 1: 파일 영역 제목 줄의 검색 버튼
                const button = await until("Boolean(document.querySelector('.file-search'))", 10000) && await pointOf('.file-search');
                if (!button) throw new Error('파일 영역 제목 줄에 검색 버튼이 없음');
                await clickAt(button);
                const opened = await until("window.__sweep.svc('PaddockWorkspace').workSidebar.filesSearchOpen", 10000);
                await sleep(800);
                const afterOpen = await searchState();
                cell(1, Boolean(opened) && afterOpen.isOpen && !afterOpen.isListShown, `${afterOpen.isOpen ? '검색 위젯이 파일 영역에 보임' : '검색 위젯이 안 보임'}·파일 목록 ${afterOpen.isListShown ? '그대로' : '가려짐'}·${afterOpen.notice ?? '범위 안내 없음'}`);
                // 2: 검색어
                await evaluate("document.querySelector('#search-input-field')?.focus()");
                await send('Input.insertText', { text: 'export const value' });
                for (const type of ['keyDown', 'keyUp']) await send('Input.dispatchKeyEvent', { type, key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
                const found = await until("(() => { const w = window.__sweep.svc('WidgetManager').tryGetWidget('search-in-workspace'); return [...w.resultTreeWidget.resultTree.values()].length > 0; })()", 15000);
                await sleep(800);
                const afterSearch = await searchState();
                cell(2, Boolean(found) && afterSearch.n_results >= 1, `결과 ${afterSearch.n_results}건`);
                row.capture = await capture(`search-${context.slug}-${state}.png`);
                // 3: 결과 줄 클릭 → 본문에 b.ts가 줄 선택으로 열리고 결과 목록은 남는다.
                const line = await evaluate(`(() => { const node = [...document.querySelectorAll('.files-search .resultLine')].find((item) => /export const value/.test(item.textContent)); if (!node) return null; const r = node.getBoundingClientRect(); return { x: r.left + 20, y: r.top + r.height / 2 }; })()`);
                if (line) await clickAt(line);
                const editorOpened = line ? await until("window.__sweep.svc('EditorManager').currentEditor?.editor.uri.path.base === 'b.ts'", 10000) : false;
                await sleep(800);
                const selection = await evaluate("(() => { const e = window.__sweep.svc('EditorManager').currentEditor; return e && e.editor.uri.path.base === 'b.ts' ? e.editor.selection : null; })()");
                const afterClick = await searchState();
                const selected = Boolean(selection) && selection.start.line === 0 && selection.end.character > selection.start.character;
                cell(3, Boolean(editorOpened) && selected && afterClick.isOpen && afterClick.n_results >= 1,
                    `${editorOpened ? '본문에 b.ts 열림' : '본문에 안 열림'}·${selected ? '줄 선택됨' : '줄 선택 안 됨'}·결과 ${afterClick.isOpen ? `${afterClick.n_results}건 유지` : '검색 위젯 닫힘'}`);
                // 4: ×
                await evaluate("document.querySelector('.files-search-close').click()");
                await sleep(800);
                const afterClose = await searchState();
                cell(4, !afterClose.isOpen && afterClose.isListShown, `${afterClose.isOpen ? '검색이 안 닫힘' : '검색 닫힘'}·파일 목록 ${afterClose.isListShown ? '복귀' : '안 보임'}`);
                // 5: Ctrl+Shift+F — 같은 자리, 앞 검색 결과가 그대로
                await evaluate("document.body.focus(); true");
                await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'F', code: 'KeyF', windowsVirtualKeyCode: 70, modifiers: 2 | 8 });
                await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'F', code: 'KeyF', windowsVirtualKeyCode: 70, modifiers: 2 | 8 });
                await sleep(1200);
                const afterKey = await searchState();
                cell(5, afterKey.isOpen && !afterKey.isListShown && afterKey.n_results >= 1, `${afterKey.isOpen ? '같은 자리에 열림' : '안 열림'}·이전 결과 ${afterKey.n_results}건`);
                // 정리: 검색을 닫고 열어 둔 파일을 닫는다.
                await evaluate("window.__sweep.svc('PaddockWorkspace').workSidebar.hideFilesSearch()");
                await evaluate("window.__sweep.closeByMatch(\"w.getResourceUri?.()?.path.base === 'b.ts'\")");
                await sleep(600);
            } catch (error) {
                cell(1, false, `점검 오류: ${error.message.slice(0, 120)}`);
            }
            await sleep(500);
            const newConsole = [...new Set(consoleErrors.slice(consoleStart))].filter((text) => !backgroundConsole.has(text));
            const newBackend = [...new Set([
                ...errorLines(readLogTail(backendLogPath, backendStart)),
                ...errorLines(readLogTail(stdoutPath, stdoutStart)),
            ])].filter((line) => !backgroundBackend.has(line));
            const logNote = [
                newConsole.length ? `프런트 error ${newConsole.length}건: ${newConsole[0].replace(/\s+/g, ' ').slice(0, 100)}` : '',
                newBackend.length ? `백엔드 ERROR ${newBackend.length}건: ${newBackend[0].slice(0, 100)}` : '',
            ].filter(Boolean).join(' / ');
            cell(6, !logNote, logNote || '새 오류 없음');
            row.cells[7] = { ok: null, note: row.capture ?? '-' };
            return row;
        }

        // 미배정 터미널(작업 폴더가 아직 없는 첫 실행 상태) 한 칸 → 작업 폴더가 생긴 뒤 한 칸 → 오른쪽에 터미널 칸을 더 나눈 두 칸 순으로 점검한다.
        // 미배정 상태의 두 칸은 점검하지 않는다(작업 폴더를 만들면 첫 터미널이 옮겨져 같은 배치를 다시 만들 수 없다).
        const results = new Map();
        for (const screen of SCREENS) results.set(screen.slug, { screen, rows: [] });
        // 검색은 본문 화면이 아니라 사이드바 파일 영역의 위젯이라 SCREENS 밖에서 따로 점검한다.
        results.set('search', { screen: { name: '검색(search-in-workspace)', slug: 'search' }, rows: [] });
        const PASSES = [
            { context: { slug: 'unassigned', label: '미배정' }, state: STATE.SINGLE },
            { context: { slug: 'work', label: '작업 폴더' }, state: STATE.SINGLE },
            { context: { slug: 'work', label: '작업 폴더' }, state: STATE.SPLIT },
        ];
        for (const pass of PASSES) {
            if (pass.context.slug === 'work' && pass.state === STATE.SINGLE) {
                // 이름이 claude인 가짜 에이전트를 작업 폴더에서 실행해, 그 폴더가 작업 폴더가 되고 터미널이 그 아래로 옮겨지게 한다.
                const fakeBin = path.join(sandbox.root, 'fake-bin');
                await fsp.mkdir(fakeBin, { recursive: true });
                await fsp.writeFile(path.join(fakeBin, 'claude'), '#!/bin/bash\necho ready\nwhile read -r line; do sleep 1; printf "done\\n> "; done\n', { mode: 0o755 });
                const typed = await until("window.__sweep.svc('PaddockWorkspace').currentWidget()?.id?.startsWith('terminal-')", 20000);
                if (!typed) throw new Error('현재 항목이 터미널이 아니어서 가짜 에이전트를 실행할 수 없습니다.');
                await evaluate(`window.__sweep.svc('PaddockWorkspace').currentWidget().sendText(${JSON.stringify(`cd ${sandbox.work}; export PATH=${fakeBin}:$PATH; claude\n`)})`);
                const made = await until(`Boolean(document.querySelector('.folder-row[data-folder=${JSON.stringify(`file://${sandbox.work}`)}]')) && document.querySelector('.folder-tabs .tab.is-active [role=tab]')?.textContent === 'claude'`, 40000);
                if (!made) throw new Error('작업 폴더가 만들어지지 않았습니다(가짜 에이전트 실행 확인 실패).');
                await sleep(2000);
            }
            if (pass.state === STATE.SPLIT) {
                await evaluate("document.querySelector('.folder-split-right').click()");
                const split = await until("[...window.__sweep.svc('ApplicationShell').mainPanel.tabBars()].length >= 2", 20000);
                if (!split) throw new Error('터미널 칸 나누기가 되지 않았습니다(.folder-split-right).');
                await sleep(2500);
            }
            for (const screen of SCREENS) {
                if (!screen.skip) {
                    const row = await sweepScreen(screen, pass.state, pass.context);
                    row.state = `${pass.context.label}·${pass.state === STATE.SINGLE ? '한 칸' : '두 칸'}`;
                    results.get(screen.slug).rows.push(row);
                }
            }
            const searchRow = await sweepSearch(pass.state, pass.context);
            searchRow.state = `${pass.context.label}·${pass.state === STATE.SINGLE ? '한 칸' : '두 칸'}`;
            results.get('search').rows.push(searchRow);
        }
        rows = [...results.values()];
    } finally {
        if (socket) socket.close();
        await stopApp(child, sandbox);
        await fsp.rm(sandbox.root, { recursive: true, force: true }).catch(() => {});
    }
    await writeReport(runDirectory, rows, backgroundNote, withWorkspace);
    return runDirectory;
}

/** 화면 × 검사 표와 실패 목록을 report.md로 쓴다. */
async function writeReport(
    runDirectory,
    results,
    backgroundNote,
    withWorkspace,
) {
    const marks = (entry) => (entry.ok === null ? entry.note : entry.ok === 'info' ? `△ ${entry.note}` : entry.ok ? `✓ ${entry.note}` : `✗ ${entry.note}`);
    const escape = (text) => String(text).replace(/\|/g, '\\|');
    const headers = ['화면', '상태', '1 영역·보임', '2 위쪽 탭', '3 이름 줄', '4 × 닫기', '5 칸 이동', '6 로그', '7 캡처'];
    const lines = [
        '# 본문 화면 전수 점검',
        '',
        `- 실행: ${new Date().toISOString()}`,
        `- 모드: ${withWorkspace ? '작업 공간 열림(--workspace, 임시 폴더를 작업 공간으로 염)' : '작업 공간 없음(기본)'}`,
        `- ${backgroundNote}`,
        '- 시작 때 Pencil 확장이 자동으로 여는 pencil-welcome.pen 탭은 점검 대상이 아니라 점검 전에 닫음',
        '- 상태: 미배정 = 작업 폴더가 없는 첫 실행 상태의 터미널, 작업 폴더 = 가짜 claude를 실행해 폴더가 생긴 상태, 두 칸 = 오른쪽에 터미널 칸을 하나 더 나눔',
        '- △ = 위쪽 탭 줄 대신 이름 줄이 같은 일을 하는 일관된 상태(실패로 세지 않음)',
        '- 검색 행의 칸 뜻은 다르다: 1 파일 영역 버튼 → 검색 위젯, 2 검색어 → 결과 1건 이상, 3 결과 클릭 → 본문에 파일(줄 선택)·결과 유지, 4 × → 파일 목록 복귀, 5 Ctrl+Shift+F → 같은 자리, 6 로그, 7 캡처',
        '- 3 기대: 위쪽 탭이 있는 한 칸 화면은 이름 줄 접힘(h<=1), 탭이 없는 화면은 펼친 이름 줄(h>1)과 닫기',
        '',
        `| ${headers.join(' | ')} |`,
        `| ${headers.map(() => '---').join(' | ')} |`,
    ];
    const failures = [];
    let n_screens = 0;
    for (const { screen, rows } of results) {
        if (screen.skip) {
            lines.push(`| ${escape(screen.name)} | - | ${escape(screen.skip)} | | | | | | |`);
        } else {
            const opened = rows.some((row) => row.info);
            if (opened) n_screens += 1;
            for (const row of rows) {
                const cells = [1, 2, 3, 4, 5, 6, 7].map((n) => escape(row.cells[n] ? marks(row.cells[n]) : '-'));
                lines.push(`| ${escape(row.screen)} | ${row.state} | ${cells.join(' | ')} |`);
                for (const n of [1, 2, 3, 4, 5, 6]) {
                    if (row.cells[n]?.ok === false) failures.push(`- ${row.screen} (${row.state}) 검사${n}: ${row.cells[n].note}`);
                }
            }
        }
    }
    lines.push('', `## 실패 칸 ${failures.length}개`, '', ...(failures.length ? failures : ['없음']), '');
    lines.push(`점검한 화면 ${n_screens}개 (열지 못한 화면 포함 정의 ${results.length}개)`, '');
    await fsp.writeFile(path.join(runDirectory, 'report.md'), lines.join('\n'));
    console.log(`${n_screens} screens / ${failures.length} failed cells`);
    console.log(path.join(runDirectory, 'report.md'));
}

main().then((directory) => {
    console.log(`결과: ${directory}`);
}).catch((error) => {
    console.error(`screen-sweep 중단: ${error.message}`);
    process.exitCode = 1;
});
