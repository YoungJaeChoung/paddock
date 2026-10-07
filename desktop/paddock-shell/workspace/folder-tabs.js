const { OS } = require('@theia/core/lib/common/os');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { ShellTerminalProfile } = require('@theia/terminal/lib/browser/shell-terminal-profile');
const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const model = require('../work-model');
const tabOverflow = require('../tab-overflow');
const { paneColumns, groupTabsByColumn, columnSpans } = require('../pane-columns');
const { tabDropTarget, tabDockRef, tabRowInsert, isSameTabPlace } = require('../tab-drop');
const wsl = require('../wsl-terminals');
const { distributionOf, withDistribution, environmentLabel } = require('../terminal-environment');
const { element, codicon, button } = require('./shared');

/**
 * 본문 위 내부 터미널·파일 탭 줄을 그리고, 탭을 끌어 칸·다른 창으로 옮기기와 셸 메뉴를 맡는다.
 *
 * 칸을 나누면 탭은 그 화면이 든 칸 바로 위에 놓인다: 왼쪽이 같은 칸들을 한 열로 묶고, 열마다 묶음을 칸의 가로 구간에 둔다.
 * 탭이 아래 줄로 내려가면 위쪽 줄이 빈 줄처럼 보이므로 탭은 위쪽 줄에 남기고 가로 위치만 칸에 맞춘다.
 */
class FolderTabs {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        // 칸 크기 변화(경계 끌기·창 크기·칸 추가 뒤 배치)를 듣고 열 묶음 위치를 다시 맞춘다. 칸 목록이 바뀔 때마다 대상을 새로 잡는다.
        this.paneObserver = null;
        this.isLayingOut = false;
    }

    /** 본문 칸마다 그 칸 탭 줄의 화면 위치. 아직 배치되지 않아 폭이 0인 칸은 뺀다(왼쪽 0으로 잘못 묶이지 않게). */
    paneRects() {
        return [...this.workspace.shell.mainPanel.tabBars()]
            .map(bar => ({ key: bar, rect: bar.node.getBoundingClientRect() }))
            .filter(({ rect }) => rect.width > 0)
            .map(({ key, rect }) => ({ key, left: rect.left, top: rect.top }));
    }

    /** 탭이 실제로 스크롤되는 상자들: 열 묶음이 있으면 묶음마다의 목록, 없으면 줄 자체. */
    scrollContainers(
        strip,
    ) {
        const lists = [...strip.querySelectorAll('.folder-tab-list')];
        return lists.length ? lists : [strip];
    }

    /**
     * Dragging the common tab strip uses the same docking modes as the native pane tabs.
     * 다른 창에 분리된 탭은 대상이 없다. 메인 칸에 직접 붙이면 보조 창의 복원 절차를 건너뛰어
     * 위젯이 보조 창 소속으로 남으므로, 되돌리기는 보조 창을 닫는 정식 경로에만 맡긴다.
     */
    tabDockTarget(id, x, y) {
        const isDetached = Boolean(this.workspace.shell.getWidgetById(id)?.secondaryWindow);
        // 탭 줄 위면 가리킨 탭 앞뒤에 끼워 넣는다. 아니면 칸 본문의 가장자리(나누기)·가운데(그 칸으로 옮기기)다.
        let dock = isDetached ? null : this.tabRowTarget(id, x, y);
        const bars = isDetached || dock ? [] : this.workspace.shell.mainPanel.tabBars();
        for (const bar of bars) {
            const current = bar.currentTitle?.owner;
            if (!dock && current?.isVisible) {
                const target = tabDropTarget(current.node.getBoundingClientRect(), x, y);
                // 자기 칸 가운데처럼 놓아도 바뀌는 것이 없으면 기준 탭이 없어 미리보기·이동을 건너뛴다.
                const refId = target ? tabDockRef(target.mode, bar.titles.map(title => title.owner.id), current.id, id) : null;
                const ref = refId && bar.titles.find(title => title.owner.id === refId)?.owner;
                if (target && ref) dock = { ...target, ref };
            }
        }
        return dock;
    }

    /**
     * 위쪽 탭 줄에 놓을 때의 대상: 가리킨 탭이 든 칸에서 그 탭 앞이나 뒤. 다른 칸의 탭이면 그 칸으로 옮기고, 같은 칸이면 순서를 바꾼다.
     * 칸을 나눈 동안은 칸마다 따로 그린 탭 묶음 중 가리킨 묶음 안에서 고른다. 다른 창에 분리된 탭은 기준으로 삼지 않는다.
     * 탭 줄 밖이거나 놓아도 순서가 그대로면 null이다. 미리보기 상자는 끼워 넣을 자리의 세로선이다.
     */
    tabRowTarget(id, x, y) {
        const shell = this.workspace.shell;
        const strip = shell.folderBar.node.querySelector('.folder-tabs');
        const mainBars = new Set(shell.mainPanel.tabBars());
        const isInside = rect => x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        const list = strip && this.scrollContainers(strip).find(container => isInside(container.getBoundingClientRect()));
        const tabs = list ? [...list.querySelectorAll('[role="tab"][data-widget-id]')]
            .filter(tab => {
                const widget = shell.getWidgetById(tab.dataset.widgetId);
                return widget && !widget.secondaryWindow && mainBars.has(shell.getTabBarFor(widget));
            })
            .map(tab => ({ id: tab.dataset.widgetId, rect: (tab.closest('.tab') ?? tab).getBoundingClientRect() })) : [];
        const insert = tabRowInsert(tabs.map(tab => ({ id: tab.id, left: tab.rect.left, right: tab.rect.right })), x);
        const ref = insert && shell.getWidgetById(insert.refId);
        const paneIds = ref ? shell.getTabBarFor(ref).titles.map(title => title.owner.id) : [];
        const tabRect = insert && tabs.find(tab => tab.id === insert.refId).rect;
        return ref && !isSameTabPlace(paneIds, id, insert.mode, insert.refId)
            ? { mode: insert.mode, ref, isInsert: true, rect: { left: insert.edge - 1, top: tabRect.top, width: 2, height: tabRect.height } }
            : null;
    }

    previewTabDock(id, x, y) {
        const target = this.tabDockTarget(id, x, y);
        this.workspace.tabDockPreview?.remove();
        if (target) {
            this.workspace.tabDockPreview = element('div', target.isInsert ? 'paddock-tab-dock-preview is-insert' : 'paddock-tab-dock-preview');
            for (const [key, value] of Object.entries(target.rect)) this.workspace.tabDockPreview.style[key] = `${value}px`;
            document.body.append(this.workspace.tabDockPreview);
        }
    }

    async dockTab(id, x, y) {
        const target = this.tabDockTarget(id, x, y);
        const widget = this.workspace.shell.getWidgetById(id);
        if (target && widget) {
            this.workspace.shell.addWidget(widget, { area: 'main', mode: target.mode, ref: target.ref });
            await this.workspace.activate(id);
            await this.workspace.refresh();
        }
    }

    /** 탭이 줄을 넘치면 활성 탭이 보이도록 스크롤하고 잘린 탭을 경계로 맞춘 뒤, 넘침 표시를 갱신한다. */
    keepActiveTabVisible(
        strip,
    ) {
        strip.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        this.alignTabsToEdge(strip);
        this.updateTabOverflow(strip);
    }

    /** 잘린 탭을 경계로 맞추되 활성 탭을 보이는 범위에 유지한다. 열 묶음이면 묶음마다 맞춘다. */
    alignTabsToEdge(
        strip,
    ) {
        for (const container of this.scrollContainers(strip)) tabOverflow.alignTabsToEdge(container);
    }

    /** 스크롤 위치를 바꾸지 않고 잘림·넘김 버튼·숨은 탭의 점을 갱신한다. 열 묶음이면 묶음마다 갱신한다. */
    updateTabOverflow(
        strip,
    ) {
        const containers = this.scrollContainers(strip);
        for (const container of containers) tabOverflow.updateTabOverflow(container);
        // 열 묶음일 때 줄 자체는 스크롤하지 않는다. 한 칸이던 때의 넘김 버튼·가장자리 흐림이 남지 않게 지운다.
        if (containers[0] !== strip) {
            strip.classList.remove('is-overflowing', 'is-scrolled');
            for (const scroller of strip.parentElement.querySelectorAll(':scope > .tabs-scroll')) scroller.hidden = true;
        }
    }

    /**
     * 탭을 본문 칸 위치에 맞춰 늘어놓는다.
     *
     * 한 칸이면 탭을 줄에 바로 두고 ＋를 줄 뒤(› 넘김 버튼 뒤)에 둔다. 칸이 둘 이상이면 왼쪽이 같은 칸들을 한 열로 묶어
     * 열마다 묶음(.folder-tab-group)을 만들고, 각 탭을 그 화면이 든 칸의 열에 넣는다. 위아래로 쌓인 칸의 탭은 같은 묶음에 위 칸부터,
     * 칸이 없는 탭(다른 창에 분리된 탭)은 마지막 묶음 뒤에 든다. ＋는 묶음마다 탭 뒤에 두고, 누르면 그 열의 칸에 새 터미널을 연다.
     * 묶음의 가로 위치·폭은 지금 칸 위치로 먼저 정하고, 칸 크기가 바뀌면 layoutTabGroups가 다시 맞춘다.
     */
    arrangeTabs(
        bar,
        tabs,
    ) {
        const contents = document.createDocumentFragment();
        const strip = bar.node.querySelector('.folder-tabs');
        const addActions = bar.node.querySelector('.folder-add-actions');
        const columns = paneColumns(this.paneRects());
        const isSplit = columns.length > 1;
        bar.node.classList.toggle('is-split', isSplit);
        if (isSplit) {
            const shell = this.workspace.shell;
            const paneOf = id => {
                const widget = shell.getWidgetById(id);
                return widget && !widget.secondaryWindow ? shell.getTabBarFor(widget) ?? null : null;
            };
            const tabsById = new Map(tabs.map(tab => [tab.querySelector('[role="tab"]').dataset.widgetId, tab]));
            const groups = groupTabsByColumn([...tabsById.keys()], paneOf, columns);
            const stripRect = strip.getBoundingClientRect();
            const spans = columnSpans(columns, stripRect.left, stripRect.width);
            groups.forEach((ids, index) => {
                const group = element('div', 'folder-tab-group');
                group.style.left = `${spans[index].left}px`;
                group.style.width = `${spans[index].width}px`;
                const list = element('div', 'folder-tab-list');
                list.append(...ids.map(id => tabsById.get(id)));
                group.append(list, this.paneAddActions(addActions, columns[index].keys));
                contents.append(group);
            });
        } else {
            contents.append(...tabs);
        }
        // 한 칸일 때만 줄의 ＋를 쓴다. 나눈 동안은 묶음마다 복제한 ＋가 그 자리를 맡는다.
        addActions.hidden = isSplit;
        return contents;
    }

    /**
     * 열 묶음 뒤에 둘 ＋. 줄의 ＋(`addActions`)를 복제해 이름·말풍선을 같게 두고, 누르면 이 열의 칸에 새 터미널을 연다.
     * `panes`는 열의 칸(탭 줄)들이다. 위아래로 쌓인 열이면 현재 탭이 든 칸, 없으면 맨 위 칸이 대상이다.
     */
    paneAddActions(
        addActions,
        panes,
    ) {
        const actions = addActions.cloneNode(true);
        actions.hidden = false;
        actions.querySelector('.folder-tab-add').addEventListener('click', event => {
            const currentPane = this.workspace.shell.getTabBarFor(this.workspace.currentWidget()) ?? null;
            const pane = panes.includes(currentPane) ? currentPane : panes[0];
            this.workspace.workSidebar.openNewTerminalMenu(event.currentTarget, undefined, pane);
        });
        return actions;
    }

    /**
     * 열 묶음의 가로 위치·폭을 칸의 실제 위치로 맞춘다. 칸 경계를 끌거나 창 폭이 바뀌어도 탭이 자기 칸 위에 머문다.
     * 열 수가 그려 둔 묶음 수와 다르면(칸이 막 생기거나 사라져 그릴 때는 자리를 못 잡았던 때) 줄을 다시 그린다.
     */
    layoutTabGroups() {
        const strip = this.workspace.shell.folderBar.node.querySelector('.folder-tabs');
        const groups = [...strip.querySelectorAll(':scope > .folder-tab-group')];
        const columns = paneColumns(this.paneRects());
        if ((columns.length > 1 ? columns.length : 0) !== groups.length) {
            if (!this.isLayingOut) {
                this.isLayingOut = true;
                try {
                    this.renderFolderTabs();
                } finally {
                    this.isLayingOut = false;
                }
            }
        } else if (groups.length) {
            const stripRect = strip.getBoundingClientRect();
            columnSpans(columns, stripRect.left, stripRect.width).forEach((span, index) => {
                groups[index].style.left = `${span.left}px`;
                groups[index].style.width = `${span.width}px`;
            });
            this.updateTabOverflow(strip);
        }
    }

    /** 본문 칸이 둘 이상이면 칸 탭 줄의 크기 변화를 듣는다. 관찰을 시작하는 순간에도 한 번 불려 막 생긴 칸의 자리를 잡는다. */
    observePanes() {
        this.paneObserver ??= new ResizeObserver(() => this.layoutTabGroups());
        this.paneObserver.disconnect();
        const bars = [...this.workspace.shell.mainPanel.tabBars()];
        if (bars.length > 1) for (const bar of bars) this.paneObserver.observe(bar.node);
    }

    /**
     * 아래 줄에 현재 묶음의 내부 터미널과 파일을 그린다. 내부 터미널을 추가하면 처음 연 터미널도 함께 보여 준다.
     * 작업 터미널 이름은 사이드바와 같은 행(`terminalRows`)을 쓴다.
     * 새 작업도 시작 터미널을 내부 탭에 표시한다.
     */
    renderFolderTabs() {
        const bar = this.workspace.shell.folderBar;
        // 터미널을 연 계정이 아니라 지금 실행 중인 도구의 계정을 보인다(계정 터미널에서 다른 도구를 실행한 경우 포함).
        const accountLabel = this.workspace.agentActivity.runningAccountLabel(this.workspace.currentWidget());
        const accountPicker = bar.node.querySelector('.account-picker');
        accountPicker.querySelector('.account-picker-label').textContent = accountLabel || 'Accounts';
        const canContinue = this.workspace.agentActivity.accountSessionRequest(this.workspace.currentWidget())?.provider === 'claude';
        accountPicker.title = canContinue ? `${accountLabel || 'Claude'} — change account or start a new conversation`
            : this.workspace.accountTerminals.isIdleShell(this.workspace.currentWidget()) ? 'Start an account in this terminal' : 'Open an account in a new terminal';
        accountPicker.disabled = Boolean(this.workspace.currentWidget()?.paddockAccountSwitching);
        const key = this.workspace.shownFolder();
        this.workspace.selectRepositoryOf(key);
        const root = key ? null : this.workspace.shownTopTerminal();
        const strip = bar.node.querySelector('.folder-tabs');
        // 탭을 먼저 모은 뒤 칸 위치에 맞춰 늘어놓는다(arrangeTabs).
        const tabs = [];
        // 나누기 버튼은 새 터미널을 넣을 묶음이 정해질 때(작업 폴더·Unassigned 묶음·파일만 남은 묶음)만 보인다.
        // 작업 폴더면 그 폴더에, Unassigned 묶음이면 그 묶음의 내부 터미널로 연다. 단축키·탭 끌기 분할은 묶음이 없어도 새 터미널을 연다.
        bar.node.querySelector('.folder-bar-actions').hidden = !(key || root || this.workspace.isFileOnlyGroup(this.workspace.currentWidget()));
        const add = bar.node.querySelector(':scope > .folder-add-actions > .folder-tab-add');
        // 어느 폴더에서 열리는지를 이름에 쓴다. 위쪽 묶음에서는 지금 보는 터미널의 폴더에서 열린다.
        const innerCwd = root ? this.workspace.cwdCache.get(this.workspace.currentWidget()?.id) : this.workspace.isFileOnlyGroup(this.workspace.currentWidget()) ? this.workspace.currentWidget().getResourceUri?.()?.parent?.toString() : null;
        add.title = key ? `New terminal in ${this.workspace.displayPath(key)}` : `New terminal in ${innerCwd ? this.workspace.displayPath(innerCwd) : 'this terminal\'s folder'}`;
        add.setAttribute('aria-label', add.title);
        add.setAttribute('aria-haspopup', this.workspace.n_knownAccounts === 0 ? 'false' : 'menu');
        if (key) {
            const current = this.workspace.currentWidget();
            bar.node.dataset.folder = key;
            for (const row of model.terminalRows(this.workspace.state, key)) {
                const terminal = this.workspace.shell.getWidgetById(row.id);
                const isActive = terminal === current;
                // 칸을 나누면 탭은 그 화면이 든 칸 위에 놓인다(arrangeTabs). 선택 표시는 현재 탭에만 두고, 다른 칸에 보이는 터미널은 도움말로도 알린다.
                const isShown = !isActive && Boolean(terminal?.isVisible);
                const tab = element('div', `tab${isActive ? ' is-active' : ''}${this.workspace.doneIds.has(row.id) ? ' is-done' : ''}`);
                const label = `${row.name}${row.suffix}`;
                const select = button(label, 'tab-select', () => this.workspace.run(() => this.workspace.activate(row.id)));
                select.setAttribute('role', 'tab');
                select.setAttribute('aria-selected', String(isActive));
                select.dataset.widgetId = row.id;
                select.title = terminal?.secondaryWindow ? `${label} · Open in another window` : terminal ? `${label} · ${this.workspace.programOf(terminal)}${isShown ? ' · Visible in another pane' : ''}` : label;
                select.addEventListener('dblclick', () => this.workspace.run(() => this.workspace.renameTerminal(row.id)));
                const close = button([codicon('close')], 'tab-close', () => this.workspace.run(() => this.workspace.closeWidget(terminal)));
                close.setAttribute('aria-label', `Close ${label}`);
                tab.append(select, close);
                this.workspace.workSidebar.attachTerminalMenu(tab, row.id);
                tabs.push(tab);
            }
            const branch = this.workspace.branchOf(key);
            bar.node.querySelector('.folder-bar-branch').hidden = !branch;
            bar.node.querySelector('.folder-bar-branch-name').textContent = branch;
        } else {
            delete bar.node.dataset.folder;
            bar.node.querySelector('.folder-bar-branch').hidden = true;
            bar.node.querySelector('.folder-bar-branch-name').textContent = '';
            if (root) {
                for (const id of this.workspace.innerTabIds(root)) {
                    const terminal = this.workspace.shell.getWidgetById(id);
                    if (terminal) {
                        const tab = element('div', `tab${terminal === this.workspace.currentWidget() ? ' is-active' : ''}${this.workspace.doneIds.has(id) ? ' is-done' : ''}`);
                        const label = this.workspace.unassignedName(terminal);
                        const select = button(label, 'tab-select', () => this.workspace.run(() => this.workspace.activate(id)));
                        select.setAttribute('role', 'tab');
                        select.setAttribute('aria-selected', String(terminal === this.workspace.currentWidget()));
                        select.dataset.widgetId = id;
                        select.title = `${label}${this.workspace.doneIds.has(id) ? ' · Unseen activity' : ''}`;
                        select.addEventListener('dblclick', () => this.workspace.run(() => this.workspace.renameTerminal(id)));
                        const close = button([codicon('close')], 'tab-close', () => this.workspace.run(() => this.workspace.closeWidget(terminal)));
                        close.setAttribute('aria-label', `Close ${label}`);
                        tab.append(select, close);
                        this.workspace.workSidebar.attachTerminalMenu(tab, id);
                        tabs.push(tab);
                    }
                }
            }
        }
        const belongs = (widget) => {
            const uri = widget.getResourceUri?.();
            return !this.workspace.isTerminal(widget) && (this.workspace.isFileOnlyGroup(widget) || (uri
                ? (key ? this.workspace.workFolderOf(widget) === key : root && this.workspace.fileRoots.get(widget.id) === root)
                : widget instanceof WebviewWidget && (key ? this.workspace.webviewFolders.get(widget.id) === key : root && this.workspace.fileRoots.get(widget.id) === root)));
        };
        const fileWidgets = [...this.workspace.shell.widgets].filter(belongs);
        // 같은 이름의 파일이 둘 이상 열려 있으면 탭에 상위 폴더를 붙여 구분한다(`notes.md · src`).
        const n_sameNames = new Map();
        for (const widget of fileWidgets) {
            const base = widget.getResourceUri?.()?.path.base;
            if (base) n_sameNames.set(base, (n_sameNames.get(base) || 0) + 1);
        }
        for (const widget of fileWidgets) {
            const uri = widget.getResourceUri?.();
            {
                const tab = element('div', `tab${widget === this.workspace.currentWidget() ? ' is-active' : ''}`);
                const base = uri?.path.base;
                const label = base ? (n_sameNames.get(base) > 1 ? `${base} · ${uri.parent.path.base || uri.parent.path.toString()}` : base) : widget.title.label;
                const isDirty = Saveable.isDirty(widget);
                const select = button(label, 'tab-select', () => this.workspace.run(() => this.workspace.activate(widget.id)));
                select.setAttribute('role', 'tab');
                select.setAttribute('aria-selected', String(widget === this.workspace.currentWidget()));
                select.setAttribute('aria-label', `${label}${isDirty ? ' — unsaved changes' : ''}`);
                select.dataset.widgetId = widget.id;
                select.title = `${uri?.path.toString() || widget.title.label}${isDirty ? ' — unsaved changes' : ''}`;
                tab.append(select);
                if (isDirty) {
                    const mark = element('span', 'tab-unsaved');
                    mark.setAttribute('aria-hidden', 'true');
                    mark.title = 'Unsaved changes';
                    tab.append(mark);
                }
                const close = button([codicon('close')], 'tab-close', () => this.workspace.run(() => this.workspace.closeWidget(widget)));
                close.setAttribute('aria-label', `Close ${label}`);
                tab.append(close);
                tab.addEventListener('contextmenu', event => {
                    event.preventDefault();
                    this.openTabWindowMenu(widget.id, { x: event.clientX, y: event.clientY });
                });
                tabs.push(tab);
            }
        }
        const source = this.workspace.markdownPreview.markdownSourceWidget();
        const viewActions = bar.node.querySelector('.markdown-view-actions');
        viewActions.hidden = !source;
        if (source) {
            const preview = this.workspace.markdownPreview.markdownPreviewWidget(source.getResourceUri());
            const mode = !preview ? 'file' : this.workspace.shell.getTabBarFor(source) === this.workspace.shell.getTabBarFor(preview) ? 'preview' : 'both';
            for (const action of viewActions.querySelectorAll('[data-markdown-view]')) {
                action.setAttribute('aria-pressed', String(action.dataset.markdownView === mode));
            }
        }
        const contents = this.arrangeTabs(bar, tabs);
        if (tabOverflow.replaceTabs(strip, contents)) this.keepActiveTabVisible(strip);
        else this.updateTabOverflow(strip);
        this.observePanes();
    }

    /**
     * 터미널을 지금 보이는 칸 옆에 나란히 둔다. 새 터미널을 만들지 않고 기존 터미널을 옮기므로 실행 중인 프로그램·출력이 그대로다.
     *
     * 기준 칸은 지금 선택한 화면이고, 그것이 옮길 터미널 자신이면 다른 보이는 칸을 쓴다. 보이는 다른 칸이 없으면 아무 일도 하지 않는다.
     */
    async showBeside(
        id,
    ) {
        const widget = this.workspace.shell.getWidgetById(id);
        const current = this.workspace.currentWidget();
        const ref = widget && (current && current !== widget && current.isVisible ? current
            : [...this.workspace.shell.mainPanel.tabBars()].map(bar => bar.currentTitle?.owner).find(owner => owner && owner !== widget && owner.isVisible));
        if (widget && ref) {
            this.workspace.shell.addWidget(widget, { area: 'main', mode: 'split-right', ref });
            await this.workspace.activate(id);
            await this.workspace.refresh();
        }
    }

    /** Move the existing widget so its process, output and unsaved edits remain alive. */
    moveTabToWindow(
        id,
    ) {
        const widget = this.workspace.shell.getWidgetById(id);
        if (!widget || widget.isDisposed) throw new Error('This tab has already closed.');
        if (widget.secondaryWindow) {
            widget.secondaryWindow.focus();
        } else {
            if (!widget.isExtractable) throw new Error('This tab cannot be moved to another window.');
            if (widget.paddockAccountSwitching) throw new Error('Wait for the account switch to finish before moving this tab.');
            // Hidden work groups have no current shell area. Bring the source into the
            // main panel before extracting so closing the new window can restore it.
            this.workspace.rootSwitcher.showRootOf(widget);
            // A WebGL context belongs to its original document. Keep xterm's
            // canvas renderer when moving the live terminal across documents.
            if (this.workspace.isTerminal(widget)) widget.webglAddon?.dispose();
            this.workspace.shell.secondaryWindowHandler.moveWidgetToSecondaryWindow(widget);
        }
    }

    openTabWindowMenu(
        id,
        point,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const item = button([codicon('link-external'), element('span', '', 'Move to New Window')], 'menu-item', () => {
            menu.hidePopover();
            this.workspace.run(() => this.moveTabToWindow(id));
        });
        item.setAttribute('role', 'menuitem');
        menu.append(item);
        menu.addEventListener('toggle', event => { if (event.newState === 'closed') menu.remove(); });
        document.body.append(menu);
        menu.style.left = `${point.x}px`;
        menu.style.top = `${point.y}px`;
        menu.showPopover();
        const size = menu.getBoundingClientRect();
        menu.style.left = `${Math.max(4, Math.min(point.x, window.innerWidth - size.width - 4))}px`;
        menu.style.top = `${Math.max(4, Math.min(point.y, window.innerHeight - size.height - 4))}px`;
    }

    /**
     * 셸 메뉴를 그린다. 실행 파일이 이 컴퓨터에 없는 셸은 빼고, 기본 셸과 같은 실행 파일을 가리키는 항목은 한 번만 보인다.
     * 'SHELL'은 운영체제 기본 셸을 가리키는 Theia 내부 이름이라 실제 실행 파일 이름(bash·zsh 등)으로 보인다.
     */
    async renderShellMenu() {
        const menu = this.workspace.shell.sidebar.node.querySelector('#shell-menu');
        menu.replaceChildren(element('div', 'shell-menu-state', 'Loading environments…'));
        const entries = await this.shellEntries();
        menu.replaceChildren();
        for (const { profile, shellPath, name, place, isDefault } of entries) {
            const label = isDefault ? `${name} (default)` : name;
            const meta = [place, isDefault ? 'default' : ''].filter(Boolean).join(' · ');
            const item = button([codicon(isDefault ? 'check' : 'blank'), element('span', 'shell-option-name', name), element('span', 'shell-option-meta', meta)], 'shell-option', () => {
                menu.hidePopover();
                this.workspace.run(async () => {
                    this.workspace.workExpanded = true;
                    await this.workspace.newExtraTerminal({ profile });
                });
            });
            item.setAttribute('role', 'menuitem');
            item.title = shellPath || label;
            menu.append(item);
        }
        if (!entries.length) menu.append(element('div', 'shell-menu-state', 'No environments available. Check the default shell in Quick settings.'));
        if (menu.matches(':popover-open')) menu.querySelector('button')?.focus({ preventScroll: true });
    }

    /**
     * 이 컴퓨터에서 열 수 있는 셸 프로필 목록. 실행 파일이 없는 셸은 빼고, 같은 실행 파일을 가리키는 항목은 한 번만 넣는다.
     * 'SHELL'은 운영체제 기본 셸을 가리키는 Theia 내부 이름이라 실제 실행 파일 이름(bash·zsh 등)을 이름으로 쓴다.
     */
    async shellEntries() {
        const entries = [];
        const isWindows = OS.backend.type() === OS.Type.Windows;
        // Windows 경로는 대소문자를 가리지 않아 C:\WINDOWS\system32\cmd.exe와 C:\Windows\System32\cmd.exe가 같은 셸이다.
        const sameFile = (left, right) => (isWindows ? left.toLowerCase() === right.toLowerCase() : left === right);
        for (const [id, profile] of this.workspace.profiles.all) {
            const shellPath = (profile instanceof ShellTerminalProfile && profile.shellPath) || (id === 'SHELL' ? this.workspace.systemShellPath : '');
            const args = profile instanceof ShellTerminalProfile ? profile.options.shellArgs || [] : [];
            const isDuplicate = entries.some(entry => shellPath && sameFile(entry.shellPath, shellPath)
                && JSON.stringify(entry.profile.options?.shellArgs || []) === JSON.stringify(args));
            if (!isDuplicate && await this.workspace.isShellUsable(profile)) {
                const shellName = shellPath ? shellPath.split(/[\\/]/).pop().replace(/\.exe$/i, '') : id;
                // Windows에서는 셸이 Windows에서 도는지 WSL의 Linux에서 도는지 함께 보인다. 같은 bash라도 쓰는 도구와 파일이 다르다.
                const isWsl = isWindows && wsl.isWslShell(shellPath);
                const distribution = isWsl ? distributionOf(profile.options) || this.workspace.defaultWslDistribution : '';
                const place = !isWindows ? OS.backend.type() : isWsl ? 'Linux' : 'Windows';
                entries.push({ id, profile, shellPath, name: isWsl && distribution ? `WSL · ${distribution}` : id === 'SHELL' ? shellName : id,
                    place, isDefault: profile === this.workspace.profiles.defaultProfile });
                if (isWsl && !distributionOf(profile.options)) {
                    for (const other of this.workspace.wslDistributions.filter(name => name.toLowerCase() !== distribution.toLowerCase())) {
                        const choice = profile.modify({ shellArgs: withDistribution(Array.isArray(args) ? args : [], other) });
                        entries.push({ id: `${id}:${other}`, profile: choice, shellPath, name: `WSL · ${other}`, place, isDefault: false });
                    }
                }
            }
        }
        return entries;
    }
}

module.exports = { FolderTabs };
