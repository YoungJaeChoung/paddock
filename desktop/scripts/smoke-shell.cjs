// 실행 중인 Paddock(시험용 설정, --remote-debugging-port)를 CDP로 조작해 작업 폴더 흐름을 확인한다.
// 사용: PADDOCK_CDP_PORT=9333 PADDOCK_SMOKE_REPO=/path/to/git/repo node scripts/smoke-shell.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');

async function main() {
    const port = process.env.PADDOCK_CDP_PORT;
    const repo = process.env.PADDOCK_SMOKE_REPO || path.join(__dirname, '..', '..');
    assert(port, '별도의 시험 설정으로 실행한 앱의 PADDOCK_CDP_PORT를 지정하세요.');
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    const target = targets.find((item) => item.type === 'page');
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve) => socket.addEventListener('open', resolve, { once: true }));
    let n_messages = 0;
    const pending = new Map();
    socket.addEventListener('message', ({ data }) => {
        const response = JSON.parse(data);
        const entry = pending.get(response.id);
        if (entry) {
            clearTimeout(entry.timeout);
            pending.delete(response.id);
            if (response.error) {
                entry.reject(new Error(response.error.message));
            } else {
                entry.resolve(response.result);
            }
        }
    });
    function send(
        method,
        params = {},
    ) {
        const id = ++n_messages;
        const response = new Promise((resolve, reject) => {
            const timeout = setTimeout(() => {
                pending.delete(id);
                reject(new Error(`응답 시간 초과: ${method}`));
            }, 20000);
            pending.set(id, { resolve, reject, timeout });
            socket.send(JSON.stringify({ id, method, params }));
        });
        return response;
    }
    async function evaluate(
        expression,
    ) {
        const response = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
        assert(!response.exceptionDetails, JSON.stringify(response.exceptionDetails));
        return response.result.value;
    }
    async function until(
        expression,
        n_attempts = 80,
    ) {
        let result = false;
        for (let n_tried = 0; n_tried < n_attempts && !result; n_tried += 1) {
            result = await evaluate(expression);
            if (!result) {
                await new Promise((resolve) => setTimeout(resolve, 150));
            }
        }
        assert(result, expression);
        return result;
    }
    async function key(
        code,
        text,
        keyCode,
        modifiers = 2 | 8,
    ) {
        // 조합키(기본 Control+Shift)를 문서에 보내 Paddock 단축키를 누른다. ctrlcmd로 건 단축키는 macOS에서 Command(4)다.
        for (const type of ['rawKeyDown', 'keyUp']) {
            await send('Input.dispatchKeyEvent', { type, modifiers, key: text, code, windowsVirtualKeyCode: keyCode });
        }
    }
    /** 실제 포인터를 누른 채 초점 변경이 일어나도 첫 클릭이 선택으로 끝나는지 확인한다. */
    async function mouseClick(
        selector,
        duringPress = null,
    ) {
        // 처음에는 로딩 덮개가 사라지는 전환 중에도 뒤의 버튼이 DOM에 있다. 실제로 누를 수 있게 된 뒤 한 번 클릭한다.
        await until(`(() => { const node = document.querySelector(${JSON.stringify(selector)}); const rect = node?.getBoundingClientRect(); return Boolean(rect?.width > 0 && rect?.height > 0 && node.contains(document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2))); })()`);
        const point = await evaluate(`(() => { const rect = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }; })()`);
        await send('Input.dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', clickCount: 1 });
        if (duringPress) await evaluate(duringPress);
        await new Promise(resolve => setTimeout(resolve, 80));
        await send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...point, button: 'left', clickCount: 1 });
    }
    // 다른 창이 OS 포커스를 가져가도 포커스 이벤트가 오게 한다. 없으면 Theia가 현재 위젯을 잃어 새 터미널 확인이 실패한다.
    await send('Emulation.setFocusEmulationEnabled', { enabled: true });
    const captureDirectory = path.join(__dirname, '..', 'captures');
    await fs.mkdir(captureDirectory, { recursive: true });
    async function capture(
        name,
    ) {
        const screenshot = await send('Page.captureScreenshot', { format: 'png' });
        await fs.writeFile(path.join(captureDirectory, `${name}.png`), Buffer.from(screenshot.data, 'base64'));
    }
    const container = 'window.theia.container';
    const service = (name) => `${container}.get([...${container}._bindingDictionary._map.keys()].find(k => k && k.name === ${JSON.stringify(name)}))`;
    const shell = service('ApplicationShell');
    const n_terminals = `${shell}.widgets.filter(w => w.id.startsWith('terminal-')).length`;
    const repoKey = `file://${repo}`;
    const homeKey = `file://${require('node:os').homedir()}`;
    let sidebarFixture = null;
    // 캡처 전에 보이는 터미널마다 셸 프롬프트가 떴는지 기다린다(빈 화면을 결함으로 오해하지 않게).
    // 가짜 에이전트를 실행 중인 터미널은 셸 프롬프트 대신 에이전트의 준비 출력(ready)을 띄운 상태로 본다.
    const untilPrompts = () => until(`${shell}.widgets.filter(w => w.id.startsWith('terminal-') && w.isVisible).every(w => { const b = w.term.buffer.active; for (let i = 0; i < b.length; i++) { if (/[$%#>] *$|^ready$/.test(b.getLine(i).translateToString(true))) return true; } return false; })`, 120);
    try {
        // 새 시험 설정은 폴더 신뢰 확인 창부터 뜨고, 답하기 전에는 확장(Git 등)이 시작되지 않는다. 시험 폴더라 신뢰로 답한다.
        await until('!!document.querySelector(".workspace-trust-dialog .theia-button.main")', 80);
        await evaluate('document.querySelector(".workspace-trust-dialog .theia-button.main").click()');
        // Step 1: 창 틀 — 레일 없음, 보기 줄 3개, 추가 터미널 줄, 상태 줄 메모리.
        assert(await evaluate('!!document.querySelector(".paddock-shell")'));
        assert.equal(await evaluate('document.querySelectorAll(".activity-rail, .rail-button").length'), 0);
        assert.equal(await evaluate('document.querySelectorAll(".view-bar [data-view]").length'), 3);
        assert.equal(await evaluate('Math.round(document.querySelector(".paddock-tabbar").getBoundingClientRect().height)'), await evaluate('document.querySelector(".paddock-shell").classList.contains("has-integrated-titlebar")') ? 0 : 28);
        assert.equal(await evaluate('document.querySelector(".tab-strip-label") === null'), true);
        assert.equal(await evaluate('getComputedStyle(document.querySelector(".paddock-tabbar")).backgroundColor === getComputedStyle(document.querySelector(".statusbar")).backgroundColor'), true);
        // 터미널에 초점이 있어도 Ctrl+B(macOS는 Cmd+B)는 셸 입력 대신 전체 사이드바를 접고, 상단 버튼으로 다시 펼친다.
        await until(`${shell}.widgets.some(widget => widget.id.startsWith('terminal-'))`);
        await evaluate(`${shell}.widgets.find(widget => widget.id.startsWith('terminal-')).activate()`);
        const sidebarModifiers = await evaluate('document.querySelector(".paddock-shell").classList.contains("is-macos")') ? 4 : 2;
        // 앱이 뜬 직후 몇 초는 단축키 처리가 준비되지 않아 누른 키가 그냥 지나간다. 사이드바가 접힐 때까지 1초 간격으로 다시 누른다.
        const isSidebarCollapsed = 'document.querySelector(".sidebar-toggle").getAttribute("aria-expanded") === "false"';
        for (let n_presses = 0; n_presses < 10 && !(await evaluate(isSidebarCollapsed)); n_presses += 1) {
            for (const type of ['rawKeyDown', 'keyUp']) {
                await send('Input.dispatchKeyEvent', { type, modifiers: sidebarModifiers, key: 'b', code: 'KeyB', windowsVirtualKeyCode: 66 });
            }
            await new Promise(resolve => setTimeout(resolve, 1000));
        }
        await until(isSidebarCollapsed);
        await mouseClick('.sidebar-toggle');
        await until('document.querySelector(".sidebar-toggle").getAttribute("aria-expanded") === "true"');
        if (await evaluate('document.querySelector(".paddock-shell").classList.contains("is-linux")')) {
            // 통합 제목 줄이 있으면 그 줄의 창 버튼을 검사한다. 아래 제목줄의 대체 버튼은 숨긴 상태여야 한다.
            if (await evaluate('document.querySelector(".paddock-shell").classList.contains("has-integrated-titlebar")')) {
                assert.equal(await evaluate('[...document.querySelectorAll("#window-controls button")].filter(button => button.getClientRects().length > 0).length'), 3);
                assert.equal(await evaluate('Math.round(document.querySelector("#minimize-button").getBoundingClientRect().width)'), 48);
                assert.equal(await evaluate('document.querySelector(".window-controls").getClientRects().length'), 0);
            } else {
                assert.equal(await evaluate('document.querySelectorAll(".window-controls button").length'), 3);
                assert.equal(await evaluate('Math.round(document.querySelector(".window-minimize").getBoundingClientRect().width)'), 28);
            }
        }
        await until('/^\\d+%$/.test(document.querySelector("[data-meter=memory] .meter-percent").textContent)');
        // AI 사용량: 아직 Claude·Codex 터미널을 고른 적이 없으면 계정을 지어내지 않고 전체 목록을 여는 Usage 버튼만 둔다.
        await until('!!document.querySelector(".ai-usage .account-usage-compact.is-only") && !document.querySelector(".ai-usage .account-usage-selected")', 60);
        // Step 1b: 상태 줄 항목은 눌러도 숨지 않는다(누르면 자세히 보기). 숨기기·다시 켜기는 빠른 설정의 체크 상자만 한다.
        await mouseClick('.memory-usage .usage-group');
        assert.equal(await evaluate('!!document.querySelector("[data-meter=memory]")'), true);
        const quickCheck = index => evaluate(`${service('PaddockWorkspace')}.quickSettings.renderQuickSettings(); document.querySelectorAll("#quick-settings .quick-check")[${index}].click()`);
        await evaluate(`${service('PaddockWorkspace')}.quickSettings.renderQuickSettings()`);
        const statusChecks = await evaluate('[...document.querySelectorAll("#quick-settings .quick-check")].map(box => box.checked)');
        assert.deepEqual(statusChecks, [true, true, true]);
        await quickCheck(2);
        await until('!document.querySelector(".memory-usage .usage-group")');
        await quickCheck(2);
        await until('/^\\d+%$/.test(document.querySelector("[data-meter=memory] .meter-percent")?.textContent || "")');

        // 첫 실행: 작업 폴더는 에이전트를 실행해야 생기므로 목록은 비어 있고 다음 행동을 안내한다.
        // 첫 터미널은 Unassigned 묶음이고, 아래 탭 줄에 그 터미널 탭 하나가 처음부터 보인다.
        // Unassigned 묶음도 나누기를 지원하므로 나누기 버튼이 보인다(단축키·탭 끌기 분할과 같은 표면).
        // 본문의 빈 상태 안내(.main-empty)도 같은 .work-empty 문단을 쓰므로 사이드바 문구는 전체에서 찾는다.
        await until('[...document.querySelectorAll(".work-empty")].some(item => item.textContent.includes("No work folders"))');
        await until('document.querySelector(".work-steps")?.textContent.includes("Run claude or codex")');
        await until('document.querySelectorAll(".unassigned-row .row-main").length === 1');
        assert.equal(await evaluate('document.querySelector(".folder-bar").classList.contains("lm-mod-hidden")'), false);
        assert.equal(await evaluate('document.querySelectorAll(".folder-tabs [role=tab]").length'), 1);
        assert.equal(await evaluate('document.querySelector(".folder-tab-add").getClientRects().length > 0'), true);
        assert.equal(await evaluate('document.querySelector(".work-add").getClientRects().length > 0'), true);
        assert.equal(await evaluate('document.querySelector(".folder-bar-actions").hidden'), false);
        await untilPrompts();
        await capture('1-first-run');

        // Step 2: 추가 터미널에서 레포로 cd하고 에이전트를 실행하면 그 폴더가 작업 폴더가 되고 터미널이 그 아래로 옮겨진다.
        // 실제 에이전트 대신 이름이 claude인 가짜 프로그램(Enter마다 7초 스피너 뒤 입력 대기)을 쓴다.
        const workspace = service('PaddockWorkspace');
        const fakeBin = await fs.mkdtemp(path.join(require('node:os').tmpdir(), 'paddock-fake-agent-'));
        await fs.writeFile(path.join(fakeBin, 'claude'), '#!/bin/bash\necho ready\nwhile read -r line; do end=$((SECONDS+7)); while [ $SECONDS -lt $end ]; do printf "\\r working"; sleep 0.15; done; printf "\\r done\\n> "; done\n', { mode: 0o755 });
        // 창이 OS 포커스 없이 떠 있으면(WSL 등) Theia의 활성 위젯이 비므로, 앱처럼 본문의 현재 항목을 지금 보는 터미널로 쓴다.
        const current = `${workspace}.currentWidget()`;
        await until(`${current}?.id?.startsWith('terminal-')`);
        const firstTerminal = await evaluate(`${current}.id`);
        // 터미널 글자를 선택하면 바로 클립보드에 복사된다(terminal.integrated.copyOnSelection).
        await evaluate(`${current}.sendText('echo PADDOCK_COPY_CHECK\\n')`);
        await until(`(() => { const b = ${current}.term.buffer.active; for (let i = 0; i < b.length; i++) { if (b.getLine(i).translateToString(true) === 'PADDOCK_COPY_CHECK') { ${current}.term.select(0, i, 'PADDOCK_COPY_CHECK'.length); return true; } } return false; })()`);
        await until(`navigator.clipboard.readText().then(text => text === 'PADDOCK_COPY_CHECK')`);
        await evaluate(`${current}.sendText(${JSON.stringify(`cd ${repo}; export PATH=${fakeBin}:$PATH; claude\n`)})`);
        await until(`document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}]')`, 80);
        await until(`document.querySelector('.terminal-row[data-widget-id="${firstTerminal}"] .row-name')?.textContent === 'claude'`);
        await until('document.querySelectorAll(".unassigned-row .row-main").length === 0');
        await until(`document.querySelector(".folder-bar")?.dataset.folder === ${JSON.stringify(repoKey)} && !document.querySelector(".folder-bar").classList.contains("lm-mod-hidden")`);
        await until(`document.querySelector('.folder-tabs .tab.is-active [role=tab]')?.textContent === 'claude'`);
        await until('document.querySelector(".folder-bar-branch-name")?.textContent.length > 0');
        assert.equal(await evaluate('document.querySelectorAll(".folder-tabs .tab-close").length'), 1);
        assert.equal(await evaluate('document.querySelector(".terminal-row .row-action") === null'), true);
        for (const selector of ['.folder-tabs .tab.is-active', `.terminal-row[data-widget-id="${firstTerminal}"]`]) {
            await evaluate(`document.querySelector(${JSON.stringify(selector)}).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 320, clientY: 100 }))`);
            await until('document.querySelector(".paddock-menu:popover-open")?.textContent.includes("Rename terminal")');
            assert.equal(await evaluate('document.querySelector(".paddock-menu:popover-open").textContent.includes("Close terminal")'), true);
            await evaluate('document.querySelector(".paddock-menu:popover-open").hidePopover()');
        }
        await capture('2-agent-promoted');

        // 작업 탭의 닫기 버튼은 취소하면 유지하고, 확인하면 그 탭만 종료한다.
        await mouseClick('.folder-tabs .tab-close');
        await until('document.querySelector(".dialogBlock .theia-button.secondary")');
        await mouseClick('.dialogBlock .theia-button.secondary');
        await until('!document.querySelector(".dialogBlock")');
        assert.equal(await evaluate('document.querySelectorAll(".folder-tabs [role=tab]").length'), 1);
        await mouseClick('.folder-tab-add');
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 2');
        const temporaryId = await evaluate(`${current}.id`);
        await mouseClick('.folder-tabs .tab.is-active .tab-close');
        await until('document.querySelector(".dialogBlock .theia-button.main")');
        await mouseClick('.dialogBlock .theia-button.main');
        await until(`!${shell}.getWidgetById(${JSON.stringify(temporaryId)})`);
        await mouseClick(`.folder-tabs [data-widget-id=${JSON.stringify(firstTerminal)}]`);
        assert.equal(await evaluate(`${current}.id`), firstTerminal);

        // 작업 폴더 탭 줄의 ＋는 그 폴더에서 새 작업 터미널을 연다.
        // 이름 번호는 Work 목록 전체에서 비어 있는 가장 작은 번호다. 첫 터미널은 이미 claude로 바뀌어 1번이 비어 있다.
        await evaluate('document.querySelector(".folder-tab-add").click()');
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 2');
        await until(`document.querySelector('.folder-tabs .tab.is-active [role=tab]')?.textContent === 'terminal 1'`);
        assert.equal(await evaluate('document.querySelector(".folder-tab-add").getClientRects().length > 0'), true);
        await untilPrompts();
        await capture('3-working');

        // Step 3: Ctrl+Shift+` 아래 칸, Ctrl+Shift+5 옆 칸 — 같은 폴더의 작업 터미널이 늘고 칸이 나뉜다.
        const n_bars = await evaluate('document.querySelectorAll(".path-bar").length');
        await key('Backquote', '`', 192);
        await until(`document.querySelectorAll(".path-bar").length === ${n_bars + 1}`);
        await key('Digit5', '5', 53, (sidebarModifiers === 4 ? 4 : 2) | 8);
        await until(`document.querySelectorAll(".path-bar").length === ${n_bars + 2}`);
        // 레포 폴더에 claude·＋로 연 터미널·나눈 터미널 2개.
        await until(`document.querySelectorAll('.terminal-row:not(.unassigned-row)').length === 4`);
        await until(`document.querySelectorAll('.folder-tabs [role=tab]').length === 4`);
        await untilPrompts();
        await capture('4-5-split');

        // Step 4: 파일 구획에서 README를 열면 터미널 옆 오른쪽 칸에 파일이 열린다.
        await mouseClick(`.terminal-row[data-widget-id=${JSON.stringify(firstTerminal)}] .row-main`, `${workspace}.refresh()`);
        await until(`${workspace}.currentWidget()?.id === ${JSON.stringify(firstTerminal)}`);
        const terminalBeforeFile = await evaluate(`${workspace}.currentWidget().id`);
        const n_panesBeforeFile = await evaluate(`[...${shell}.mainPanel.tabBars()].length`);
        await mouseClick('.file-row[data-uri$="README.md"]');
        await until(`${workspace}.currentWidget()?.getResourceUri?.()?.path.base === "README.md"`);
        // 칸이 나뉘면 칸마다 실제 탭 줄을 보여(칸 사이 탭 끌기) 경로 줄과 위쪽 폴더 탭 줄은 숨긴다.
        await until('document.querySelector("#theia-main-content-panel").classList.contains("has-split-tabs")');
        assert.equal(await evaluate('document.querySelector(".folder-tabs").getClientRects().length'), 0);
        assert.equal(await evaluate('Math.round(document.querySelector(".path-bar.is-active").getBoundingClientRect().height)'), 0);
        await until(`[...${shell}.mainPanel.tabBars()].length === ${n_panesBeforeFile + 1}`);
        assert.equal(await evaluate(`(() => { const terminal = ${shell}.getWidgetById(${JSON.stringify(terminalBeforeFile)}); const file = ${workspace}.currentWidget(); return terminal.isVisible && ${shell}.getTabBarFor(file) !== ${shell}.getTabBarFor(terminal) && ${shell}.getTabBarFor(file).node.getBoundingClientRect().left > ${shell}.getTabBarFor(terminal).node.getBoundingClientRect().left; })()`), true);
        // 폴더 탭 줄은 숨겨도 이 폴더의 터미널 4개와 파일 1개를 그대로 담고 있어, 칸을 합치면 다시 보인다.
        assert.equal(await evaluate('document.querySelectorAll(".folder-tabs [role=tab]").length'), 5);
        const fileTabId = await evaluate(`${workspace}.currentWidget().id`);
        // 칸별 탭으로 터미널과 파일을 오간다.
        const paneTab = id => `[id=${JSON.stringify(`shell-tab-${id}`)}]`;
        await mouseClick(paneTab(firstTerminal));
        await until(`${workspace}.currentWidget()?.id === ${JSON.stringify(firstTerminal)}`);
        await mouseClick(paneTab(fileTabId));
        await until(`${workspace}.currentWidget()?.id === ${JSON.stringify(fileTabId)}`);

        // Step 4b: 내용이 같은 파일 목록은 행·선택·스크롤을 유지하고, 실제 파일 변경과 하위 폴더 펼침은 반영한다.
        await until('document.querySelector(".file-list .file-row.is-current")?.textContent.includes("README.md")');
        await evaluate('window.__sidebarBeforeRefresh = document.querySelector("[data-host=work]").firstChild');
        for (let n_refreshes = 0; n_refreshes < 3; n_refreshes += 1) {
            await evaluate(`${workspace}.refresh().then(() => true)`);
            assert.equal(await evaluate('document.querySelector("[data-host=work]").firstChild === window.__sidebarBeforeRefresh'), true);
        }
        sidebarFixture = await fs.mkdtemp(path.join(repo, '.paddock-smoke-files-'));
        const fixtureKey = `file://${sidebarFixture}`;
        await fs.writeFile(path.join(sidebarFixture, 'added.txt'), 'sidebar update\n');
        await until(`document.querySelector('.file-row[data-uri=${JSON.stringify(fixtureKey)}]')`);
        await mouseClick(`.file-row[data-uri=${JSON.stringify(fixtureKey)}]`);
        await until(`document.querySelector('.file-row[data-uri=${JSON.stringify(`${fixtureKey}/added.txt`)}]')`);
        await fs.rename(path.join(sidebarFixture, 'added.txt'), path.join(sidebarFixture, 'renamed.txt'));
        await until(`document.querySelector('.file-row[data-uri=${JSON.stringify(`${fixtureKey}/renamed.txt`)}]') && !document.querySelector('.file-row[data-uri=${JSON.stringify(`${fixtureKey}/added.txt`)}]')`);
        await fs.rm(sidebarFixture, { recursive: true, force: true });
        sidebarFixture = null;
        await until(`!document.querySelector('.file-row[data-uri=${JSON.stringify(fixtureKey)}]')`);
        await until('document.querySelector(".file-list .file-row.is-current")?.textContent.includes("README.md")');
        await evaluate('delete window.__sidebarBeforeRefresh');

        // Step 5: 한 폴더에 터미널이 8개를 넘으면 8줄 + "Show N more"로 묶이고 현재 터미널은 보인다.
        for (let n_added = 0; n_added < 7; n_added += 1) {
            await evaluate('document.querySelector(".folder-tab-add").click()');
            await until(`document.querySelectorAll('.folder-row[data-folder=${JSON.stringify(repoKey)}] .row-meta')[0]?.textContent === '›_ ${5 + n_added}'`);
        }
        await until('document.querySelector(".more-row")?.textContent.includes("Show 2 more")');
        assert(await evaluate('!!document.querySelector(".terminal-row.is-current")'));
        await untilPrompts();
        await capture('7-many-terminals');

        // Step 6: 보기 줄 — Source control(저장소·변경·본문 비교), Extensions(추천·검색)가 사이드바 안에 그려진다.
        await mouseClick('[data-view=scm]');
        await until('document.querySelector("[data-host=scm] .theia-view-container:not(.lm-mod-hidden)")?.getBoundingClientRect().height > 100');
        // 저장소가 하나면 보기에서 이름을 생략하므로 실제 선택된 Git 범위를 시험 폴더와 대조한다.
        await until(`${service('ScmService')}.selectedRepository?.provider.rootUri === ${JSON.stringify(repoKey)}`);
        await evaluate(`[...document.querySelectorAll('[data-host=scm] .scmItem, [data-host=scm] .theia-scm-resource')].find(n => n.textContent.includes('README.md') || n.textContent.includes('.md'))?.click()`);
        await capture('8-source-control');
        await evaluate('document.querySelector("[data-view=extensions]").click()');
        await until('document.querySelector("[data-host=extensions] .theia-view-container:not(.lm-mod-hidden)")?.getBoundingClientRect().height > 100');
        await until('document.querySelector("[data-host=extensions]").innerText.includes("Pencil") || document.querySelector("[data-host=extensions]").innerText.includes("pen.dev")', 200);
        await capture('9-extensions');
        await evaluate(`(() => { const input = document.querySelector('[data-host=extensions] input'); input.focus(); input.value = 'markdown preview'; input.dispatchEvent(new Event('input', { bubbles: true })); })()`);
        await until('document.querySelector("[data-host=extensions]").innerText.includes("Markdown Preview")', 200);
        await capture('10-extension-search');
        await evaluate('document.querySelector("[data-view=work]").click()');

        // Step 7: 폴더 메뉴 → 목록 이름 바꾸기 → 목록에서 빼기(확인). 폴더는 디스크에 남는다.
        // 폴더 행의 ＋(folder-add)는 새 터미널이고, 이름 바꾸기·목록에서 빼기는 옆 … 메뉴에 있다.
        const folderAction = `document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}] .row-action:not(.folder-add)')`;
        await until(folderAction);
        await evaluate(`${folderAction}.click()`);
        await until('document.querySelector(".paddock-menu .menu-note")');
        await capture('6-folder-menu');
        await evaluate(`[...document.querySelectorAll('.paddock-menu .menu-item')].find(n => n.textContent.includes('Rename in list')).click()`);
        await until('document.querySelector(".dialogBlock input")');
        await evaluate(`(() => { const input = document.querySelector('.dialogBlock input'); input.value = 'paddock app'; input.dispatchEvent(new Event('input', { bubbles: true })); })()`);
        // 입력 검증이 끝나기 전의 클릭은 무시되므로 창이 닫힐 때까지 확인을 누른다.
        await until(`(() => { document.querySelector('.dialogBlock .theia-button.main')?.click(); return !document.querySelector('.dialogBlock'); })()`);
        await until(`document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}] .row-name')?.textContent === 'paddock app'`);
        const before = await evaluate(n_terminals);
        await evaluate(`${folderAction}.click()`);
        await until('document.querySelector(".paddock-menu .menu-note")');
        await evaluate(`[...document.querySelectorAll('.paddock-menu .menu-item')].find(n => n.textContent.includes('Remove from list')).click()`);
        await until('document.querySelector(".dialogBlock .theia-button.main")');
        await evaluate('document.querySelector(".dialogBlock .theia-button.main").click()');
        await until(`!document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}]')`);
        await until(`${n_terminals} === ${before - 11}`);
        // 선택한 폴더를 빼도 아래 줄은 남고 작업 탭은 비어 있다.
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 0');
        assert.equal(await evaluate('document.querySelector(".folder-bar").classList.contains("lm-mod-hidden")'), false);

        // Work ＋는 Unassigned에 새 터미널을 열고 프로젝트 아래의 터미널 수는 유지한다.
        const n_rows = await evaluate(`document.querySelectorAll('.terminal-row:not(.unassigned-row)').length`);
        await evaluate('document.querySelector(".work-add").click()');
        await until('document.querySelectorAll(".unassigned-row .row-main").length === 1');
        assert.equal(await evaluate(`document.querySelectorAll('.terminal-row:not(.unassigned-row)').length`), n_rows);
        // 터미널 바로 위 ＋도 항상 보인다. 작업 폴더가 없으면 현재 위쪽 터미널의 내부 탭에 연다.
        assert.equal(await evaluate('document.querySelector(".folder-tab-add").getClientRects().length > 0'), true);
        const extraCwd = await evaluate(`${workspace}.readCwd(${workspace}.currentWidget())`);
        const extraId = await evaluate(`${workspace}.currentWidget().id`);
        await evaluate('document.querySelector(".folder-tab-add").click()');
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 2');
        assert.equal(await evaluate('document.querySelectorAll(".unassigned-row .row-main").length'), 1);
        assert.equal(await evaluate(`${workspace}.workFolderOf(${workspace}.currentWidget())`), null);
        assert.equal(await evaluate('document.querySelector(".folder-tabs .tab.is-active [role=tab]")?.textContent'), 'terminal 2');
        await mouseClick(`.folder-tabs [data-widget-id=${JSON.stringify(extraId)}]`);
        await until(`${workspace}.currentWidget()?.id === ${JSON.stringify(extraId)}`);
        assert.equal(await evaluate(`${workspace}.readCwd(${workspace}.currentWidget())`), extraCwd);
        assert.equal(await evaluate(`document.querySelectorAll('.terminal-row:not(.unassigned-row)').length`), n_rows);

        // Step 7b: 보고 있지 않은 터미널의 에이전트가 오래 일하다 멈추면 소리와 완료 표시(초록 점)가 붙고, 그 터미널을 열면 지워진다.
        // 홈의 추가 터미널에서 에이전트를 실행하므로 홈도 작업 폴더로 올라간다.
        await evaluate(`(() => { window.__paddockRings = 0; const activity = ${workspace}.agentActivity; const play = activity.playDoneSound.bind(activity); activity.playDoneSound = () => { window.__paddockRings += 1; play(); }; })()`);
        const extraTerminal = await evaluate(`${shell}.widgets.find(w => w.id.startsWith('terminal-') && w.isVisible).id`);
        const topRoot = await evaluate(`${workspace}.selectedTopTerminal`);
        await evaluate(`${workspace}.newExtraTerminal().then(() => true)`);
        const agentId = await evaluate(`${workspace}.currentWidget().id`);
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 1');
        await evaluate(`${shell}.activateWidget(${JSON.stringify(topRoot)}).then(() => true)`);
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 2');
        await evaluate(`${shell}.activateWidget(${JSON.stringify(agentId)}).then(() => true)`);
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 1');
        await evaluate(`${shell}.getWidgetById('${agentId}').sendText(${JSON.stringify(`export PATH=${fakeBin}:$PATH; claude\n`)})`);
        await until(`${workspace}.programOf(${shell}.getWidgetById('${agentId}')) === 'claude'`, 60);
        await until(`document.querySelector('.folder-row[data-folder=${JSON.stringify(homeKey)}]')`, 60);
        // 사용자가 Enter를 친 것처럼 입력 이벤트도 낸다(sendText는 입력 이벤트를 거치지 않는다).
        await evaluate(`(() => { const t = ${shell}.getWidgetById('${agentId}'); t.sendText('fix the bug\\r'); t.onDataEmitter.fire('fix the bug\\r'); })()`);
        await evaluate(`${shell}.activateWidget('${extraTerminal}').then(() => true)`);
        await until(`document.querySelector('.terminal-row[data-widget-id="${agentId}"]')?.classList.contains('is-done')`, 150);
        assert.equal(await evaluate('window.__paddockRings'), 1);
        await capture('12-agent-done');
        await evaluate(`${shell}.activateWidget('${agentId}').then(() => true)`);
        await until(`!document.querySelector('.terminal-row[data-widget-id="${agentId}"]')?.classList.contains('is-done')`);
        await fs.rm(fakeBin, { recursive: true, force: true });

        // Step 8: 원격 표시는 Theia 원격 선택 명령을 연다.
        await evaluate('document.querySelector(".remote-indicator").click()');
        await until('document.querySelector(".quick-input-widget")?.style.display !== "none"');
        await capture('11-remote-pick');
        await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
        // 다른 창의 목록 요청이 멈춰도 두 ＋는 각각 자기 창의 터미널을 바로 열어야 한다.
        await evaluate(`(() => { window.__workPresenceFetch = window.fetch; window.fetch = (input, options) => String(input).includes('/paddock/work-presence') ? new Promise(() => {}) : window.__workPresenceFetch(input, options); })()`);
        const folderKey = await evaluate('document.querySelector(".folder-bar").dataset.folder');
        const n_workTabs = await evaluate('document.querySelectorAll(".folder-tabs [role=tab]").length');
        await evaluate('document.querySelector(".folder-tab-add").click()');
        await until(`document.querySelectorAll('.folder-tabs [role=tab]').length === ${n_workTabs + 1}`);
        assert.equal(await evaluate(`${workspace}.workFolderOf(${workspace}.currentWidget())`), folderKey);
        const newWorkTerminalId = await evaluate(`${workspace}.currentWidget().id`);
        const n_extraTabs = await evaluate('document.querySelectorAll(".unassigned-row .row-main").length');
        assert.equal(await evaluate('document.querySelector(".work-add").getClientRects().length > 0'), true);
        await evaluate('document.querySelector(".work-add").click()');
        await until(`document.querySelectorAll('.unassigned-row .row-main').length === ${n_extraTabs + 1}`);
        assert.equal(await evaluate(`${workspace}.workFolderOf(${workspace}.currentWidget())`), null);
        const previousTopId = await evaluate('document.querySelector(".unassigned-row .row-main").dataset.widgetId');
        const newestTopId = await evaluate('document.querySelector(".unassigned-row:last-child .row-main").dataset.widgetId');
        await mouseClick('.unassigned-row .row-main');
        await until(`${workspace}.currentWidget()?.id === ${JSON.stringify(previousTopId)}`);
        await mouseClick('.unassigned-row:last-child .row-main');
        await until(`${workspace}.currentWidget()?.id === ${JSON.stringify(newestTopId)}`);
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length === 1');
        assert.equal(await evaluate('document.querySelector(".folder-bar").classList.contains("lm-mod-hidden")'), false);
        assert.equal(await evaluate('document.querySelector(".folder-tab-add").getClientRects().length > 0'), true);
        assert.equal(await evaluate('document.querySelector(".folder-bar-actions").hidden'), false);
        // Unassigned 묶음에서 나누기 버튼을 누르면 새 터미널이 그 묶음의 내부 탭으로 들어간다.
        const n_innerBefore = await evaluate(`${workspace}.innerTabIds(${JSON.stringify(newestTopId)}).length`);
        await mouseClick('.folder-split-right');
        await until(`${workspace}.innerTabIds(${JSON.stringify(newestTopId)}).length === ${n_innerBefore + 1}`);
        await until(`document.querySelectorAll('.unassigned-row .row-main').length === ${n_extraTabs + 1}`);
        await evaluate(`${shell}.activateWidget(${JSON.stringify(newWorkTerminalId)}).then(() => true)`);
        await until('document.querySelectorAll(".folder-tabs [role=tab]").length > 0');
        assert.equal(await evaluate('document.querySelector(".folder-tab-add").getClientRects().length > 0'), true);
        assert.equal(await evaluate('document.querySelector(".folder-bar-actions").hidden'), false);
        await evaluate('window.fetch = window.__workPresenceFetch; delete window.__workPresenceFetch');
        console.log('PASS: 창 틀·메모리·AI 사용량 게이지, 상태 줄 항목 켜고 끄기, 첫 실행 빈 목록 안내, 드래그 복사, 에이전트 실행 시 작업 폴더 승격, 작업 폴더 탭 줄 ＋, 분할 단축키, 파일 경로 줄, 많은 터미널 묶기, SCM·확장(추천·검색), 목록 이름 바꾸기·빼기, 추가 터미널, 에이전트 완료 알림, 원격 선택, 목록 동기화 지연 중 두 ＋');
    } finally {
        socket.close();
        if (sidebarFixture) await fs.rm(sidebarFixture, { recursive: true, force: true });
    }
}
main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
