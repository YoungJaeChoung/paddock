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

test('MN1 꺼져 있어도 시작 동기화는 끄기를 한 번 보내고 등록 계정에는 따로 보내지 않는다', async () => {
    const { notice, requests } = fixture({}, [CLAUDE, CODEX]);
    await notice.syncAll();
    assert.deepEqual(requests, ['POST /paddock/memory-notice?enabled=false&minimumMb=4096']);
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

test('MN3 실패하면 첫 문장을 한 번만 알리고, 오류가 사라지면 다시 알릴 수 있다', async () => {
    const { notice, requests, warnings, workspace } = fixture({ [MEMORY_NOTICE.ENABLED]: true }, [CLAUDE]);
    let isFailing = true;
    workspace.fetchJson = async (path, method, query) => {
        requests.push(query);
        if (isFailing) throw new Error('The Paddock settings file could not be written.');
        return { state: 'on' };
    };
    await notice.syncAll();
    await notice.syncAll();
    assert.equal(requests.length, 2);
    assert.deepEqual(warnings, ['Memory notice: The Paddock settings file could not be written.']);
    isFailing = false;
    await notice.syncAll();
    isFailing = true;
    await notice.syncAll();
    assert.equal(warnings.length, 2);
});

test('MN4 요청은 하나씩 차례로 보낸다', async () => {
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
    await Promise.all([notice.syncAll(), notice.syncAll(), notice.syncAll()]);
    assert.equal(peak, 1);
});
