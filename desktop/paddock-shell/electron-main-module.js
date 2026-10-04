Object.defineProperty(exports, "__esModule", { value: true });
// Paddock 창을 실행 중인 OS에 맞추고, 백엔드가 뜨기 전에 확장·설정 폴더를 정한다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { app, screen } = require('electron');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { ElectronMainApplication } = require('@theia/core/lib/electron-main/electron-main-application');
const { isOSX } = require('@theia/core/lib/common/os');
const { moveLegacyDirectory } = require('./legacy-directory');
const { isViewportStale } = require('./window-fit');

// macOS에서만 의미 있는 창 옵션. Windows에서 titleBarStyle을 넘기면 창 틀과 닫기 버튼이 사라진다.
const MAC_ONLY_WINDOW_OPTIONS = ['titleBarStyle', 'trafficLightPosition'];

/**
 * 사용자 확장·설정 폴더와 앱에 동봉한 기본 확장 폴더를 환경 변수로 지정한다.
 *
 * 개발 실행(`npm start`)과 설치본이 같은 폴더를 쓰도록 메인 프로세스에서 정한다.
 * 백엔드 프로세스는 이 환경 변수를 물려받는다. `PADDOCK_EXTENSIONS_DIR`·`PADDOCK_CONFIG_DIR`가 있으면
 * 그 경로를, 없으면 `~/.paddock/extensions`·`~/.paddock/config`를 만들어 쓴다.
 * 이전 이름으로 쓰던 `~/.herdr`가 있으면 기본 폴더를 만들기 전에 그 폴더를 옮겨 이어 쓴다.
 * 앱 폴더의 `plugins`(Source control 보기에 쓰는 VS Code 내장 git 확장)와 `themes`(기본 색 테마 Paddock Dark·Light와
 * Cursor에서 쓰던 Mermaid Dark·Light를 담은 테마 전용 확장, 저장소에 둔다)는 있을 때만 더한다.
 */
function usePaddockDirectories() {
    if (!process.env.PADDOCK_EXTENSIONS_DIR && !process.env.PADDOCK_CONFIG_DIR) {
        const claudeSettingsPath = path.join(process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'), 'settings.json');
        try {
            moveLegacyDirectory(os.homedir(), path.join(os.homedir(), '.paddock'), claudeSettingsPath);
        } catch (error) {
            // 옮기지 못해도 앱은 새 폴더로 시작한다. 옛 폴더는 그대로 남아 손으로 옮길 수 있다.
            console.warn('Could not move ~/.herdr to ~/.paddock:', error);
        }
    }
    const extensionDirectory = path.resolve(process.env.PADDOCK_EXTENSIONS_DIR || path.join(os.homedir(), '.paddock', 'extensions'));
    const configDirectory = path.resolve(process.env.PADDOCK_CONFIG_DIR || path.join(os.homedir(), '.paddock', 'config'));
    fs.mkdirSync(extensionDirectory, { recursive: true });
    fs.mkdirSync(configDirectory, { recursive: true });
    process.env.THEIA_CONFIG_DIR = configDirectory;
    useWindowsFontsOnWsl(configDirectory);
    // Windows 경로(C:\...)도 확장 목록 주소로 읽히도록 /C:/... 형태로 바꿔 넘긴다.
    const extensionEntry = `local-dir:${pathToFileURL(extensionDirectory).pathname}`;
    const bundledDirectory = path.join(process.env.THEIA_APP_PROJECT_PATH || process.cwd(), 'plugins');
    const bundledEntry = fs.existsSync(bundledDirectory) ? `local-dir:${pathToFileURL(bundledDirectory).pathname}` : undefined;
    const themesDirectory = path.join(process.env.THEIA_APP_PROJECT_PATH || process.cwd(), 'themes');
    const themesEntry = fs.existsSync(themesDirectory) ? `local-dir:${pathToFileURL(themesDirectory).pathname}` : undefined;
    process.env.THEIA_DEFAULT_PLUGINS = [process.env.THEIA_DEFAULT_PLUGINS, bundledEntry, themesEntry, extensionEntry].filter(Boolean).join(',');
}

// WSL에서 보이는 Windows 글꼴 폴더. Cursor 등 Windows 앱이 쓰는 Consolas·맑은 고딕이 여기 있다.
const WINDOWS_FONTS_DIRECTORY = '/mnt/c/Windows/Fonts';

/**
 * WSL에서 실행되면 Windows 글꼴 폴더를 글꼴 목록에 더한다.
 *
 * WSL의 Linux에는 Consolas·맑은 고딕이 없어 터미널이 DejaVu Sans Mono와 중국어 글꼴(한글)로 그려져
 * 같은 PC의 Windows 앱과 모양이 크게 달라진다. 시스템 설정을 포함한 글꼴 설정 파일을 `configDirectory`에 쓰고
 * `FONTCONFIG_FILE`로 가리킨다. 이미 `FONTCONFIG_FILE`이 있거나 WSL이 아니면 아무것도 하지 않는다.
 * 첫 실행 때 Windows 글꼴을 한 번 훑느라 약 2초 걸리고, 이후에는 글꼴 캐시를 쓴다.
 */
function useWindowsFontsOnWsl(
    configDirectory,
) {
    if (process.platform === 'linux' && !process.env.FONTCONFIG_FILE && fs.existsSync(WINDOWS_FONTS_DIRECTORY)) {
        const fontConfigPath = path.join(configDirectory, 'fonts.conf');
        fs.writeFileSync(fontConfigPath, [
            '<?xml version="1.0"?>',
            '<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">',
            '<fontconfig>',
            '  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>',
            `  <dir>${WINDOWS_FONTS_DIRECTORY}</dir>`,
            '</fontconfig>',
            '',
        ].join('\n'));
        process.env.FONTCONFIG_FILE = fontConfigPath;
    }
}

// WSL이 Windows GPU를 Linux에 넘겨 주는 장치.
const WSL_GPU_DEVICE = '/dev/dxg';

/**
 * WSL에서 실행되면 Windows GPU로 화면을 그리게 한다.
 *
 * Chromium은 WSL의 GPU를 차단 목록에 올려 WebGL을 끈다. 그러면 WebGL로 그리는 확장(예: pen.dev)이
 * "Hardware acceleration unavailable"로 멈추고, 차단을 풀기만 하면 CPU로 흉내 내는 그리기(llvmpipe)가 잡혀 느리다.
 * 여기서는 차단 목록만 무시한다 — 이것만으로 WebGL이 켜진다(CPU 그리기). 실제 GPU(예: Intel Arc)로 그리는
 * Mesa D3D12 드라이버(`GALLIUM_DRIVER=d3d12`)는 기본으로 켜지 않는다. 이 드라이버로 실행했을 때 Windows GPU가
 * 응답을 멈춰 화면 전체가 굳고 강제 종료해야 했다. 원하는 사용자는 실행 전에 이 변수를 직접 정할 수 있다.
 * 앱이 준비되기 전에 정해야 GPU 프로세스에 적용되므로 모듈을 읽을 때 실행한다.
 */
function useWslGpu() {
    if (process.platform === 'linux' && fs.existsSync(WSL_GPU_DEVICE)) {
        app.commandLine.appendSwitch('ignore-gpu-blocklist');
    }
}
useWslGpu();

// 최대화하거나 모니터 배율이 바뀐 뒤 화면이 창 크기를 따라왔는지 확인하기까지 기다리는 시간.
// 배율이 다른 모니터에서는 정상이어도 화면이 따라오는 데 0.7초쯤 걸려, 그보다 넉넉히 둔다.
const MAXIMIZE_CHECK_MS = 1500;

/**
 * 최대화한 창의 내용이 창 크기를 따라오지 못하고 멈춰 있으면 최대화를 한 번 풀었다 다시 건다.
 * Windows에서 최대화한 창이 배율(DPI)이 다른 모니터에 걸리면 창 틀만 커지고 내용은 이전 크기로 남는 경우가 있다 —
 * 사용자가 최소화했다 복원해야 맞춰졌다. 정상이면 아무것도 하지 않고, 한 번 바로잡은 뒤에는 다시 시도하지 않는다.
 */
function keepMaximizedContentFitted(
    window,
) {
    let timer = null;
    // 바로잡으려고 다시 최대화하면 이 확인이 또 돈다. 맞춰지면 0으로 돌리고, 맞춰지지 않아도 한 번만 다시 건다.
    let n_refits = 0;
    const check = () => {
        clearTimeout(timer);
        timer = setTimeout(async () => {
            if (!window.isDestroyed() && window.isMaximized() && !window.isMinimized()) {
                const viewport = await window.webContents.executeJavaScript('[innerWidth, innerHeight]').catch(() => null);
                const isStale = Boolean(viewport) && isViewportStale(window.getContentSize(), viewport, window.webContents.getZoomFactor());
                if (!isStale) {
                    n_refits = 0;
                } else if (n_refits < 1) {
                    n_refits += 1;
                    window.unmaximize();
                    window.maximize();
                }
            }
        }, MAXIMIZE_CHECK_MS);
    };
    window.on('maximize', check);
    screen.on('display-metrics-changed', check);
    window.once('closed', () => {
        clearTimeout(timer);
        screen.removeListener('display-metrics-changed', check);
    });
}

/** Paddock 설정을 적용한 뒤 Theia 데스크톱 앱을 시작한다. */
class PaddockMainApplication extends ElectronMainApplication {
    getTitleBarStyle(
        config,
    ) {
        // 이전 창의 기본 native 값이 저장돼 있어도 새 제목줄을 한 번 적용한다.
        // 창 위치·크기는 보존하고, 이후 사용자가 바꾼 제목줄 설정은 그대로 따른다.
        if (!isOSX && !this.electronStore.get('paddock.integratedTitlebar')) {
            const windowState = this.electronStore.get('windowstate');
            if (windowState) this.electronStore.set('windowstate', { ...windowState, frame: false });
            this.electronStore.set('paddock.integratedTitlebar', true);
        }
        return super.getTitleBarStyle(config);
    }

    async start(
        config,
    ) {
        usePaddockDirectories();
        if (process.platform === 'win32') app.on('browser-window-created', (event, window) => keepMaximizedContentFitted(window));
        await super.start(config);
    }

    getDefaultOptions() {
        const options = super.getDefaultOptions();
        if (!isOSX) {
            for (const key of MAC_ONLY_WINDOW_OPTIONS) {
                delete options[key];
            }
            // 창 왼쪽 위·작업 표시줄 아이콘. 설치본은 실행 파일에 아이콘이 들어 있지만, 개발 실행은 Electron 아이콘이 보여 직접 정한다.
            // macOS는 창 아이콘이 없고 Dock은 앱 묶음의 아이콘을 쓴다.
            const icon = path.join(app.getAppPath(), 'resources', process.platform === 'win32' ? 'icon.ico' : 'icon.png');
            if (fs.existsSync(icon)) options.icon = icon;
        }
        return options;
    }
}
decorate(injectable(), PaddockMainApplication);

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ElectronMainApplication).to(PaddockMainApplication).inSingletonScope();
});
