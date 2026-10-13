/**
 * 실행 중인 Paddock 창의 작업 목록 정보만 백엔드에 보관한다.
 * 터미널 출력·입력·프로세스는 여기에 들어오지 않는다.
 * 창이 닫히거나 15초 넘게 소식이 없으면 그 창의 기록은 '퇴역'(retired)이 된다. 창은 사라져도 백엔드의 터미널은 살아 있을 수 있으므로,
 * 퇴역 기록은 살아 있는 터미널만 남겨 다른 창이 넘겨받거나 끝낼 수 있게 보여 준다. 살아 있는지는 호출자가 `isAlive(terminalId)`로 알려 준다.
 */

const PRESENCE_TIMEOUT_MS = 15000;

class WorkPresenceRegistry {
    constructor() {
        this.windows = new Map();
    }

    /**
     * 창이 보낸 목록 정보를 저장하고, 그 창을 뺀 나머지 창의 기록을 돌려준다.
     * 이 창이 보고한 `terminalId`는 퇴역 기록에서 지운다. 퇴역 터미널을 어느 창이 넘겨받으면 그 창이 곧 보고하므로, 별도 요청 없이 소유가 옮겨진다.
     *
     * Examples
     * --------
     * | 상황                                         | 결과                                   |
     * |----------------------------------------------|----------------------------------------|
     * | 퇴역 기록에 terminalId 7, 창 B가 7을 보고    | 퇴역 기록에서 7이 빠지고 B의 기록에 남음 |
     * | folder가 ''인 터미널(작업 폴더 밖)            | 그대로 저장                            |
     */
    update(
        windowId,
        snapshot,
        now = Date.now(),
        isAlive = () => true,
    ) {
        const folders = Array.isArray(snapshot?.folders) ? snapshot.folders.filter(folder => typeof folder?.key === 'string').map(folder => ({ key: folder.key, expanded: folder.expanded !== false, label: typeof folder.label === 'string' ? folder.label : '' })) : [];
        const terminals = Array.isArray(snapshot?.terminals) ? snapshot.terminals.filter(terminal => typeof terminal?.id === 'string' && typeof terminal?.folder === 'string' && typeof terminal?.name === 'string').map(terminal => ({ id: terminal.id, folder: terminal.folder, name: terminal.name, program: typeof terminal.program === 'string' ? terminal.program : '', ...(Number.isSafeInteger(terminal.terminalId) && terminal.terminalId >= 0 ? { terminalId: terminal.terminalId } : {}) })) : [];
        this.windows.set(windowId, { updatedAt: now, folders, terminals, retired: false });
        const reported = new Set(terminals.filter(terminal => 'terminalId' in terminal).map(terminal => terminal.terminalId));
        for (const [otherId, other] of this.windows) {
            if (other.retired && otherId !== windowId) {
                other.terminals = other.terminals.filter(terminal => !reported.has(terminal.terminalId));
                if (!other.terminals.length) this.windows.delete(otherId);
            }
        }
        return this.list(windowId, now, isAlive);
    }

    /**
     * 창 밖 기록을 돌려준다. 퇴역 기록은 시간으로 만료되지 않고, 호출 때마다 죽은 터미널을 걸러 비면 사라진다.
     * 살아 있는 창의 기록은 15초 넘게 갱신이 없으면 퇴역으로 바뀐다(창이 비정상 종료한 경우).
     *
     * Examples
     * --------
     * | 상황                                    | 결과                                  |
     * |-----------------------------------------|---------------------------------------|
     * | 퇴역 기록의 터미널 둘 중 하나가 끝남     | 남은 하나만 `retired: true`로 반환     |
     * | 퇴역 기록의 터미널이 모두 끝남           | 그 기록이 목록에서 사라짐              |
     * | 창 기록이 15초 넘게 갱신 없음            | 살아 있는 터미널만 남겨 퇴역 처리       |
     */
    list(
        excludeId,
        now = Date.now(),
        isAlive = () => true,
    ) {
        const snapshots = [];
        for (const [windowId, snapshot] of [...this.windows]) {
            if (!snapshot.retired && now - snapshot.updatedAt > PRESENCE_TIMEOUT_MS) {
                this.retire(windowId, isAlive);
            } else if (snapshot.retired) {
                snapshot.terminals = snapshot.terminals.filter(terminal => isAlive(terminal.terminalId));
                if (!snapshot.terminals.length) this.windows.delete(windowId);
            }
        }
        for (const [windowId, snapshot] of this.windows) {
            if (windowId !== excludeId) {
                snapshots.push({ windowId, folders: snapshot.folders, terminals: snapshot.terminals, ...(snapshot.retired ? { retired: true } : {}) });
            }
        }
        return snapshots;
    }

    /**
     * 창의 기록을 퇴역으로 바꾼다. 연결할 수 있는 `terminalId`를 가졌고 `isAlive`가 참인 터미널만 남기며, 남는 것이 없으면 기록을 지운다.
     */
    retire(
        windowId,
        isAlive = () => true,
    ) {
        const snapshot = this.windows.get(windowId);
        if (snapshot) {
            const alive = snapshot.terminals.filter(terminal => 'terminalId' in terminal && isAlive(terminal.terminalId));
            if (alive.length) {
                this.windows.set(windowId, { ...snapshot, terminals: alive, retired: true });
            } else {
                this.windows.delete(windowId);
            }
        }
    }
}

module.exports = { WorkPresenceRegistry };
