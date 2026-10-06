const { OS } = require('@theia/core/lib/common/os');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { ShellTerminalProfile } = require('@theia/terminal/lib/browser/shell-terminal-profile');
const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const model = require('../work-model');
const tabOverflow = require('../tab-overflow');
const { tabDropTarget, tabDockRef } = require('../tab-drop');
const wsl = require('../wsl-terminals');
const { distributionOf, withDistribution, environmentLabel } = require('../terminal-environment');
const { element, codicon, button } = require('./shared');

/**
 * 본문 위 내부 터미널·파일 탭 줄을 그리고, 탭을 끌어 칸·다른 창으로 옮기기와 셸 메뉴를 맡는다.
 */
class FolderTabs {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
    }

    /**
     * Dragging the common tab strip uses the same docking modes as the native pane tabs.
     * 다른 창에 분리된 탭은 대상이 없다. 메인 칸에 직접 붙이면 보조 창의 복원 절차를 건너뛰어
     * 위젯이 보조 창 소속으로 남으므로, 되돌리기는 보조 창을 닫는 정식 경로에만 맡긴다.
     */
    tabDockTarget(id, x, y) {
        const bars = this.workspace.shell.getWidgetById(id)?.secondaryWindow ? [] : this.workspace.shell.mainPanel.tabBars();
        for (const bar of bars) {
            const current = bar.currentTitle?.owner;
            if (current?.isVisible) {
                const target = tabDropTarget(current.node.getBoundingClientRect(), x, y);
                // 자기 칸 가운데처럼 놓아도 바뀌는 것이 없으면 기준 탭이 없어 미리보기·이동을 건너뛴다.
                const refId = target ? tabDockRef(target.mode, bar.titles.map(title => title.owner.id), current.id, id) : null;
                const ref = refId && bar.titles.find(title => title.owner.id === refId)?.owner;
                if (target && ref) return { ...target, ref };
            }
        }
        return null;
    }

    previewTabDock(id, x, y) {
        const target = this.tabDockTarget(id, x, y);
        this.workspace.tabDockPreview?.remove();
        if (target) {
            this.workspace.tabDockPreview = element('div', 'paddock-tab-dock-preview');
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

    /** 잘린 탭을 경계로 맞추되 활성 탭을 보이는 범위에 유지한다. */
    alignTabsToEdge(
        strip,
    ) {
        tabOverflow.alignTabsToEdge(strip);
    }

    /** 스크롤 위치를 바꾸지 않고 잘림·넘김 버튼·숨은 탭의 점을 갱신한다. */
    updateTabOverflow(
        strip,
    ) {
        tabOverflow.updateTabOverflow(strip);
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
        const contents = document.createDocumentFragment();
        // 나누기 버튼은 새 터미널을 넣을 묶음이 정해질 때(작업 폴더·Unassigned 묶음·파일만 남은 묶음)만 보인다.
        // 작업 폴더면 그 폴더에, Unassigned 묶음이면 그 묶음의 내부 터미널로 연다. 단축키·탭 끌기 분할은 묶음이 없어도 새 터미널을 연다.
        bar.node.querySelector('.folder-bar-actions').hidden = !(key || root || this.workspace.isFileOnlyGroup(this.workspace.currentWidget()));
        const add = bar.node.querySelector('.folder-tab-add');
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
                // 탭 줄과 본문 칸의 위치는 일치하지 않는다. 다른 칸에 보이는 터미널은 도움말로 알리고 선택 표시는 현재 탭에만 둔다.
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
                contents.append(tab);
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
                        contents.append(tab);
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
                contents.append(tab);
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
        if (tabOverflow.replaceTabs(strip, contents)) this.keepActiveTabVisible(strip);
        else this.updateTabOverflow(strip);
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
