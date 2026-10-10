// 작업 화면 협력 클래스들이 함께 쓰는 DOM 도우미·저장 키·명령 정의.

// 작업 목록, 상단 터미널 묶음의 내부 탭, 창을 닫을 때 보던 위젯과 묶음별 칸 배치, 상태 줄의 도구별 마지막 사용량 계정을 각각 저장한다.
class STORAGE {
    static WORK_FOLDERS = 'paddock.work-folders.v1';
    static INNER_TABS = 'paddock.inner-tabs.v1';
    static SHOWN_WIDGET = 'paddock.shown-widget.v1';
    static ROOT_LAYOUTS = 'paddock.root-layouts.v1';
    static USAGE_SELECTION = 'paddock.usage-selection.v1';
}

// 상태 줄 오른쪽 항목별 표시 설정. 설정 화면·빠른 설정이 같은 값을 바꾸고, 사용량 항목은 전체 계정 목록을 연다. 기본은 모두 켜짐이다.
class STATUS_ITEMS {
    static CLAUDE = 'paddock.statusBar.claude';
    static CODEX = 'paddock.statusBar.codex';
    static MEMORY = 'paddock.statusBar.memory';
}

const MARKDOWN_PREVIEW = {
    COMMANDS: {
        preview: 'markdown-preview-enhanced.openPreview',
        both: 'markdown-preview-enhanced.openPreviewToTheSide',
    },
    VIEW_TYPE: 'markdown-preview-enhanced',
    EXTENSIONS: new Set(['.md', '.markdown', '.mdown', '.mkdn', '.mkd', '.rmd', '.qmd']),
};

function element(
    tag,
    className,
    text,
) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
}

function codicon(
    name,
) {
    return element('span', `codicon codicon-${name}`);
}

function button(
    label,
    className,
    action,
) {
    const node = element('button', className);
    node.type = 'button';
    if (typeof label === 'string') node.textContent = label;
    else if (label) node.append(...label);
    node.addEventListener('click', (event) => {
        event.stopPropagation();
        action(event);
    });
    return node;
}

/**
 * 메뉴 항목 버튼 하나를 만든다. 모든 메뉴(＋·계정·환경·우클릭)가 같은 모양과 role을 쓰게 하는 유일한 자리다.
 * `icon`·`meta`가 없으면 그 칸을 만들지 않는다. `meta`가 빈 문자열이면 칸은 있되 CSS가 숨긴다.
 */
function menuItem(
    { icon, label, meta, className = '' },
    action,
) {
    const children = [];
    if (icon) children.push(codicon(icon));
    children.push(element('span', 'menu-item-label', label));
    if (meta !== undefined) children.push(element('span', 'menu-item-meta', meta));
    const item = button(children, `menu-item ${className}`.trim(), action);
    item.setAttribute('role', 'menuitem');
    return item;
}

/** 메뉴를 창 안쪽으로 당긴다. 크기는 실제로 그려진 뒤에만 알 수 있어 `showPopover` 뒤에 부른다. */
function clampToWindow(
    menu,
    left,
    top,
) {
    const size = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(left, window.innerWidth - size.width - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(top, window.innerHeight - size.height - 4))}px`;
}

/** 동적으로 만든 메뉴 상자를 만든다. 닫히면 스스로 DOM에서 빠진다. */
function createPopupMenu(
    ariaLabel,
) {
    const menu = element('div', 'paddock-menu');
    menu.setAttribute('popover', '');
    menu.setAttribute('role', 'menu');
    if (ariaLabel) menu.setAttribute('aria-label', ariaLabel);
    menu.addEventListener('toggle', (event) => {
        if (event.newState === 'closed') menu.remove();
    });
    return menu;
}

/** `createPopupMenu`로 만든 메뉴를 (left, top)에 열고 창 안쪽으로 당긴다. */
function showPopupMenuAt(
    menu,
    left,
    top,
) {
    document.body.append(menu);
    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;
    menu.showPopover();
    clampToWindow(menu, left, top);
}

/**
 * 메뉴 안에서 ↑↓·Home·End로 항목 사이를 옮긴다. 쓸 수 없는(disabled) 항목은 건너뛴다.
 * `onEscape`가 있으면 Esc를 가로채 그 동작을 부르고, 없으면 팝오버의 기본 닫기에 맡긴다.
 */
function attachMenuKeys(
    menu,
    onEscape,
) {
    menu.addEventListener('keydown', (event) => {
        const items = [...menu.querySelectorAll('[role="menuitem"]:not(:disabled)')];
        const index = items.indexOf(document.activeElement);
        let next;
        if (items.length) {
            if (event.key === 'ArrowDown') next = (index + 1) % items.length;
            else if (event.key === 'ArrowUp') next = (index - 1 + items.length) % items.length;
            else if (event.key === 'Home') next = 0;
            else if (event.key === 'End') next = items.length - 1;
        }
        if (next !== undefined) {
            items[next].focus({ preventScroll: true });
            event.preventDefault();
        } else if (event.key === 'Escape' && onEscape) {
            onEscape();
            event.preventDefault();
        }
    });
}

module.exports = { STORAGE, STATUS_ITEMS, MARKDOWN_PREVIEW, element, codicon, button, menuItem, clampToWindow, createPopupMenu, showPopupMenuAt, attachMenuKeys };
