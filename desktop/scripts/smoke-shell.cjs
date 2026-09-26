// 실행 중인 Herdr(시험용 설정, --remote-debugging-port)를 CDP로 조작해 작업 폴더 흐름을 확인한다.
// 사용: HERDR_CDP_PORT=9333 HERDR_SMOKE_REPO=/path/to/git/repo node scripts/smoke-shell.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');

async function main() {
    const port = process.env.HERDR_CDP_PORT;
    const repo = process.env.HERDR_SMOKE_REPO || path.join(__dirname, '..', '..');
    assert(port, '별도의 시험 설정으로 실행한 앱의 HERDR_CDP_PORT를 지정하세요.');
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
    ) {
        // Control+Shift 조합을 문서에 보내 Herdr 단축키를 누른다.
        for (const type of ['rawKeyDown', 'keyUp']) {
            await send('Input.dispatchKeyEvent', { type, modifiers: 2 | 8, key: text, code, windowsVirtualKeyCode: keyCode });
        }
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
    // 캡처 전에 보이는 터미널마다 셸 프롬프트가 떴는지 기다린다(빈 화면을 결함으로 오해하지 않게).
    const untilPrompts = () => until(`${shell}.widgets.filter(w => w.id.startsWith('terminal-') && w.isVisible).every(w => { const b = w.term.buffer.active; for (let i = 0; i < b.length; i++) { if (/[$%#>] *$/.test(b.getLine(i).translateToString(true))) return true; } return false; })`, 120);
    try {
        // Step 1: 창 틀 — 레일 없음, 보기 줄 3개, 추가 터미널 줄, 상태 줄 메모리.
        assert(await evaluate('!!document.querySelector(".herdr-shell")'));
        assert.equal(await evaluate('document.querySelectorAll(".activity-rail, .rail-button").length'), 0);
        assert.equal(await evaluate('document.querySelectorAll(".view-bar [data-view]").length'), 3);
        assert.equal(await evaluate('Math.round(document.querySelector(".herdr-tabbar").getBoundingClientRect().height)'), 28);
        await until('/^\\d+%$/.test(document.querySelector("[data-meter=memory] .meter-percent").textContent)');
        // AI 사용량: Claude는 첫 실행에 켜져 게이지(값이 아직 없으면 —)나 켜기 버튼 하나로 선다.
        // 값이 있는 게이지는 채움 폭이 숫자와 같고, 말풍선이 남은 양을 말한다.
        await until('!!document.querySelector(".ai-usage [data-meter^=claude], .ai-usage .usage-toggle")', 60);
        const usageMeters = await evaluate('[...document.querySelectorAll(".ai-usage .meter:not(.is-empty)")].map(m => ({ percent: m.querySelector(".meter-percent").textContent, width: m.querySelector(".meter-fill").style.width, title: m.title }))');
        for (const meter of usageMeters) {
            assert.match(meter.percent, /^\d+%$/);
            assert.equal(meter.width, meter.percent);
            assert.match(meter.title, /% left/);
        }
        // 첫 실행: 터미널이 열린 폴더(홈)가 곧 작업 폴더라 목록이 비어 있지 않고, 첫 터미널은 그 아래 작업 터미널이다.
        await until(`document.querySelector('.folder-row[data-folder=${JSON.stringify(homeKey)}]')`);
        await until('document.querySelector(".terminal-row.is-current")');
        assert.equal(await evaluate('document.querySelectorAll(".work-empty").length'), 0);
        assert.equal(await evaluate('document.querySelectorAll("#tab-strip [role=tab]").length'), 0);
        await untilPrompts();
        await capture('1-first-run');

        // Step 2: 그 작업 터미널에서 레포로 cd → 새 작업 터미널이 그 폴더를 따라간다.
        await until(`${shell}.currentWidget?.id?.startsWith('terminal-')`);
        await evaluate(`${shell}.currentWidget.sendText(${JSON.stringify(`cd ${repo}\n`)})`);
        await until(`document.querySelector(".new-work")?.dataset.followFolder === ${JSON.stringify(repoKey)}`);
        await capture('2-follow-folder');
        // 터미널 글자를 선택하면 바로 클립보드에 복사된다(terminal.integrated.copyOnSelection).
        await evaluate(`${shell}.currentWidget.sendText('echo HERDR_COPY_CHECK\\n')`);
        await until(`(() => { const b = ${shell}.currentWidget.term.buffer.active; for (let i = 0; i < b.length; i++) { if (b.getLine(i).translateToString(true) === 'HERDR_COPY_CHECK') { ${shell}.currentWidget.term.select(0, i, 16); return true; } } return false; })()`);
        await until(`navigator.clipboard.readText().then(text => text === 'HERDR_COPY_CHECK')`);
        await evaluate('document.querySelector(".new-work").click()');
        await until(`document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}]')`);
        await until('document.querySelector(".terminal-row.is-current")');
        await until('document.querySelector(".path-bar.is-active .path-branch")?.textContent.length > 0');
        await untilPrompts();
        await capture('3-working');

        // Step 3: Ctrl+Shift+` 아래 칸, Ctrl+Shift+5 옆 칸 — 같은 폴더의 작업 터미널이 늘고 칸이 나뉜다.
        const n_bars = await evaluate('document.querySelectorAll(".path-bar").length');
        await key('Backquote', '`', 192);
        await until(`document.querySelectorAll(".path-bar").length === ${n_bars + 1}`);
        await key('Digit5', '5', 53);
        await until(`document.querySelectorAll(".path-bar").length === ${n_bars + 2}`);
        // 홈 폴더의 첫 작업 터미널 1개 + 레포 폴더 3개.
        assert.equal(await evaluate(`document.querySelectorAll('.terminal-row').length`), 4);
        await untilPrompts();
        await capture('4-5-split');

        // Step 4: 파일 구획에서 README를 열면 본문 경로 줄이 파일을 가리킨다.
        await evaluate(`[...document.querySelectorAll('.file-row')].find(n => n.textContent.includes('README.md')).click()`);
        await until(`document.querySelector('.path-bar.is-active .path-item')?.textContent === 'README.md'`);

        // Step 5: 한 폴더에 터미널이 8개를 넘으면 8줄 + "Show N more"로 묶이고 현재 터미널은 보인다.
        for (let n_added = 0; n_added < 7; n_added += 1) {
            await evaluate('document.querySelector(".new-work").click()');
            await until(`document.querySelectorAll('.folder-row[data-folder=${JSON.stringify(repoKey)}] .row-meta')[0]?.textContent === '›_ ${4 + n_added}'`);
        }
        await until('document.querySelector(".more-row")?.textContent.includes("Show 1 more")');
        assert(await evaluate('!!document.querySelector(".terminal-row.is-current")'));
        await untilPrompts();
        await capture('7-many-terminals');

        // Step 6: 보기 줄 — Source control(저장소·변경·본문 비교), Extensions(추천·검색)가 사이드바 안에 그려진다.
        await evaluate('document.querySelector("[data-view=scm]").click()');
        await until('document.querySelector("[data-host=scm] .theia-view-container:not(.lm-mod-hidden)")?.getBoundingClientRect().height > 100');
        await until('document.querySelector("[data-host=scm]").innerText.includes("herdr-terminal")');
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
        const folderAction = `document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}] .row-action')`;
        await until(folderAction);
        await evaluate(`${folderAction}.click()`);
        await until('document.querySelector(".herdr-menu .menu-note")');
        await capture('6-folder-menu');
        await evaluate(`[...document.querySelectorAll('.herdr-menu .menu-item')].find(n => n.textContent.includes('Rename in list')).click()`);
        await until('document.querySelector(".dialogBlock input")');
        await evaluate(`(() => { const input = document.querySelector('.dialogBlock input'); input.value = 'herdr app'; input.dispatchEvent(new Event('input', { bubbles: true })); document.querySelector('.dialogBlock .theia-button.main').click(); })()`);
        await until(`document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}] .row-name')?.textContent === 'herdr app'`);
        const before = await evaluate(n_terminals);
        await evaluate(`${folderAction}.click()`);
        await until('document.querySelector(".herdr-menu .menu-note")');
        await evaluate(`[...document.querySelectorAll('.herdr-menu .menu-item')].find(n => n.textContent.includes('Remove from list')).click()`);
        await until('document.querySelector(".dialogBlock .theia-button.main")');
        await evaluate('document.querySelector(".dialogBlock .theia-button.main").click()');
        await until(`!document.querySelector('.folder-row[data-folder=${JSON.stringify(repoKey)}]')`);
        await until(`${n_terminals} === ${before - 10}`);

        // 위쪽 ＋는 작업 목록에 들어가지 않는 추가 터미널을 연다.
        const n_rows = await evaluate(`document.querySelectorAll('.terminal-row').length`);
        await evaluate('document.querySelector(".tab-add").click()');
        await until('document.querySelectorAll("#tab-strip [role=tab]").length === 1');
        assert.equal(await evaluate(`document.querySelectorAll('.terminal-row').length`), n_rows);

        // Step 7b: 보고 있지 않은 터미널의 에이전트가 오래 일하다 멈추면 소리와 완료 표시(초록 점)가 붙고, 그 터미널을 열면 지워진다.
        // 실제 에이전트 대신 이름이 claude인 가짜 프로그램(7초 스피너 뒤 입력 대기)을 쓴다.
        const fakeBin = await fs.mkdtemp(path.join(require('node:os').tmpdir(), 'herdr-fake-agent-'));
        await fs.writeFile(path.join(fakeBin, 'claude'), '#!/bin/bash\necho ready\nwhile read -r line; do end=$((SECONDS+7)); while [ $SECONDS -lt $end ]; do printf "\\r working"; sleep 0.15; done; printf "\\r done\\n> "; done\n', { mode: 0o755 });
        const workspace = service('HerdrWorkspace');
        await evaluate(`(() => { window.__herdrRings = 0; const hw = ${workspace}; const play = hw.playDoneSound.bind(hw); hw.playDoneSound = () => { window.__herdrRings += 1; play(); }; })()`);
        const firstTerminal = await evaluate(`${shell}.widgets.find(w => w.id.startsWith('terminal-') && w.isVisible).id`);
        await evaluate(`${workspace}.newWorkTerminal({ folderKey: ${JSON.stringify(homeKey)} }).then(() => true)`);
        const agentId = await evaluate(`${workspace}.currentWidget().id`);
        await evaluate(`${shell}.getWidgetById('${agentId}').sendText(${JSON.stringify(`export PATH=${fakeBin}:$PATH; claude\n`)})`);
        await until(`${workspace}.programOf(${shell}.getWidgetById('${agentId}')) === 'claude'`, 60);
        // 사용자가 Enter를 친 것처럼 입력 이벤트도 낸다(sendText는 입력 이벤트를 거치지 않는다).
        await evaluate(`(() => { const t = ${shell}.getWidgetById('${agentId}'); t.sendText('fix the bug\\r'); t.onDataEmitter.fire('fix the bug\\r'); })()`);
        await evaluate(`${shell}.activateWidget('${firstTerminal}').then(() => true)`);
        await until(`document.querySelector('.terminal-row[data-widget-id="${agentId}"]')?.classList.contains('is-done')`, 150);
        assert.equal(await evaluate('window.__herdrRings'), 1);
        await capture('12-agent-done');
        await evaluate(`${shell}.activateWidget('${agentId}').then(() => true)`);
        await until(`!document.querySelector('.terminal-row[data-widget-id="${agentId}"]')?.classList.contains('is-done')`);
        await fs.rm(fakeBin, { recursive: true, force: true });

        // Step 8: 원격 표시는 Theia 원격 선택 명령을 연다.
        await evaluate('document.querySelector(".remote-indicator").click()');
        await until('document.querySelector(".quick-input-widget")?.style.display !== "none"');
        await capture('11-remote-pick');
        await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
        console.log('PASS: 창 틀·메모리·AI 사용량 게이지, 첫 실행 작업 폴더, 폴더 따라가기, 드래그 복사, 작업 터미널, 분할 단축키, 파일 경로 줄, 많은 터미널 묶기, SCM·확장(추천·검색), 목록 이름 바꾸기·빼기, 추가 터미널, 에이전트 완료 알림, 원격 선택');
    } finally {
        socket.close();
    }
}
main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
