const { OS } = require('@theia/core/lib/common/os');
const { PreferenceScope } = require('@theia/core/lib/common/preferences/preference-scope');
const { ThemeService } = require('@theia/core/lib/browser/theming');
const { STATUS_ITEMS, element, codicon, button } = require('./shared');

// 빠른 설정(보기 줄 오른쪽 끝)의 값. 글자 크기는 터미널과 파일 보기에 함께 적용한다.
const APPEARANCE = {
    FONT_SIZE_MIN: 10,
    FONT_SIZE_MAX: 24,
    SIDEBAR_INDENT_MIN: 6,
    SIDEBAR_INDENT_MAX: 24,
    INTERFACE_FONTS: ['Segoe UI', 'Inter', 'Noto Sans', 'Noto Sans CJK KR', 'Malgun Gothic', 'Apple SD Gothic Neo', 'Arial'],
    // 터미널 글꼴 후보. 이 컴퓨터에 설치된 것만 보인다. 한글은 뒤의 한글 글꼴이 채운다.
    TERMINAL_FONTS: ['Consolas', 'Cascadia Mono', 'JetBrains Mono', 'D2Coding', 'Menlo', 'SF Mono', 'Fira Code', 'DejaVu Sans Mono'],
    HANGUL_FALLBACK: "'Malgun Gothic', 'Apple SD Gothic Neo', 'Noto Sans Mono CJK KR', monospace",
};

// 운영체제별 기본 셸 설정 이름의 끝부분(terminal.integrated.defaultProfile.<끝부분>).
const OS_PREFERENCE_KEY = { [OS.Type.Windows]: 'windows', [OS.Type.Linux]: 'linux', [OS.Type.OSX]: 'osx' };

/**
 * 보기 줄 오른쪽 끝 빠른 설정 창을 그리고, 글자 크기·글꼴·사이드바 들여쓰기 같은 화면 설정을 적용한다.
 */
class QuickSettings {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
    }

    /** 이 컴퓨터에 설치된 글꼴인지. 대체 글꼴과 글자 폭이 다르면 설치된 것으로 본다. */
    isFontInstalled(
        family,
    ) {
        const context = document.createElement('canvas').getContext('2d');
        const sample = 'mmmmmmmmmmlli10OO';
        return ['monospace', 'serif'].some((fallback) => {
            context.font = `20px ${fallback}`;
            const base = context.measureText(sample).width;
            context.font = `20px '${family}', ${fallback}`;
            return context.measureText(sample).width !== base;
        });
    }

    /** 사이드바의 한 단계 들여쓰기 폭을 설정 범위 안에서 읽는다. */
    sidebarIndent() {
        const value = this.workspace.preferences.get('paddock.sidebarIndent', 12);
        return Math.max(APPEARANCE.SIDEBAR_INDENT_MIN, Math.min(APPEARANCE.SIDEBAR_INDENT_MAX, Number(value) || 12));
    }

    /** 화면 글꼴과 사이드바 들여쓰기를 현재 창에 적용한다. */
    applyInterfacePreferences() {
        const font = this.workspace.preferences.get('paddock.interfaceFontFamily', '');
        if (font) {
            const family = font.replaceAll('\\', '\\\\').replaceAll("'", "\\'");
            this.workspace.shell.node.style.setProperty('--p-interface-font', `'${family}', -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`);
        } else {
            this.workspace.shell.node.style.removeProperty('--p-interface-font');
        }
        this.workspace.shell.node.style.setProperty('--p-sidebar-indent', `${this.sidebarIndent()}px`);
    }

    /** 설정 하나를 사용자 설정 파일에 저장한다. 바로 화면에 적용된다. */
    async setPreference(
        key,
        value,
    ) {
        await this.workspace.preferences.set(key, value, PreferenceScope.User);
    }

    /**
     * 빠른 설정 팝오버를 그린다: 색 테마, 글자 크기, 터미널·화면 글꼴, 사이드바 들여쓰기, 전체 설정 열기.
     * 자주 바꾸는 겉모습만 한곳에 모으고, 나머지는 전체 설정(Theia 설정 화면)으로 넘긴다.
     */
    renderQuickSettings() {
        const panel = this.workspace.shell.sidebar.node.querySelector('#quick-settings');
        const focusedSetting = panel.contains(document.activeElement) ? document.activeElement.dataset.setting : null;
        const scrollTop = panel.scrollTop;
        const themes = this.workspace.container.get(ThemeService);
        const current = themes.getCurrentTheme().id;
        panel.replaceChildren(element('p', 'quick-title', 'Quick settings'));

        const themeRow = element('label', 'quick-row');
        const themeSelect = element('select', 'quick-select');
        themeSelect.dataset.setting = 'theme';
        const sorted = [...themes.getThemes()].sort((left, right) => left.type.localeCompare(right.type) || left.label.localeCompare(right.label));
        for (const theme of sorted) {
            const option = element('option', '', theme.label);
            option.value = theme.id;
            option.selected = theme.id === current;
            themeSelect.append(option);
        }
        themeSelect.title = themes.getCurrentTheme().label;
        themeSelect.addEventListener('change', () => themes.setCurrentTheme(themeSelect.value));
        // 색 테마는 창 틀·터미널·파일 화면 전체에 한 번에 적용된다 — 고르는 즉시 보이므로 설명 문구를 두지 않는다.
        themeRow.append(element('span', 'quick-label', 'Color theme'), element('span', 'quick-space'), themeSelect);
        panel.append(themeRow);

        const size = this.workspace.preferences.get('terminal.integrated.fontSize', 14);
        const sizeRow = element('div', 'quick-row');
        const setSize = (next) => this.workspace.run(async () => {
            const value = Math.max(APPEARANCE.FONT_SIZE_MIN, Math.min(APPEARANCE.FONT_SIZE_MAX, next));
            await this.setPreference('terminal.integrated.fontSize', value);
            await this.setPreference('editor.fontSize', value);
            this.renderQuickSettings();
        });
        const smaller = button([codicon('remove')], 'quick-step', () => setSize(size - 1));
        smaller.dataset.setting = 'smaller-text';
        smaller.setAttribute('aria-label', 'Smaller text');
        smaller.disabled = size <= APPEARANCE.FONT_SIZE_MIN;
        const larger = button([codicon('add')], 'quick-step', () => setSize(size + 1));
        larger.dataset.setting = 'larger-text';
        larger.setAttribute('aria-label', 'Larger text');
        larger.disabled = size >= APPEARANCE.FONT_SIZE_MAX;
        sizeRow.append(element('span', 'quick-label', 'Terminal & editor size'), element('span', 'quick-space'), smaller, element('span', 'quick-value', `${size}px`), larger);
        panel.append(sizeRow);

        const family = this.workspace.preferences.get('terminal.integrated.fontFamily', '');
        const fontRow = element('label', 'quick-row');
        const select = element('select', 'quick-select');
        select.dataset.setting = 'code-font';
        select.title = 'Font for terminals and code editors';
        const installed = APPEARANCE.TERMINAL_FONTS.filter(name => this.isFontInstalled(name));
        const chosen = installed.find(name => family.trim().replace(/^['"]/, '').startsWith(name)) || '';
        for (const name of installed) {
            const option = element('option', '', name);
            option.value = name;
            option.selected = name === chosen;
            select.append(option);
        }
        select.addEventListener('change', () => this.workspace.run(async () => {
            const value = `'${select.value}', ${APPEARANCE.HANGUL_FALLBACK}`;
            await this.setPreference('terminal.integrated.fontFamily', value);
            await this.setPreference('editor.fontFamily', value);
        }));
        fontRow.append(element('span', 'quick-label', 'Code font'), element('span', 'quick-space'), select);
        panel.append(fontRow);

        // 새 터미널(＋)이 여는 셸. 고른 값은 이 운영체제의 기본 셸 설정에 저장되고, 이미 열린 터미널은 그대로다.
        const shellRow = element('label', 'quick-row');
        const shellSelect = element('select', 'quick-select');
        shellSelect.dataset.setting = 'default-shell';
        shellSelect.title = 'Shell for new terminals';
        shellRow.append(element('span', 'quick-label', 'Default shell'), element('span', 'quick-space'), shellSelect);
        panel.append(shellRow);
        void this.workspace.folderTabs.shellEntries().then((entries) => {
            for (const { id, name, place } of entries) {
                const option = element('option', '', place ? `${name} (${place})` : name);
                option.value = id;
                option.selected = this.workspace.profiles.getProfile(id) === this.workspace.profiles.defaultProfile;
                shellSelect.append(option);
            }
        });
        shellSelect.addEventListener('change', () => this.workspace.run(() => this.setPreference(`terminal.integrated.defaultProfile.${OS_PREFERENCE_KEY[OS.backend.type()]}`, shellSelect.value)));

        const interfaceFont = this.workspace.preferences.get('paddock.interfaceFontFamily', '');
        const interfaceRow = element('label', 'quick-row');
        const interfaceSelect = element('select', 'quick-select');
        interfaceSelect.dataset.setting = 'interface-font';
        const systemFont = element('option', '', 'System default');
        systemFont.value = '';
        interfaceSelect.append(systemFont);
        const installedInterfaceFonts = APPEARANCE.INTERFACE_FONTS.filter(name => this.isFontInstalled(name));
        if (interfaceFont && !installedInterfaceFonts.includes(interfaceFont)) {
            installedInterfaceFonts.unshift(interfaceFont);
        }
        for (const name of installedInterfaceFonts) {
            const option = element('option', '', name);
            option.value = name;
            interfaceSelect.append(option);
        }
        interfaceSelect.value = interfaceFont;
        interfaceSelect.addEventListener('change', () => this.workspace.run(() => this.setPreference('paddock.interfaceFontFamily', interfaceSelect.value)));
        interfaceRow.append(element('span', 'quick-label', 'Interface font'), element('span', 'quick-space'), interfaceSelect);
        panel.append(interfaceRow);

        const indent = this.sidebarIndent();
        const indentRow = element('div', 'quick-row');
        const setIndent = (next) => this.workspace.run(() => this.setPreference('paddock.sidebarIndent', next));
        const decreaseIndent = button([codicon('remove')], 'quick-step', () => setIndent(indent - 1));
        decreaseIndent.dataset.setting = 'decrease-indent';
        decreaseIndent.setAttribute('aria-label', 'Decrease sidebar indent');
        decreaseIndent.disabled = indent <= APPEARANCE.SIDEBAR_INDENT_MIN;
        const increaseIndent = button([codicon('add')], 'quick-step', () => setIndent(indent + 1));
        increaseIndent.dataset.setting = 'increase-indent';
        increaseIndent.setAttribute('aria-label', 'Increase sidebar indent');
        increaseIndent.disabled = indent >= APPEARANCE.SIDEBAR_INDENT_MAX;
        indentRow.append(element('span', 'quick-label', 'Sidebar indent'), element('span', 'quick-space'), decreaseIndent, element('span', 'quick-value', `${indent}px`), increaseIndent);
        panel.append(indentRow);

        // 상태 줄 오른쪽 항목을 켜고 끈다. 사용량은 Quick settings에서 숨기며, 끄면 Claude의 원래 상태 줄도 되돌린다.
        panel.append(element('p', 'quick-title quick-section', 'Status bar'));
        for (const [preferenceName, label] of [[STATUS_ITEMS.CLAUDE, 'Claude usage'], [STATUS_ITEMS.CODEX, 'Codex usage'], [STATUS_ITEMS.MEMORY, 'Memory']]) {
            const row = element('label', 'quick-row');
            const checkbox = element('input', 'quick-check');
            checkbox.dataset.setting = preferenceName;
            checkbox.type = 'checkbox';
            checkbox.checked = this.workspace.isStatusItemOn(preferenceName);
            checkbox.addEventListener('change', () => this.workspace.run(() => this.setPreference(preferenceName, checkbox.checked)));
            row.append(element('span', 'quick-label', label), element('span', 'quick-space'), checkbox);
            panel.append(row);
        }

        const all = button([codicon('settings-gear'), element('span', '', 'All settings')], 'quick-all', () => {
            panel.hidePopover();
            this.workspace.run(() => this.workspace.commands.executeCommand('preferences:open'));
        });
        panel.append(all);
        // 값을 바꿔 요소를 다시 만들어도 조작 중인 입력의 초점과 스크롤 위치는 유지한다.
        if (focusedSetting) panel.querySelector(`[data-setting="${CSS.escape(focusedSetting)}"]`)?.focus({ preventScroll: true });
        panel.scrollTop = scrollTop;
    }
}

module.exports = { QuickSettings };
