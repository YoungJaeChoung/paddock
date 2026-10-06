const { URI } = require('@theia/core');
const { ConfirmDialog, SingleTextInputDialog } = require('@theia/core/lib/browser/dialogs');
const model = require('../work-model');
const { fileMarks, colorVariable } = require('../file-marks');
const { element, codicon, button } = require('./shared');

/**
 * 사이드바 파일 목록: 폴더 내용을 읽어 파일 행을 만들고 Git 변경 표시와 파일 메뉴를 붙인다.
 */
class FileTree {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.expandedDirectories = new Set();
    }

    /** 표시할 파일·폴더와 펼친 하위 목록. 읽기 실패와 빈 폴더를 구별하고, 화면에 쓰지 않는 파일 내용·수정 시각은 담지 않는다. */
    async readDirectory(
        uri,
    ) {
        const directoryKey = uri.toString();
        const n_revision = this.workspace.n_directoryRevision;
        let stat;
        try {
            const provider = await this.workspace.files.activateProvider(uri.scheme);
            // 폴더가 그대로면 파일 이름 목록 대신 수정 시각 하나만 읽는다. 시각 정보가 없으면 실제 목록을 확인한다.
            const modified = (await provider.stat(uri)).mtime;
            const cached = this.workspace.directoryEntries.get(directoryKey);
            if (typeof modified === 'number' && cached?.modified === modified) {
                stat = cached.stat;
            } else {
                stat = await this.workspace.files.resolve(uri);
                if (typeof modified === 'number' && n_revision === this.workspace.n_directoryRevision) {
                    this.workspace.directoryEntries.set(directoryKey, { modified, stat });
                }
            }
        } catch {
            this.workspace.directoryEntries.delete(directoryKey);
        }
        const hidden = this.hiddenNames();
        const entries = [...(stat?.children || [])].filter(entry => !hidden.has(entry.name)).sort((left, right) => Number(right.isDirectory) - Number(left.isDirectory) || left.name.localeCompare(right.name));
        const children = [];
        for (const entry of entries) {
            const key = entry.resource.toString();
            const expanded = entry.isDirectory && this.expandedDirectories.has(key);
            children.push({
                uri: key,
                name: entry.name,
                isDirectory: entry.isDirectory,
                expanded,
                directory: expanded ? await this.readDirectory(entry.resource) : null,
            });
        }
        return { found: Boolean(stat), empty: !stat?.children?.length, children };
    }

    /** 열린 모든 저장소의 바뀐 파일. `[{ uri, letter, color, tooltip }]` — 파일 목록의 변경 표시와 다시 그리기 판단에 쓴다. */
    gitChanges() {
        const changes = [];
        for (const repository of this.workspace.scm.repositories) {
            for (const group of repository.provider.groups) {
                for (const resource of group.resources) {
                    const { letter = '', color = '', tooltip = '' } = resource.decorations ?? {};
                    changes.push({ uri: resource.sourceUri.toString().replace(/\/+$/, ''), letter, color, tooltip });
                }
            }
        }
        return changes;
    }

    appendDirectory(
        parent,
        directory,
        depth,
        marks,
    ) {
        if (!directory.found) {
            parent.append(element('p', 'work-empty', 'Folder not found.'));
        } else if (directory.empty) {
            parent.append(element('p', 'work-empty', 'Empty folder.'));
        }
        const current = this.workspace.currentWidget()?.getResourceUri?.()?.toString();
        for (const entry of directory.children) {
            const key = entry.uri;
            const uri = new URI(key);
            const expanded = entry.expanded;
            const icons = entry.isDirectory ? [codicon(expanded ? 'chevron-down' : 'chevron-right'), codicon(expanded ? 'folder-opened' : 'folder')] : [codicon('file')];
            // Git 변경: 파일은 상태 글자와 색, 바뀐 파일을 품은 폴더는 색만 붙인다(Source control 보기로 가지 않아도 보이게).
            const mark = entry.isDirectory ? null : marks.file(key);
            const markColor = entry.isDirectory ? marks.folder(key) : mark?.color;
            const name = element('span', 'row-name', entry.name);
            const parts = [...icons, name];
            if (mark?.letter) {
                const letter = element('span', 'file-mark', mark.letter);
                letter.style.color = colorVariable(mark.color);
                parts.push(letter);
            }
            const row = button(parts, `file-row${entry.isDirectory ? '' : ' is-file'}${key === current ? ' is-current' : ''}${markColor ? ' is-changed' : ''}`, () => this.workspace.run(async () => {
                if (entry.isDirectory) {
                    if (expanded) this.expandedDirectories.delete(key);
                    else this.expandedDirectories.add(key);
                    await this.workspace.refresh();
                } else {
                    await this.workspace.openFile(uri);
                }
            }));
            row.style.paddingLeft = `${8 + depth * this.workspace.quickSettings.sidebarIndent()}px`;
            if (markColor) name.style.color = colorVariable(markColor);
            row.title = mark?.tooltip ? `${uri.path.toString()} · ${mark.tooltip}` : uri.path.toString();
            row.dataset.uri = key;
            row.addEventListener('contextmenu', (event) => {
                event.preventDefault();
                const point = { x: event.clientX, y: event.clientY };
                const open = () => this.openFileMenu(uri, point, entry.isDirectory);
                if (event.buttons & 2) window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
                else open();
            });
            parent.append(row);
            if (entry.isDirectory && expanded) {
                this.appendDirectory(parent, entry.directory, depth + 1, marks);
            }
        }
    }

    async createFileEntry(
        parent,
        isDirectory,
    ) {
        const name = await new SingleTextInputDialog({
            title: isDirectory ? 'New Folder' : 'New File',
            validate: value => !value.trim() || value === '.' || value === '..' || /[\\/\x00-\x1f]/.test(value)
                ? 'Enter a name without path separators.' : '',
        }).open();
        if (name === undefined) return;
        const uri = parent.resolve(name);
        if (await this.workspace.files.exists(uri)) throw new Error(`“${name}” already exists. Choose another name.`);
        if (isDirectory) await this.workspace.files.createFolder(uri);
        else await this.workspace.files.createFile(uri);
        this.workspace.n_directoryRevision += 1;
        this.workspace.directoryEntries.clear();
        this.workspace.filesExpanded = true;
        this.expandedDirectories.add(parent.toString());
        if (isDirectory) this.expandedDirectories.add(uri.toString());
        else await this.workspace.openFile(uri);
        await this.workspace.refresh();
    }

    /** 폴더에는 생성 동작을, 파일에는 설치된 확장의 편집기 선택을 제공한다. */
    openFileMenu(
        uri,
        point,
        isDirectory = false,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const items = isDirectory ? [
            ['new-file', 'New File...', () => this.createFileEntry(uri, false)],
            ['new-folder', 'New Folder...', () => this.createFileEntry(uri, true)],
        ] : [
            ['file', 'Open', () => this.workspace.openFile(uri)],
            ['open-preview', 'Open With...', () => this.openFileWith(uri)],
            ['terminal', 'Command Palette...', async () => {
                const opened = [...this.workspace.shell.mainPanel.widgets()].find(widget => widget.getResourceUri?.()?.isEqual(uri));
                if (opened) await this.workspace.activate(opened.id);
                else await this.workspace.openFile(uri);
                await this.workspace.commands.executeCommand('workbench.action.showCommands');
            }],
        ];
        for (const [icon, label, action] of items) {
            const item = button([codicon(icon), element('span', '', label)], 'menu-item', () => {
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

    /** Theia의 편집기 선택 목록에서 고른 화면을 터미널 옆 파일 칸에 둔다. */
    async openFileWith(
        uri,
    ) {
        await this.workspace.plugins.willStart;
        const fileRoot = this.workspace.fileRoots.get(this.workspace.currentWidget()?.id) || this.workspace.shownTopTerminal();
        const widget = await this.workspace.openWith.openWith(uri);
        if (widget) {
            const bar = this.workspace.shell.getTabBarFor(widget);
            if (bar?.titles.some(title => this.workspace.isTerminal(title.owner))) {
                const terminal = bar.titles.find(title => this.workspace.isTerminal(title.owner))?.owner;
                await this.workspace.shell.addWidget(widget, { area: 'main', mode: 'split-right', ref: terminal });
                await this.workspace.activate(widget.id);
            }
            if (fileRoot && !model.folderContaining(this.workspace.state, uri.toString())) {
                this.workspace.fileRoots.set(widget.id, fileRoot);
                await this.workspace.saveInnerTabs();
            }
            this.workspace.refreshSoon();
        }
    }

    /**
     * 파일 구획에서 숨길 이름. `files.exclude` 설정(기본값은 VS Code와 같은 `.git`·`.DS_Store` 등 + `__pycache__`)에서
     * 켜진 패턴 중 이름 하나로 된 것을 모은다. 앞에 붙은 "모든 하위 폴더"(별표 둘 + 슬래시)는 떼고 본다.
     * 와일드카드가 든 패턴(예: 모든 `.pyc` 파일)은 다루지 않는다.
     */
    hiddenNames() {
        const patterns = this.workspace.preferences.get('files.exclude', {}) || {};
        const names = Object.keys(patterns)
            .filter(pattern => patterns[pattern] === true)
            .map(pattern => pattern.replace(/^\*\*\//, ''))
            .filter(name => !/[*?[\]{}/]/.test(name));
        return new Set(names);
    }
}

module.exports = { FileTree };
