/**
 * 실행 중인 Paddock 창의 작업 목록 정보만 백엔드에 보관한다.
 * 터미널 출력·입력·프로세스는 여기에 들어오지 않는다. 창이 사라지면 기록도 만료된다.
 */

const PRESENCE_TIMEOUT_MS = 15000;

class WorkPresenceRegistry {
    constructor() {
        this.windows = new Map();
    }

    update(
        windowId,
        snapshot,
        now = Date.now(),
    ) {
        const folders = Array.isArray(snapshot?.folders) ? snapshot.folders.filter(folder => typeof folder?.key === 'string').map(folder => ({ key: folder.key, expanded: folder.expanded !== false, label: typeof folder.label === 'string' ? folder.label : '' })) : [];
        const terminals = Array.isArray(snapshot?.terminals) ? snapshot.terminals.filter(terminal => typeof terminal?.id === 'string' && typeof terminal?.folder === 'string' && typeof terminal?.name === 'string').map(terminal => ({ id: terminal.id, folder: terminal.folder, name: terminal.name, program: typeof terminal.program === 'string' ? terminal.program : '' })) : [];
        this.windows.set(windowId, { updatedAt: now, folders, terminals });
        return this.list(windowId, now);
    }

    list(
        excludeId,
        now = Date.now(),
    ) {
        const snapshots = [];
        for (const [windowId, snapshot] of this.windows) {
            if (now - snapshot.updatedAt > PRESENCE_TIMEOUT_MS) {
                this.windows.delete(windowId);
            } else if (windowId !== excludeId) {
                snapshots.push({ windowId, folders: snapshot.folders, terminals: snapshot.terminals });
            }
        }
        return snapshots;
    }

    remove(
        windowId,
    ) {
        this.windows.delete(windowId);
    }
}

module.exports = { WorkPresenceRegistry };
