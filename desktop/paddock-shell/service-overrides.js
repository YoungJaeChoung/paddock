// Paddock이 Theia 백엔드 서비스를 바꿔 끼우는 지점.
// Paddock의 백엔드 모듈은 Theia 터미널·확장 모듈보다 먼저 로드돼, 모듈 안에서 rebind하면 바꿀 대상이 아직 없어 시작이 실패한다.
// 모든 모듈이 로드된 뒤 시작 기여(initialize)로 컨테이너를 받아 바꾼다.
const os = require('node:os');
const path = require('node:path');
const { MultiRingBuffer } = require('@theia/process/lib/node/multi-ring-buffer');
const { PluginPathsService } = require('@theia/plugin-ext/lib/main/common/plugin-paths-protocol');
const bufferingStream = require('@theia/terminal/lib/node/buffering-stream');
const { ShellProcessFactory } = require('@theia/terminal/lib/node/shell-process');
const { PaddockReplayBuffer } = require('./terminal-replay-buffer');
const { PaddockOutputStream } = require('./terminal-output-stream');
const { PaddockPluginPathsService } = require('./plugin-paths');
const { terminalHistoryService } = require('./terminal-history-service');
const { ClaudeWrapper, withWrapperPath } = require('./claude-wrapper');

/**
 * 백엔드가 시작될 때 Theia 서비스 구현을 Paddock 것으로 교체한다.
 *
 * 교체한 서비스는 이 시점 뒤에 처음 만들어져야 새 구현을 받는다. 시작 기여 생성 때 이미 주입된 서비스는 교체되지 않는다.
 *
 * | 서비스 | 교체 구현 | 이유 |
 * |---|---|---|
 * | `MultiRingBuffer` | `PaddockReplayBuffer` | 터미널 지난 출력 재생이 링이 한 바퀴 돈 뒤 중복되고 한글이 깨진다 |
 * | `PluginPathsService` | `PaddockPluginPathsService` | 작업 공간을 열지 않으면 확장의 작업 공간 저장소가 저장되지 않는다 |
 * | `StringBufferingStream` | `PaddockOutputStream` | 터미널 출력을 모두 16ms(Windows에서는 약 31ms) 붙잡아 키 에코가 늦게 보인다 |
 * | `ShellProcessFactory` | 같은 셸 프로세스를 만든 뒤 `TerminalHistory`에 추적을 맡기는 함수 | 앱이 터미널 프로세스째 사라져도 마지막 화면·폴더·대화를 파일에 남겨 되살린다 |
 *
 * `ShellProcessFactory` 교체는 셸의 PATH 맨 앞에 Paddock의 `claude` 실행 래퍼 폴더도 붙인다(claude-wrapper.js).
 * 일반 터미널에서 친 `claude`에도 Paddock 전용 설정이 붙게 하려는 것이다. `wsl.exe`가 띄우는 셸은 WSL 안의 PATH와 무관하고, 환경을 그대로 쓰라고 한 셸(`strictEnv`)은 환경을 보태지 않아야 하므로 붙이지 않는다.
 */
class PaddockServiceOverrides {
    constructor(
        container,
    ) {
        this.container = container;
    }

    initialize() {
        // MultiRingBuffer는 터미널을 새로 열 때마다 만들어지므로(일시 범위) 지금 바꾸면 이후 모든 터미널이 새 구현을 쓴다.
        this.container.rebind(MultiRingBuffer).to(PaddockReplayBuffer).inTransientScope();
        // 터미널 출력 버퍼는 컨테이너가 아니라 터미널 연결이 열릴 때마다 모듈의 클래스를 직접 `new`로 만든다.
        // 모듈이 내보낸 클래스를 바꿔 두면 이후 열리는 모든 터미널 연결이 새 구현을 쓴다.
        bufferingStream.StringBufferingStream = PaddockOutputStream;
        // 확장 저장소 경로는 확장 호스트가 뜰 때와 확장이 저장할 때 처음 쓰이므로 지금 바꿔도 이전 구현이 주입된 곳이 없다.
        this.container.rebind(PluginPathsService).to(PaddockPluginPathsService).inSingletonScope();
        // 셸 터미널은 ShellProcessFactory로 만든다. 같은 프로세스를 돌려주되 화면 저장 장치에 추적을 맡긴다.
        // 터미널 서버는 연결마다 만들어져 이 시점 뒤에 이 함수를 받는다.
        const { history } = terminalHistoryService();
        const createShellProcess = this.container.get(ShellProcessFactory);
        const wrapperDirectory = path.join(process.env.THEIA_CONFIG_DIR || path.join(os.homedir(), '.paddock', 'config'), ClaudeWrapper.DIRECTORY);
        this.container.rebind(ShellProcessFactory).toConstantValue(options => {
            const isWslShell = /(?:^|[\\/])wsl(?:\.exe)?$/i.test(options.shell || '');
            const shellProcess = createShellProcess(isWslShell || options.strictEnv === true ? options : { ...options, env: withWrapperPath(options.env, wrapperDirectory) });
            history.track(shellProcess);
            return shellProcess;
        });
        // 앱이 터미널을 끝내려는 중이면 그때 끝나는 터미널의 저장분을 남긴다(다시 열 때 되살린다). 사용자가 닫은 터미널과 구분하는 신호다.
        for (const event of ['SIGTERM', 'SIGINT', 'exit']) process.on(event, () => history.beginStop());
    }
}

module.exports = { PaddockServiceOverrides };
