const { MessageService } = require('@theia/core/lib/common/message-service');
const { URI } = require('@theia/core');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { version } = require('../package.json');
const { Widget } = require('@lumino/widgets');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { OpenerService } = require('@theia/core/lib/browser/opener-service');
const { CommandRegistry } = require('@theia/core/lib/common/command');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');
const { FileDialogService } = require('@theia/filesystem/lib/browser/file-dialog/file-dialog-service');
const { WorkspaceService } = require('@theia/workspace/lib/browser/workspace-service');
const { TerminalService } = require('@theia/terminal/lib/browser/base/terminal-service');
const { HostedPluginSupport } = require('@theia/plugin-ext/lib/hosted/browser/hosted-plugin');
const { CustomEditorWidget } = require('@theia/plugin-ext/lib/main/browser/custom-editors/custom-editor-widget');
const { TerminalProfileService, NULL_PROFILE } = require('@theia/terminal/lib/browser/terminal-profile-service');
const { ShellTerminalProfile } = require('@theia/terminal/lib/browser/shell-terminal-profile');
const { ConfirmDialog } = require('@theia/core/lib/browser/dialogs');

function button(
    label,
    className,
    action,
) {
    const element = document.createElement('button');
    element.type = 'button';
    element.className = className;
    element.textContent = label;
    element.addEventListener('click', action);
    return element;
}

/** 파일 선택·세션·확장 설치를 Herdr의 탐색기와 탭에 연결한다. */
class HerdrWorkspace {
    constructor(
        container,
    ) {
        this.container = container;
        this.rail = 'terminal';
        this.n_sidebarRevision = 0;
        this.expandedDirectories = new Set();
        this.installing = false;
    }

    onStart() {
        this.shell = this.container.get(ApplicationShell);
        this.messages = this.container.get(MessageService);
        this.files = this.container.get(FileService);
        this.fileDialogs = this.container.get(FileDialogService);
        this.workspace = this.container.get(WorkspaceService);
        this.terminals = this.container.get(TerminalService);
        this.profiles = this.container.get(TerminalProfileService);
        this.commands = this.container.get(CommandRegistry);
        this.opener = this.container.get(OpenerService);
        this.plugins = this.container.get(HostedPluginSupport);
        this.shell.onDidAddWidget((widget) => {
            widget.title.changed.connect(() => this.renderTabs());
            this.renderTabs();
        });
        this.shell.onDidRemoveWidget(() => {
            this.renderTabs();
            queueMicrotask(() => {
                if (this.layoutReady && this.shell.mainPanel.isEmpty) {
                    this.run(() => this.showWelcome());
                }
                if (this.rail === 'terminal') {
                    this.run(() => this.renderSidebar());
                }
            });
        });
        this.shell.mainPanel.onDidChangeCurrent(() => this.renderTabs());
        const tabResize = new ResizeObserver(() => {
            this.shell.tabs.node.querySelector('.is-active')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        });
        tabResize.observe(this.shell.tabs.node);
        this.shell.disposed.connect(() => tabResize.disconnect());
        this.workspace.onWorkspaceChanged(() => this.run(() => this.renderSidebar()));
        this.plugins.onDidChangePlugins(() => {
            if (this.rail === 'extensions') {
                this.run(() => this.renderSidebar());
            }
        });
        this.shell.rail.node.addEventListener('click', (event) => {
            const target = event.target.closest('[data-rail]');
            if (target) {
                this.rail = target.dataset.rail;
                this.run(() => this.renderSidebar());
            } else if (event.target.closest('[data-action="about"]')) {
                this.showAbout();
            }
        });
        this.shell.header.node.querySelector('.tab-add').addEventListener('click', () => this.run(() => this.newTerminal()));
        const picker = this.shell.header.node.querySelector('.shell-picker');
        const menu = this.shell.header.node.querySelector('#shell-menu');
        menu.addEventListener('beforetoggle', (event) => {
            if (event.newState === 'open') {
                this.renderShellMenu();
                const bounds = picker.getBoundingClientRect();
                menu.style.left = `${Math.min(bounds.left, window.innerWidth - 240)}px`;
            }
        });
        menu.addEventListener('keydown', (event) => {
            if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault();
                const items = [...menu.querySelectorAll('button')];
                const index = items.indexOf(document.activeElement);
                const next = index < 0 ? (event.key === 'ArrowDown' ? 0 : items.length - 1)
                    : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
                items[next]?.focus();
            }
        });
        this.files.onDidFilesChange(() => {
            if (this.rail === 'explorer') {
                this.run(() => this.renderSidebar());
            }
        });
    }

    async onDidInitializeLayout() {
        await this.workspace.roots;
        this.layoutReady = true;
        for (const terminal of this.terminals.all) {
            if (this.shell.getAreaFor(terminal) !== 'main') {
                await this.shell.addWidget(terminal, { area: 'main' });
            }
        }
        const current = this.shell.mainPanel.currentTitle?.owner;
        if (current) {
            await this.shell.activateWidget(current.id);
        } else {
            await this.showWelcome();
        }
        await this.renderSidebar();
        this.renderTabs();
    }

    async run(
        action,
    ) {
        try {
            await action();
        } catch (error) {
            this.messages.error(error instanceof Error ? error.message : String(error));
        }
    }

    emptyMessage(
        text,
    ) {
        const note = document.createElement('p');
        note.className = 'sidebar-description';
        note.textContent = text;
        return note;
    }

    showAbout() {
        const dialog = document.createElement('dialog');
        dialog.className = 'herdr-about';
        dialog.setAttribute('aria-label', 'Herdr Terminal 정보');
        dialog.innerHTML = '<span class="welcome-mark" aria-hidden="true">›_</span><h1>Herdr Terminal</h1><p>터미널과 파일을 한 작업 공간에서.</p>';
        const release = document.createElement('p');
        release.textContent = `버전 ${version}`;
        dialog.append(release, button('닫기', 'primary-button', () => dialog.close()));
        dialog.addEventListener('close', () => dialog.remove(), { once: true });
        document.body.append(dialog);
        dialog.showModal();
    }

    async installVsix() {
        if (!this.installing) {
            this.installing = true;
            try {
                await this.renderSidebar();
                await this.commands.executeCommand('vsxExtensions.installFromVSIX');
            } finally {
                this.installing = false;
                await this.renderSidebar();
            }
        }
    }

    /** 선택한 프로젝트가 실제로 열렸는지 확인하고, 실패 이유를 호출자에게 전달한다. */
    async openProject() {
        const [currentRoot] = await this.workspace.roots;
        const selected = await this.fileDialogs.showOpenDialog({
            title: '프로젝트 폴더 열기',
            canSelectFolders: true,
            canSelectFiles: false,
            canSelectMany: false,
        }, currentRoot);
        const uri = Array.isArray(selected) ? selected[0] : selected;
        if (uri && !uri.isEqual(this.workspace.workspace?.resource)) {
            // 작업 공간 열기의 비동기 실패까지 기다려 기존 탭을 보존하고 오류를 보여 준다.
            await this.workspace.openWorkspace(uri);
        }
    }

    syncContext() {
        const current = this.shell.mainPanel.currentTitle?.owner;
        const uri = current?.getResourceUri?.();
        for (const item of this.shell.sidebar.node.querySelectorAll('[data-widget-id], [data-uri]')) {
            const active = Boolean(current && item.dataset.widgetId === current.id) || Boolean(uri && item.dataset.uri === uri.toString());
            item.classList.toggle('is-active', active);
            if (active) item.setAttribute('aria-current', 'true');
            else item.removeAttribute('aria-current');
        }
        const status = this.shell.footer.node.querySelector('#statusbar-right');
        const saveable = current && Saveable.get(current);
        status.replaceChildren();
        const label = document.createElement('span');
        label.textContent = saveable ? (saveable.dirty ? '변경 사항 있음' : '저장됨')
            : this.terminals.all.includes(current) ? `${current.title.label} · 로컬 셸` : '준비됨';
        status.append(label);
        if (saveable?.dirty) {
            const save = button('저장', 'save-action', () => this.run(async () => {
                save.disabled = true;
                try {
                    await saveable.save();
                } finally {
                    this.syncContext();
                }
            }));
            status.append(save);
        }
        status.title = uri?.path.toString() || current?.title.label || '';
    }

    async showWelcome() {
        let widget = this.shell.getWidgetById('herdr-welcome');
        if (!widget) {
            widget = new Widget();
            widget.id = 'herdr-welcome';
            widget.title.label = '시작';
            widget.title.closable = true;
            widget.addClass('herdr-welcome');
            widget.node.innerHTML = '<span class="welcome-mark" aria-hidden="true">›_</span><h1>작업을 시작하세요</h1><p>프로젝트 폴더를 열거나 새 터미널을 시작하세요.</p>';
            widget.node.append(
                button('프로젝트 폴더 열기', 'primary-button', () => this.run(() => this.openProject())),
                button('새 터미널', 'outline-button', () => this.run(() => this.newTerminal())),
            );
        }
        if (this.shell.getAreaFor(widget) !== 'main') {
            await this.shell.addWidget(widget, { area: 'main' });
        }
        await this.shell.activateWidget(widget.id);
    }

    async newTerminal(
        profile = this.profiles.defaultProfile,
    ) {
        const root = this.workspace.tryGetRoots()[0]?.resource;
        let terminal;
        if (profile && profile !== NULL_PROFILE) {
            const selected = profile instanceof ShellTerminalProfile ? profile.modify({ cwd: root?.toString(), title: profile.shellPath?.split(/[\\/]/).pop() }) : profile;
            if (selected instanceof ShellTerminalProfile) {
                // 절대 경로(/bin/zsh, C:\...\pwsh.exe)만 미리 확인한다. 이름만 적힌 셸은 PATH에서 찾는다.
                if (/^(\/|[A-Za-z]:[\\/])/.test(selected.shellPath ?? '')) {
                    const executable = await this.files.resolve(URI.fromFilePath(selected.shellPath)).catch(() => undefined);
                    if (!executable?.isFile) {
                        throw new Error(`셸을 시작할 수 없습니다: ${selected.shellPath}. 실행 파일 경로를 확인하세요.`);
                    }
                }
                terminal = await this.terminals.newTerminal(selected.options);
                try {
                    await terminal.start();
                } catch (error) {
                    terminal.dispose();
                    throw new Error(`셸을 시작할 수 없습니다: ${selected.shellPath || '기본 셸'}. 실행 파일 경로를 확인하세요.`, { cause: error });
                }
            } else {
                terminal = await selected.start();
            }
        } else {
            terminal = await this.terminals.newTerminal({ cwd: root?.toString() });
            await terminal.start();
        }
        await this.terminals.open(terminal, { widgetOptions: { area: 'main' }, mode: 'activate' });
        this.rail = 'terminal';
        await this.renderSidebar();
    }

    async closeTab(
        widget,
    ) {
        let close = true;
        if (this.terminals.all.includes(widget)) {
            close = await new ConfirmDialog({ title: '터미널 종료', msg: '이 탭의 셸과 실행 중인 작업을 종료할까요?', ok: '종료', cancel: '취소' }).open();
        }
        if (close) {
            // 문서의 변경 여부와 저장 확인은 편집기의 저장 계약을 따른다.
            await this.shell.closeWidget(widget.id);
            if (Array.from(this.shell.mainPanel.widgets()).length === 0) {
                await this.showWelcome();
            }
        }
        await this.renderSidebar();
    }

    renderTabs() {
        const strip = this.shell.tabs.node;
        const current = this.shell.mainPanel.currentTitle?.owner;
        const widgets = [...this.shell.mainPanel.widgets()];
        const existing = [...strip.children];
        const sameTabs = existing.length === widgets.length
            && widgets.every((widget, index) => existing[index].querySelector('[role=tab]')?.dataset.widgetId === widget.id);
        if (!sameTabs) {
            strip.replaceChildren();
            for (const widget of widgets) {
                const tab = document.createElement('div');
                const select = button('', 'tab-select', () => this.run(() => this.shell.activateWidget(widget.id)));
                select.dataset.widgetId = widget.id;
                select.setAttribute('role', 'tab');
                select.addEventListener('keydown', (event) => {
                    const items = [...strip.querySelectorAll('[role=tab]')];
                    const index = items.indexOf(select);
                    let next = index;
                    if (event.key === 'ArrowRight') next = (index + 1) % items.length;
                    if (event.key === 'ArrowLeft') next = (index + items.length - 1) % items.length;
                    if (event.key === 'Home') next = 0;
                    if (event.key === 'End') next = items.length - 1;
                    if (next !== index) {
                        event.preventDefault();
                        const targetId = items[next].dataset.widgetId;
                        this.keyboardTabTarget = targetId;
                        this.run(() => this.shell.activateWidget(targetId));
                    }
                });
                const close = button('×', 'tab-close', () => this.run(() => this.closeTab(widget)));
                tab.append(select, close);
                strip.append(tab);
            }
        }
        for (const [index, widget] of widgets.entries()) {
            const tab = strip.children[index];
            const select = tab.querySelector('[role=tab]');
            const dirty = widget.title.className.includes('theia-mod-dirty');
            tab.className = `tab${widget === current ? ' is-active' : ''}`;
            select.textContent = `${dirty ? '● ' : ''}${widget.title.label}`;
            select.tabIndex = widget === current ? 0 : -1;
            select.setAttribute('aria-selected', String(widget === current));
            select.title = widget.title.caption || widget.title.label;
            tab.querySelector('.tab-close').setAttribute('aria-label', `${widget.title.label} 닫기`);
        }
        if (this.keyboardTabTarget === current?.id) {
            const targetId = this.keyboardTabTarget;
            this.keyboardTabTarget = null;
            setTimeout(() => {
                if (this.shell.mainPanel.currentTitle?.owner?.id === targetId) {
                    [...strip.querySelectorAll('[role=tab]')].find(item => item.dataset.widgetId === targetId)?.focus({ preventScroll: true });
                }
            }, 200);
        }
        this.syncContext();
        strip.querySelector('.is-active')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }

    renderShellMenu() {
        const menu = this.shell.header.node.querySelector('#shell-menu');
        menu.replaceChildren();
        for (const [id, profile] of this.profiles.all) {
            if (profile !== NULL_PROFILE) {
                const label = id === 'SHELL' ? '기본 셸' : id;
                const item = button(label, 'shell-option', () => {
                    menu.hidePopover();
                    this.run(() => this.newTerminal(profile));
                });
                item.setAttribute('role', 'menuitem');
                item.autofocus = menu.childElementCount === 0;
                item.title = profile instanceof ShellTerminalProfile ? profile.shellPath || label : label;
                menu.append(item);
            }
        }
        if (!menu.childElementCount) {
            menu.append(button('기본 셸', 'shell-option', () => {
                menu.hidePopover();
                this.run(() => this.newTerminal());
            }));
        }
    }

    async renderSidebar() {
        const n_revision = ++this.n_sidebarRevision;
        const node = document.createElement('div');
        const root = this.workspace.tryGetRoots()[0]?.resource;
        const name = root?.path.base || '프로젝트 미선택';

        this.shell.footer.node.querySelector('#statusbar-project').textContent = `◆ HERDR · ${name}`;
        for (const item of this.shell.rail.node.querySelectorAll('[data-rail]')) {
            item.classList.toggle('is-active', item.dataset.rail === this.rail);
            item.setAttribute('aria-pressed', String(item.dataset.rail === this.rail));
        }
        const heading = document.createElement('div');
        heading.className = 'sidebar-header';
        heading.textContent = { terminal: '터미널', explorer: '프로젝트 파일', extensions: '파일 보기 확장' }[this.rail];
        node.append(heading);
        const project = document.createElement('div');
        project.className = 'sidebar-project';
        project.textContent = name;
        project.title = root?.path.toString() || name;
        node.append(project);
        if (this.rail === 'terminal') {
            if (!this.terminals.all.length) {
                node.append(this.emptyMessage('열린 터미널이 없습니다. 상단 ＋로 시작하세요.'));
            }
            for (const terminal of this.terminals.all) {
                const item = button(`›_  ${terminal.title.label}`, 'side-item', () => this.run(() => this.shell.activateWidget(terminal.id)));
                item.dataset.widgetId = terminal.id;
                item.title = terminal.title.label;
                node.append(item);
            }
        } else if (this.rail === 'explorer') {
            const actions = document.createElement('div');
            actions.className = 'sidebar-actions';
            actions.append(button('폴더 열기', 'outline-button', () => this.run(() => this.openProject())));
            node.append(actions);
            if (root) {
                await this.appendDirectory(node, root);
            } else {
                node.append(this.emptyMessage('폴더를 열면 프로젝트 파일이 여기에 표시됩니다.'));
            }
        } else {
            const actions = document.createElement('div');
            actions.className = 'sidebar-actions';
            const install = button(this.installing ? '설치 중…' : 'VSIX 파일 설치…', 'outline-button', () => this.run(() => this.installVsix()));
            install.disabled = this.installing;
            actions.append(install);
            node.append(actions);
            const note = document.createElement('p');
            note.className = 'sidebar-description';
            note.textContent = '확장 파일은 계정 없이 설치할 수 있습니다. 설치한 확장은 자체 서비스에 연결하거나 로컬 파일에 접근할 수 있습니다. Herdr는 기본 터미널 입력·출력을 수집하거나 전송하지 않습니다.';
            node.append(note);
            if (!this.plugins.plugins.length) {
                node.append(this.emptyMessage('설치된 확장이 없습니다.'));
            }
            for (const plugin of [...this.plugins.plugins].sort((a, b) => a.model.name.localeCompare(b.model.name))) {
                const card = document.createElement('article');
                card.className = 'extension-card';
                const label = document.createElement('strong');
                label.textContent = plugin.model.displayName || plugin.model.name;
                const version = document.createElement('small');
                version.textContent = `${plugin.model.publisher || plugin.model.id.split('.')[0]} · ${plugin.model.version}`;
                card.title = plugin.model.id;
                card.append(label, version);
                node.append(card);
            }
        }
        if (n_revision === this.n_sidebarRevision) {
            this.shell.sidebar.node.replaceChildren(node);
            this.syncContext();
        }
    }

    async openFile(
        uri,
    ) {
        await this.plugins.willStart;
        const options = { mode: 'activate', widgetOptions: { area: 'main' }, preview: false };
        const opener = await this.opener.getOpener(uri, options);
        const missingId = `herdr-missing-editor:${uri}`;
        if (uri.path.ext.toLowerCase() === '.pen' && !opener.id.startsWith('custom-editor-')) {
            let widget = this.shell.getWidgetById(missingId);
            if (!widget) {
                widget = new Widget();
                widget.id = missingId;
                widget.title.label = uri.path.base;
                widget.title.closable = true;
                widget.addClass('herdr-welcome');
                widget.node.innerHTML = '<h1>이 파일을 열 확장이 필요합니다.</h1><p>.pen 파일을 지원하는 VSIX 확장을 설치한 뒤 다시 열어 주세요.</p>';
                widget.node.append(button('VSIX 파일 설치', 'primary-button', () => this.run(async () => {
                    await this.installVsix();
                    await this.openFile(uri);
                })));
            }
            if (this.shell.getAreaFor(widget) !== 'main') {
                await this.shell.addWidget(widget, { area: 'main' });
            }
            await this.shell.activateWidget(widget.id);
        } else {
            let canOpen = true;
            if (opener.id.startsWith('custom-editor-')) {
                // 같은 파일의 일반 편집기가 열려 있으면 저장 여부를 먼저 결정한다.
                for (const widget of this.shell.mainPanel.widgets()) {
                    if (!(widget instanceof CustomEditorWidget) && widget.getResourceUri?.()?.isEqual(uri)) {
                        await this.shell.closeWidget(widget.id);
                        canOpen = this.shell.getAreaFor(widget) !== 'main';
                    }
                }
            }
            if (canOpen) {
                await opener.open(uri, options);
                if (this.shell.getWidgetById(missingId)) {
                    await this.shell.closeWidget(missingId);
                }
            }
        }
    }

    async appendDirectory(
        parent,
        uri,
    ) {
        const stat = await this.files.resolve(uri);
        if (!stat.children?.length) {
            parent.append(this.emptyMessage('빈 폴더입니다.'));
        }
        const entries = [...(stat.children || [])].sort((left, right) => Number(right.isDirectory) - Number(left.isDirectory) || left.name.localeCompare(right.name));
        for (const entry of entries) {
            const row = document.createElement('div');
            const children = document.createElement('div');
            children.className = 'herdr-file-children';
            const directoryKey = entry.resource.toString();
            let expanded = this.expandedDirectories.has(directoryKey);
            const select = button(`${entry.isDirectory ? '▸' : '◩'}  ${entry.name}`, 'file-item', () => this.run(async () => {
                if (entry.isDirectory) {
                    expanded = !expanded;
                    select.setAttribute('aria-expanded', String(expanded));
                    if (expanded) {
                        this.expandedDirectories.add(directoryKey);
                    } else {
                        this.expandedDirectories.delete(directoryKey);
                    }
                    children.replaceChildren();
                    select.textContent = `${expanded ? '▾' : '▸'}  ${entry.name}`;
                    if (expanded) {
                        await this.appendDirectory(children, entry.resource);
                    }
                } else {
                    // 확장 등록이 완료된 뒤 열어 첫 클릭이 텍스트 편집기로 빠지는 것을 막는다.
                    await this.openFile(entry.resource);
                }
            }));
            select.title = entry.resource.path.toString();
            select.dataset.uri = entry.resource.toString();
            if (entry.isDirectory) select.setAttribute('aria-expanded', String(expanded));
            row.append(select, children);
            parent.append(row);
            if (entry.isDirectory && expanded) {
                select.textContent = `▾  ${entry.name}`;
                await this.appendDirectory(children, entry.resource);
            }
        }
    }
}
module.exports = { HerdrWorkspace };
