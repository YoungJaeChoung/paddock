const { Widget } = require('@lumino/widgets');
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { MAIN_MENU_BAR } = require('@theia/core/lib/common/menu');
const { PreferenceScope } = require('@theia/core/lib/common/preferences');
const { ElectronMenuContribution } = require('@theia/core/lib/electron-browser/menu/electron-menu-contribution');

/** 제품명·기존 메뉴·창 조작을 한 줄에 배치한다. */
class PaddockMenuContribution extends ElectronMenuContribution {
    async handleTitleBarStyling(
        app,
    ) {
        // 저장된 이전 제목줄 설정을 먼저 맞춘다. 초기 동기화를 사용자 변경으로
        // 받으면 이미 새 창으로 시작했는데도 다시 시작하라는 대화상자가 열린다.
        await this.preferenceService.ready;
        const style = await window.electronTheiaCore.getTitleBarStyleAtStartup();
        if (this.preferenceService.get('window.titleBarStyle') !== style) {
            await this.preferenceService.set('window.titleBarStyle', style, PreferenceScope.User);
        }
        super.handleTitleBarStyling(app);
    }

    createLogo() {
        const brand = new Widget();
        brand.id = 'paddock-titlebar-brand';
        brand.addClass('paddock-titlebar-brand');
        brand.node.textContent = 'Paddock';
        return brand;
    }

    createCustomTitleBar(
        app,
    ) {
        super.createCustomTitleBar(app);
        app.shell.addClass('has-integrated-titlebar');
        app.shell.header.hide();
    }

    /** 좁은 창에서도 같은 메뉴 명령을 펼치며, 문서 제목은 본문의 탭에 남긴다. */
    createCustomTitleWidget(
        app,
    ) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'paddock-titlebar-menu';
        button.textContent = '☰';
        button.title = 'Application menu';
        button.setAttribute('aria-label', 'Application menu');
        button.setAttribute('aria-haspopup', 'menu');
        button.setAttribute('aria-expanded', 'false');
        let previousFocusedElement;
        button.addEventListener('focus', (event) => {
            previousFocusedElement = event.relatedTarget;
        });
        // 마우스로 메뉴를 열어도 복사·붙여넣기의 대상인 편집기 초점을 빼앗지 않는다.
        button.addEventListener('pointerdown', (event) => {
            if (event.button === 0) event.preventDefault();
        });
        button.addEventListener('click', () => {
            const context = document.activeElement === button ? previousFocusedElement : document.activeElement;
            if (context instanceof HTMLElement) context.focus({ preventScroll: true });
            const menu = this.factory.createContextMenu(
                MAIN_MENU_BAR,
                this.factory.menuProvider.getMenuNode(MAIN_MENU_BAR),
                this.factory.contextKeyService,
            );
            // 접힌 메뉴의 하위 Edit 메뉴에도 동일한 편집 대상이 필요하다.
            const preserveFocus = (item) => {
                item.preserveFocusedElement(context);
                for (const entry of item.items) {
                    if (entry.submenu) preserveFocus(entry.submenu);
                }
            };
            preserveFocus(menu);
            const bounds = button.getBoundingClientRect();
            button.setAttribute('aria-expanded', 'true');
            menu.aboutToClose.connect(() => {
                button.setAttribute('aria-expanded', 'false');
                requestAnimationFrame(() => menu.dispose());
            });
            menu.open(bounds.left, bounds.bottom);
        });
        const compactMenu = new Widget({ node: button });
        compactMenu.id = 'paddock-titlebar-menu';
        app.shell.addWidget(compactMenu, { area: 'top' });
        const updateVisibility = () => {
            const visibility = this.preferenceService.get('window.menuBarVisibility', 'classic');
            app.shell.topPanel.node.dataset.menuVisibility = visibility;
        };
        this.preferenceService.ready.then(updateVisibility);
        this.preferenceService.onPreferenceChanged((event) => {
            if (event.preferenceName === 'window.menuBarVisibility') updateVisibility();
        });
    }

    /** 창 버튼을 키보드와 화면 읽기 프로그램에서도 조작하고, 창 크기와 글꼴에 관계없이 기호를 표시한다. */
    createControlButton(
        id,
        handler,
    ) {
        const button = document.createElement('button');
        button.type = 'button';
        button.id = `${id}-button`;
        button.className = 'control-button';
        // 기호를 글꼴로 그리면 화면 배율에 따라 가는 최소화 선과 사각형의 두께가 달라진다.
        // 같은 크기의 SVG 안에 선을 그려 세 버튼의 중심과 두께를 맞춘다.
        const shapes = {
            minimize: '<path d="M1 6.5h10" />',
            maximize: '<rect x="1.5" y="1.5" width="9" height="9" />',
            restore: '<path d="M4 3.5v-2h6.5V8H9M1.5 4H8v6.5H1.5z" />',
            close: '<path d="m1.5 1.5 9 9m0-9-9 9" />',
        };
        const icon = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        icon.setAttribute('viewBox', '0 0 12 12');
        icon.setAttribute('aria-hidden', 'true');
        icon.innerHTML = shapes[id];
        button.append(icon);
        const label = `${id[0].toUpperCase()}${id.slice(1)} window`;
        button.title = label;
        button.setAttribute('aria-label', label);
        button.addEventListener('click', handler);
        return button;
    }
}
decorate(injectable(), PaddockMenuContribution);

module.exports = { PaddockMenuContribution };
