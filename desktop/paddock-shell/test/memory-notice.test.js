const test = require('node:test');
const assert = require('node:assert/strict');
const { MemoryNotice, MEMORY_NOTICE } = require('../workspace/memory-notice');

function fixture(
    preferences,
    profiles = [],
) {
    const requests = [];
    const warnings = [];
    const workspace = {
        preferences: { get: (name, fallback) => (name in preferences ? preferences[name] : fallback) },
        accounts: { list: async () => profiles },
        messages: { warn: async message => { warnings.push(message); } },
        fetchJson: async (path, method, query) => {
            requests.push(`${method} ${path}${query}`);
            return { state: 'on' };
        },
    };
    return { notice: new MemoryNotice(workspace), requests, warnings, workspace };
}

const CLAUDE = { id: 'aaaaaaaa-0000-4000-8000-000000000001', provider: 'claude' };
const CODEX = { id: 'aaaaaaaa-0000-4000-8000-000000000002', provider: 'codex' };

test('MN1 꺼져 있어도 시작 동기화는 기본 환경과 Claude 계정에만 끄기를 보낸다', async () => {
    const { notice, requests } = fixture({}, [CLAUDE, CODEX]);
    await notice.syncAll();
    assert.deepEqual(requests, [
        'POST /paddock/memory-notice?enabled=false&minimumMb=4096',
        `POST /paddock/memory-notice?enabled=false&minimumMb=4096&accountId=${CLAUDE.id}`,
    ]);
});

test('MN2 켜면 기준값을 함께 보내고, 잘못된 기준값은 기본값으로 바꾼다', async () => {
    const on = fixture({ [MEMORY_NOTICE.ENABLED]: true, [MEMORY_NOTICE.MINIMUM_MB]: 2048.9 });
    await on.notice.syncAll();
    assert.deepEqual(on.requests, ['POST /paddock/memory-notice?enabled=true&minimumMb=2048']);
    for (const bad of [-1, 'abc', NaN]) {
        const { notice } = fixture({ [MEMORY_NOTICE.MINIMUM_MB]: bad });
        assert.equal(notice.minimumMb(), 4096);
    }
});

test('MN3 계정 터미널을 띄울 때는 켜져 있고 Claude 계정일 때만 그 계정에 보낸다', async () => {
    const off = fixture({});
    await off.notice.prepareLaunch(CLAUDE);
    assert.deepEqual(off.requests, []);
    const on = fixture({ [MEMORY_NOTICE.ENABLED]: true });
    await on.notice.prepareLaunch(CODEX);
    await on.notice.prepareLaunch(CLAUDE);
    assert.deepEqual(on.requests, [`POST /paddock/memory-notice?enabled=true&minimumMb=4096&accountId=${CLAUDE.id}`]);
});

test('MN4 한 곳이 실패해도 나머지를 계속하고 같은 오류는 한 번만 알린다', async () => {
    const { notice, requests, warnings, workspace } = fixture({ [MEMORY_NOTICE.ENABLED]: true }, [CLAUDE]);
    workspace.fetchJson = async (path, method, query) => {
        requests.push(query);
        if (!query.includes('accountId')) throw new Error('Claude settings contain invalid JSON.');
        return { state: 'on' };
    };
    await notice.syncAll();
    await notice.syncAll();
    assert.equal(requests.length, 4);
    assert.deepEqual(warnings, ['Memory notice: Claude settings contain invalid JSON.']);
});

test('MN5 요청은 하나씩 차례로 보낸다', async () => {
    const { notice, workspace } = fixture({ [MEMORY_NOTICE.ENABLED]: true }, [CLAUDE]);
    let running = 0;
    let peak = 0;
    workspace.fetchJson = async () => {
        running += 1;
        peak = Math.max(peak, running);
        await new Promise(resolve => setTimeout(resolve, 5));
        running -= 1;
        return {};
    };
    await Promise.all([notice.syncAll(), notice.syncAll(), notice.prepareLaunch(CLAUDE)]);
    assert.equal(peak, 1);
});
