const test = require('node:test');
const assert = require('node:assert/strict');
const { repoKeyOf } = require('../worktree-repo');

const DIRECTORY = { directory: true };
const file = (content) => ({ content });

test('WR-1: .git 디렉터리면 그 폴더가 저장소 루트다(끝 슬래시는 뗀다)', () => {
    assert.equal(repoKeyOf('file:///tmp/main', DIRECTORY), 'file:///tmp/main');
    assert.equal(repoKeyOf('file:///tmp/main/', DIRECTORY), 'file:///tmp/main');
});

test('WR-2: 연결된 워크트리의 .git 파일은 주 저장소 폴더를 가리킨다', () => {
    assert.equal(repoKeyOf('file:///tmp/wt1', file('gitdir: /tmp/main/.git/worktrees/wt1\n')), 'file:///tmp/main');
    assert.equal(repoKeyOf('file:///tmp/wt1', file('gitdir: /tmp/main/.git/worktrees/wt1')), 'file:///tmp/main');
    assert.equal(repoKeyOf('file:///tmp/wt1', file('gitdir: /tmp/main/.git/worktrees/wt1\r\n')), 'file:///tmp/main');
});

test('WR-3: 상대 gitdir은 폴더 기준으로 푼다', () => {
    assert.equal(repoKeyOf('file:///tmp/wt1', file('gitdir: ../main/.git/worktrees/wt1')), 'file:///tmp/main');
    assert.equal(repoKeyOf('file:///a/b/wt', file('gitdir: ../../../c/main/.git/worktrees/wt')), 'file:///c/main');
});

test('WR-4: 서브모듈·그 밖의 .git 파일·없음·읽기 실패는 묶지 않는다', () => {
    assert.equal(repoKeyOf('file:///tmp/sub', file('gitdir: ../.git/modules/sub')), null);
    assert.equal(repoKeyOf('file:///tmp/x', file('gitdir: /tmp/main/.git')), null);
    assert.equal(repoKeyOf('file:///tmp/x', file('not a gitdir file')), null);
    assert.equal(repoKeyOf('file:///tmp/x', file('')), null);
    assert.equal(repoKeyOf('file:///tmp/x', { none: true }), null);
    assert.equal(repoKeyOf('file:///tmp/x', null), null);
    assert.equal(repoKeyOf('file:///tmp/x', undefined), null);
});

test('WR-5: WSL 폴더의 gitdir(Linux 경로)에는 폴더 키의 배포판 접두를 붙인다', () => {
    const folder = 'file://wsl.localhost/Ubuntu/home/me/wt1';
    assert.equal(repoKeyOf(folder, file('gitdir: /home/me/main/.git/worktrees/wt1')), 'file://wsl.localhost/Ubuntu/home/me/main');
    assert.equal(repoKeyOf('file://wsl.localhost/Ubuntu/home/me/main', DIRECTORY), 'file://wsl.localhost/Ubuntu/home/me/main');
    // 상대 경로는 폴더 경로(배포판 이름 포함) 안에서 푼다.
    assert.equal(repoKeyOf(folder, file('gitdir: ../main/.git/worktrees/wt1')), 'file://wsl.localhost/Ubuntu/home/me/main');
    // 배포판이 다른 폴더의 같은 Linux 경로는 다른 저장소다.
    assert.notEqual(repoKeyOf('file://wsl.localhost/Debian/home/me/wt1', file('gitdir: /home/me/main/.git/worktrees/wt1')),
        repoKeyOf(folder, file('gitdir: /home/me/main/.git/worktrees/wt1')));
});

test('WR-6: 인코딩된 경로·Windows 드라이브 경로를 폴더 키 형식으로 되돌린다', () => {
    assert.equal(repoKeyOf('file:///home/me/my%20wt', file('gitdir: /home/me/my main/.git/worktrees/my wt')), 'file:///home/me/my%20main');
    assert.equal(repoKeyOf('file:///c%3A/dev/wt', file('gitdir: C:/dev/main/.git/worktrees/wt')), 'file:///c%3A/dev/main');
    assert.equal(repoKeyOf('file:///c%3A/dev/wt', file('gitdir: C:\\dev\\main\\.git\\worktrees\\wt')), 'file:///c%3A/dev/main');
    assert.equal(repoKeyOf('file:///c%3A/dev/wt', file('gitdir: ../main/.git/worktrees/wt')), 'file:///c%3A/dev/main');
});

test('WR-7: Theia가 \\wsl$를 인코딩한 서버 이름(wsl%24)도 WSL 폴더로 본다', () => {
    const folder = 'file://wsl%24/Ubuntu/home/me/wt1';
    assert.equal(repoKeyOf(folder, file('gitdir: /home/me/main/.git/worktrees/wt1')), 'file://wsl%24/Ubuntu/home/me/main');
    assert.equal(repoKeyOf(folder, file('gitdir: ../main/.git/worktrees/wt1')), 'file://wsl%24/Ubuntu/home/me/main');
    assert.equal(repoKeyOf('file://wsl%24/Ubuntu/home/me/main', DIRECTORY), 'file://wsl%24/Ubuntu/home/me/main');
});

test('WR-8: Windows 드라이브 폴더에서 WSL git이 만든 워크트리(/mnt/<문자>/…)는 드라이브 키로 되돌린다', () => {
    assert.equal(repoKeyOf('file:///c%3A/dev/wt', file('gitdir: /mnt/c/dev/main/.git/worktrees/wt')), 'file:///c%3A/dev/main');
    assert.equal(repoKeyOf('file:///d%3A/dev/wt', file('gitdir: /mnt/c/dev/main/.git/worktrees/wt')), 'file:///c%3A/dev/main');
    // 드라이브 폴더가 아니면 /mnt/c는 그냥 Linux 경로다.
    assert.equal(repoKeyOf('file:///home/me/wt', file('gitdir: /mnt/c/dev/main/.git/worktrees/wt')), 'file:///mnt/c/dev/main');
});
