const test = require('node:test');
const assert = require('node:assert/strict');
const Module = require('node:module');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function fixture(
    context,
) {
    const dialogs = [];
    class Element {
        constructor() { this.children = []; this.listeners = {}; this.style = {}; }
        appendChild(child) { this.children.push(child); }
        replaceChildren(...children) { this.children = children; }
        setAttribute() {}
        addEventListener(name, action) { this.listeners[name] = action; }
        click() { if (!this.disabled) this.listeners.click?.(); }
    }
    class Dialog {
        constructor() {
            this.node = { ownerDocument: { createElement: () => new Element() }, querySelector: () => new Element() };
            this.contentNode = new Element();
            this.closeCrossNode = new Element();
            this.titleNode = new Element();
        }
        addClass() {}
        appendCloseButton() { this.closeButton = new Element(); this.closeButton.addEventListener('click', () => this.close()); }
        open() { dialogs.push(this); return new Promise(resolve => { this.resolve = resolve; }); }
        close() { this.resolve(undefined); }
        async accept() { this.resolve(this.value); }
    }
    const load = Module._load;
    context.mock.method(Module, '_load', function (name, parent, main) {
        return name === '@theia/core/lib/browser/dialogs' ? { AbstractDialog: Dialog } : load.call(this, name, parent, main);
    });
    const filename = require.resolve('../account-access-dialog');
    delete require.cache[filename];
    context.after(() => { delete require.cache[filename]; });
    const { retryAccountStorage } = require('../account-access-dialog');
    const click = text => {
        const buttons = dialogs.at(-1).contentNode.children.flatMap(node => node.children);
        const button = buttons.find(node => node.textContent === text);
        assert.ok(button, `Button ${text} exists`);
        button.click();
    };
    return { dialogs, retryAccountStorage, click, tick: () => new Promise(resolve => setImmediate(resolve)) };
}

test('AD01 Cancel preserves the pending action and never grants access or retries', async context => {
    const setup = fixture(context);
    let n_actions = 0;
    let n_grants = 0;
    const error = new Error('access denied');
    const service = { inspectStorageAccess: async () => ({ status: 'repair', message: 'Allow storage access.' }), allowStorageAccess: async () => { n_grants += 1; } };
    const pending = setup.retryAccountStorage(async () => { n_actions += 1; throw error; }, service, { runtime: 'native' });
    const rejected = assert.rejects(pending, value => value === error);
    await setup.tick();
    setup.dialogs[0].closeButton.click();
    await rejected;
    assert.equal(n_actions, 1);
    assert.equal(n_grants, 0);
});

test('AD02 Allow access retries the same saved action and returns its result without a second dialog', async context => {
    const setup = fixture(context);
    let granted = false;
    let n_actions = 0;
    const scope = { accountId: 'saved-account' };
    const value = { prepared: 'same-account' };
    const service = {
        inspectStorageAccess: async request => { assert.deepEqual(request, scope); return { status: 'repair', message: 'Allow storage access.' }; },
        allowStorageAccess: async request => { assert.deepEqual(request, scope); granted = true; },
    };
    const pending = setup.retryAccountStorage(async () => { n_actions += 1; if (!granted) throw new Error('access denied'); return value; }, service, scope);
    await setup.tick();
    assert.equal(granted, false);
    setup.click('Allow access');
    assert.equal(await pending, value);
    assert.equal(n_actions, 2);
    assert.equal(setup.dialogs.length, 1);
});

test('AD03 OS settings waits for explicit Retry, while unrelated errors never open an access dialog', async context => {
    const setup = fixture(context);
    let restored = false;
    let n_actions = 0;
    let n_settings = 0;
    const service = {
        inspectStorageAccess: async () => ({ status: 'settings', message: 'Use OS permissions.', canOpenSettings: true, settingsLabel: 'Open folder settings' }),
        openStorageSettings: async () => { n_settings += 1; restored = true; },
    };
    const pending = setup.retryAccountStorage(async () => { n_actions += 1; if (!restored) throw new Error('access denied'); return 'opened'; }, service, {});
    await setup.tick();
    setup.click('Open folder settings');
    await setup.tick();
    assert.equal(n_settings, 1);
    assert.equal(n_actions, 1);
    setup.click('Retry');
    assert.equal(await pending, 'opened');
    const error = new Error('CLI is not installed');
    await assert.rejects(setup.retryAccountStorage(async () => { throw error; }, { inspectStorageAccess: async () => ({ status: 'ready' }) }, {}), value => value === error);
    assert.equal(setup.dialogs.length, 1);
});

test('AD04 restored folder access removes obsolete Allow access and preserves the new action error', async context => {
    const setup = fixture(context);
    let restored = false;
    const nextError = new Error('The selected CLI is unavailable.');
    const service = {
        inspectStorageAccess: async () => ({ status: restored ? 'ready' : 'repair', message: restored ? '' : 'Storage is blocked.' }),
        allowStorageAccess: async () => { restored = true; },
    };
    const pending = setup.retryAccountStorage(async () => { throw restored ? nextError : new Error('storage denied'); }, service, {});
    const rejected = assert.rejects(pending, error => error === nextError);
    await setup.tick();
    setup.click('Allow access');
    await setup.tick();
    const nodes = setup.dialogs[0].contentNode.children;
    assert.ok(nodes.some(node => node.textContent?.includes('selected CLI is unavailable')));
    assert.equal(nodes.flatMap(node => node.children).some(node => node.textContent === 'Allow access'), false);
    setup.dialogs[0].closeButton.click();
    await rejected;
});

test('AD05 cancelling storage recovery during the existing terminal switch never stops or replaces its process', async context => {
    const setup = fixture(context);
    const source = fs.readFileSync(path.join(__dirname, '../workspace/account-terminals.js'), 'utf8');
    const method = source.slice(source.indexOf('    async switchTerminalAccount('), source.indexOf('    async renderAccountMenu('));
    const accountTerminals = vm.runInNewContext(`({${method}})`, { retryAccountStorage: setup.retryAccountStorage });
    const options = { paddockAccount: { id: 'original', label: 'Main', provider: 'claude' }, paddockResume: { sessionId: 'original-conversation' } };
    let n_stops = 0;
    let n_replacements = 0;
    const terminal = { id: 'original-tab', options, processId: Promise.resolve(99), replaceProcess: async () => { n_replacements += 1; } };
    accountTerminals.workspace = {
        isTerminal: () => true, refresh: async () => {},
        folderTabs: { renderFolderTabs: () => {} },
        agentActivity: { accountSessionRequest: () => ({ terminalId: terminal.id, accountId: 'original', provider: 'claude', shellPid: 99 }) },
        accounts: {
            prepareResume: async () => { throw new Error('Account storage is blocked.'); },
            inspectStorageAccess: async () => ({ status: 'repair', message: 'Allow storage access.' }),
            stopSession: async () => { n_stops += 1; },
        },
    };
    const pending = accountTerminals.switchTerminalAccount('target', terminal);
    const rejected = assert.rejects(pending, /Account storage is blocked/);
    await setup.tick();
    assert.equal(terminal.paddockAccountSwitching, true);
    setup.dialogs[0].closeButton.click();
    await rejected;
    assert.equal(n_stops, 0);
    assert.equal(n_replacements, 0);
    assert.equal(terminal.options, options);
    assert.equal(terminal.id, 'original-tab');
    assert.equal(terminal.paddockAccountSwitching, false);
});

test('AD06 failed access diagnosis reports uncertainty and preserves the action error without offering stale permission repair', async context => {
    const setup = fixture(context);
    let allowed = false;
    const nextError = new Error('The account environment disconnected.');
    const service = {
        inspectStorageAccess: async () => { if (allowed) throw new Error('diagnosis disconnected'); return { status: 'repair', message: 'Storage is blocked.' }; },
        allowStorageAccess: async () => { allowed = true; },
    };
    const pending = setup.retryAccountStorage(async () => { throw allowed ? nextError : new Error('storage denied'); }, service, {});
    const rejected = assert.rejects(pending, error => error === nextError);
    await setup.tick();
    setup.click('Allow access');
    await setup.tick();
    const nodes = setup.dialogs[0].contentNode.children;
    const text = nodes.map(node => node.textContent || '').join(' ');
    assert.match(text, /could not check folder access/);
    assert.match(text, /account environment disconnected/);
    assert.doesNotMatch(text, /still blocked|Storage is blocked/);
    assert.equal(nodes.flatMap(node => node.children).some(node => node.textContent === 'Allow access'), false);
    setup.dialogs[0].closeButton.click();
    await rejected;
});
