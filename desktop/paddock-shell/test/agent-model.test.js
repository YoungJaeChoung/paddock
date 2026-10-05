const test = require('node:test');
const assert = require('node:assert/strict');
const agent = require('../agent-model');

test('C-agent-A1.1: 명령줄에서 프로그램 이름을 뽑고 에이전트는 에이전트 이름으로 적는다', () => {
    assert.equal(agent.programName(['/home/me/.local/bin/claude', '--resume']), 'claude');
    assert.equal(agent.programName(['node', '/usr/lib/node_modules/@openai/codex/bin/codex.js']), 'codex');
    assert.equal(agent.programName(['node', '/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js']), 'claude');
    assert.equal(agent.programName(['C:\\Tools\\codex.exe']), 'codex');
    assert.equal(agent.programName(['node', 'server.js']), 'server');
    assert.equal(agent.programName(['-bash']), 'bash');
    assert.equal(agent.programName(['/bin/bash', '/home/me/bin/claude']), 'claude');
    assert.equal(agent.programName(['/bin/bash', '--rcfile', '/dev/fd/56', '-i']), 'bash');
    assert.equal(agent.programName(['/bin/zsh', '-l']), 'zsh');
    assert.equal(agent.programName([]), null);
    assert.equal(agent.isAgent('claude'), true);
    assert.equal(agent.isAgent('bash'), false);
});

test('C-account-idle-judge: 셸 이름만 빈 셸로 보고 셸이 실행한 스크립트·에이전트·다른 프로그램·모름은 빈 셸이 아니다', () => {
    for (const argv of [['-bash'], ['/bin/zsh', '-l'], ['/usr/bin/fish'], ['C:\\Program Files\\PowerShell\\7\\pwsh.exe'], ['powershell.exe', '-NoLogo'], ['cmd.exe']]) {
        assert.equal(agent.isShell(agent.programName(argv)), true, argv.join(' '));
    }
    for (const argv of [['/bin/bash', '/home/me/build.sh'], ['/home/me/.local/bin/claude'], ['node', 'server.js'], ['vim', 'a.txt']]) {
        assert.equal(agent.isShell(agent.programName(argv)), false, argv.join(' '));
    }
    assert.equal(agent.isShell(null), false);
    assert.equal(agent.isShell(undefined), false);
});

test('C-agent-A2.1: Enter 뒤 오래 일하던 에이전트가 조용해지면 한 번 알린다', () => {
    const { QUIET_MS, MIN_BUSY_MS } = agent.ACTIVITY;
    let a = agent.noteInput(agent.idle(), 'fix the bug\r', 0);
    for (let t = 100; t <= MIN_BUSY_MS + 1000; t += 200) a = agent.noteOutput(a, t);
    const last = MIN_BUSY_MS + 1000;
    let r = agent.settle(a, last + 500, true);
    assert.equal(r.finished, false, '아직 조용한 시간이 짧다');
    r = agent.settle(r.activity, last + QUIET_MS, true);
    assert.equal(r.finished, true);
    assert.equal(agent.settle(r.activity, last + QUIET_MS * 3, true).finished, false, '한 번만 알린다');
});

test('C-agent-A2.2: 짧은 응답·Enter 없는 출력·에이전트가 아닌 프로그램은 알리지 않는다', () => {
    const { QUIET_MS } = agent.ACTIVITY;
    let a = agent.noteOutput(agent.noteInput(agent.idle(), '\r', 0), 1000);
    assert.equal(agent.settle(a, 1000 + QUIET_MS, true).finished, false, '짧게 끝난 응답');
    a = agent.idle();
    for (let t = 0; t <= 10000; t += 200) a = agent.noteOutput(a, t);
    assert.equal(agent.settle(a, 10000 + QUIET_MS, true).finished, false, 'Enter 없이 나온 출력(시작 화면 등)');
    a = agent.noteInput(agent.idle(), 'npm test\r', 0);
    for (let t = 100; t <= 10000; t += 200) a = agent.noteOutput(a, t);
    assert.equal(agent.settle(a, 10000 + QUIET_MS, false).finished, false, '에이전트가 아닌 긴 명령');
});

test('C-agent-A2.3: 일하는 동안 에이전트였으면 끝나며 셸로 돌아가도 알린다', () => {
    const { QUIET_MS } = agent.ACTIVITY;
    let a = agent.noteInput(agent.idle(), 'claude -p "summarize"\r', 0);
    for (let t = 100; t <= 8000; t += 200) a = agent.noteOutput(a, t);
    a = agent.settle(a, 8100, true).activity;
    assert.equal(agent.settle(a, 8000 + QUIET_MS, false).finished, true);
});

test('C-agent-A3.1: 입력 전·응답 전·출력 중을 구분하고 출력이 3초 멎으면 대기로 표시한다', () => {
    const { QUIET_MS } = agent.ACTIVITY;
    const initial = agent.idle();
    assert.equal(agent.activityState(initial, 0, true), 'quiet');
    assert.equal(agent.activityState(agent.noteInput(initial, 'typing', 100), 100, true), 'quiet');
    const submitted = agent.noteInput(initial, 'run\r', 200);
    assert.equal(agent.activityState(submitted, 300, true), 'waiting');
    const output = agent.noteOutput(submitted, 400);
    assert.equal(agent.activityState(output, 400 + QUIET_MS - 1, true), 'working');
    assert.equal(agent.activityState(output, 400 + QUIET_MS, true), 'quiet');
    assert.equal(agent.activityState(agent.settle(output, 400 + QUIET_MS, true).activity, 400 + QUIET_MS, true), 'quiet');
});

test('C-agent-A3.2: 에이전트가 종료돼 셸로 돌아오면 활동 기록이 남아도 상태를 표시하지 않는다', () => {
    const output = agent.noteOutput(agent.noteInput(agent.idle(), '\r', 0), 100);
    assert.equal(agent.activityState(output, 200, false), null);
    assert.equal(agent.activityState(agent.idle(), 200, false), null);
});

test('C-agent-A3.3: 한동안 조용하다가 Enter 없이 출력이 재개돼도 작업 중 표시가 돌아온다', () => {
    const { QUIET_MS } = agent.ACTIVITY;
    const output = agent.noteOutput(agent.noteInput(agent.idle(), '\r', 0), 100);
    const paused = agent.settle(output, 100 + QUIET_MS, true).activity;
    assert.equal(agent.activityState(paused, 100 + QUIET_MS, true), 'quiet');
    const resumed = agent.noteOutput(paused, 5000);
    assert.equal(agent.activityState(resumed, 5000, true), 'working');
    assert.equal(agent.settle(resumed, 5000 + QUIET_MS, true).finished, false, '무입력 출력은 완료 알림을 반복하지 않는다');
    assert.equal(agent.activityState(agent.noteOutput(agent.idle(), 0), 1, true), 'working', '시작 화면 출력도 최근 활동으로 표시한다');
});

test('C-agent-A3.4: 입력 후 출력이 없어도 3초 경계에서 Sent가 끝나고 Quiet가 된다', () => {
    const sent = agent.noteInput(agent.idle(), '\r', 100);
    assert.equal(agent.activityState(sent, 100 + agent.ACTIVITY.QUIET_MS - 1, true), 'waiting');
    assert.equal(agent.activityState(sent, 100 + agent.ACTIVITY.QUIET_MS, true), 'quiet');
    assert.equal(agent.activityState(sent, 60000, true), 'quiet');
    const lateOutput = agent.noteOutput(sent, 60001);
    assert.equal(agent.activityState(lateOutput, 60001, true), 'working');
});

test('C-agent-A3.5: 시작·입력·응답·재출력·셸 복귀를 빠짐없이 한 상태로 표시한다', () => {
    const histories = [
        [agent.idle(), ['quiet', 'quiet', 'quiet', 'quiet']],
        [agent.noteInput(agent.idle(), '\r', 100), ['waiting', 'quiet', 'quiet', 'quiet']],
        [agent.noteOutput(agent.idle(), 200), ['working', 'working', 'quiet', 'quiet']],
        [agent.noteOutput(agent.noteInput(agent.idle(), '\r', 100), 200), ['working', 'working', 'quiet', 'quiet']],
    ];
    for (const [history, expectedStates] of histories) {
        for (const [index, now] of [200, 3100, 3200, 10000].entries()) {
            const expected = expectedStates[index];
            assert.equal(agent.activityState(history, now, true), expected);
            assert.equal(agent.activityState(history, now, false), null);
            assert.equal(agent.activityState(history, now, null), 'unknown');
        }
    }
});
