const test = require('node:test');
const assert = require('node:assert/strict');
const { UsageData, accountName } = require('../usage-panel');

function deferred() {
    let resolve;
    const promise = new Promise(done => { resolve = done; });
    return { promise, resolve };
}

function response(
    used,
) {
    return {
        claude: { state: 'on', windows: [{ label: '5h', used, resetsAt: 9999999999 }], updatedAt: 100 },
        codex: { windows: [{ label: '5h', used, resetsAt: 9999999999 }], updatedAt: 100 },
    };
}

test('C-account-usage-isolation: each source keeps its own usage while only the active account is selected', async () => {
    const claude = { id: 'c', provider: 'claude', label: 'Main' };
    const codex = { id: 'x', provider: 'codex', label: 'Main' };
    const data = new UsageData({
        listProfiles: async () => [claude, codex],
        isEnabled: () => true,
        fetchJson: async (_path, _method, query) => response(query.includes('=c') ? 12 : query.includes('=x') ? 73 : 99),
    });
    await data.refresh();
    data.select(claude);
    data.select(codex);
    await data.refresh();
    assert.equal(data.snapshot(claude).data.windows[0].used, 12);
    assert.equal(data.snapshot(codex).data.windows[0].used, 73);
    assert.equal(data.selected.claude.id, 'c');
    assert.equal(data.selected.codex.id, 'x');
    assert.equal(data.active.id, 'x');
    assert.equal(data.snapshot({ provider: 'codex' }).data.windows[0].used, 99);
    assert.equal(accountName({ provider: 'codex' }), 'Current CLI');
    assert.equal(accountName({ ...codex, label: 'Default' }), 'Default');
});

test('C-account-usage-active: shell selection clears the active account and an unregistered CLI never inherits its identity', async () => {
    const profile = { id: 'c', provider: 'claude', label: 'Main' };
    let n_changes = 0;
    const data = new UsageData({
        listProfiles: async () => [profile],
        isEnabled: () => true,
        fetchJson: async (_path, _method, query) => response(query ? 17 : 88),
        onChange: () => { n_changes += 1; },
    });
    assert.equal(data.active, null);
    data.select(profile);
    await data.refresh();
    assert.equal(data.snapshot(data.active).data.windows[0].used, 17);
    data.select(null);
    assert.equal(data.active, null);
    assert.equal(data.selected.claude.id, 'c');
    data.select({ provider: 'claude' });
    await data.refresh();
    assert.equal(data.active.id, undefined);
    assert.equal(accountName(data.active), 'Current CLI');
    assert.equal(data.snapshot(data.active).data.windows[0].used, 88);
    const n_before = n_changes;
    data.select(profile);
    assert.equal(data.active.id, 'c');
    assert.equal(data.snapshot(data.active).data.windows[0].used, 17);
    assert.equal(n_changes, n_before + 1);
});

test('C-account-usage-empty: initial failure and empty records have explicit different states', async () => {
    let fails = true;
    const data = new UsageData({
        listProfiles: async () => [],
        isEnabled: () => true,
        fetchJson: async () => {
            if (fails) throw new Error('private backend failure');
            return { claude: { state: 'on', windows: [] }, codex: { windows: [] } };
        },
    });
    await data.refresh();
    assert.deepEqual(data.snapshot({ provider: 'claude' }), { status: 'unavailable' });
    assert.deepEqual(data.snapshot({ provider: 'codex' }), { status: 'unavailable' });
    fails = false;
    await data.refresh();
    assert.equal(data.snapshot({ provider: 'codex' }).status, 'ready');
    assert.deepEqual(data.snapshot({ provider: 'codex' }).data.windows, []);
});

test('C-account-usage-stale: A → B → A rejects the first A response even after the second A completes', async () => {
    const first = deferred();
    let n_aRequests = 0;
    const a = { id: 'a', provider: 'codex', label: 'A' };
    const b = { id: 'b', provider: 'codex', label: 'B' };
    const data = new UsageData({
        listProfiles: async () => [a, b],
        isEnabled: () => true,
        fetchJson: async (_path, _method, query) => {
            let value = response(44);
            if (query.includes('=a')) {
                n_aRequests += 1;
                value = n_aRequests === 1 ? await first.promise : response(22);
            }
            return value;
        },
    });
    await data.updateProfiles();
    data.select(a);
    const oldRequest = data.read(a);
    data.select(b);
    await data.read(b);
    data.select(a);
    await data.read(a);
    first.resolve(response(99));
    await oldRequest;
    assert.equal(data.snapshot(a).data.windows[0].used, 22);
    assert.equal(data.snapshot(b).data.windows[0].used, 44);
});

test('C-account-usage-isolation: renaming updates the source label; removing it makes its saved selection unavailable', async () => {
    const account = { id: 'a', provider: 'codex', label: 'Original' };
    let profiles = [account];
    const data = new UsageData({ listProfiles: async () => profiles, isEnabled: () => true, fetchJson: async () => response(45) });
    data.select(account);
    await data.refresh();
    profiles = [{ ...account, label: 'Renamed' }];
    await data.updateProfiles();
    assert.equal(data.current(data.selected.codex).label, 'Renamed');
    profiles = [];
    await data.updateProfiles();
    assert.deepEqual(data.snapshot(account), { status: 'unavailable', removed: true });
    assert.equal(data.selected.codex.id, account.id);
});

test('C-account-usage-settings: global off restores every registered Claude and Default even if GET fails', async () => {
    const posts = [];
    const data = new UsageData({
        listProfiles: async () => [{ id: 'c1', provider: 'claude' }, { id: 'c2', provider: 'claude' }, { id: 'x', provider: 'codex' }],
        isEnabled: provider => provider !== 'claude',
        fetchJson: async (_path, method, query) => {
            if (method === 'POST') posts.push(query);
            else throw new Error('unreadable usage record');
            return { state: 'off' };
        },
    });
    await data.refresh({ settings: true });
    assert.deepEqual(posts.sort(), ['?enabled=false', '?enabled=false&accountId=c1', '?enabled=false&accountId=c2']);
});

test('C-account-usage-settings: a slow enable finishes before the newer global off and cannot turn collection back on', async () => {
    const enable = deferred();
    const entered = deferred();
    let enabled = true;
    let state = 'unset';
    const writes = [];
    const data = new UsageData({
        listProfiles: async () => [],
        isEnabled: provider => provider === 'claude' ? enabled : true,
        fetchJson: async (_path, method, query) => {
            let result = { claude: { state, windows: [] }, codex: { windows: [] } };
            if (method === 'POST') {
                const next = query.includes('enabled=true') ? 'on' : 'off';
                if (next === 'on') {
                    entered.resolve();
                    await enable.promise;
                }
                state = next;
                writes.push(next);
                result = { state };
            }
            return result;
        },
    });
    const oldRequest = data.read({ provider: 'claude' });
    await entered.promise;
    enabled = false;
    data.preferencesChanged();
    const disabling = data.refresh({ settings: true });
    enable.resolve();
    await Promise.all([oldRequest, disabling]);
    assert.deepEqual(writes, ['on', 'off']);
    assert.equal(state, 'off');
});

test('C-account-usage-settings: hook failures are scoped and retried only after an explicit refresh or preference change', async () => {
    let n_posts = 0;
    const data = new UsageData({
        listProfiles: async () => [],
        isEnabled: () => true,
        fetchJson: async (_path, method) => {
            if (method === 'POST') {
                n_posts += 1;
                throw new Error('no runtime');
            }
            return { claude: { state: 'unset', windows: [] }, codex: { windows: [] } };
        },
    });
    await data.refresh();
    await data.refresh();
    assert.equal(n_posts, 1);
    assert.equal(data.snapshot({ provider: 'claude' }).status, 'unavailable');
    assert.equal(data.snapshot({ provider: 'codex' }).status, 'ready');
    data.preferencesChanged();
    await data.refresh();
    assert.equal(n_posts, 2);
});
