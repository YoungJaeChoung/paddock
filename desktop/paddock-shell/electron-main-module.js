Object.defineProperty(exports, "__esModule", { value: true });
// Paddock 창을 실행 중인 OS에 맞추고, 백엔드가 뜨기 전에 확장·설정 폴더를 정한다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const childProcess = require('node:child_process');
const { app, dialog, powerMonitor, screen } = require('electron');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { ElectronMainApplication } = require('@theia/core/lib/electron-main/electron-main-application');
const { isOSX } = require('@theia/core/lib/common/os');
const { moveLegacyDirectory } = require('./legacy-directory');
const { isViewportStale, fillsWorkArea, boundsWithin } = require('./window-fit');
const BackendSession = require('./backend-session');
const QuitPreference = require('./quit-preference');
const { startPerformanceProbe } = require('./performance-probe');

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
 * 최대화 표시만 남고 창이 작업 영역을 채우지 않으면 건드리지 않는다. 다시 걸 때는 지금 모니터에 다시 최대화한다 —
 * 그렇지 않으면 창이 최대화 전 자리(다른 모니터일 수 있다)의 최대화 크기로 튄다.
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
            const workArea = window.isDestroyed() ? null : screen.getDisplayMatching(window.getBounds()).workArea;
            if (workArea && window.isMaximized() && !window.isMinimized() && fillsWorkArea(window.getBounds(), workArea)) {
                const viewport = await window.webContents.executeJavaScript('[innerWidth, innerHeight]').catch(() => null);
                const isStale = Boolean(viewport) && isViewportStale(window.getContentSize(), viewport, window.webContents.getZoomFactor());
                if (!isStale) {
                    n_refits = 0;
                } else if (n_refits < 1 && !window.isDestroyed()) {
                    n_refits += 1;
                    window.unmaximize();
                    // 최대화 전 자리가 다른 모니터면 지금 모니터 안으로 옮긴 뒤 다시 최대화한다.
                    const restored = window.getBounds();
                    const inside = boundsWithin(restored, workArea);
                    if (inside.x !== restored.x || inside.y !== restored.y || inside.width !== restored.width || inside.height !== restored.height) window.setBounds(inside);
                    window.maximize();
                }
            }
        }, MAXIMIZE_CHECK_MS);
    };
    window.on('maximize', check);
    // Moving between existing displays does not emit display-metrics-changed.
    window.on('moved', check);
    window.on('resize', check);
    screen.on('display-metrics-changed', check);
    window.once('closed', () => {
        clearTimeout(timer);
        screen.removeListener('display-metrics-changed', check);
    });
}

/** 터미널 수를 문장에 넣을 말로 바꾼다. 읽지 못했으면(null) 수를 말하지 않는다. */
function terminalsPhrase(
    n_terminals,
) {
    let phrase = 'Terminals';
    if (n_terminals === 1) phrase = '1 terminal';
    else if (n_terminals > 1) phrase = `${n_terminals} terminals`;
    return phrase;
}

/** 포트가 열릴 때까지 기다리는 최대 횟수(100ms 간격). */
const N_START_POLLS = 600;

/** Paddock 설정을 적용한 뒤 Theia 데스크톱 앱을 시작한다. */
class PaddockMainApplication extends ElectronMainApplication {
    /**
     * 앱을 닫아도 터미널이 살아 있도록 백엔드를 앱과 분리해 띄우고, 다시 열면 남아 있는 백엔드에 붙는다.
     * 기본 구현은 앱이 종료될 때 백엔드를 죽여 터미널 프로세스가 함께 끝난다.
     *
     * 남은 백엔드가 이 앱과 다른 버전이면 붙지 않는다. 화면과 백엔드가 주고받는 형식이 달라 동작이 깨지기 때문이다.
     * 열려 있는 터미널이 있으면 닫고 새로 띄울지 사용자에게 묻고, 거절하면 접속 정보를 지우지 않고 앱을 닫는다.
     */
    async startBackend() {
        const directory = process.env.THEIA_CONFIG_DIR;
        const fingerprint = BackendSession.fingerprintOf(app.getVersion(), this.globals.THEIA_BACKEND_MAIN_PATH);
        let port;
        let isDeclined = false;
        // 두 앱이 동시에 시작해 서로 다른 백엔드를 띄우거나 같은 백엔드를 서로 끝내지 않도록, 읽기부터 기록까지를 한 번에 한 앱만 한다.
        const release = await BackendSession.acquireLock(directory);
        try {
            const state = BackendSession.readState(directory);
            if (state) {
                if (BackendSession.isAlive(state.pid) && await BackendSession.isListening(state.port)) {
                    if (state.fingerprint === fingerprint) {
                        // 남아 있는 백엔드는 자기를 띄운 앱의 토큰만 받아들이므로 같은 토큰을 이어 쓴다.
                        this.electronSecurityToken.value = state.token;
                        port = state.port;
                        this.backend = { pid: state.pid, port: state.port, token: state.token };
                        // 백엔드가 뜬 뒤 바뀐 PATH·계정 환경 변수를 새 터미널이 받도록 지금 앱의 값을 넘긴다.
                        await BackendSession.shareEnvironment(state.port, state.token, process.env);
                    } else {
                        isDeclined = !(await this.replaceOutdatedBackend(state));
                    }
                }
                if (port === undefined && !isDeclined) BackendSession.clearState(directory);
            }
            if (!isDeclined && port === undefined) port = await this.launchBackend(directory, fingerprint);
        } finally {
            if (release) release();
        }
        if (isDeclined) {
            // 사용자가 교체를 거절했다. 남은 터미널을 그대로 두고 앱만 닫는다. 새 백엔드를 띄우거나 접속 정보를 지우지 않는다.
            app.exit(0);
            await new Promise(() => {});
        }
        return port;
    }

    /**
     * 백엔드를 앱의 하위 프로세스가 아니게 새로 띄우고 포트가 열릴 때까지 기다려 포트를 돌려준다.
     * 접속 정보는 PID를 알자마자 쓴다. 이후 시작이 실패해도 다음 실행이 남은 백엔드를 찾아 정리할 수 있다.
     */
    async launchBackend(
        directory,
        fingerprint,
    ) {
        process.env.THEIA_ELECTRON_VERSION = process.versions.electron;
        const port = await BackendSession.freePort();
        const args = [...this.processArgv.getProcessArgvWithoutBin(), `--port=${port}`];
        const options = await this.getForkOptions();
        // `theia start`가 넣어 둔 부모 PID를 백엔드가 감시하다 부모가 사라지면 스스로 끝나므로, 분리한 백엔드에는 넘기지 않는다.
        delete options.env.THEIA_PARENT_PID;
        const logFd = fs.openSync(path.join(directory, 'backend.log'), 'a');
        const child = childProcess.spawn(process.execPath, [this.globals.THEIA_BACKEND_MAIN_PATH, ...args], {
            env: { ...options.env, ELECTRON_RUN_AS_NODE: '1' },
            // Windows에서 detached는 새 프로세스 그룹을 만들어 터미널의 Ctrl+C를 막으므로 Unix에서만 쓴다.
            detached: process.platform !== 'win32',
            windowsHide: true,
            stdio: ['ignore', logFd, logFd],
        });
        fs.closeSync(logFd);
        child.unref();
        const { pid } = child;
        BackendSession.writeState(directory, { pid, port, token: this.electronSecurityToken.value, fingerprint });
        this.backend = { pid, port, token: this.electronSecurityToken.value };
        let hasExited = false;
        child.once('exit', () => { hasExited = true; });
        for (let n_polls = 0; !(await BackendSession.isListening(port)); n_polls += 1) {
            if (hasExited || n_polls > N_START_POLLS) {
                await BackendSession.stop({ pid, port, token: this.electronSecurityToken.value });
                BackendSession.clearState(directory);
                this.backend = undefined;
                throw new Error(`The backend did not start. See ${path.join(directory, 'backend.log')}.`);
            }
            await new Promise((resolve) => setTimeout(resolve, 100));
        }
        return port;
    }

    /**
     * 앱과 버전이 다른 남은 백엔드를 끝내고 끝냈으면 true를 돌려준다. 열려 있는 터미널이 있으면 사용자가 허락해야 하고,
     * 거절하면 false다. 터미널 수를 읽지 못해도(null) 있다고 보고 묻는다.
     */
    async replaceOutdatedBackend(
        state,
    ) {
        const n_terminals = await BackendSession.countTerminals(state.port, state.token);
        let isAllowed = n_terminals === 0;
        if (!isAllowed) {
            await app.whenReady();
            const { response } = await dialog.showMessageBox({
                type: 'question',
                buttons: ['Close terminals and update', 'Quit'],
                defaultId: 1,
                cancelId: 1,
                title: 'Paddock was updated',
                message: 'Paddock needs to restart its background session to use the new version.',
                detail: `${terminalsPhrase(n_terminals)} still running from the previous version will be closed. Choose Quit to keep them and update later.`,
            });
            isAllowed = response === 0;
        }
        if (isAllowed) await BackendSession.stop(state);
        return isAllowed;
    }

    /**
     * 창이 모두 닫힌 뒤 남은 터미널을 어떻게 할지 정하고 백엔드를 그에 맞게 처리한다.
     * 열린 터미널이 없으면 백엔드를 남길 이유가 없어 설정과 무관하게 끝낸다. 있으면 앱 화면의 "모두 끝내고 종료" 요청,
     * 설정(paddock.backend.onQuit) 순으로 따르고, 설정이 ask면 묻는다. 묻는 창에서 "다시 묻지 않기"를 고르면 그 선택을 설정에 적는다.
     * 이 앱이 백엔드를 띄웠거나 붙어 있는 경우에만 동작하며, 그 포트가 아직 열려 있을 때만 끝낸다(PID 재사용 방지).
     */
    async settleBackendOnQuit() {
        const directory = process.env.THEIA_CONFIG_DIR;
        const backend = this.backend;
        if (backend && BackendSession.isAlive(backend.pid) && await BackendSession.isListening(backend.port)) {
            const n_terminals = await BackendSession.countTerminals(backend.port, backend.token);
            const settingsPath = path.join(directory, 'settings.json');
            const isStopRequested = BackendSession.takeStopOnce(directory, Date.now());
            let settings = '';
            try { settings = fs.readFileSync(settingsPath, 'utf8'); } catch { /* 설정 파일이 아직 없다. */ }
            let onQuit = QuitPreference.readOnQuit(settings);
            let isStopped = n_terminals === 0 || isStopRequested || onQuit === QuitPreference.ON_QUIT.STOP;
            // OS가 종료·로그아웃 중이면 사용자가 답할 수 없다. 묻지 않고 유지로 본다(어차피 OS가 프로세스를 정리한다).
            if (!isStopped && onQuit === QuitPreference.ON_QUIT.ASK && !this.isSystemShutdown) {
                const { response, checkboxChecked } = await dialog.showMessageBox({
                    type: 'question',
                    buttons: ['Keep running in background', 'Stop all and quit'],
                    defaultId: 0,
                    cancelId: 0,
                    title: 'Quit Paddock',
                    message: `${terminalsPhrase(n_terminals)} still running.`,
                    detail: 'Keep them running to pick up where you left off the next time you open Paddock. You can change this later in Settings under "Backend: On Quit" (paddock.backend.onQuit).',
                    checkboxLabel: "Don't ask again",
                });
                isStopped = response === 1;
                if (checkboxChecked) {
                    onQuit = isStopped ? QuitPreference.ON_QUIT.STOP : QuitPreference.ON_QUIT.KEEP;
                    const updated = QuitPreference.withOnQuit(settings, onQuit);
                    // 설정 파일이 읽히지 않는 형식이면 쓰지 않는다. 다음 종료에서 다시 묻는다.
                    if (updated !== null) fs.writeFileSync(settingsPath, updated);
                }
            }
            if (isStopped) {
                await BackendSession.stop(backend);
                BackendSession.clearState(directory);
            }
        }
    }

    async start(
        config,
    ) {
        usePaddockDirectories();
        if (process.platform === 'win32') app.on('browser-window-created', (event, window) => keepMaximizedContentFitted(window));
        // 창이 모두 닫혀 종료가 확정된 시점(will-quit)에 백엔드를 어떻게 할지 정한다. 저장하지 않은 편집기 때문에 창 닫기가
        // 거부되면 이 이벤트가 오지 않으므로 백엔드는 그대로다. 결정이 끝나기 전의 종료 요청은 계속 막는다.
        app.whenReady().then(() => {
            // 멈춤·메모리 기록은 설정 폴더의 logs/performance.log에 쌓인다(performance-probe.js).
            startPerformanceProbe(
                path.join(process.env.THEIA_CONFIG_DIR, 'logs'),
                () => app.getAppMetrics().map(metric => ({ type: metric.type, workingSetKb: metric.memory.workingSetSize })),
            );
            // Linux·macOS는 powerMonitor가, Windows는 창의 session-end가 OS 종료를 알린다.
            powerMonitor.on('shutdown', () => { this.isSystemShutdown = true; });
        });
        app.on('browser-window-created', (event, window) => window.on('session-end', () => { this.isSystemShutdown = true; }));
        let settling;
        app.on('will-quit', (event) => {
            if (this.backend) {
                event.preventDefault();
                if (!settling) {
                    settling = this.settleBackendOnQuit()
                        .catch((error) => console.error('Could not settle the backend on quit:', error))
                        .finally(() => {
                            this.backend = undefined;
                            app.quit();
                        });
                }
            }
        });
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
