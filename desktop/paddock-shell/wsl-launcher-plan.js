// 화면 쪽 코드(setup-notices.js)와 시험이 함께 쓰는 순수 판단. Node·브라우저 어느 쪽에서도 불러올 수 있게 아무것도 불러오지 않는다.

/** WSL 일반 터미널의 claude 실행 래퍼 설정 키. 'ask'(아직 안 물음)·'on'(~/.bashrc에 넣음)·'off'(거절하거나 뺌). */
const WSL_LAUNCHER_KEY = 'paddock.wsl.claudeLauncher';

/**
 * WSL 일반 터미널용 claude 실행 래퍼 설정을 보고 시작할 때 할 일을 정한다.
 *
 * 'prompt'는 사용자에게 한 번 묻는다는 뜻이고, 'add'는 이미 허락했으니 ~/.bashrc에 줄이 있는지 확인해 없으면 다시 넣는다는 뜻이다
 * (WSL 안에서 .bashrc가 바뀌었을 때를 위해서다). 거절했거나 Windows·WSL이 아니면 'none'이다.
 *
 * Examples
 * --------
 * | 설정 | Windows 앱 | WSL 있음 | 결과     |
 * | ---- | ---------- | -------- | -------- |
 * | ask  | 예         | 예       | `prompt` |
 * | on   | 예         | 예       | `add`    |
 * | off  | 예         | 예       | `none`   |
 * | ask  | 예         | 아니오   | `none`   |
 * | ask  | 아니오     | -        | `none`   |
 */
function wslLauncherPlan(
    preference,
    { isWindows, isWslReady },
) {
    let plan = 'none';
    if (isWindows && isWslReady) {
        if (preference === 'ask') plan = 'prompt';
        else if (preference === 'on') plan = 'add';
    }
    return plan;
}

module.exports = { WSL_LAUNCHER_KEY, wslLauncherPlan };
