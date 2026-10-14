// Paddock이 Theia 백엔드 서비스를 바꿔 끼우는 지점.
// Paddock의 백엔드 모듈은 Theia 터미널·확장 모듈보다 먼저 로드돼, 모듈 안에서 rebind하면 바꿀 대상이 아직 없어 시작이 실패한다.
// 모든 모듈이 로드된 뒤 시작 기여(initialize)로 컨테이너를 받아 바꾼다.
const { MultiRingBuffer } = require('@theia/process/lib/node/multi-ring-buffer');
const { PluginPathsService } = require('@theia/plugin-ext/lib/main/common/plugin-paths-protocol');
const { PaddockReplayBuffer } = require('./terminal-replay-buffer');
const { PaddockPluginPathsService } = require('./plugin-paths');

/**
 * 백엔드가 시작될 때 Theia 서비스 구현을 Paddock 것으로 교체한다.
 *
 * 교체한 서비스는 이 시점 뒤에 처음 만들어져야 새 구현을 받는다. 시작 기여 생성 때 이미 주입된 서비스는 교체되지 않는다.
 *
 * | 서비스 | 교체 구현 | 이유 |
 * |---|---|---|
 * | `MultiRingBuffer` | `PaddockReplayBuffer` | 터미널 지난 출력 재생이 링이 한 바퀴 돈 뒤 중복되고 한글이 깨진다 |
 * | `PluginPathsService` | `PaddockPluginPathsService` | 작업 공간을 열지 않으면 확장의 작업 공간 저장소가 저장되지 않는다 |
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
        // 확장 저장소 경로는 확장 호스트가 뜰 때와 확장이 저장할 때 처음 쓰이므로 지금 바꿔도 이전 구현이 주입된 곳이 없다.
        this.container.rebind(PluginPathsService).to(PaddockPluginPathsService).inSingletonScope();
    }
}

module.exports = { PaddockServiceOverrides };
