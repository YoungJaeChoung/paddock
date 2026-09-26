const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { moveLegacyDirectory } = require('../legacy-directory');

function makeHome() {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-home-'));
    const legacy = path.join(home, '.herdr');
    fs.mkdirSync(path.join(legacy, 'config', 'usage'), { recursive: true });
    fs.mkdirSync(path.join(legacy, 'extensions'));
    fs.writeFileSync(path.join(legacy, 'config', 'settings.json'), '{\n  // 소리 끔\n  "herdr.agentDoneSound": false\n}\n');
    fs.mkdirSync(path.join(home, '.claude'));
    const script = path.join(legacy, 'config', 'usage', 'claude-statusline.cjs');
    const settings = { statusLine: { type: 'command', command: `"node" "${script}"` }, note: path.join(home, '.herdr-other', 'x') };
    fs.writeFileSync(path.join(home, '.claude', 'settings.json'), JSON.stringify(settings, null, 2));
    fs.writeFileSync(path.join(home, '.claude', 'settings.json.herdr-backup'), '{}');
    return home;
}

test('옛 폴더를 새 폴더로 옮기고 설정 키·상태 줄 경로·백업 이름을 바꾼다', () => {
    const home = makeHome();
    const directory = path.join(home, '.paddock');
    const claudeSettings = path.join(home, '.claude', 'settings.json');
    assert.equal(moveLegacyDirectory(home, directory, claudeSettings), true);
    assert.equal(fs.existsSync(path.join(home, '.herdr')), false);
    assert.equal(fs.existsSync(path.join(directory, 'extensions')), true);
    assert.match(fs.readFileSync(path.join(directory, 'config', 'settings.json'), 'utf8'), /\/\/ 소리 끔\n {2}"paddock\.agentDoneSound": false/);
    const settings = JSON.parse(fs.readFileSync(claudeSettings, 'utf8'));
    assert.equal(settings.statusLine.command, `"node" "${path.join(directory, 'config', 'usage', 'claude-statusline.cjs')}"`);
    assert.equal(settings.note, path.join(home, '.herdr-other', 'x'));
    assert.equal(fs.existsSync(`${claudeSettings}.paddock-backup`), true);
    assert.equal(fs.existsSync(`${claudeSettings}.herdr-backup`), false);
});

test('새 폴더가 이미 있으면 아무것도 옮기지 않는다', () => {
    const home = makeHome();
    const directory = path.join(home, '.paddock');
    fs.mkdirSync(directory);
    assert.equal(moveLegacyDirectory(home, directory, path.join(home, '.claude', 'settings.json')), false);
    assert.equal(fs.existsSync(path.join(home, '.herdr')), true);
});

test('옛 폴더가 없으면 아무것도 하지 않는다', () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-home-'));
    assert.equal(moveLegacyDirectory(home, path.join(home, '.paddock'), path.join(home, '.claude', 'settings.json')), false);
});
