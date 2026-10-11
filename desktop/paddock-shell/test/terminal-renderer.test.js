const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

// Run real xterm/WebGL instances in Electron: mocked models cannot detect stale glyph coordinates.
async function checkRendering(
    paths,
) {
    const assert = require('node:assert/strict');
    const { Terminal } = require(paths.xterm);
    const { WebglAddon } = require(paths.webgl);
    const { refreshGlyphs, synchronizeTextureAtlas } = require(paths.repair);
    const frames = async () => {
        for (let i = 0; i < 4; i++) await new Promise(requestAnimationFrame);
    };
    const write = async (term, text) => {
        await new Promise(resolve => term.write(text, resolve));
        await frames();
    };
    const instances = [];
    const create = async (fontSize = 16) => {
        const host = document.body.appendChild(document.createElement('div'));
        Object.assign(host.style, { position: 'absolute', top: '0', left: '0' });
        const term = new Terminal({
            cols: 90, rows: 26, fontSize, cursorBlink: false,
            fontFamily: "'DejaVu Sans Mono', 'Malgun Gothic', 'Noto Sans CJK KR', monospace",
            theme: { background: '#1e1e1e', foreground: '#dddddd' },
        });
        const registration = synchronizeTextureAtlas(term);
        term.open(host);
        const addon = new WebglAddon(true);
        term.loadAddon(addon);
        const instance = { host, term, addon, registration };
        instances.push(instance);
        await frames();
        return instance;
    };
    const pixels = ({ addon }, requireGlyphs = true) => {
        const gl = addon._renderer._gl;
        gl.finish();
        const data = new Uint8Array(gl.canvas.width * gl.canvas.height * 4);
        gl.readPixels(0, 0, gl.canvas.width, gl.canvas.height, gl.RGBA, gl.UNSIGNED_BYTE, data);
        assert.equal(gl.getError(), gl.NO_ERROR);
        if (requireGlyphs) assert.ok(data.some((value, index) => index % 4 !== 3 && value > 50), 'glyph pixels must exist');
        return data;
    };
    const buffer = term => Array.from({ length: term.rows }, (_, row) => term.buffer.active.getLine(row).translateToString(true));
    const dispose = instance => {
        instance.registration.dispose();
        instance.addon.dispose();
        instance.term.dispose();
        instance.host.remove();
        instances.splice(instances.indexOf(instance), 1);
    };
    const sample = '\x1b[?25l\x1b[2J\x1b[H' + Array.from({ length: 20 }, (_, row) =>
        `행 ${row} 안녕하세요 터미널 글자 위치 ABCDEFG 0123456789`).join('\r\n');
    const other = '\x1b[?25l\x1b[2J\x1b[H' + Array.from({ length: 20 }, (_, row) =>
        String.fromCharCode(0xac00 + row).repeat(40)).join('\r\n');
    const passed = [];
    try {
        const a = await create();
        const b = await create();
        assert.equal(a.addon._renderer._charAtlas, b.addon._renderer._charAtlas, 'test requires a shared atlas');
        await write(a.term, sample);
        const baseline = pixels(a);
        const originalBuffer = buffer(a.term);

        a.host.style.display = 'none';
        await frames();
        b.term.clearTextureAtlas();
        await write(b.term, other);
        a.host.style.display = '';
        a.term.refresh(0, a.term.rows - 1);
        await frames();
        assert.deepEqual(pixels(a), baseline, 'hidden owner must rebuild glyph positions after another owner clears');
        assert.deepEqual(buffer(a.term), originalBuffer);
        passed.push('TR01: shared clear preserves a hidden terminal and its buffer');

        const c = await create(17);
        assert.notEqual(a.addon._renderer._charAtlas, c.addon._renderer._charAtlas);
        await write(c.term, sample);
        const unrelated = pixels(c);
        b.term.clearTextureAtlas();
        await write(b.term, other);
        await frames();
        assert.deepEqual(pixels(c), unrelated);
        passed.push('TR02: clearing a shared atlas preserves unrelated font atlases');
        dispose(c);

        const closed = await create();
        dispose(closed);
        b.term.clearTextureAtlas();
        await write(b.term, other);
        await frames();
        assert.deepEqual(pixels(a), baseline);
        passed.push('TR03: closed terminals are removed from synchronization');

        // Bypass the public Terminal hook to reproduce the old failure and test the repair independently.
        b.addon.clearTextureAtlas();
        await write(b.term, other);
        a.term.refresh(0, a.term.rows - 1);
        await frames();
        assert.notDeepEqual(pixels(a, false), baseline, 'regression fixture must reproduce stale coordinates');
        refreshGlyphs(a.term);
        await frames();
        assert.deepEqual(pixels(a), baseline);
        assert.deepEqual(buffer(a.term), originalBuffer);
        passed.push('TR04: recovery repairs stale coordinates without rewriting output');

        b.addon.dispose();
        b.addon = { dispose() {} };
        b.term.clearTextureAtlas();
        await write(b.term, sample);
        refreshGlyphs(b.term);
        await frames();
        assert.ok(b.term.element.querySelector('.xterm-rows'), 'DOM fallback must remain usable');
        assert.deepEqual(pixels(a), baseline);
        passed.push('TR05: DOM fallback can clear and redraw without affecting WebGL owners');
        return passed;
    } finally {
        for (const instance of [...instances]) dispose(instance);
    }
}

if (process.versions.electron) {
    const { app, BrowserWindow } = require('electron');
    app.commandLine.appendSwitch('disable-features', 'CalculateNativeWinOcclusion');
    if (process.platform === 'linux') {
        app.commandLine.appendSwitch('use-angle', 'swiftshader');
        app.commandLine.appendSwitch('enable-unsafe-swiftshader');
    }
    app.whenReady().then(async () => {
        const window = new BrowserWindow({
            width: 1200, height: 1000,
            webPreferences: { nodeIntegration: true, contextIsolation: false, backgroundThrottling: false },
        });
        const css = fs.readFileSync(require.resolve('xterm/css/xterm.css'), 'utf8');
        await window.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(`<style>${css}body{margin:0;background:#1e1e1e}</style>`)}`);
        const paths = {
            xterm: require.resolve('xterm'),
            webgl: require.resolve('xterm-addon-webgl'),
            repair: path.resolve(__dirname, '..', 'terminal-renderer.js'),
        };
        const result = await window.webContents.executeJavaScript(`(${checkRendering.toString()})(${JSON.stringify(paths)})`);
        console.log(JSON.stringify(result));
        app.exit(0);
    }).catch(error => {
        console.error(error);
        app.exit(1);
    });
} else {
    const test = require('node:test');
    const { spawn } = require('node:child_process');
    test('TR01–TR05: real shared WebGL atlas, hidden tabs, recovery, disposal and DOM fallback', { timeout: 60000 }, async () => {
        const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-renderer-test-'));
        try {
            const child = spawn(require('electron'), ['--no-sandbox', `--user-data-dir=${profile}`, __filename], { stdio: ['ignore', 'pipe', 'pipe'] });
            let output = '';
            let errors = '';
            child.stdout.on('data', data => { output += data; });
            child.stderr.on('data', data => { errors += data; });
            const timer = setTimeout(() => child.kill(), 55000);
            const code = await new Promise((resolve, reject) => {
                child.on('error', reject);
                child.on('exit', resolve);
            }).finally(() => clearTimeout(timer));
            assert.equal(code, 0, `${output}\n${errors}`);
            const result = JSON.parse(output.trim().split('\n').at(-1));
            assert.equal(result.length, 5);
            for (const message of result) console.log(message);
        } finally {
            fs.rmSync(profile, { recursive: true, force: true });
        }
    });
}
