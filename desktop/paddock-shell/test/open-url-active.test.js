const test = require('node:test');
const assert = require('node:assert/strict');
const { markOpenedWidgetActive } = require('../open-url-active');

function widgetInWindow({ hasFocus, isDisposed = false }) {
    const events = [];
    class FocusEvent {
        constructor(type) {
            this.type = type;
        }
    }
    const node = {
        ownerDocument: { hasFocus: () => hasFocus, defaultView: { FocusEvent } },
        dispatchEvent: event => events.push(event.type),
    };
    return { widget: { node, isDisposed }, events };
}

test('포커스 없는 창의 위젯에는 focus 이벤트를 보낸다', () => {
    const { widget, events } = widgetInWindow({ hasFocus: false });
    assert.equal(markOpenedWidgetActive(widget), true);
    assert.deepEqual(events, ['focus']);
});

test('포커스 있는 창에서는 실제 이벤트에 맡긴다', () => {
    const { widget, events } = widgetInWindow({ hasFocus: true });
    assert.equal(markOpenedWidgetActive(widget), false);
    assert.deepEqual(events, []);
});

test('닫힌 위젯에는 보내지 않는다', () => {
    const { widget, events } = widgetInWindow({ hasFocus: false, isDisposed: true });
    assert.equal(markOpenedWidgetActive(widget), false);
    assert.deepEqual(events, []);
});

test('위젯이 아닌 열기 결과는 무시한다', () => {
    assert.equal(markOpenedWidgetActive(undefined), false);
    assert.equal(markOpenedWidgetActive({}), false);
});
