const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
    return { promise, resolve, reject };
}

async function settle() {
    await new Promise(setImmediate);
}

function fixture() {
    const channels = new Map();
    const timers = new Map();
    const processes = new Set([7]);
    const exitListeners = new Set();
    const dataListeners = new Set();
    let n_timers = 0;
    const backend = {
        closed: [], created: [], nextId: 8, automaticClose: true, failCreate: false,
        create: async options => {
            backend.created.push(options);
            if (backend.failCreate) throw new Error('Shell could not start');
            if (backend.createGate) await backend.createGate.promise;
            const id = backend.nextId++;
            processes.add(id);
            return id;
        },
        attach: async id => processes.has(id) ? id : -1,
        getProcessInfo: async id => {
            if (backend.processInfoGate) await backend.processInfoGate.promise;
            if (backend.failProcessInfo) throw new Error('Shell information could not be read');
            if (!processes.has(id)) throw new Error('Shell no longer exists');
            return { executable: '/bin/bash', arguments: [] };
        },
        close: async id => {
            backend.closed.push(id);
            if (terminal.terminalId === id) terminal.finalizeAndDispose();
            if (backend.n_ignoredCloses > 0) backend.n_ignoredCloses -= 1;
            else if (backend.automaticClose) {
                processes.delete(id);
                for (const listener of [...exitListeners]) listener({ terminalId: id });
                closeChannel(id);
            }
        },
    };
    function channel(id) {
        if (!channels.has(id)) {
            const listeners = new Set();
            const messages = new Set();
            channels.set(id, {
                listeners, messages, sent: [], closed: false,
                onClose: listener => { listeners.add(listener); return { dispose: () => listeners.delete(listener) }; },
                onMessage: listener => { messages.add(listener); return { dispose: () => messages.delete(listener) }; },
                close: () => closeChannel(id),
                getWriteBuffer: () => ({ writeString: data => ({ commit: () => channel(id).sent.push(data) }) }),
            });
        }
        return channels.get(id);
    }
    function closeChannel(id) {
        channel(id).closed = true;
        for (const listener of [...channel(id).listeners]) listener();
    }
    class DisposableCollection {
        constructor() { this.entries = []; }
        push(entry) { this.entries.push(entry); }
        dispose() { for (const entry of this.entries.splice(0)) entry.dispose(); }
    }
    class TerminalWidgetImpl {
        constructor() {
            this.id = 'terminal-same-widget';
            this._terminalId = 7;
            this.options = { id: this.id, created: 'original', shellPath: '/bin/bash', shellArgs: ['-i'], cwd: '/work', title: 'Claude · A', paddockAccount: { id: 'a', provider: 'claude', label: 'A', runtime: 'native' } };
            this.title = { label: 'Claude · A', caption: 'Claude · A' };
            this.term = {
                options: { disableStdin: false }, write: (_text, callback) => queueMicrotask(callback),
                onData: listener => { dataListeners.add(listener); return { dispose: () => dataListeners.delete(listener) }; },
                onBinary: () => ({ dispose: () => {} }),
            };
            this.buffer = ['existing terminal output'];
            this.shellTerminalServer = backend;
            this.terminalWatcher = { onTerminalExit: listener => { exitListeners.add(listener); return { dispose: () => exitListeners.delete(listener) }; } };
            this.connectionProvider = {
                listen: (connectionPath, callback) => {
                    const id = Number(connectionPath.split('/').at(-1));
                    if (backend.connectionGate) backend.connectionGate.promise.then(() => callback(connectionPath, channel(id)));
                    else callback(connectionPath, channel(id));
                },
            };
            this.toDispose = new DisposableCollection();
            this.toDisposeOnConnect = new DisposableCollection();
            this.deviceStatusCodes = new Set();
            this.preferences = {};
            this.onShellTypeChangedEmiter = { fire: () => {} };
            this.isDisposed = false;
            this.connectionClosed = false;
            this.connectTerminalProcess();
        }
        get terminalId() { return this._terminalId; }
        async start(id) {
            // Theia assigns the ID only after createTerminal finishes reading process metadata.
            const attached = typeof id === 'number' ? await backend.attach(id) : -1;
            this._terminalId = attached >= 0 ? attached : await this.createTerminal();
            this.connectTerminalProcess();
            return this.terminalId;
        }
        async createTerminal() {
            const terminalId = await backend.create(this.options);
            await backend.getProcessInfo(terminalId);
            return terminalId;
        }
        async reconnectTerminalProcess() { await this.start(this.terminalId); }
        disableEnterWhenAttachCloseListener() { return false; }
        write(text) { this.buffer.push(text); }
        storeState() { return { terminalId: this.terminalId, titleLabel: this.title.label }; }
        restoreState(state) { this.restoredOptions = this.options; this.restoredState = state; this.restoreCompletion = this.start(state.terminalId); }
        finalizeAndDispose() { this.term.write('', () => this.dispose()); }
        dispose() { this.isDisposed = true; }
        close() { this.dispose(); }
    }
    const exports = {};
    const context = {
        module: { exports }, exports, Promise, queueMicrotask,
        setTimeout: callback => { const id = ++n_timers; timers.set(id, callback); return id; },
        clearTimeout: id => timers.delete(id),
        require: name => {
            let result;
            if (name.endsWith('/terminal-widget-impl')) result = { TerminalWidgetImpl };
            else if (name.endsWith('/application-shell')) result = { ApplicationShell: class {} };
            else if (name.endsWith('/widgets')) result = { waitForClosed: async () => {} };
            else if (name.endsWith('/disposable')) result = { DisposableCollection };
            else if (name.endsWith('/promise-util')) result = { Deferred: class { constructor() { Object.assign(this, deferred()); } } };
            else if (name.endsWith('/os')) result = { OS: { backend: { type: () => 'Linux' } } };
            else if (name.endsWith('/base-terminal-protocol')) result = { IBaseTerminalServer: { validateId: id => typeof id === 'number' && id !== -1 } };
            else if (name.endsWith('/terminal-protocol')) result = { terminalsPath: '/terminals' };
            else if (name.endsWith('/shell-type')) result = { guessShellTypeFromExecutable: () => 'bash' };
            else if (name.endsWith('/inversify')) result = { injectable: () => () => {}, inject: () => () => {}, decorate: () => {} };
            else if (name === 'xterm-addon-unicode11') result = { Unicode11Addon: class {} };
            else if (name === './emoji-width') result = require('../emoji-width');
            else if (name === './wsl-terminals' || name === './paste-paths') result = require(`.${name}`);
            else if (/(file-service|env-variables|message-service|buffer|file-uri|common\/uri)$/.test(name)) result = { default: class {}, FileService: {}, EnvVariablesServer: {}, MessageService: {}, BinaryBuffer: {}, FileUri: {} };
            else throw new Error(`Unexpected dependency: ${name}`);
            return result;
        },
    };
    vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../terminal.js'), 'utf8'), context);
    const Terminal = context.module.exports.PaddockTerminal;
    const terminal = new Terminal();
    const options = {
        shellPath: '/bin/bash', shellArgs: ['--rcfile', '/safe/launcher'], cwd: '/work', title: 'Claude · B',
        paddockAccount: { id: 'b', provider: 'claude', label: 'B', runtime: 'native' },
        paddockResume: { provider: 'claude', sessionId: 'same-session', sourceAccountId: 'a', runtime: 'native', cwd: '/work', transcriptPath: '/account-a/projects/session.jsonl' },
        env: { CLAUDE_CONFIG_DIR: '/account-b', ANTHROPIC_API_KEY: null, BASH_ENV: null, ENV: null },
    };
    return { terminal, Terminal, backend, options, channel, closeChannel, timers, dataListeners };
}

test('C-terminal-replacement-preserve: one replacement keeps widget, buffer and tab identity until the old connection closes', async () => {
    const { terminal, backend, options, closeChannel } = fixture();
    backend.automaticClose = false;
    const buffer = terminal.buffer;
    const first = terminal.replaceProcess(options);
    const second = terminal.replaceProcess(options);
    await settle();
    assert.equal(terminal.id, 'terminal-same-widget');
    assert.equal(terminal.isDisposed, false);
    assert.equal(terminal.term.options.disableStdin, true);
    assert.equal(backend.created.length, 0);
    closeChannel(7);
    await Promise.all([first, second]);
    assert.equal(backend.created.length, 1);
    assert.deepEqual(backend.closed, [7]);
    assert.equal(terminal.buffer, buffer);
    assert.equal(terminal.options.paddockAccount.id, 'b');
    assert.equal(terminal.terminalId, 8);
    assert.equal(terminal.term.options.disableStdin, false);
    assert.equal(terminal.isDisposed, false);
});

test('C-terminal-replacement-retry: failed shell creation retains the source account and retries in the same widget', async () => {
    const { terminal, backend, options } = fixture();
    const before = terminal.options;
    backend.failCreate = true;
    await assert.rejects(terminal.replaceProcess(options), /Shell could not start/);
    assert.equal(terminal.options, before);
    assert.equal(terminal.title.label, 'Claude · A');
    assert.equal(terminal.isDisposed, false);
    assert.equal(terminal.term.options.disableStdin, true);
    backend.failCreate = false;
    await terminal.replaceProcess(options);
    assert.equal(terminal.options.paddockAccount.id, 'b');
    assert.equal(terminal.term.options.disableStdin, false);
    assert.equal(terminal.isDisposed, false);
});

test('C-terminal-replacement-timeout: a delayed old connection leaves the tab available for retry', async () => {
    const { terminal, backend, options, closeChannel, timers } = fixture();
    backend.automaticClose = false;
    const replacing = terminal.replaceProcess(options);
    const rejected = assert.rejects(replacing, /timed out/i);
    await settle();
    [...timers.values()][0]();
    await rejected;
    assert.equal(terminal.isDisposed, false);
    assert.equal(terminal.options.paddockAccount.id, 'a');
    closeChannel(7);
    backend.automaticClose = true;
    await terminal.replaceProcess(options);
    assert.equal(terminal.terminalId, 8);
});

test('C-account-idle-stop-repeat: a shell that ignores the first stop request is stopped by a repeated request', async () => {
    const { terminal, backend, options, timers } = fixture();
    backend.n_ignoredCloses = 1;
    const replacing = terminal.replaceProcess(options);
    await settle();
    assert.deepEqual(backend.closed, [7]);
    // The first timer is the overall timeout; the next one repeats the stop request.
    const [, repeat] = [...timers.values()];
    repeat();
    await replacing;
    assert.deepEqual(backend.closed, [7, 7]);
    assert.equal(terminal.terminalId, 8);
    assert.equal(terminal.options.paddockAccount.id, 'b');
});

test('C-terminal-replacement-process-info: a shell created before metadata fails is stopped before retry starts another shell', async () => {
    const { terminal, backend, options } = fixture();
    backend.failProcessInfo = true;
    await assert.rejects(terminal.replaceProcess(options), /Shell information could not be read/);
    await settle();
    assert.deepEqual(backend.closed, [7, 8]);
    assert.equal(await backend.attach(8), -1);
    assert.equal(terminal.options.paddockAccount.id, 'a');
    assert.equal(terminal.isDisposed, false);
    assert.equal(terminal.term.options.disableStdin, true);
    backend.failProcessInfo = false;
    await terminal.replaceProcess(options);
    assert.equal(terminal.terminalId, 9);
    assert.equal(terminal.options.paddockAccount.id, 'b');
    assert.equal(terminal.isDisposed, false);
});

test('C-terminal-replacement-late-start: a timed-out creation is stopped before retrying another account', async () => {
    const { terminal, backend, options, timers } = fixture();
    backend.createGate = deferred();
    const replacing = terminal.replaceProcess(options);
    const rejected = assert.rejects(replacing, /timed out/i);
    await settle();
    [...timers.values()][0]();
    await rejected;
    assert.equal(terminal.isDisposed, false);
    assert.equal(terminal.options.paddockAccount.id, 'a');
    backend.createGate.resolve();
    await settle();
    assert.ok(backend.closed.includes(8));
    backend.createGate = null;
    await terminal.replaceProcess(options);
    assert.equal(terminal.terminalId, 9);
    assert.equal(terminal.isDisposed, false);
});

test('C-terminal-replacement-no-connection: a shell whose data channel never opens is stopped and can be retried', async () => {
    const { terminal, backend, options, channel, timers, dataListeners } = fixture();
    const connectionGate = backend.connectionGate = deferred();
    const replacing = terminal.replaceProcess(options);
    const rejected = assert.rejects(replacing, /Connecting the account shell timed out/i);
    await settle();
    [...timers.values()][0]();
    await rejected;
    await settle();
    assert.ok(backend.closed.includes(8), 'Close must be requested without waiting for the data channel');
    assert.equal(terminal.isDisposed, false);
    backend.connectionGate = null;
    await terminal.replaceProcess(options);
    assert.equal(terminal.terminalId, 9);
    connectionGate.resolve();
    await settle();
    assert.equal(channel(8).closed, true, 'A late connection to the stopped process must close immediately');
    assert.equal(terminal.connectionClosed, false);
    assert.equal(terminal.isDisposed, false);
    for (const listener of dataListeners) listener('hello');
    assert.deepEqual(channel(8).sent, []);
    assert.deepEqual(channel(9).sent, ['hello']);
});

test('C-terminal-replacement-old-close: delayed channel events cannot close or write into the newly attached shell', async () => {
    const { terminal, backend, options, channel, closeChannel } = fixture();
    await terminal.replaceProcess(options);
    const previous = channel(8);
    await terminal.replaceProcess({ ...options, paddockAccount: { ...options.paddockAccount, id: 'c', label: 'C' } });
    const buffer = [...terminal.buffer];
    terminal.waitingForConnectionCloseToDispose = true;
    closeChannel(8);
    for (const listener of previous.messages) listener(() => ({ readString: () => 'old output' }));
    await settle();
    assert.equal(terminal.terminalId, 9);
    assert.equal(terminal.connectionClosed, false);
    assert.equal(terminal.isDisposed, false);
    assert.deepEqual(terminal.buffer, buffer);
    assert.deepEqual(backend.closed, [7, 8]);
});

test('C-terminal-replacement-reconnect: backend reconnect does not create an extra shell during replacement or failure', async () => {
    const { terminal, backend, options } = fixture();
    backend.createGate = deferred();
    const replacing = terminal.replaceProcess(options);
    await settle();
    await terminal.reconnectTerminalProcess();
    assert.equal(backend.created.length, 1);
    backend.createGate.resolve();
    await replacing;
    backend.createGate = null;
    backend.failCreate = true;
    await assert.rejects(terminal.replaceProcess(options), /Shell could not start/);
    const n_created = backend.created.length;
    await terminal.reconnectTerminalProcess();
    assert.equal(backend.created.length, n_created);
});

test('C-terminal-replacement-restore: state restores the new launcher and original transcript metadata without credential values', async () => {
    const { terminal, Terminal, options } = fixture();
    await terminal.replaceProcess(options);
    terminal.options.env.UNRELATED_SECRET = 'must not persist';
    terminal.options.env.ANTHROPIC_AUTH_TOKEN = 'must not persist';
    const state = terminal.storeState();
    assert.equal(JSON.stringify(state).includes('must not persist'), false);
    const restored = new Terminal();
    restored.restoreState(JSON.parse(JSON.stringify(state)));
    assert.equal(restored.restoredOptions.paddockAccount.id, 'b');
    assert.equal(restored.restoredOptions.env.CLAUDE_CONFIG_DIR, '/account-b');
    assert.equal(restored.restoredOptions.paddockResume.sourceAccountId, 'a');
    assert.equal(restored.restoredOptions.paddockResume.sessionId, 'same-session');
    assert.equal(restored.id, terminal.id);
});

test('C-terminal-replacement-close: explicit close still disposes a failed replacement tab', async () => {
    const { terminal, backend, options } = fixture();
    backend.failCreate = true;
    await assert.rejects(terminal.replaceProcess(options));
    await terminal.closeWithoutSaving();
    assert.equal(terminal.isDisposed, true);
});

test('C-terminal-replacement-close: closing during shell creation stops a late process instead of reviving the tab', async () => {
    const { terminal, backend, options } = fixture();
    backend.createGate = deferred();
    const replacing = terminal.replaceProcess(options);
    const rejected = assert.rejects(replacing, /already closed/);
    await settle();
    await terminal.closeWithoutSaving();
    backend.createGate.resolve();
    await rejected;
    await settle();
    assert.equal(terminal.isDisposed, true);
    assert.ok(backend.closed.includes(8));
});

test('C-terminal-replacement-exit: normal process exit after a successful switch retains the original close behavior', async () => {
    const { terminal, backend, options } = fixture();
    await terminal.replaceProcess(options);
    await backend.close(terminal.terminalId);
    await settle();
    assert.equal(terminal.isDisposed, true);
});

test('C-terminal-replacement-restore: an automatic save during a second switch keeps the last successful account', async () => {
    const { terminal, backend, options } = fixture();
    await terminal.replaceProcess(options);
    backend.createGate = deferred();
    const replacing = terminal.replaceProcess({ ...options, paddockAccount: { ...options.paddockAccount, id: 'c', label: 'C' } });
    await settle();
    const state = terminal.storeState();
    assert.equal(state.paddockLauncher.paddockAccount.id, 'b');
    assert.equal(state.paddockAccountLabel, 'B');
    assert.equal(state.terminalId, -1);
    backend.createGate.resolve();
    await replacing;
    assert.equal(terminal.storeState().paddockLauncher.paddockAccount.id, 'c');
});

test('C-terminal-replacement-restore-created: a saved replacement awaiting process metadata attaches to the existing target process', async () => {
    const { terminal, Terminal, backend, options } = fixture();
    backend.processInfoGate = deferred();
    const replacing = terminal.replaceProcess(options);
    await settle();
    const state = terminal.storeState();
    assert.equal(state.terminalId, 8);
    assert.equal(state.paddockLauncher.paddockAccount.id, 'b');
    assert.equal(state.paddockAccountLabel, 'B');
    const restored = new Terminal();
    restored.restoreState(JSON.parse(JSON.stringify(state)));
    await restored.restoreCompletion;
    assert.equal(restored.terminalId, 8);
    assert.equal(restored.options.paddockAccount.id, 'b');
    assert.equal(backend.created.length, 1);
    backend.processInfoGate.resolve();
    await replacing;
});

test('C-terminal-replacement-restore-connecting: a saved replacement awaiting its data channel attaches without creating another process', async () => {
    const { terminal, Terminal, backend, options } = fixture();
    backend.connectionGate = deferred();
    const replacing = terminal.replaceProcess(options);
    await settle();
    const state = terminal.storeState();
    assert.equal(state.terminalId, 8);
    assert.equal(state.paddockLauncher.paddockAccount.id, 'b');
    const restored = new Terminal();
    restored.restoreState(JSON.parse(JSON.stringify(state)));
    await restored.restoreCompletion;
    assert.equal(restored.terminalId, 8);
    assert.equal(backend.created.length, 1);
    backend.connectionGate.resolve();
    await replacing;
});

test('C-terminal-replacement-restore-cleaned: after a failed created process is closed, saving returns to the original account', async () => {
    const { terminal, backend, options } = fixture();
    terminal.hasReplacedProcess = true;
    backend.failProcessInfo = true;
    await assert.rejects(terminal.replaceProcess(options), /Shell information could not be read/);
    await settle();
    assert.equal(await backend.attach(8), -1);
    const state = terminal.storeState();
    assert.equal(state.terminalId, -1);
    assert.equal(state.paddockLauncher.paddockAccount.id, 'a');
    assert.equal(state.paddockAccountLabel, 'A');
});

test('C-terminal-replacement-restore-late-created: creation reported after timeout still saves the account that actually launched', async () => {
    const { terminal, backend, options, timers } = fixture();
    backend.createGate = deferred();
    backend.processInfoGate = deferred();
    const replacing = terminal.replaceProcess(options);
    const rejected = assert.rejects(replacing, /timed out/i);
    await settle();
    [...timers.values()][0]();
    await rejected;
    assert.equal(terminal.options.paddockAccount.id, 'a');
    backend.createGate.resolve();
    await settle();
    const state = terminal.storeState();
    assert.equal(state.terminalId, 8);
    assert.equal(state.paddockLauncher.paddockAccount.id, 'b');
    backend.processInfoGate.resolve();
    await settle();
    assert.equal(await backend.attach(8), -1);
    assert.equal(terminal.storeState().paddockAccountLabel, 'A');
});

test('공유 터미널은 종료된 셸 대신 새 셸을 생성하지 않는다', async () => {
    const { terminal, backend } = fixture();
    terminal.options.paddockShared = true;
    assert.equal(await terminal.attachTerminal(7), 7);
    await assert.rejects(terminal.attachTerminal(999), /other window has closed/);
    assert.deepEqual(backend.created, []);
});

test('공유 탭은 닫을 때 프로세스 종료를 비활성화하고 공유 상태를 보존한다', async () => {
    const { terminal, options } = fixture();
    terminal.options.paddockShared = true;
    assert.equal(terminal.storeState().paddockShared, true);
    await assert.rejects(terminal.replaceProcess(options), /original terminal window/);
    terminal.closeOnDispose = true;
    terminal.dispose();
    assert.equal(terminal.closeOnDispose, false);
    assert.equal(terminal.isDisposed, true);
});

test('이동 중 측정할 수 없는 터미널 크기는 이전 크기를 유지하고 유효해지면 프로세스에도 전달한다', () => {
    const { terminal } = fixture();
    const resized = [];
    let processResizes = 0;
    let dimensions;
    terminal.fitAddon = { proposeDimensions: () => dimensions };
    terminal.term.resize = (cols, rows) => resized.push([cols, rows]);
    terminal.resizeTerminalProcess = () => { processResizes += 1; };
    for (dimensions of [undefined, { cols: NaN, rows: NaN }, { cols: Infinity, rows: 20 }, { cols: 0, rows: 20 }, { cols: 80, rows: 1 }]) {
        terminal.doResizeTerminal();
    }
    assert.deepEqual(resized, []);
    assert.equal(processResizes, 0);
    dimensions = { cols: 120, rows: 40 };
    terminal.doResizeTerminal();
    assert.deepEqual(resized, [[120, 39]]);
    assert.equal(processResizes, 1);
    terminal.isDisposed = true;
    terminal.doResizeTerminal();
    assert.equal(processResizes, 1);
});

test('C-terminal-number-restore: the default-name number survives a restart and ignores invalid saved values', () => {
    const { terminal, Terminal } = fixture();
    terminal.paddockNumber = 3;
    const state = terminal.storeState();
    const restored = new Terminal();
    restored.restoreState(JSON.parse(JSON.stringify(state)));
    assert.equal(restored.paddockNumber, 3);
    const invalid = new Terminal();
    invalid.restoreState({ ...JSON.parse(JSON.stringify(state)), paddockNumber: -1 });
    assert.equal(invalid.paddockNumber, undefined);
});
