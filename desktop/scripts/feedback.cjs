const fs = require('node:fs/promises');
const net = require('node:net');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');
const esbuild = require('esbuild');

const projectRoot = path.resolve(__dirname, '../..');
const memoRoot = path.join(projectRoot, 'out', 'memo');

function getPort() {
    const result = new Promise((resolve, reject) => {
        const server = net.createServer();
        server.once('error', reject);
        server.listen(0, '127.0.0.1', () => {
            const port = server.address().port;
            server.close(() => resolve(port));
        });
    });
    return result;
}

async function buildOverlay() {
    const output = await esbuild.build({
        entryPoints: [path.join(__dirname, '..', 'feedback', 'overlay.jsx')],
        bundle: true,
        format: 'iife',
        platform: 'browser',
        write: false,
        define: { 'process.env.NODE_ENV': '"development"' },
        plugins: [{
            name: 'agentation-modal-host',
            setup(build) {
                build.onLoad({ filter: /agentation\/dist\/index\.m?js$/ }, async (args) => {
                    const source = await fs.readFile(args.path, 'utf8');
                    const original = '    document.body\n  );\n}';
                    const offset = source.lastIndexOf(original);
                    if (offset < 0) throw new Error('Agentation의 부착 위치가 변경됐습니다. 피드백 도구 연결을 확인하세요.');
                    const dragOriginal = '            isMultiSelect: true,\n            // Forensic data from first element';
                    if (!source.includes(dragOriginal)) throw new Error('Agentation의 드래그 선택 자료가 변경됐습니다. 피드백 도구 연결을 확인하세요.');
                    const dragUpdated = `            isMultiSelect: true,\n            elementBoundingBoxes: finalElements.map(({ rect }) => ({ x: rect.left, y: rect.top + window.scrollY, width: rect.width, height: rect.height })),\n            // Forensic data from first element`;
                    const withDragBoxes = source.replace(dragOriginal, dragUpdated);
                    const portalOffset = withDragBoxes.lastIndexOf(original);
                    return {
                        contents: `${withDragBoxes.slice(0, portalOffset)}    window.__paddockFeedbackPortal || document.body\n  );\n}${withDragBoxes.slice(portalOffset + original.length)}`,
                        loader: 'js',
                        resolveDir: path.dirname(args.path),
                    };
                });
            },
        }],
    });
    return output.outputFiles[0].text;
}

class DebugPage {
    constructor(
        socket,
    ) {
        this.socket = socket;
        this.pending = new Map();
        this.n_requests = 0;
        this.socket.addEventListener('message', ({ data }) => {
            const message = JSON.parse(data);
            if (typeof message.id === 'number' && this.pending.has(message.id)) {
                this.pending.get(message.id)(message);
                this.pending.delete(message.id);
            } else if (message.method === 'Runtime.bindingCalled') {
                void this.handleBinding(message.params.payload);
            } else if (message.method === 'Page.loadEventFired' && this.onLoad) {
                void this.onLoad();
            }
        });
    }

    send(
        method,
        params = {},
    ) {
        const id = ++this.n_requests;
        const result = new Promise((resolve) => {
            this.pending.set(id, resolve);
            this.socket.send(JSON.stringify({ id, method, params }));
        });
        return result;
    }

    async evaluate(
        expression,
    ) {
        const result = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
        if (result.result?.exceptionDetails) throw new Error(result.result.exceptionDetails.text);
        return result.result?.result?.value;
    }

    async handleBinding(
        payload,
    ) {
        let request;
        let reply;
        try {
            if (Buffer.byteLength(payload) > 512_000) throw new Error('메모가 너무 깁니다.');
            request = JSON.parse(payload);
            if (request.operation === 'list') reply = { ok: true, ids: await listMemoIds() };
            else if (request.operation === 'save') reply = { ok: true, ...await saveMemo(this, request.payload) };
            else throw new Error('지원하지 않는 피드백 요청입니다.');
        } catch (error) {
            reply = { ok: false, message: error instanceof Error ? error.message : String(error) };
        }
        if (typeof request?.id === 'number') {
            await this.evaluate(`window.__paddockFeedbackReply(${request.id}, ${JSON.stringify(reply)})`).catch(() => undefined);
        }
    }
}

async function listMemoIds() {
    const ids = [];
    const days = await fs.readdir(memoRoot, { withFileTypes: true }).catch(() => []);
    for (const day of days) {
        if (!day.isDirectory() || !/^\d{4}-\d{2}-\d{2}$/.test(day.name)) continue;
        const files = await fs.readdir(path.join(memoRoot, day.name));
        for (const name of files) {
            if (!name.endsWith('.json')) continue;
            try {
                const note = JSON.parse(await fs.readFile(path.join(memoRoot, day.name, name), 'utf8'));
                if (typeof note.id === 'string') ids.push(note.id);
            } catch {
                // 손상된 메모 파일은 다른 메모의 표시를 막지 않는다.
            }
        }
    }
    return ids;
}

function readCrop(
    note,
    image,
) {
    const boxes = note.targets.map((target) => target.box)
        .filter((box) => [box.x, box.y, box.width, box.height].every(Number.isFinite) && box.width > 0 && box.height > 0);
    let crop = null;
    if (boxes.length > 0 && image.length >= 24) {
        const imageWidth = image.readUInt32BE(16);
        const imageHeight = image.readUInt32BE(20);
        const scaleX = imageWidth / note.viewport.width;
        const scaleY = imageHeight / note.viewport.height;
        const left = Math.max(0, Math.floor(Math.min(...boxes.map((box) => box.x - 24)) * scaleX));
        const top = Math.max(0, Math.floor(Math.min(...boxes.map((box) => box.y - 24)) * scaleY));
        const right = Math.min(imageWidth, Math.ceil(Math.max(...boxes.map((box) => box.x + box.width + 24)) * scaleX));
        const bottom = Math.min(imageHeight, Math.ceil(Math.max(...boxes.map((box) => box.y + box.height + 24)) * scaleY));
        if (right > left && bottom > top) crop = { left, top, width: right - left, height: bottom - top };
    }
    return crop;
}

async function cropImage(
    source,
    destination,
    crop,
) {
    const result = new Promise((resolve) => {
        const child = spawn('sips', [
            '--cropToHeightWidth', String(crop.height), String(crop.width),
            '--cropOffset', String(crop.top), String(crop.left),
            source, '--out', destination,
        ], { stdio: 'ignore' });
        child.once('error', () => resolve(false));
        child.once('exit', (code) => resolve(code === 0));
    });
    return result;
}

async function capturePage(
    page,
) {
    await page.evaluate(`document.querySelectorAll('[data-paddock-feedback], [data-agentation-root], [data-agentation-toolbar]').forEach((item) => { item.dataset.feedbackDisplay = item.style.getPropertyValue('display'); item.dataset.feedbackDisplayPriority = item.style.getPropertyPriority('display'); item.style.setProperty('display', 'none', 'important'); })`);
    let image;
    try {
        await new Promise((resolve) => setTimeout(resolve, 100));
        const result = await page.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
        if (!result.result?.data) throw new Error('화면 캡처에 실패했습니다.');
        image = Buffer.from(result.result.data, 'base64');
    } finally {
        await page.evaluate(`document.querySelectorAll('[data-feedback-display]').forEach((item) => { if (item.dataset.feedbackDisplay) item.style.setProperty('display', item.dataset.feedbackDisplay, item.dataset.feedbackDisplayPriority); else item.style.removeProperty('display'); delete item.dataset.feedbackDisplay; delete item.dataset.feedbackDisplayPriority; })`).catch(() => undefined);
    }
    return image;
}

async function saveMemo(
    page,
    note,
) {
    if (!note || typeof note.id !== 'string' || !/^[a-zA-Z0-9_-]{1,128}$/.test(note.id)) throw new Error('메모 식별자가 올바르지 않습니다.');
    if (typeof note.comment !== 'string' || !note.comment.trim() || note.comment.length > 5000) throw new Error('메모는 1~5000자로 작성하세요.');
    if (typeof note.takenAt !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(note.takenAt)
        || !Number.isFinite(Date.parse(note.takenAt)) || !Array.isArray(note.targets) || note.targets.length > 50) throw new Error('메모 자료가 올바르지 않습니다.');
    if (!Number.isFinite(note.viewport?.width) || !Number.isFinite(note.viewport?.height)
        || note.viewport.width < 1 || note.viewport.height < 1) throw new Error('화면 크기가 올바르지 않습니다.');
    const day = note.takenAt.slice(0, 10);
    const stamp = note.takenAt.replace(/[-:]/g, '').replace(/\.\d+Z$/, '').replace('T', '-');
    const base = `${stamp}-${note.id}`;
    const folder = path.join(memoRoot, day);
    const jsonPath = path.join(folder, `${base}.json`);
    const imagePath = path.join(folder, `${base}.png`);
    const cropPath = path.join(folder, `${base}.crop.png`);
    await fs.mkdir(folder, { recursive: true });
    let image = null;
    try {
        image = await capturePage(page);
        await fs.writeFile(imagePath, image, { flag: 'wx' });
        const crop = readCrop(note, image);
        if (crop) {
            const cropped = await cropImage(imagePath, cropPath, crop);
            if (!cropped) await fs.rm(cropPath, { force: true });
        }
    } catch (error) {
        image = null;
        note.capture = `failed: ${error instanceof Error ? error.message : String(error)}`;
    }
    await fs.writeFile(jsonPath, JSON.stringify(note, null, 2), { flag: 'wx' });
    console.log(`피드백 저장: ${jsonPath}`);
    return { jsonPath, imagePath: image ? imagePath : '', cropPath: await fs.access(cropPath).then(() => cropPath).catch(() => '') };
}

async function waitForPage(
    port,
    child,
) {
    let target;
    for (let n_attempts = 0; n_attempts < 200 && !target; n_attempts += 1) {
        if (child.exitCode !== null) throw new Error('제품 실행이 종료됐습니다.');
        try {
            const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
            target = pages.find((page) => page.type === 'page' && page.url.includes('/frontend/index.html'));
        } catch {
            await new Promise((resolve) => setTimeout(resolve, 100));
        }
        if (!target) await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (!target) throw new Error('피드백 화면 연결 시간이 초과됐습니다.');
    return target;
}

async function main() {
    const [port, overlay] = await Promise.all([getPort(), buildOverlay()]);
    const profile = path.join(projectRoot, 'out', 'feedback-profile');
    await fs.mkdir(profile, { recursive: true });
    const child = spawn(process.execPath, [
        path.join(__dirname, 'start.cjs'),
        `--remote-debugging-port=${port}`,
        '--remote-debugging-address=127.0.0.1',
        `--user-data-dir=${profile}`,
        ...process.argv.slice(2),
    ], { cwd: projectRoot, stdio: 'inherit', detached: process.platform !== 'win32' });
    // 프로세스 그룹 종료는 POSIX 전용이다. Windows는 taskkill로 앱의 하위 프로세스까지 끝낸다.
    const stop = () => {
        if (process.platform === 'win32') {
            spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
        } else {
            try { process.kill(-child.pid, 'SIGTERM'); } catch { /* 이미 종료됨 */ }
        }
    };
    process.once('SIGINT', stop);
    process.once('SIGTERM', stop);
    try {
        const target = await waitForPage(port, child);
        const socket = new WebSocket(target.webSocketDebuggerUrl);
        await new Promise((resolve) => socket.addEventListener('open', resolve, { once: true }));
        const page = new DebugPage(socket);
        await Promise.all([page.send('Runtime.enable'), page.send('Page.enable')]);
        await page.send('Runtime.addBinding', { name: 'paddockFeedbackBridge' });
        page.onLoad = () => page.evaluate(overlay).catch((error) => console.error('피드백 화면 재연결 실패:', error));
        for (let n_attempts = 0; n_attempts < 100; n_attempts += 1) {
            const ready = await page.evaluate('!!document.body && !!document.querySelector(".paddock-shell")').catch(() => false);
            if (ready) break;
            await new Promise((resolve) => setTimeout(resolve, 100));
        }
        await page.evaluate(overlay);
        console.log('제품 피드백 모드: 우하단 도구를 켜고 요소를 선택해 메모를 남기세요.');
        console.log(`저장 위치: ${memoRoot}`);
        await new Promise((resolve) => child.once('exit', resolve));
        socket.close();
    } finally {
        stop();
    }
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
