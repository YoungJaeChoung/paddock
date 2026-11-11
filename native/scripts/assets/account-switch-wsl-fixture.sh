#!/bin/bash
# 시험 자료(계정 바꾸기, WSL): 임시 홈 $1에 등록 WSL Claude 계정 A·B(배포판 Ubuntu)를 만들고, 실행기가 `WSLTEST`로 알려 준 WSL 임시 홈(예: /tmp/paddock-test-account-switch-wsl)에
# 대화를 흉내 내는 가짜 claude(bash 스크립트)와 PATH를 잡는 .bashrc를 둔다. 앱 자동 점검 빌드는 PADDOCK_TEST_WSL_HOME이 같은 폴더를 가리킬 때만 WSL 계정을 열고 그 셸의 HOME을 이 폴더로 덮는다.
# 가짜 claude: `STARTED <ID>`(`--resume <경로>`면 `RESUMED <경로>`)·`CFG <폴더>`를 찍고, 계정 폴더에 sessions/<pid>.json과 projects/<폴더 이름>/<ID>.jsonl을 남긴다.
# TERM을 받으면 `BYE`를 찍고 `$HOME/bye.log`에도 한 줄 남긴 뒤 끝난다. 표준 입력의 `delete-transcript`는 자기 대화 파일을 지운다.
H=$1
D=${WSLTEST:?WSLTEST is required}
IDA=11111111-aaaa-4aaa-8aaa-111111111111
IDB=22222222-bbbb-4bbb-8bbb-222222222222
C=$H/.paddock/config
mkdir -p $C $H/.paddock/native
printf '{\n  "version": 1,\n  "profiles": [\n    {"id": "%s", "provider": "claude", "label": "A", "runtime": "wsl", "wslDistribution": "Ubuntu"},\n    {"id": "%s", "provider": "claude", "label": "B", "runtime": "wsl", "wslDistribution": "Ubuntu"}\n  ]\n}\n' $IDA $IDB > $C/account-profiles.json
printf '%s' '{ "terminal.integrated.defaultProfile.windows": "PowerShell", "paddock.wsl.claudeLauncher": "off" }' > $H/.paddock/native/settings.json
rm -rf "$D"; mkdir -p "$D/bin" "$D/work"
printf 'export PATH="$HOME/bin:$PATH"\n' > "$D/.bashrc"
cat > "$D/bin/claude" <<'FAKE'
#!/bin/bash
resume=""
while [ $# -gt 0 ]; do
    case "$1" in
        --resume) resume=$2; shift 2 ;;
        --settings) shift 2 ;;
        *) shift ;;
    esac
done
cfg=${CLAUDE_CONFIG_DIR:-$HOME/.claude}
cwd=$(pwd)
if [ -n "$resume" ]; then
    sid=$(basename "$resume" .jsonl)
    transcript=$resume
else
    sid=${FAKE_SESSION:-aaaaaaaa-1111-4111-8111-000000000001}
    project=$(printf '%s' "$cwd" | sed 's/[^a-zA-Z0-9]/-/g')
    mkdir -p "$cfg/projects/$project"
    transcript="$cfg/projects/$project/$sid.jsonl"
    echo '{}' > "$transcript"
fi
mkdir -p "$cfg/sessions"
printf '{"pid":%s,"sessionId":"%s","kind":"interactive","cwd":"%s","startedAt":%s}\n' "$$" "$sid" "$cwd" "$(date +%s%3N)" > "$cfg/sessions/$$.json"
if [ -n "$resume" ]; then echo "RESUMED $resume"; else echo "STARTED $sid"; fi
trap 'echo BYE; echo "BYE $sid $$" >> "$HOME/bye.log"; exit 0' TERM
echo "CFG $cfg"
while :; do
    if read -r -t 0.2 line; then
        if [ "$line" = "delete-transcript" ]; then rm -f "$transcript"; echo TRANSCRIPT-DELETED; fi
    fi
done
FAKE
chmod +x "$D/bin/claude"
