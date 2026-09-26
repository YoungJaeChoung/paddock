const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');

async function main() {
    const port = process.env.HERDR_CDP_PORT;
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
            }, 15000);
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
    ) {
        let result = false;
        for (let n_attempts = 0; n_attempts < 50 && !result; n_attempts += 1) {
            result = await evaluate(expression);
            if (!result) {
                await new Promise((resolve) => setTimeout(resolve, 100));
            }
        }
        assert(result, expression);
        return result;
    }
    const shell = 'window.theia.container.get([...window.theia.container._bindingDictionary._map.keys()].find(k => k.name === "ApplicationShell"))';
    try {
        assert(await evaluate('!!document.querySelector(".herdr-shell")'));
        assert.equal(await evaluate('document.querySelectorAll("webview").length'), 0);
        assert.equal(await evaluate('[...document.querySelectorAll(".lm-TabBar")].filter(n => n.getBoundingClientRect().height > 0).length'), 0);
        const n_before = await evaluate(`${shell}.widgets.filter(w => w.id.startsWith('terminal-')).length`);
        await evaluate('document.querySelector(".tab-add").click()');
        await until(`${shell}.widgets.filter(w => w.id.startsWith('terminal-')).length === ${n_before + 1}`);
        await until(`${shell}.mainPanel.currentTitle?.owner.id.startsWith('terminal-')`);
        const id = await evaluate(`${shell}.mainPanel.currentTitle.owner.id`);
        const terminal = `${shell}.getWidgetById(${JSON.stringify(id)})`;
        await evaluate(`${terminal}.sendText(${JSON.stringify("printf 'HERDR_RUNTIME_%s\\n' OK\n")})`);
        await until(`${terminal}.buffer.getLines(0, ${terminal}.buffer.length, true).some(line => line === 'HERDR_RUNTIME_OK')`);
        await evaluate(`${terminal}.sendText(${JSON.stringify('exit\n')})`);
        await until(`!${shell}.getWidgetById(${JSON.stringify(id)})`);
        await until(`document.querySelectorAll('.sidebar .side-item').length === ${n_before}`);
        const captureDirectory = path.join(__dirname, '..', 'captures');
        await fs.mkdir(captureDirectory, { recursive: true });
        const screenshot = await send('Page.captureScreenshot', { format: 'png' });
        await fs.writeFile(path.join(captureDirectory, 'shell-smoke.png'), Buffer.from(screenshot.data, 'base64'));
        console.log('PASS: Herdr 창, 중첩 IDE 없음, 터미널 생성·출력·자연 종료·목록 갱신');
    } finally {
        socket.close();
    }
}
main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
