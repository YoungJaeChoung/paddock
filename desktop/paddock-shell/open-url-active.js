/**
 * 주소 열기(`--open-url`)로 연 위젯을, 창에 포커스가 없어도 활성 위젯으로 만든다.
 *
 * Theia의 활성 위젯은 DOM의 focus 이벤트로만 바뀌는데, 포커스 없는 창에서는 `activate()`가 이 이벤트를 부르지 않는다.
 * 그러면 이미 탭으로 열린 파일을 다시 열어도 확장이 활성 변경을 받지 못한다. Pencil은 이 변경으로 MCP가 쓸 문서를 정하므로,
 * 에이전트가 다른 문서에 쓰게 된다. 창 포커스를 뺏으면 그 PC를 쓰는 사람의 입력을 끊으므로, 이벤트만 보내 추적기를 맞춘다.
 *
 * Examples
 * --------
 * | 창 포커스 | 위젯    | 결과             |
 * | --------- | ------- | ---------------- |
 * | 있음      | 살아 있음 | 그대로 (실제 이벤트가 처리) |
 * | 없음      | 살아 있음 | focus 이벤트를 보냄 |
 * | 없음      | 닫힘·없음 | 그대로           |
 */
function markOpenedWidgetActive(
    widget,
) {
    const node = widget?.node;
    const document = node?.ownerDocument;
    if (!document || widget.isDisposed || document.hasFocus()) return false;
    node.dispatchEvent(new document.defaultView.FocusEvent('focus'));
    return true;
}

module.exports = { markOpenedWidgetActive };
