const test = require('node:test');
const assert = require('node:assert/strict');
const { UsageData, UsagePanel, accountName, scopeKey } = require('../usage-panel');

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

test('C-account-usage-isolation: each provider keeps its last selected account and its own usage', async () => {
    const claude = { id: 'c', provider: 'claude', label: 'Main' };
    const codex = { id: 'x', provider: 'codex', label: 'Main' };
    const data = new UsageData({
        listProfiles: async () => [claude, codex],
        isEnabled: () => true,
        fetchJson: async (_path, _method, query) => response(query === '?accountId=c' ? 12 : query === '?accountId=x' ? 73 : 99),
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
    assert.equal(accountName({ provider: 'codex' }), 'Default');
    assert.equal(accountName({ ...codex, label: 'Default' }), 'Default');
});

test('C-account-usage-active: shell selection clears the active account and an unregistered CLI never inherits its identity', async () => {
    const profile = { id: 'c', provider: 'claude', label: 'Main' };
    let n_changes = 0;
    const data = new UsageData({
        listProfiles: async () => [profile],
        isEnabled: () => true,
        fetchJson: async (_path, _method, query) => response(query.includes('accountId=') ? 17 : 88),
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
    assert.equal(accountName(data.active), 'Default');
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

test('C-account-usage-settings: a failed automatic setup still shows usage that was already recorded', async () => {
    let n_posts = 0;
    const data = new UsageData({
        listProfiles: async () => [],
        isEnabled: () => true,
        fetchJson: async (_path, method) => {
            if (method === 'POST') {
                n_posts += 1;
                throw new Error('EISDIR');
            }
            return { claude: { state: 'unset', windows: [{ label: '5h', used: 20, resetsAt: 9999999999 }], updatedAt: 100 }, codex: { windows: [] } };
        },
    });
    await data.refresh();
    assert.equal(n_posts, 1);
    assert.equal(data.snapshot({ provider: 'claude' }).status, 'ready', 'the setup failure does not hide the recorded value');
    assert.equal(data.snapshot({ provider: 'claude' }).data.windows[0].used, 20);
    await data.refresh();
    assert.equal(n_posts, 1, 'the failed setup is not retried every poll');
    assert.equal(data.snapshot({ provider: 'claude' }).status, 'ready');
});

test('C-account-usage-custom: an unregistered folder never reads or borrows Current CLI usage', async () => {
    const queries = [];
    const data = new UsageData({
        listProfiles: async () => [],
        isEnabled: () => true,
        fetchJson: async (_path, method, query) => {
            queries.push(`${method} ${query}`);
            return response(55);
        },
    });
    const custom = { provider: 'codex', custom: true };
    data.select(custom);
    await data.refresh();
    assert.equal(queries.includes('GET ?provider=codex'), false, 'the status bar never reads Current CLI for an unregistered folder');
    await data.refresh({ all: true });
    assert.equal(queries.filter(query => query === 'GET ?provider=codex').length, 1, 'only the popup list reads Current CLI as its own row');
    assert.equal(data.snapshot(custom).status, 'unavailable');
    assert.match(data.snapshot(custom).message, /not a saved account/);
    assert.equal(data.snapshot({ provider: 'codex' }).data.windows[0].used, 55);
    assert.equal(scopeKey(custom), 'codex:custom');
    assert.equal(accountName(custom), 'Unregistered folder');
    assert.equal(queries.some(query => query.startsWith('POST')), false);
});

test('C-account-usage-observe: a different CLI in an account terminal selects that CLI and returns to the account afterwards', () => {
    const workClaude = { id: 'c', provider: 'claude', label: 'Work Claude' };
    const selections = [];
    let n_refreshes = 0;
    const panel = { data: { select: profile => selections.push(profile) }, refresh: async () => { n_refreshes += 1; } };
    UsagePanel.prototype.observe.call(panel, 'w1', workClaude, 'claude');
    UsagePanel.prototype.observe.call(panel, 'w1', workClaude, 'claude');
    UsagePanel.prototype.observe.call(panel, 'w1', { provider: 'codex' }, 'codex');
    UsagePanel.prototype.observe.call(panel, 'w1', workClaude, 'bash');
    UsagePanel.prototype.observe.call(panel, 'w1', workClaude, 'claude');
    UsagePanel.prototype.observe.call(panel, 'w2', null, 'bash');
    assert.deepEqual(selections, [workClaude, { provider: 'codex' }, workClaude, workClaude, null]);
    assert.equal(n_refreshes, 5);
});

test('C-account-usage-custom: leaving an unregistered folder restores the previous selection instead of keeping it in the bar', () => {
    const workCodex = { id: 'x', provider: 'codex', label: 'Work Codex' };
    const data = new UsageData({ listProfiles: async () => [workCodex], isEnabled: () => true, fetchJson: async () => response(1) });
    data.select(workCodex);
    data.select({ provider: 'codex', custom: true });
    assert.equal(data.selected.codex.custom, true, 'the unregistered folder is shown while its terminal is selected');
    data.select(null);
    assert.deepEqual(data.selected.codex, workCodex, 'a shell terminal brings back the account chosen before the unregistered folder');
    data.select({ provider: 'codex', custom: true });
    data.select({ provider: 'claude' });
    assert.deepEqual(data.selected.codex, workCodex, 'choosing another provider also clears the unregistered folder');
    const fresh = new UsageData({ listProfiles: async () => [], isEnabled: () => true, fetchJson: async () => response(1) });
    fresh.select({ provider: 'codex', custom: true });
    fresh.select(null);
    assert.deepEqual(fresh.selected.codex, { provider: 'codex' }, 'without an earlier choice the bar returns to Current CLI');
});

test('C-account-usage-linked: a default CLI verified as a saved account is shown as that account with one usage reading', async () => {
    const work = { id: 'x', provider: 'codex', label: 'Work Codex' };
    const other = { id: 'y', provider: 'codex', label: 'Other Codex' };
    let defaultAccount = { id: 'x', label: 'Work Codex' };
    let accountUpdatedAt = 50;
    const data = new UsageData({
        listProfiles: async () => [work, other],
        isEnabled: provider => provider === 'codex',
        fetchJson: async (_path, _method, query) => {
            const value = response(query === '?accountId=x' ? 10 : query === '?accountId=y' ? 20 : 30);
            if (query === '?provider=codex') value.codex = { ...value.codex, ...(defaultAccount ? { account: defaultAccount } : {}) };
            if (query === '?accountId=x') value.codex.updatedAt = accountUpdatedAt;
            return value;
        },
    });
    data.select({ provider: 'codex' });
    await data.refresh({ all: true });
    assert.equal(data.linkedProfile('codex').id, 'x');
    assert.equal(data.current({ provider: 'codex' }).id, 'x', 'the status bar shows the saved account instead of Current CLI');
    assert.equal(data.current({ provider: 'codex', custom: true }).custom, true);
    assert.equal(data.snapshot(work).data.windows[0].used, 30, 'the newer default reading of the same account replaces the older one');
    assert.equal(data.snapshot(other).data.windows[0].used, 20, 'other accounts keep their own reading');
    // 연결된 동안 주기 읽기는 계정 폴더를 건너뛴다. 계정 자체 값을 직접 읽었을 때 그것이 더 최근이면 그 값을 쓴다.
    accountUpdatedAt = 200;
    await data.read(work);
    assert.equal(data.snapshot(work).data.windows[0].used, 10, 'the account keeps its own reading when it is newer');

    // 모호하거나 다른 로그인이면 기본 CLI는 따로 남는다.
    defaultAccount = null;
    await data.refresh({ all: true });
    assert.equal(data.linkedProfile('codex'), null);
    assert.equal(data.current({ provider: 'codex' }).id, undefined);
    assert.equal(data.snapshot(work).data.windows[0].used, 10);

    // 터미널 판정의 연결(linked)은 사용량 응답이 오기 전에도 계정으로 보인다. 목록에 없는 계정이면 연결하지 않는다.
    data.select({ provider: 'codex', linked: { id: 'x', label: 'Work Codex' } });
    assert.equal(data.current(data.selected.codex).id, 'x');
    data.select({ provider: 'codex', linked: { id: 'gone', label: 'Gone' } });
    assert.equal(data.current(data.selected.codex).id, undefined);
});

test('C-account-usage-linked: the bar keeps reading Current CLI while it is shown as the saved account', async () => {
    const work = { id: 'x', provider: 'codex', label: 'Work Codex' };
    const queries = [];
    const data = new UsageData({
        listProfiles: async () => [work],
        isEnabled: provider => provider === 'codex',
        fetchJson: async (_path, _method, query) => {
            queries.push(query);
            const value = response(5);
            value.codex.account = { id: 'x', label: 'Work Codex' };
            return value;
        },
    });
    data.select({ provider: 'codex' });
    await data.refresh();
    await data.refresh({ all: true });
    await data.refresh();
    assert.equal(queries.filter(query => query === '?accountId=x').length, 0, 'once linked, the same account is not read again through its own folder');
    assert.equal(queries.filter(query => query === '?provider=codex').length, 3, 'Current CLI is read each time so a changed sign-in unlinks it');
    assert.equal(data.snapshot(work).data.windows[0].used, 5, 'the account row shows the one Current CLI reading');
});

test('C-account-usage-linked: a terminal link ends when the agent exits, so signing in to another account unlinks it', async () => {
    const work = { id: 'x', provider: 'codex', label: 'Work Codex' };
    let defaultAccount = { id: 'x', label: 'Work Codex' };
    let n_changes = 0;
    const data = new UsageData({
        listProfiles: async () => [work],
        isEnabled: provider => provider === 'codex',
        onChange: () => { n_changes += 1; },
        fetchJson: async (_path, _method, query) => {
            const value = response(query === '?provider=codex' ? 40 : 10);
            if (query === '?provider=codex' && defaultAccount) value.codex.account = defaultAccount;
            return value;
        },
    });
    // 1. 터미널의 기본 codex가 Work Codex와 같은 로그인으로 판정된다.
    data.select({ provider: 'codex', linked: { id: 'x', label: 'Work Codex' } });
    await data.refresh();
    assert.equal(data.current(data.selected.codex).id, 'x');
    // 2. codex를 끝내고 셸에서 등록되지 않은 계정으로 다시 로그인한다. 셸 상태는 select(null)이다.
    const before = n_changes;
    data.select(null);
    assert.equal(data.selected.codex.linked, undefined, 'the terminal link is dropped once the agent is gone');
    assert.ok(n_changes > before, 'the bar is redrawn even though the active scope key did not change');
    assert.equal(data.current(data.selected.codex).id, 'x', 'Current CLI usage still confirms the same account until it changes');
    defaultAccount = null;
    await data.refresh();
    assert.equal(data.current(data.selected.codex).id, undefined, 'the new sign-in shows as Current CLI again');
    assert.equal(data.linkedProfile('codex'), null);
    assert.equal(data.snapshot(data.current(data.selected.codex)).data.windows[0].used, 40);
    await data.refresh({ all: true });
    data.select({ provider: 'codex' });
    defaultAccount = { id: 'x', label: 'Work Codex' };
    await data.refresh({ all: true });
    defaultAccount = null;
    // 연결된 동안 건너뛴 계정은, 이번 응답에서 연결이 풀리면 같은 주기에 읽혀 'Loading…'으로 남지 않는다.
    data.snapshots.delete('codex:x');
    await data.refresh({ all: true });
    assert.equal(data.snapshot(work).status, 'ready');
    assert.equal(data.snapshot(work).data.windows[0].used, 10);

    // 다른 도구의 터미널로 옮겨도 이 도구의 터미널 연결은 지운다. 같은 도구의 판정은 새 판정으로 바뀐다.
    data.select({ provider: 'codex', linked: { id: 'x', label: 'Work Codex' } });
    data.select({ provider: 'claude' });
    assert.equal(data.selected.codex.linked, undefined);
});

test('C-account-usage-linked: a terminal link names the account, but only a Current CLI reading of that account replaces its value', async () => {
    const main = { id: 'm', provider: 'claude', label: 'Main' };
    let defaultAccount = null;
    const data = new UsageData({
        listProfiles: async () => [main],
        isEnabled: () => true,
        fetchJson: async (_path, _method, query) => {
            const value = response(query === '?accountId=m' ? 84 : 97);
            // 계정 폴더의 자체 기록은 오래됐다. 기본 CLI 응답에 account가 없으면 백엔드가 두 환경의 로그인이 다르다고 본 것이다.
            value.claude.updatedAt = query === '?accountId=m' ? 100 : 5000;
            if (query === '?provider=claude' && defaultAccount) value.claude.account = defaultAccount;
            return value;
        },
    });
    data.select({ provider: 'claude', linked: { id: 'm', label: 'Main' } });
    await data.refresh({ all: true });
    const shown = data.current(data.selected.claude);
    assert.equal(shown.id, 'm', 'the bar names the saved account');
    // 더 최근 기본 CLI 값(97)은 이 컴퓨터 쪽 다른 로그인의 기록일 수 있어 Main 이름으로 보이지 않는다.
    assert.equal(data.snapshot(shown).data.windows[0].used, 84, 'without a matching account in the Current CLI reading, the account keeps its own reading');
    defaultAccount = { id: 'm', label: 'Main' };
    await data.refresh({ all: true });
    assert.equal(data.snapshot(shown).data.windows[0].used, 97, 'a Current CLI reading of the same account replaces the older one');
});

test('C-account-usage-age: old or recorded values carry their age next to the value', () => {
    const elements = [];
    const fakeElement = tag => {
        const element = { tag, className: '', textContent: '', title: '', children: [], classes: new Set() };
        element.append = (...items) => element.children.push(...items);
        element.classList = { toggle: (name, on) => (on ? element.classes.add(name) : element.classes.delete(name)) };
        elements.push(element);
        return element;
    };
    const previousDocument = global.document;
    global.document = { createElement: fakeElement };
    try {
        const now = Math.floor(Date.now() / 1000);
        const render = data => {
            const target = fakeElement('button');
            const panel = {
                data: { snapshot: () => ({ status: 'ready', data: { state: 'on', ...data } }) },
                meter: (id, label, used) => ({ meter: label, used }),
                displayName: () => 'Main',
            };
            UsagePanel.prototype.appendValues.call(panel, target, { provider: 'codex' });
            return target;
        };
        const fresh = render({ windows: [{ label: 'week', used: 16, resetsAt: now + 600 }], updatedAt: now - 60 });
        assert.equal(fresh.classes.has('is-stale'), false);
        assert.equal(fresh.children.some(child => child.className === 'account-usage-age'), false);
        const old = render({ windows: [{ label: 'week', used: 85, resetsAt: now + 600 }], updatedAt: now - 18 * 3600 });
        assert.equal(old.classes.has('is-stale'), true);
        assert.equal(old.children.find(child => child.className === 'account-usage-age').textContent, '18h ago');
        const recorded = render({ windows: [{ label: 'week', used: 85, resetsAt: now + 600 }], updatedAt: now - 120, recorded: true });
        const label = recorded.children.find(child => child.className === 'account-usage-age');
        assert.equal(label.textContent, '2m ago', 'a session record is labeled even when it is recent');
        assert.match(label.title, /Live usage could not be read/);
        // Claude는 자동 켜기가 물러선('unset') 설정 폴더에서도 기록 값을 보이고, 사용자가 끈('off') 경우만 숨긴다.
        const renderClaude = state => {
            const target = fakeElement('button');
            const panel = {
                data: { snapshot: () => ({ status: 'ready', data: { state, windows: [{ label: '5h', used: 21, resetsAt: now + 600 }], updatedAt: now - 60 } }) },
                meter: (id, label, used) => ({ meter: label, used }),
                displayName: () => 'Default',
            };
            UsagePanel.prototype.appendValues.call(panel, target, { provider: 'claude' });
            return target;
        };
        assert.equal(renderClaude('unset').children.filter(child => child.meter).length, 1);
        assert.equal(renderClaude('on').children.filter(child => child.meter).length, 1);
        assert.equal(renderClaude('off').children.some(child => child.meter), false);
    } finally {
        global.document = previousDocument;
    }
});

test('C-account-usage-launch: a new Claude account gets the status line before its terminal starts', async () => {
    const fresh = { id: 'n', provider: 'claude', label: 'New' };
    const calls = [];
    let profiles = [];
    const data = new UsageData({
        listProfiles: async () => profiles,
        isEnabled: () => true,
        fetchJson: async (path, method, query) => {
            calls.push(`${method} ${path}${query}`);
            return path === '/paddock/usage/claude' ? { state: 'on' } : { claude: { state: 'unset', windows: [], updatedAt: null } };
        },
    });
    await data.refresh();
    profiles = [fresh];
    await data.prepareLaunch(fresh);
    assert.ok(calls.includes('POST /paddock/usage/claude?enabled=true&accountId=n&automatic=true'));
    calls.length = 0;
    await data.prepareLaunch({ id: 'x', provider: 'codex', label: 'Codex' });
    assert.deepEqual(calls, []);
});
