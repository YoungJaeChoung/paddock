const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { usageFileFor, userStatusLineCommand } = require('../claude-statusline.cjs');

const SCRIPT = path.join(__dirname, '..', 'claude-statusline.cjs');
const ACCOUNT_ID = '00000000-0000-4000-8000-000000000001';
const INPUT = JSON.stringify({ model: { display_name: 'Opus' }, context_window: { used_percentage: 41.6 }, rate_limits: { five_hour: { used_percentage: 12 } } });

function folder(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'statusline-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

function runScript(
    argumentsList,
    environment,
    input = INPUT,
) {
    return spawnSync(process.execPath, [SCRIPT, ...argumentsList], { input, encoding: 'utf8', env: { PATH: process.env.PATH, ...environment } });
}

test('C-statusline.1: 등록 계정 폴더면 그 폴더의 기록 파일, 아니면 기본 파일을 쓴다', () => {
    const account = `/home/me/.paddock/config/agent-profiles/${ACCOUNT_ID}/claude`;
    assert.equal(usageFileFor(account, '/default/claude.json'), path.join(account, 'paddock-usage', 'claude.json'));
    assert.equal(usageFileFor('/home/me/.claude', '/default/claude.json'), '/default/claude.json');
    assert.equal(usageFileFor(undefined, '/default/claude.json'), '/default/claude.json');
    assert.equal(usageFileFor(`C:\\Users\\me\\.paddock\\config\\agent-profiles\\${ACCOUNT_ID}\\claude`, '/d.json'), path.join(`C:\\Users\\me\\.paddock\\config\\agent-profiles\\${ACCOUNT_ID}\\claude`, 'paddock-usage', 'claude.json'));
});

test('C-statusline.2: 등록 계정 환경에서는 계정 폴더에, 기본 환경에서는 인자 파일에 사용량을 남기고 기본 출력을 보인다', context => {
    const root = folder(context);
    const account = path.join(root, 'agent-profiles', ACCOUNT_ID, 'claude');
    fs.mkdirSync(account, { recursive: true });
    const defaultFile = path.join(root, 'usage', 'claude.json');
    const result = runScript([defaultFile], { CLAUDE_CONFIG_DIR: account });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, 'Opus · 42% context');
    const record = JSON.parse(fs.readFileSync(path.join(account, 'paddock-usage', 'claude.json'), 'utf8'));
    assert.deepEqual(record.rate_limits, { five_hour: { used_percentage: 12 } });
    assert.equal(fs.existsSync(defaultFile), false);
    const other = path.join(root, 'plain');
    fs.mkdirSync(other);
    assert.equal(runScript([defaultFile], { CLAUDE_CONFIG_DIR: other }).status, 0);
    assert.equal(JSON.parse(fs.readFileSync(defaultFile, 'utf8')).rate_limits.five_hour.used_percentage, 12);
});

test('C-statusline.3: 사용자 설정의 원래 상태 줄을 같은 입력으로 불러 그 출력을 이어 보인다', context => {
    const root = folder(context);
    const configDir = path.join(root, 'config');
    fs.mkdirSync(configDir);
    fs.writeFileSync(path.join(configDir, 'settings.json'), JSON.stringify({ statusLine: { type: 'command', command: 'cat >/dev/null; printf user-line' } }));
    const result = runScript([path.join(root, 'claude.json')], { CLAUDE_CONFIG_DIR: configDir });
    assert.equal(result.stdout, 'user-line');
});

test('C-statusline.4: 원래 상태 줄이 이 스크립트를 가리키면 자기 자신을 다시 부르지 않고 기본 출력을 보인다', context => {
    const root = folder(context);
    const configDir = path.join(root, 'config');
    fs.mkdirSync(configDir);
    fs.writeFileSync(path.join(configDir, 'settings.json'), JSON.stringify({ statusLine: { type: 'command', command: '"/usr/local/bin/node" "/Users/me/.paddock/config/usage/claude-statusline.cjs" "/x/claude.json"' } }));
    assert.equal(userStatusLineCommand(configDir), null);
    const result = runScript([path.join(root, 'claude.json')], { CLAUDE_CONFIG_DIR: configDir });
    assert.equal(result.stdout, 'Opus · 42% context');
});

test('C-statusline.5: 설정이 없거나 깨졌거나 명령 종류가 아니면 원래 상태 줄은 없다', context => {
    const root = folder(context);
    assert.equal(userStatusLineCommand(root), null);
    fs.writeFileSync(path.join(root, 'settings.json'), '{ broken');
    assert.equal(userStatusLineCommand(root), null);
    fs.writeFileSync(path.join(root, 'settings.json'), JSON.stringify({ statusLine: { type: 'text', command: 'x' } }));
    assert.equal(userStatusLineCommand(root), null);
});

test('C-statusline.6: 기록 폴더를 만들 수 없어도 상태 줄 출력은 그대로 나온다', context => {
    const root = folder(context);
    fs.writeFileSync(path.join(root, 'blocker'), '');
    const result = runScript([path.join(root, 'blocker', 'claude.json')], { CLAUDE_CONFIG_DIR: path.join(root, 'none') });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, 'Opus · 42% context');
});
