const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const backend = require('../backend-module');

function fixture(
    context,
) {
    let Contribution;
    backend.default.registry(token => {
        if (token?.name === 'PaddockStatusRoutes') Contribution = token;
        return { toSelf: () => ({ inSingletonScope() {} }), toService() {}, toConstantValue() {} };
    });
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-usage-route-'));
    const previous = process.env.THEIA_CONFIG_DIR;
    process.env.THEIA_CONFIG_DIR = root;
    context.after(() => { if (previous === undefined) delete process.env.THEIA_CONFIG_DIR; else process.env.THEIA_CONFIG_DIR = previous; fs.rmSync(root, { recursive: true, force: true }); });
    const contribution = new Contribution();
    const routes = new Map();
    contribution.configure({ get: (url, route) => routes.set(url, route), post() {}, delete() {} });
    const call = async query => {
        let value;
        let status = 200;
        await routes.get('/paddock/usage')({ query }, { json: data => { value = data; }, status: code => { status = code; return { json: data => { value = data; } }; } });
        return { value, status };
    };
    return { root, contribution, call };
}

test('UR01 a Claude-only default lookup never starts Codex even if its CLI is installed', async context => {
    const { root, contribution, call } = fixture(context);
    fs.mkdirSync(path.join(root, 'usage'));
    fs.writeFileSync(path.join(root, 'usage/claude.json'), JSON.stringify({ rate_limits: { five_hour: { used_percentage: 15 } }, updated_at: 100 }));
    const read = context.mock.method(contribution.liveCodexUsage, 'readAvailable', async () => { throw new Error('Codex must remain idle'); });
    const response = await call({ provider: 'claude' });
    assert.equal(response.status, 200);
    assert.equal(response.value.claude.windows[0].used, 15);
    assert.equal(read.mock.callCount(), 0);
});

test('UR02 a Codex-only default lookup ignores an unreadable Claude summary and rejects invalid provider names', async context => {
    const { root, contribution, call } = fixture(context);
    fs.mkdirSync(path.join(root, 'usage'));
    fs.writeFileSync(path.join(root, 'usage/claude.json'), '{unfinished');
    context.mock.method(contribution.liveCodexUsage, 'readAvailable', async () => ({ windows: [{ label: 'week', used: 83 }], updatedAt: 100 }));
    const response = await call({ provider: 'codex' });
    assert.equal(response.status, 200);
    assert.equal(response.value.codex.windows[0].used, 83);
    assert.equal((await call({ provider: ['claude', 'codex'] })).status, 400);
    assert.equal((await call({ provider: 'unknown' })).status, 400);
});
