// 확장 호스트와 주고받는 Theia 기본 구현 중 Paddock이 바꾸는 것을 끼워 넣는다.
const { ILogger } = require('@theia/core/lib/common/logger');
const { PLUGIN_RPC_CONTEXT, OutputChannelRegistryFactory } = require('@theia/plugin-ext/lib/common/plugin-api-rpc');
const { PaddockMessageRegistryMain } = require('./plugin-messages');
const { withOutputLog } = require('./plugin-output-log');

/**
 * 호스트와 주고받는 연결(rpc)에 이미 등록된 처리기를 `instance`로 바꾼다.
 *
 * `rpc.set`은 같은 식별자가 이미 있으면 새 것을 무시하므로, 먼저 등록된 기본 처리기를 지운 뒤 등록한다.
 * `locals`는 Theia가 공개하지 않은 내부 Map이라 Theia를 올릴 때 이 함수의 동작(교체가 실제로 일어나는지)을 다시 확인해야 한다.
 */
function replaceLocal(
    rpc,
    identifier,
    instance,
) {
    rpc.locals.delete(identifier.id);
    rpc.set(identifier, instance);
}

/**
 * 확장 호스트마다 한 번 불리며, 기본 등록기가 만들어진 뒤 Paddock 것으로 바꾼다.
 * Theia는 기본 등록기를 모두 만든 다음에 이 제공자들을 부르므로 바꿀 대상이 이미 있다.
 *
 * | 등록기 | 바꾼 동작 |
 * |---|---|
 * | 메시지 | 오류 모달은 알림으로, 같은 문구는 하나로 (plugin-messages.js) |
 * | 출력 채널 | 'Git' 채널의 오류·경고 줄을 로그에도 남김 (plugin-output-log.js) |
 */
class PaddockPluginApiOverrides {
    initialize(
        rpc,
        container,
    ) {
        replaceLocal(rpc, PLUGIN_RPC_CONTEXT.MESSAGE_REGISTRY_MAIN, new PaddockMessageRegistryMain(container));
        const outputChannelRegistry = container.get(OutputChannelRegistryFactory)();
        replaceLocal(rpc, PLUGIN_RPC_CONTEXT.OUTPUT_CHANNEL_REGISTRY_MAIN, withOutputLog(outputChannelRegistry, container.get(ILogger)));
    }
}

module.exports = { PaddockPluginApiOverrides };
