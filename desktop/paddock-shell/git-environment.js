// Windows의 Git 보기(VS Code 내장 git 확장)가 WSL 폴더의 저장소를 열 수 있게 하는 환경 변수를 만드는 순수 함수.

// WSL 폴더를 Windows에서 가리키는 두 경로 접두. `%(prefix)/`는 git이 `//`로 시작하는 네트워크 경로를 그대로 비교하게 한다.
// 끝의 `/*`는 그 아래 모든 저장소를 뜻한다(git 2.46 이상). 그보다 오래된 git은 이 값을 아무 저장소와도 맞추지 않을 뿐이다.
const WSL_REPOSITORY_PATTERNS = [
    '%(prefix)///wsl.localhost/*',
    '%(prefix)///wsl$/*',
];

/**
 * Windows git이 WSL 폴더의 저장소를 신뢰하도록 git 설정을 환경 변수로 덧붙인다.
 *
 * WSL 파일은 Windows에서 소유자를 확인할 수 없어 Windows git이 "소유자가 의심스러운 저장소(dubious ownership)"로 보고
 * 모든 명령을 거부한다. 그러면 Git 보기·브랜치 표시·변경 수가 WSL 작업 폴더에서 모두 비어 "No Git repository"로 보인다.
 * `safe.directory`는 저장소 안 설정으로는 켤 수 없고 전역·명령줄 설정만 인정되는데, `GIT_CONFIG_COUNT`·`GIT_CONFIG_KEY_n`·
 * `GIT_CONFIG_VALUE_n` 환경 변수는 명령줄 설정으로 취급된다. 사용자 전역 설정(.gitconfig)은 건드리지 않는다.
 *
 * Parameters
 * ----------
 * env : object
 *     git을 실행할 프로세스에 넘길 환경 변수. 제자리에서 고친다.
 * platform : string
 *     `process.platform` 값. 'win32'가 아니면 아무것도 바꾸지 않는다.
 *
 * Notes
 * -----
 * 이미 `GIT_CONFIG_COUNT`로 넘긴 설정이 있으면 그 뒤 번호에 덧붙여 기존 설정을 덮지 않는다.
 * 숫자가 아닌 `GIT_CONFIG_COUNT`는 git도 거부하므로 0으로 보고 새로 쓴다.
 *
 * Examples
 * --------
 * | 입력 env                     | platform | 결과                                                    |
 * |------------------------------|----------|---------------------------------------------------------|
 * | {}                           | win32    | COUNT=2, KEY_0·KEY_1=safe.directory, VALUE_0·1=WSL 접두 |
 * | { GIT_CONFIG_COUNT: '1', … } | win32    | 기존 0번 유지, 1·2번에 덧붙이고 COUNT=3                 |
 * | {}                           | linux    | 바뀌지 않음                                             |
 */
function trustWslRepositories(
    env,
    platform,
) {
    if (platform === 'win32') {
        const n_existing = Number.parseInt(env.GIT_CONFIG_COUNT, 10);
        let n_configs = Number.isInteger(n_existing) && n_existing > 0 ? n_existing : 0;
        for (const pattern of WSL_REPOSITORY_PATTERNS) {
            env[`GIT_CONFIG_KEY_${n_configs}`] = 'safe.directory';
            env[`GIT_CONFIG_VALUE_${n_configs}`] = pattern;
            n_configs += 1;
        }
        env.GIT_CONFIG_COUNT = String(n_configs);
    }
    return env;
}

module.exports = { trustWslRepositories, WSL_REPOSITORY_PATTERNS };
