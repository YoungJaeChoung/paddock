const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PaddockPluginPathsService, WORKSPACE_ID } = require('../plugin-paths');

/** 설정 폴더를 임시 폴더로 둔 서비스. 기본 구현이 주입받는 값 중 저장 경로 계산에 쓰이는 것만 채운다. */
function service(
    context,
) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-plugin-paths-'));
    context.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const paths = new PaddockPluginPathsService();
    paths.envServer = { getConfigDirUri: async () => `file://${root}` };
    paths.untitledWorkspaceService = { getUntitledWorkspaceUri: async () => ({ toString: () => 'file:///untitled' }) };
    return { paths, root };
}

test('작업 공간이 없어도 고정 폴더를 만들어 그 경로를 돌려준다', async context => {
    const { paths, root } = service(context);
    const first = await paths.getHostStoragePath(undefined, []);
    assert.equal(typeof first, 'string');
    assert.equal(path.dirname(first), path.join(root, 'workspace-storage'));
    assert.equal(fs.statSync(first).isDirectory(), true);
    // 호출마다 같은 폴더다. 같은 값(고정 id)을 직접 넘겨도 같다.
    assert.equal(await paths.getHostStoragePath(undefined, []), first);
    assert.equal(await paths.getHostStoragePath('', []), first);
    assert.equal(await paths.getHostStoragePath(WORKSPACE_ID, []), first);
});

test('작업 공간이 열려 있으면 그 작업 공간의 폴더를 쓴다', async context => {
    const { paths } = service(context);
    const fixed = await paths.getHostStoragePath(undefined, []);
    const opened = await paths.getHostStoragePath('file:///home/me/app', []);
    assert.notEqual(opened, fixed);
});
