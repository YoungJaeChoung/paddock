const { Saveable } = require('@theia/core/lib/browser/saveable');
const { getCurrentPort } = require('@theia/core/lib/electron-browser/messaging/electron-local-ws-connection-source');
const { STATUS_ITEMS, element, button } = require('./shared');
const model = require('../work-model');

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
    // 메모리 안내 코드(백엔드 memoryAdvice)별로 사용자가 고칠 곳. 값은 이 PC의 .wslconfig [wsl2] 항목이다.
    // 이 만큼 이상 swap을 쓰면 느려진 이유로 알린다.
    static SWAP_NOTICE_BYTES = 512 * 1024 ** 2;

    static MEMORY_ADVICE = {
        'wsl-share': 'WSL leaves Windows under 8 GB; lower memory= in .wslconfig so Windows keeps room.',
        'no-swap': 'WSL has no swap; set swap= in .wslconfig so a spike slows things down instead of killing processes.',
    };

    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.remote = { alive: false };
        // 90%를 넘어 경고를 이미 띄웠는지. 80% 아래로 내려가면 다시 false가 된다.
        this.isMemoryWarned = false;
    }

    renderStatus() {
        const current = this.workspace.currentWidget();
        const status = this.workspace.shell.footer.node.querySelector('#statusbar-right');
        // Each gauge reads its selected CLI environment; default values never describe a named account.
        // 범위는 실행 중인 에이전트 프로세스의 환경으로 정한다. 에이전트가 없으면 터미널을 연 계정이다.
        const scope = this.workspace.agentActivity.runningScope(current);
        this.workspace.usagePanel?.observe(current?.id, scope, this.workspace.programs.get(current?.id));
        const saveable = current && Saveable.get(current);
        const content = document.createDocumentFragment();
        const accountLabel = this.workspace.agentActivity.runningAccountLabel(current);
        const label = element('span', '', accountLabel ? accountLabel : saveable ? (saveable.dirty ? 'Unsaved changes' : 'Saved') : this.workspace.isTerminal(current) ? (this.remote.alive ? `${this.remote.name}` : 'Local shell') : 'Ready');
        content.append(label);
        if (saveable?.dirty) {
            const save = button('Save', 'save-action', () => this.workspace.run(async () => {
                save.disabled = true;
                await saveable.save();
                await this.workspace.refresh();
            }));
            content.append(save);
        }
        // 내용이 같으면 바꾸지 않는다. 같은 내용으로 바꿔도 창 전체의 배치 계산이 일어나 키 입력 처리가 늦어진다.
        // Save 버튼이 저장할 화면이 달라지지 않도록 화면 id도 비교한다.
        // 지금 화면의 HTML과 비교하므로, 눌린 뒤 꺼진 버튼처럼 그 사이 바뀐 상태는 다시 그려 되돌린다.
        const html = [...content.childNodes].map(node => node.outerHTML).join('');
        if (status.paddockWidgetId !== current?.id || status.innerHTML !== html) {
            status.paddockWidgetId = current?.id;
            status.replaceChildren(content);
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
                const report = await this.workspace.fetchJson('/paddock/memory');
                if (report.percent !== null) {
                    const details = this.memoryDetails(report);
                    this.fillMeter(host.querySelector('[data-meter="memory"]'), report.percent, `${details.summary} Hide it in Quick settings.`);
                    this.warnMemory(report.percent, details.warning, details.target);
                }
            } catch {
                // 요청이 실패하면 마지막 값을 그대로 둔다. 처음부터 실패면 "—"가 남는다.
            }
        }
    }

    /**
     * Memory 게이지 제목과 경고 문장. 어느 쪽(Windows·WSL)이 빡빡한지, 가장 큰 WSL 터미널, WSL이 swap을 쓰는 양, 고칠 설정 문제를 덧붙인다.
     * 가장 큰 터미널은 어느 쪽이 빡빡하든 보인다. Windows가 먼저 차는 설정에서도 WSL 안 어느 터미널이 큰지가 사용자가 바로 줄일 수 있는 정보다.
     * 터미널 이름은 이 창의 터미널과 id가 맞을 때만 쓰고, 셸이 닫힌 뒤 남은 프로세스·다른 창의 터미널은 그렇게 적는다.
     */
    memoryDetails(
        report,
    ) {
        const scope = report.source === 'wsl' ? 'WSL memory' : 'System memory';
        const heaviest = model.heaviestTerminal(report.terminals);
        const terminal = heaviest && this.workspace.terminals.all.find(item => item.id === heaviest.id);
        const isDetached = Boolean(heaviest) && !terminal && (report.detached || []).includes(heaviest.id);
        let owner = '';
        if (terminal) owner = this.workspace.terminalDisplayName(terminal);
        else if (isDetached) owner = 'leftover processes of a closed terminal';
        else if (heaviest) owner = 'a terminal in another window';
        const gigabytes = heaviest ? (heaviest.bytes / 1024 ** 3).toFixed(1) : '';
        const largest = heaviest ? ` Largest WSL terminal: ${owner} (${gigabytes} GB).` : '';
        const swapUsed = report.wsl?.swapUsed || 0;
        const swapping = swapUsed >= StatusBar.SWAP_NOTICE_BYTES ? ` WSL is swapping ${(swapUsed / 1024 ** 3).toFixed(1)} GB, which slows everything.` : '';
        const advice = (report.advice || []).map(code => StatusBar.MEMORY_ADVICE[code]).filter(Boolean).join(' ');
        const rest = `${largest}${swapping}${advice ? ` ${advice}` : ''}`;
        // 알림은 상태 한 문장과 할 일 한 문장만 말한다. swap·설정 안내는 제목에만 둔다.
        const full = `${scope} is ${report.percent}% full.`;
        let action;
        if (terminal) action = `${owner} uses ${gigabytes} GB. Stop its work or close it.`;
        else if (isDetached) action = `A closed terminal left processes using ${gigabytes} GB. Stop them.`;
        else if (heaviest) action = `A terminal in another window uses ${gigabytes} GB. Stop its work or close it there.`;
        else action = report.source === 'wsl' ? 'Close terminals you are not using.' : 'Close apps you are not using.';
        return {
            summary: `${scope} in use: ${report.percent}%.${rest}`,
            warning: `${full} ${action}`,
            // 알림에 붙일 버튼: 가장 큰 터미널이 이 창에 있을 때만 그 터미널로 이동한다. 대신 닫거나 멈추지는 않는다.
            target: terminal ? { id: terminal.id, label: `Go to ${owner}` } : null,
        };
    }

    /**
     * 사용률이 처음 90%를 넘을 때 경고를 한 번 띄운다. 80% 아래로 내려가야 다시 무장하므로 경계에서 오르내려도 반복하지 않는다.
     * 알림은 알릴 뿐이고 에이전트나 명령을 멈추지 않는다.
     */
    warnMemory(
        percent,
        message,
        target,
    ) {
        if (percent >= model.MemoryGuard.WARN_PERCENT && !this.isMemoryWarned) {
            this.isMemoryWarned = true;
            const actions = target ? [target.label] : [];
            void this.workspace.messages.warn(message, ...actions).then((chosen) => {
                if (target && chosen === target.label) return this.workspace.activate(target.id);
                return undefined;
            }).catch(() => {});
        } else if (percent < model.MemoryGuard.REARM_PERCENT) {
            this.isMemoryWarned = false;
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
