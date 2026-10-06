const { Saveable } = require('@theia/core/lib/browser/saveable');
const { getCurrentPort } = require('@theia/core/lib/electron-browser/messaging/electron-local-ws-connection-source');
const { STATUS_ITEMS, element, button } = require('./shared');

// 상태 줄 게이지 묶음의 출처 표지. 막대 색이 사용량 수준을 뜻하므로 출처는 색이 아니라 표지 모양·이름으로 가른다.
// Memory는 표지 없이 이름만 둔다 — 출처가 하나라 가를 대상이 없고, 이름이 이미 출처를 말한다.
const SOURCE_MARKS = {
    claude: '✱',
    codex: '◎',
};

/**
 * 상태 줄 오른쪽의 원격 연결 표시, Memory 게이지, Claude·Codex 사용량 묶음을 그린다.
 */
class StatusBar {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.remote = { alive: false };
    }

    renderStatus() {
        const current = this.workspace.currentWidget();
        const status = this.workspace.shell.footer.node.querySelector('#statusbar-right');
        // Each gauge reads its selected CLI environment; default values never describe a named account.
        // 범위는 실행 중인 에이전트 프로세스의 환경으로 정한다. 에이전트가 없으면 터미널을 연 계정이다.
        const scope = this.workspace.agentActivity.runningScope(current);
        this.workspace.usagePanel?.observe(current?.id, scope, this.workspace.programs.get(current?.id));
        const saveable = current && Saveable.get(current);
        status.replaceChildren();
        const accountLabel = this.workspace.agentActivity.runningAccountLabel(current);
        const label = element('span', '', accountLabel ? accountLabel : saveable ? (saveable.dirty ? 'Unsaved changes' : 'Saved') : this.workspace.isTerminal(current) ? (this.remote.alive ? `${this.remote.name}` : 'Local shell') : 'Ready');
        status.append(label);
        if (saveable?.dirty) {
            const save = button('Save', 'save-action', () => this.workspace.run(async () => {
                save.disabled = true;
                await saveable.save();
                await this.workspace.refresh();
            }));
            status.append(save);
        }
    }

    /**
     * 게이지 하나를 그린다. 채워진 막대가 사용량, 빈 부분이 남은 양이다.
     * `percent`가 null이면 값이 아직 없다는 뜻으로 "—"를 보인다.
     */
    fillMeter(
        node,
        percent,
        title,
    ) {
        const hasValue = typeof percent === 'number';
        node.querySelector('.meter-percent').textContent = hasValue ? `${percent}%` : 'no data';
        node.querySelector('.meter-fill').style.width = hasValue ? `${Math.min(100, percent)}%` : '0';
        node.classList.toggle('is-empty', !hasValue);
        node.classList.toggle('is-high', hasValue && percent >= 75 && percent < 90);
        node.classList.toggle('is-critical', hasValue && percent >= 90);
        node.title = title;
    }

    meter(
        id,
        label,
        percent,
        title,
    ) {
        const node = element('span', 'meter');
        node.dataset.meter = id;
        if (label) node.append(element('span', 'meter-label', label));
        node.append(element('span', 'meter-bar'), element('span', 'meter-percent'));
        node.querySelector('.meter-bar').append(element('span', 'meter-fill'));
        this.fillMeter(node, percent, title);
        return node;
    }

    /** 상태 줄의 Memory 항목을 그린다. 꺼져 있으면 비워 두고, Claude·Codex 사용량처럼 Quick settings에서만 다시 켠다. */
    async refreshMemory() {
        const host = this.workspace.shell.footer.node.querySelector('.memory-usage');
        const isOn = this.workspace.isStatusItemOn(STATUS_ITEMS.MEMORY);
        if (host.dataset.state !== (isOn ? 'on' : 'off')) {
            host.dataset.state = isOn ? 'on' : 'off';
            if (isOn) host.replaceChildren(this.memoryGroup());
            else host.replaceChildren();
        }
        if (isOn) {
            try {
                const { percent } = await this.workspace.fetchJson('/paddock/memory');
                if (percent !== null) {
                    this.fillMeter(host.querySelector('[data-meter="memory"]'), percent, `System memory in use: ${percent}%. Hide it in Quick settings.`);
                }
            } catch {
                // 요청이 실패하면 마지막 값을 그대로 둔다. 처음부터 실패면 "—"가 남는다.
            }
        }
    }

    /**
     * 켜진 Memory 항목. 누를 수 없는 표시만 둔다.
     * 상태 줄 항목을 누르면 자세히 보기(Claude·Codex는 계정 목록)라는 규칙을 지키고, 실수로 눌러 사라지지 않게 숨기기는 Quick settings에만 둔다.
     */
    memoryGroup() {
        const group = this.usageGroup('memory', 'Memory', 'span');
        const meter = this.meter('memory', '', null, 'System memory in use. Hide it in Quick settings.');
        meter.querySelector('.meter-percent').textContent = '—';
        group.append(meter);
        return group;
    }

    /** Reads account-scoped values; empty data and failed requests remain visible in the status bar. */
    async refreshUsage(
        options = {},
    ) {
        await this.workspace.usagePanel.refresh(options);
    }

    usageGroup(
        source,
        name,
        tag,
    ) {
        const group = element(tag, 'usage-group');
        if (tag === 'button') group.type = 'button';
        group.dataset.source = source;
        // 표지가 없는 출처(Memory)는 이름부터 시작한다.
        if (SOURCE_MARKS[source]) group.append(element('span', 'source-mark', SOURCE_MARKS[source]));
        if (name) group.append(element('span', 'source-name', name));
        return group;
    }

    async refreshRemote() {
        const port = getCurrentPort();
        const status = port ? await this.workspace.remoteStatus.getStatus(Number(port)).catch(() => ({ alive: false })) : { alive: false };
        this.remote = status;
        const indicator = this.workspace.shell.footer.node.querySelector('.remote-indicator');
        indicator.classList.toggle('is-connected', Boolean(status.alive));
        indicator.querySelector('.remote-label').textContent = status.alive ? `${status.type}: ${status.name}` : '';
        indicator.title = status.alive ? `Editing on ${status.name}` : 'Open a remote window';
        await this.refreshMemory();
        await this.refreshUsage();
    }
}

module.exports = { StatusBar };
