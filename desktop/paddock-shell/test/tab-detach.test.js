const test = require('node:test');
const assert = require('node:assert/strict');
const { attachTabDrag } = require('../tab-detach');

function fixture(
    dock = {},
) {
    const listeners = new Map();
    const viewListeners = new Map();
    const captures = new Set();
    const previews = new Set();
    const moves = [];
    const tab = { dataset: { widgetId: 'terminal-a' }, textContent: 'terminal A' };
    const view = { innerWidth: 1000, innerHeight: 700, addEventListener: (type, fn) => viewListeners.set(type, fn), setTimeout() {} };
    const strip = {
        ownerDocument: {
            defaultView: view,
            createElement: () => ({ style: {}, remove() { previews.delete(this); } }),
            body: { append: node => previews.add(node) },
        },
        addEventListener: (type, fn) => listeners.set(type, fn),
        contains: target => target === tab,
        setPointerCapture: id => captures.add(id),
        hasPointerCapture: id => captures.has(id),
        releasePointerCapture: id => captures.delete(id),
    };
    attachTabDrag(strip, id => moves.push(id), dock);
    const emit = (type, extra = {}) => {
        const event = { button: 0, pointerId: 1, clientX: 50, clientY: 30, target: { closest: () => tab }, preventDefault() { this.prevented = true; }, stopPropagation() {}, ...extra };
        (listeners.get(type) || viewListeners.get(type))?.(event);
        return event;
    };
    return { emit, captures, previews, moves, tab };
}

test('TD01 normal clicks do not capture the pointer or consume selection', () => {
    const f = fixture();
    f.emit('pointerdown');
    assert.equal(f.captures.size, 0);
    f.emit('pointerup');
    assert.equal(f.emit('click').prevented, undefined);
    assert.deepEqual(f.moves, []);
});

test('TD02 moving within the window cancels extraction and consumes the drag click', () => {
    const f = fixture();
    f.emit('pointerdown');
    f.emit('pointermove', { clientY: 80 });
    assert.equal(f.previews.size, 1);
    assert.equal(f.captures.size, 1);
    f.emit('pointerup', { clientY: 80 });
    assert.equal(f.previews.size, 0);
    assert.equal(f.captures.size, 0);
    assert.equal(f.emit('click').prevented, true);
    assert.deepEqual(f.moves, []);
});

for (const point of [{ clientX: -1 }, { clientX: 1000 }, { clientY: -1 }, { clientY: 700 }]) {
    test(`TD03 release outside ${JSON.stringify(point)} moves the original tab once`, () => {
        const f = fixture();
        f.emit('pointerdown');
        f.emit('pointermove', { clientY: 80 });
        f.emit('pointerup', point);
        f.emit('pointerup', point);
        assert.deepEqual(f.moves, ['terminal-a']);
        assert.equal(f.previews.size, 0);
    });
}

for (const type of ['pointercancel', 'keydown']) {
    test(`TD04 ${type} cancels without moving or closing the tab`, () => {
        const f = fixture();
        f.emit('pointerdown');
        f.emit('pointermove', { clientY: 80 });
        f.emit(type, { key: 'Escape' });
        f.emit('pointerup', { clientX: -1 });
        assert.deepEqual(f.moves, []);
        assert.equal(f.previews.size, 0);
        assert.equal(f.captures.size, 0);
    });
}

test('TD06 native capture loss immediately before an outside release preserves the drag intent', () => {
    const f = fixture();
    f.emit('pointerdown');
    f.emit('pointermove', { clientY: 80 });
    f.captures.clear();
    f.emit('lostpointercapture', { clientX: 1001 });
    f.emit('pointerup', { clientX: 1001 });
    assert.deepEqual(f.moves, ['terminal-a']);
    assert.equal(f.previews.size, 0);
});

test('TD05 close buttons and secondary pointer buttons never start a drag', () => {
    const f = fixture();
    f.emit('pointerdown', { button: 2 });
    f.emit('pointermove', { clientY: 80 });
    assert.equal(f.previews.size, 0);
    f.emit('pointerdown', { target: { closest: () => null } });
    f.emit('pointermove', { clientY: 80 });
    assert.equal(f.previews.size, 0);
});

function nativeFixture() {
    const { attachNativeTabDrag } = require('../tab-detach');
    const panelEvents = new Map();
    const windowEvents = new Map();
    const timers = [];
    const moves = [];
    const panel = {
        ownerDocument: {
            defaultView: {
                innerWidth: 1000,
                innerHeight: 700,
                addEventListener: (type, fn) => windowEvents.set(type, fn),
                setTimeout: fn => timers.push(fn),
            },
        },
        addEventListener: (type, fn) => panelEvents.set(type, fn),
    };
    attachNativeTabDrag(panel, () => 'native-a', id => moves.push(id));
    const event = extra => ({ button: 0, pointerId: 1, clientX: 50, clientY: 30, target: { closest: () => null }, ...extra });
    const flush = () => timers.splice(0).forEach(fn => fn());
    return { panelEvents, windowEvents, moves, event, flush };
}

test('TD07 native pane tabs keep Lumino docking and extract only on an outside release', async () => {
    const { panelEvents, windowEvents, moves, event, flush } = nativeFixture();
    // Lumino 탭 줄은 pointer 이벤트만 만든다. mouse 이벤트에 기대면 실제로는 아무것도 듣지 못한다.
    assert.equal(panelEvents.has('mousedown'), false);
    panelEvents.get('pointerdown')(event());
    windowEvents.get('pointerup')(event({ clientX: 600 }));
    flush();
    assert.deepEqual(moves, []);
    panelEvents.get('pointerdown')(event());
    windowEvents.get('pointerup')(event({ clientX: 1001 }));
    flush();
    assert.deepEqual(moves, ['native-a']);
    panelEvents.get('pointerdown')(event());
    windowEvents.get('keydown')({ key: 'Escape' });
    windowEvents.get('pointerup')(event({ clientX: 1001 }));
    flush();
    assert.deepEqual(moves, ['native-a']);
    panelEvents.get('pointerdown')(event({ target: { closest: () => ({}) } }));
    windowEvents.get('pointerup')(event({ clientX: 1001 }));
    flush();
    assert.deepEqual(moves, ['native-a']);
});

test('TD08 native extraction waits for a later task so Lumino drag cleanup finishes first', async () => {
    const { panelEvents, windowEvents, moves, event, flush } = nativeFixture();
    panelEvents.get('pointerdown')(event());
    windowEvents.get('pointerup')(event({ clientX: -1 }));
    await Promise.resolve();
    assert.deepEqual(moves, []);
    flush();
    assert.deepEqual(moves, ['native-a']);
});

test('TD09 native drag ignores other pointers and clears on pointercancel', () => {
    const { panelEvents, windowEvents, moves, event, flush } = nativeFixture();
    panelEvents.get('pointerdown')(event());
    windowEvents.get('pointerup')(event({ pointerId: 2, clientX: -1 }));
    flush();
    assert.deepEqual(moves, []);
    windowEvents.get('pointerup')(event({ clientX: -1 }));
    flush();
    assert.deepEqual(moves, ['native-a']);
    panelEvents.get('pointerdown')(event());
    windowEvents.get('pointercancel')(event());
    windowEvents.get('pointerup')(event({ clientX: -1 }));
    flush();
    assert.deepEqual(moves, ['native-a']);
    panelEvents.get('pointerdown')(event({ button: 2 }));
    windowEvents.get('pointerup')(event({ clientX: -1 }));
    flush();
    assert.deepEqual(moves, ['native-a']);
});

test('TD10 restored widgets leave the secondary window list so the empty window can close', () => {
    const { forgetRestoredWidgets } = require('../tab-detach');
    const root = {};
    const dock = { parent: root };
    const staying = { id: 'staying', parent: dock };
    const restored = { id: 'restored-dirty', parent: { parent: { parent: undefined } } };
    const disposed = { id: 'disposed', parent: dock, isDisposed: true };
    root.widgets = [staying, restored, disposed];
    const widgets = root.widgets;
    assert.equal(forgetRestoredWidgets(root), 2);
    assert.equal(root.widgets, widgets);
    assert.deepEqual(root.widgets.map(widget => widget.id), ['staying']);
    assert.equal(forgetRestoredWidgets(undefined), 0);
});

test('TD11 a tab that cannot dock (detached to another window) never promises an edge split', () => {
    const detached = fixture({ drop() {}, canDock: id => id !== 'terminal-a' });
    detached.emit('pointerdown');
    detached.emit('pointermove', { clientY: 80 });
    assert.equal([...detached.previews][0].textContent, 'terminal A · Drag outside to open in a new window');
    const docked = fixture({ drop() {}, canDock: () => true });
    docked.emit('pointerdown');
    docked.emit('pointermove', { clientY: 80 });
    assert.equal([...docked.previews][0].textContent, 'terminal A · Drag to an edge to split, or outside for a new window');
});
