const { URI } = require('@theia/core');
const { Widget } = require('@lumino/widgets');
const { MessageLoop } = require('@lumino/messaging');
const model = require('../work-model');
const agent = require('../agent-model');
const wsl = require('../wsl-terminals');
const { fileMarks, colorVariable } = require('../file-marks');
const { AccountLaunch, accountTerminalOptions, refreshAccountResume } = require('../account-launch');
const { element, codicon, button } = require('./shared');

// Theia 보기 컨테이너 id. 사이드바 보기 줄의 Source control·Extensions가 이 보기를 품는다.
const VIEW_CONTAINER = {
    scm: 'scm-view-container',
    extensions: 'vsx-extensions-view-container',
};

/**
 * 사이드바의 보기 줄과 Work 목록(작업 폴더·Unassigned 행)을 그리고, 행에서 여는 메뉴를 맡는다.
 */
class WorkSidebar {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.showAll = new Set();
        this.n_sidebarRevision = 0;
        this.remoteWorkRows = new Map();
    }

    async showView(
        view,
    ) {
        this.workspace.view = view;
        const sidebar = this.workspace.shell.sidebar.node;
        for (const item of sidebar.querySelectorAll('[data-view]')) {
            item.classList.toggle('is-active', item.dataset.view === view);
            item.setAttribute('aria-pressed', String(item.dataset.view === view));
        }
        for (const host of sidebar.querySelectorAll('[data-host]')) {
            host.hidden = host.dataset.host !== view;
        }
        if (VIEW_CONTAINER[view]) {
            const host = sidebar.querySelector(`[data-host="${view}"]`);
            const widget = await this.workspace.widgets.getOrCreateWidget(VIEW_CONTAINER[view]);
            if (!host.contains(widget.node)) {
                // Theia가 이 보기를 화면에 없는 왼쪽 패널에 먼저 넣어 두므로, 그 부모에서 떼어 사이드바로 옮긴다.
                if (widget.parent) widget.parent = null;
                if (widget.isAttached) Widget.detach(widget);
                Widget.attach(widget, host);
            }
            // 왼쪽 패널에서 숨겨 둔 상태가 따라오므로 다시 보이게 한다.
            widget.show();
            // 사이드바 안에 직접 붙인 보기라 크기 변경을 대신 알려 준다.
            MessageLoop.sendMessage(widget, Widget.ResizeMessage.UnknownSize);
            widget.update();
        }
        await this.workspace.refresh();
    }

    renderViewBadge() {
        // 배지는 Git 보기가 보여 주는 저장소(지금 작업 폴더의 저장소)의 변경 수만 센다 — 다른 폴더의 변경까지 더하면 보기를 열었을 때 숫자가 맞지 않는다.
        let n_changes = 0;
        for (const group of this.workspace.scm.selectedRepository?.provider.groups ?? []) {
            n_changes += group.resources.length;
        }
        const badge = this.workspace.shell.sidebar.node.querySelector('.view-badge');
        badge.hidden = n_changes === 0;
        badge.textContent = n_changes > 99 ? '99+' : String(n_changes);
    }

    sectionHeader(
        label,
        expanded,
        toggle,
    ) {
        const header = button([codicon(expanded ? 'chevron-down' : 'chevron-right'), element('span', 'section-label', label)], 'section-header', toggle);
        header.setAttribute('aria-expanded', String(expanded));
        return header;
    }

    async renderWork() {
        const n_revision = ++this.n_sidebarRevision;
        const current = this.workspace.currentWidget();
        const currentId = current?.id;
        const localProcessIds = new Set(this.workspace.terminals.all.map(terminal => terminal.terminalId).filter(id => id >= 0));
        const snapshots = this.workspace.remoteWorkSnapshots.map(snapshot => ({ ...snapshot, terminals: snapshot.terminals.filter(terminal => !localProcessIds.has(terminal.terminalId)) }));
        const merged = model.mergeWorkPresence(this.workspace.state, snapshots);
        this.workspace.displayWorkState = merged.state;
        this.remoteWorkRows = merged.remote;
        const rows = model.visibleRows(merged.state, currentId, this.showAll);
        const unassigned = this.workspace.topTerminals();
        const selectedRoot = this.workspace.shownTopTerminal();
        const filesFolder = this.filesFolder();
        if (filesFolder !== this.directoryFolder) {
            this.workspace.directoryEntries.clear();
            this.directoryFolder = filesFolder;
        }
        const directory = filesFolder && this.workspace.filesExpanded ? await this.workspace.fileTree.readDirectory(new URI(filesFolder)) : null;
        // 표시할 내용이 같으면 기존 행을 유지한다. 큰 파일 목록의 재생성·배치가 터미널 입력을 막지 않게 한다.
        const key = JSON.stringify([
            merged.state.folders, rows, currentId, this.workspace.workExpanded, this.workspace.filesExpanded, filesFolder, directory,
            current?.getResourceUri?.()?.toString(), this.workspace.quickSettings.sidebarIndent(),
            [...this.workspace.doneIds], selectedRoot, [...new Set(this.workspace.terminals.all.map(terminal => this.workspace.terminalEnvironment(terminal)))], filesFolder && this.workspace.filesExpanded ? this.workspace.fileTree.gitChanges() : [], unassigned.map(terminal => [terminal.id, this.workspace.unassignedName(terminal), this.workspace.programOf(terminal), this.workspace.agentActivity.runningAccountLabel(terminal), this.workspace.terminalEnvironment(terminal),
                this.workspace.cwdCache.get(terminal.id), this.workspace.doneIds.has(terminal.id), this.workspace.innerTabIds(terminal.id).length,
                agent.activityState(this.workspace.activity.get(terminal.id) ?? agent.idle(), Date.now(), this.workspace.programs.has(terminal.id) ? agent.isAgent(this.workspace.programs.get(terminal.id)) : null)]),
            rows.map(row => {
                const terminal = this.workspace.shell.getWidgetById(row.id);
                return [this.remoteWorkRows.get(row.id), terminal ? this.workspace.programOf(terminal) : '', terminal ? this.workspace.agentActivity.runningAccountLabel(terminal) : '', terminal ? this.workspace.terminalEnvironment(terminal) : '', this.workspace.doneIds.has(row.id),
                    agent.activityState(this.workspace.activity.get(row.id) ?? agent.idle(), Date.now(), this.workspace.programs.has(row.id) ? agent.isAgent(this.workspace.programs.get(row.id)) : null)];
            }),
        ]);
        if (n_revision === this.n_sidebarRevision && this.workspace.view === 'work' && key !== this.workRenderKey) {
            const node = element('div', 'work-view');
            const toggle = this.workspace.shell.sidebar.node.querySelector('.work-toggle');
            toggle.setAttribute('aria-expanded', String(this.workspace.workExpanded));
            toggle.querySelector('.codicon').className = `codicon codicon-chevron-${this.workspace.workExpanded ? 'down' : 'right'}`;
            if (this.workspace.workExpanded) {
                const list = element('div', 'work-list');
                list.id = 'work-list';
                // 처음 켜면 기본 터미널이 Unassigned에 하나 있으므로, 작업 폴더가 없으면 그 터미널이 있어도 시작 안내를 보인다.
                if (!merged.state.folders.length) {
                    // 작업 폴더는 에이전트를 실행하면 생긴다. 등록 버튼 대신 그 다음 행동을 알려 준다.
                    list.append(element('p', 'work-empty-title', 'Start a work session'));
                    list.append(element('p', 'work-empty', 'No work folders yet.'));
                    const steps = element('ol', 'work-steps');
                    // 이미 열린 터미널이 있으면 그 터미널을 쓰면 되므로 새로 열기를 첫 단계로 요구하지 않는다.
                    steps.append(element('li', '', unassigned.length ? 'Use the terminal under Unassigned, or + for another.'
                        : 'Use + to open a terminal, or choose an environment with the arrow.'));
                    const directoryStep = element('li', '', 'Go to your project in the terminal.');
                    directoryStep.append(element('code', 'work-command', 'cd /path/to/project'));
                    const agentStep = element('li', '', 'Run claude or codex.');
                    steps.append(directoryStep, agentStep);
                    list.append(steps);
                    list.append(element('p', 'work-empty work-empty-note', 'Its terminals and files will appear here together.'));
                }
                // 실행 환경 줄(예: bash · Linux, WSL · Ubuntu)은 환경이 둘 이상 섞일 때만 터미널을 가르는 정보가 된다.
                // 모두 같으면 행마다 같은 글자가 반복될 뿐이라 숨기고, 행의 도움말에만 남긴다.
                this.showEnvironments = new Set(this.workspace.terminals.all.map(terminal => this.workspace.terminalEnvironment(terminal))).size > 1;
                for (const row of rows) {
                    list.append(this.renderRow(row, currentId));
                }
                if (unassigned.length) {
                    list.append(element('div', 'unassigned-heading', 'Unassigned'));
                    for (const terminal of unassigned) {
                        const row = this.renderRow({ kind: 'terminal', id: terminal.id, name: this.workspace.unassignedName(terminal), suffix: '', unassigned: true }, currentId);
                        row.classList.add('unassigned-row');
                        const unseen = this.workspace.innerTabIds(terminal.id).some(id => this.workspace.doneIds.has(id));
                        row.classList.toggle('is-done', unseen);
                        row.classList.toggle('is-current', terminal.id === selectedRoot);
                        if (terminal.id === selectedRoot) row.setAttribute('aria-current', 'true');
                        const cwd = this.workspace.cwdCache.get(terminal.id);
                        row.querySelector('.row-main').title = [this.workspace.programOf(terminal), this.workspace.terminalEnvironment(terminal), cwd && this.workspace.displayPath(cwd), unseen && 'Unseen activity in this group'].filter(Boolean).join(' · ') + ' — double-click to rename';
                        // 묶음 안 터미널 수. 작업 폴더 행처럼 보이되, 터미널이 하나뿐이면 행 자체가 그 터미널이라 생략한다.
                        // 좁은 사이드바의 두 줄 배치(프로그램·상태 칸)를 흔들지 않게 이름 옆에 둔다.
                        const n_terminals = this.workspace.innerTabIds(terminal.id).length;
                        if (n_terminals > 1) {
                            const count = element('span', 'row-suffix row-count', `›_ ${n_terminals}`);
                            count.title = `${n_terminals} terminals in this group`;
                            row.querySelector('.row-title').append(count);
                        }
                        // 행은 묶음 전체를 가리키므로 x는 묶음의 모든 터미널을 닫는다. 확인 창이 닫을 터미널 수를 보여 준다.
                        const close = button([codicon('close')], 'work-close', () => this.workspace.run(() => this.workspace.closeTerminalGroup(terminal.id)));
                        close.setAttribute('aria-label', n_terminals > 1 ? `Close ${n_terminals} terminals in this group` : `Close ${this.workspace.unassignedName(terminal)}`);
                        close.title = close.getAttribute('aria-label');
                        row.append(close);
                        list.append(row);
                    }
                }
                node.append(list);
            }
            if (filesFolder) {
                const toolbar = element('div', 'files-toolbar');
                toolbar.append(this.sectionHeader(`${this.workspace.folderName(filesFolder).toUpperCase()} FILES`, this.workspace.filesExpanded, () => {
                    this.workspace.filesExpanded = !this.workspace.filesExpanded;
                    this.workspace.refreshSoon();
                }));
                for (const [icon, label, isDirectory] of [['new-file', 'New File...', false], ['new-folder', 'New Folder...', true]]) {
                    const action = button([codicon(icon)], 'file-create', () => this.workspace.run(() => this.workspace.fileTree.createFileEntry(this.workspace.fileTree.creationFolder(new URI(filesFolder)), isDirectory)));
                    action.title = label;
                    action.setAttribute('aria-label', label);
                    toolbar.append(action);
                }
                // 폴더 감시가 놓친 변경(네트워크 드라이브·감시 제외 폴더·너무 많은 변경)을 사용자가 바로 다시 읽게 한다.
                const reload = button([codicon('refresh')], 'file-create', () => this.workspace.run(() => this.workspace.fileTree.reloadDirectory()));
                reload.title = 'Refresh';
                reload.setAttribute('aria-label', 'Refresh file list');
                toolbar.append(reload);
                node.append(toolbar);
                if (this.workspace.filesExpanded) {
                    const files = element('div', 'file-list');
                    // 목록의 빈 곳에 놓은 파일은 이 폴더 맨 위에 복사한다.
                    this.workspace.fileTree.acceptFileDrops(files, new URI(filesFolder));
                    this.workspace.fileTree.appendDirectory(files, directory, 0, fileMarks(this.workspace.fileTree.gitChanges()));
                    node.append(files);
                }
            }
            if (this.workspace.pointerPressed) {
                // 파일 목록을 읽는 동안 클릭이 시작됐으면 기존 행을 유지하고 다음 갱신에서 교체한다.
                this.workspace.refreshAfterPointer = true;
            } else {
                const content = this.workspace.shell.sidebar.node.querySelector('.work-content');
                const focused = content.contains(document.activeElement) ? document.activeElement.closest('[data-widget-id]')?.dataset.widgetId : null;
                const wasClose = document.activeElement?.classList.contains('work-close');
                const host = content.parentElement;
                const scrollTop = host.scrollTop;
                content.replaceChildren(node);
                host.scrollTop = scrollTop;
                if (focused) content.querySelector(`[data-widget-id="${focused}"] ${wasClose ? '.work-close' : '.row-main'}`)?.focus({ preventScroll: true });
                this.workRenderKey = key;
            }
        }
    }

    /**
     * 파일 구획이 보여 줄 폴더: 작업 폴더가 없으면 지금 보는 상단·내부 터미널의 현재 폴더.
     * 폴더를 모르거나 홈이면 구획을 숨긴다(홈 전체 목록은 쓸모가 없다).
     */
    filesFolder() {
        const current = this.workspace.currentWidget();
        const root = this.workspace.shownTopTerminal();
        const key = this.workspace.shownFolder() || (this.workspace.isTerminal(current) ? this.workspace.cwdCache.get(current.id) : root ? this.workspace.cwdCache.get(root) : this.workspace.isFileOnlyGroup(current) ? current.getResourceUri?.()?.parent?.toString() : null) || null;
        // 홈·파일 시스템 루트는 프로젝트가 아니다. 홈 전체나 루트의 수십 개 시스템 폴더를 파일 목록으로 보이지 않는다.
        const path = key ? new URI(key).path.toString() : '';
        const isHome = key && path === this.workspace.homePathOf(key);
        const isRoot = key && /^\/([A-Za-z]:\/?)?$/.test(path);
        return key && !isHome && !isRoot ? key : null;
    }

    renderRow(
        row,
        currentId,
    ) {
        let node;
        if (row.kind === 'folder') {
            const folder = this.workspace.displayWorkState.folders.find(item => item.key === row.key);
            const unseen = Object.entries(this.workspace.state.terminals).some(([id, terminal]) => terminal.folder === row.key && this.workspace.doneIds.has(id));
            node = element('div', `work-row folder-row${row.current ? ' is-current' : ''}${unseen ? ' is-done' : ''}`);
            const toggle = button([codicon(row.expanded ? 'chevron-down' : 'chevron-right'), codicon(row.expanded ? 'folder-opened' : 'folder'), element('span', 'row-name', this.workspace.folderName(row.key))], 'row-main', () => {
                this.workspace.state = model.setExpanded(model.ensureFolder(this.workspace.state, row.key), row.key, !folder.expanded);
                this.workspace.save();
                this.workspace.refreshSoon();
            });
            toggle.title = `${this.workspace.displayPath(row.key)}${unseen ? ' · Unseen activity' : ''}`;
            if (unseen) toggle.append(element('span', 'unseen-dot'));
            toggle.setAttribute('aria-expanded', String(row.expanded));
            const count = element('span', 'row-meta', `›_ ${row.n_terminals}`);
            const add = button([codicon('add')], 'row-action folder-add', (event) => this.openNewTerminalMenu(event.currentTarget, row.key));
            add.setAttribute('aria-label', `New terminal in ${this.workspace.folderName(row.key)}`);
            add.setAttribute('aria-haspopup', 'menu');
            add.title = 'New terminal or agent account in this folder';
            const menu = button([codicon('ellipsis')], 'row-action', (event) => this.openFolderMenu(row.key, event.currentTarget));
            menu.setAttribute('aria-label', `${this.workspace.folderName(row.key)} actions`);
            node.append(toggle, count, add, menu);
            node.dataset.folder = row.key;
            // 우클릭은 ⋯와 같은 메뉴를 누른 자리에 연다. ⋯는 우클릭을 모르는 사람이 메뉴를 찾는 단서라 남겨 둔다.
            // Linux는 버튼을 누르는 순간 우클릭 이벤트가 오고, 바로 뒤 버튼을 떼는 동작이 메뉴 바깥 클릭으로 읽혀 메뉴가 닫힌다 — 버튼을 뗀 뒤에 연다.
            node.addEventListener('contextmenu', (event) => {
                event.preventDefault();
                const target = event.currentTarget;
                const point = { x: event.clientX, y: event.clientY };
                const open = () => this.openFolderMenu(row.key, target, point);
                if (event.buttons & 2) {
                    window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
                } else {
                    open();
                }
            });
        } else if (row.kind === 'terminal' && this.remoteWorkRows.has(row.id)) {
            const remote = this.remoteWorkRows.get(row.id);
            node = element('div', 'work-row terminal-row is-remote');
            const label = button([element('span', 'prompt-mark', '›_'), element('span', 'row-name', row.name), element('span', 'row-suffix', row.suffix)], 'row-main', () => this.workspace.run(() => this.workspace.openRemoteTerminal(row, remote)));
            label.disabled = !Number.isSafeInteger(remote.terminalId) || remote.terminalId < 0;
            node.append(label, element('span', 'row-meta', 'other window'));
            this.forwardRowClicks(node, label);
            node.title = `${row.name}${remote.program ? ` · ${remote.program}` : ''} — open in another Paddock window`;
        } else if (row.kind === 'terminal') {
            const terminal = this.workspace.shell.getWidgetById(row.id);
            const unseen = row.unassigned ? this.workspace.innerTabIds(row.id).some(id => this.workspace.doneIds.has(id)) : this.workspace.doneIds.has(row.id);
            node = element('div', `work-row terminal-row${row.id === currentId ? ' is-current' : ''}${unseen ? ' is-done' : ''}`);
            const title = element('span', 'row-title');
            title.append(element('span', 'row-name', row.name), element('span', 'row-suffix', row.suffix));
            if (unseen) title.append(element('span', 'unseen-dot'));
            const label = element('span', 'row-label');
            label.append(title);
            const showEnvironment = Boolean(terminal) && this.showEnvironments;
            if (showEnvironment) label.append(element('span', 'row-environment', this.workspace.terminalEnvironment(terminal)));
            // Unassigned 행은 묶음을 가리키므로 그 묶음에서 마지막으로 쓴 터미널로 돌아간다.
            const select = button([element('span', 'prompt-mark', '›_'), label], 'row-main', () => this.workspace.run(() => this.workspace.activate(row.unassigned ? this.workspace.groupTerminal(row.id) : row.id)));
            select.dataset.widgetId = row.id;
            if (showEnvironment) node.classList.add('has-environment');
            select.addEventListener('dblclick', () => this.workspace.run(() => this.workspace.renameTerminal(row.id)));
            select.title = `${row.name}${terminal ? ` · ${this.workspace.terminalEnvironment(terminal)}` : ''}${this.workspace.doneIds.has(row.id) ? ' · Unseen activity' : ''} — double-click to rename`;
            const programName = terminal ? this.workspace.programOf(terminal) : '';
            const state = agent.activityState(this.workspace.activity.get(row.id) ?? agent.idle(), Date.now(), this.workspace.programs.has(row.id) ? agent.isAgent(this.workspace.programs.get(row.id)) : null);
            // 기본 이름이 프로그램명과 같으면 한 번만 적는다. 사용자가 바꾼 이름 옆에는 실행 프로그램을 유지한다.
            // 실행 중인 도구의 계정이 행 이름(계정 터미널의 이름)과 다르면 프로그램 옆에 그 계정을 붙인다.
            const accountName = terminal ? this.workspace.rowAccountName(terminal, row.name) : '';
            // 셸만 떠 있으면 아래 줄의 실행 환경(예: bash · Linux)이 이미 그 셸을 말하므로 프로그램 칸에 다시 적지 않는다.
            const shellName = String(terminal?.options.shellPath || '').split(/[\\/]/).pop().replace(/\.exe$/i, '').toLowerCase();
            const isShellOnly = Boolean(terminal) && programName.toLowerCase() === shellName;
            const program = element('span', 'row-meta', [row.name.toLowerCase() === programName.toLowerCase() || isShellOnly ? '' : programName, accountName].filter(Boolean).join(' · '));
            node.append(select, program);
            // 앞쪽 프로그램을 아직 모르는 순간(막 연 터미널 등)은 상태를 그리지 않는다. 대부분 곧 셸로 밝혀져
            // 상태 칸이 사라지므로, 잠깐 'Unknown'을 보이면 뜻 없는 글자가 깜빡일 뿐이다.
            if (terminal && state && state !== 'unknown') {
                const labels = {
                    working: ['Output', 'Terminal output was detected in the last 3 seconds; this can include prompt redraws. This does not confirm the agent is working.'],
                    waiting: ['Sent', 'Enter was sent less than 3 seconds ago; no subsequent terminal output has been detected. This does not confirm the agent received a request.'],
                    quiet: ['Quiet', 'No terminal output in the last 3 seconds. The agent may be waiting for input or still thinking.'],
                };
                const [label, detail] = labels[state];
                const status = element('span', `agent-status is-${state}`, label);
                status.title = `${programName} — ${detail} Estimated from terminal input and output.`;
                status.setAttribute('aria-label', `${programName}: ${label}`);
                node.append(status);
                node.dataset.agentState = state;
            }
            this.forwardRowClicks(node, select);
            this.attachTerminalMenu(node, row.id);
            node.dataset.widgetId = row.id;
            if (row.id === currentId) node.setAttribute('aria-current', 'true');
        } else {
            node = button([codicon('chevron-down'), element('span', '', `Show ${row.n_hidden} more`)], 'work-row more-row', () => {
                this.showAll.add(row.folder);
                this.workspace.refreshSoon();
            });
        }
        return node;
    }

    /**
     * 터미널 줄의 이름 버튼 밖(실행 프로그램·계정·활동 상태 글자, 빈 칸)을 눌러도 이름을 누른 것과 같게 한다.
     * 이 글자들은 이름 버튼의 형제라 그대로 두면 줄 오른쪽 절반이 눌리지 않는다. 줄 안의 다른 버튼(닫기 등)은 제 동작을 지킨다.
     */
    forwardRowClicks(
        node,
        select,
    ) {
        for (const type of ['click', 'dblclick']) {
            node.addEventListener(type, (event) => {
                if (!event.target.closest('button') && !select.disabled) select.dispatchEvent(new MouseEvent(type, event));
            });
        }
    }

    openFolderMenu(
        key,
        anchor,
        point = null,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const n_terminals = model.terminalsOf(this.workspace.state, key).length;
        const n_remote = [...this.remoteWorkRows.keys()].filter(id => model.folderOf(this.workspace.displayWorkState, id) === key).length;
        const items = [
            ['add', 'New work terminal here', () => this.workspace.newWorkTerminal({ folderKey: key })],
            ['edit', 'Rename in list', () => this.workspace.renameFolder(key)],
            ['folder-opened', 'Reveal folder', () => this.workspace.commands.executeCommand('revealFileInOS', new URI(key))],
        ];
        if (!n_remote) items.push(['close', 'Remove from list', () => this.workspace.removeFolder(key), true]);
        for (const [icon, label, action, danger] of items) {
            const item = button([codicon(icon), element('span', '', label)], `menu-item${danger ? ' is-danger' : ''}`, () => {
                menu.hidePopover();
                this.workspace.run(action);
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
        }
        menu.append(element('p', 'menu-note', n_remote ? `${n_remote} terminal${n_remote === 1 ? '' : 's'} open in another window` : `Closes ${n_terminals} terminal${n_terminals === 1 ? '' : 's'} · folder is kept`));
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        document.body.append(menu);
        // ⋯ 버튼은 마우스를 올렸을 때만 보이므로 위치는 폴더 행 전체를 기준으로 잡는다. 우클릭이면 누른 자리에 연다.
        const bounds = (anchor.closest('.work-row') || anchor).getBoundingClientRect();
        const left = point ? point.x : bounds.right + 4;
        const top = point ? point.y : bounds.top;
        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;
        menu.showPopover();
        // 창 오른쪽·아래 끝을 넘으면 안쪽으로 당긴다.
        const size = menu.getBoundingClientRect();
        menu.style.left = `${Math.max(4, Math.min(left, window.innerWidth - size.width - 4))}px`;
        menu.style.top = `${Math.max(4, Math.min(top, window.innerHeight - size.height - 4))}px`;
    }

    /**
     * ＋의 새 터미널 메뉴 — 일반 터미널을 열지, Claude·Codex 계정 터미널을 열지 고른다. 사이드바 폴더 줄과 본문 위 탭 줄의 ＋가 함께 쓴다.
     * `folderKey`가 있으면(폴더 줄 ＋) 그 작업 폴더의 최상위에 연다. 없으면(탭 줄 ＋) 탭 줄 ＋와 같은 자리 — 보이는 폴더나 현재 터미널의 묶음 — 에 연다.
     * 첫 항목(Terminal)에 초점을 두어 ＋ 뒤 Enter 한 번이면 예전 ＋처럼 일반 터미널이 열린다.
     * 계정 목록은 메뉴를 띄운 뒤 채운다. 읽지 못하면 안내 문구와 Manage accounts…만 남는다.
     * 등록 계정이 없다고 이미 알면 메뉴 없이 일반 터미널을 바로 연다(`openNewTerminalMenu`).
     * `pane`(칸을 나눈 동안 묶음별 ＋)이 있으면 고른 터미널을 그 칸에 연다. 칸의 보이는 탭을 먼저 현재 화면으로 만들어 폴더·묶음 판정도 그 칸을 따른다.
     */
    openNewTerminalMenu(
        anchor,
        folderKey,
        pane,
    ) {
        // 등록 계정이 없다고 이미 알면 메뉴의 고를 항목이 Terminal 하나뿐이므로 바로 연다. 계정 진입은 오른쪽 위 Accounts에 남는다.
        if (this.workspace.n_knownAccounts === 0) {
            this.workspace.run(this.inPane(pane, () => folderKey ? this.workspace.newWorkTerminal({ folderKey }) : this.workspace.newTerminalFromFolderBar()));
        } else {
            this.showNewTerminalMenu(anchor, folderKey, pane);
        }
    }

    /** `action`을 `pane`의 보이는 탭을 현재 화면으로 만든 뒤 실행하는 동작으로 감싼다. `pane`이 없으면 `action` 그대로다. */
    inPane(
        pane,
        action,
    ) {
        const widget = pane?.currentTitle?.owner;
        return async () => {
            if (widget && !widget.isDisposed && widget !== this.workspace.currentWidget()) await this.workspace.activate(widget.id);
            return action();
        };
    }

    showNewTerminalMenu(
        anchor,
        folderKey,
        pane,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        menu.setAttribute('aria-label', folderKey ? `New terminal in ${this.workspace.folderName(folderKey)}` : 'New terminal here');
        const addItem = (icon, label, meta, action) => {
            const item = button([codicon(icon), element('span', 'menu-item-label', label), element('span', 'menu-item-meta', meta)], 'menu-item', () => {
                menu.hidePopover();
                this.workspace.run(this.inPane(pane, action));
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
            return item;
        };
        const terminalItem = addItem('terminal', 'Terminal', '', () => folderKey ? this.workspace.newWorkTerminal({ folderKey }) : this.workspace.newTerminalFromFolderBar());
        const hint = element('p', 'menu-note', 'Loading accounts…');
        menu.append(hint);
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        menu.addEventListener('keydown', (event) => {
            const items = [...menu.querySelectorAll('[role="menuitem"]')];
            const index = items.indexOf(document.activeElement);
            let next;
            if (event.key === 'ArrowDown') next = (index + 1) % items.length;
            else if (event.key === 'ArrowUp') next = (index - 1 + items.length) % items.length;
            if (next !== undefined) {
                items[next].focus();
                event.preventDefault();
            }
        });
        document.body.append(menu);
        // ＋ 바로 아래에 열고, 창 오른쪽·아래 끝을 넘으면 안쪽으로 당긴다. 계정이 채워져 높이가 바뀌면 다시 맞춘다.
        const place = () => {
            const bounds = anchor.getBoundingClientRect();
            const size = menu.getBoundingClientRect();
            menu.style.left = `${Math.max(4, Math.min(bounds.left, window.innerWidth - size.width - 4))}px`;
            menu.style.top = `${Math.max(4, Math.min(bounds.bottom + 4, window.innerHeight - size.height - 4))}px`;
        };
        menu.showPopover();
        place();
        terminalItem.focus({ preventScroll: true });
        this.workspace.accountTerminals.updateAccountLabels().then((profiles) => {
            hint.remove();
            for (const profile of profiles) {
                const item = addItem('account',
                    `${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`,
                    profile.runtime === 'wsl' ? 'WSL' : '',
                    () => this.workspace.accountTerminals.openAccount(profile.id, { folderKey }));
                item.title = `New conversation with ${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`;
            }
            if (!profiles.length) menu.append(element('p', 'menu-note', 'No accounts yet. Add one to open Claude or Codex here.'));
        }, () => {
            hint.textContent = 'Accounts could not be loaded. Open Manage accounts to retry.';
        }).finally(() => {
            addItem('settings-gear', 'Manage accounts…', '', () => this.workspace.accountTerminals.manageAccounts({ folderKey }));
            if (menu.matches(':popover-open')) place();
        });
    }

    /** 작업 터미널 행과 탭의 우클릭 메뉴를 연결한다. Linux에서는 버튼을 놓은 뒤 메뉴를 연다. */
    attachTerminalMenu(
        node,
        id,
    ) {
        node.addEventListener('contextmenu', (event) => {
            event.preventDefault();
            const point = { x: event.clientX, y: event.clientY };
            const open = () => this.openTerminalMenu(id, point);
            if (event.buttons & 2) window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
            else open();
        });
    }

    /** 이름 바꾸기와 닫기를 작업 터미널의 우클릭 메뉴에 모은다. */
    openTerminalMenu(
        id,
        point,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const items = [
            ['edit', 'Rename terminal', () => this.workspace.renameTerminal(id), false],
            ['split-horizontal', 'Show to the Side', () => this.workspace.folderTabs.showBeside(id), false],
            ['link-external', 'Move to New Window', () => this.workspace.folderTabs.moveTabToWindow(id), false],
            ['close', 'Close terminal', () => this.workspace.closeWidget(this.workspace.shell.getWidgetById(id)), true],
        ];
        for (const [icon, label, action, danger] of items) {
            const item = button([codicon(icon), element('span', '', label)], `menu-item${danger ? ' is-danger' : ''}`, () => {
                menu.hidePopover();
                this.workspace.run(action);
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
        }
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        document.body.append(menu);
        menu.style.left = `${point.x}px`;
        menu.style.top = `${point.y}px`;
        menu.showPopover();
        const size = menu.getBoundingClientRect();
        menu.style.left = `${Math.max(4, Math.min(point.x, window.innerWidth - size.width - 4))}px`;
        menu.style.top = `${Math.max(4, Math.min(point.y, window.innerHeight - size.height - 4))}px`;
    }
}

module.exports = { WorkSidebar };
