#!/bin/sh
# 에이전트(Claude Code·Codex)의 훅 이벤트를 터미널 상태 신호(OSC 7501)로 바꿔 터미널에 쓰는 훅.
#
# 에이전트가 OSC 7501을 스스로 보내지 못하는 터미널(Windows의 WSL 터미널은 ConPTY가 입력 방향 OSC를 버려
# 에이전트가 지원 질의의 답을 받지 못한다)에서도 Paddock이 working·blocked·done·idle·error를 알 수 있게 한다.
# 출력 방향의 OSC는 ConPTY를 지나므로 훅이 `/dev/tty`에 쓰기만 하면 앱에 닿는다.
#
# 입력: 에이전트가 stdin으로 주는 훅 JSON. 이벤트 이름(hook_event_name)과 알림 종류(notification_type) 두 칸만 읽고 나머지는 버린다.
#       프롬프트·도구 입력·메시지 본문은 저장도 출력도 하지 않는다.
# 출력: stdout은 비운다(훅 stdout은 에이전트 문맥에 들어갈 수 있다). 신호는 `/dev/tty`에만 쓴다. 쓸 수 없으면 아무것도 하지 않는다.
# 종료: 어떤 경우에도 0으로 끝낸다. 상태 표시가 에이전트 실행을 막으면 안 된다.
#
# 앱: 환경 변수 PADDOCK_STATUS_APP(claude-code 또는 codex)이 신호 본문의 app 칸이 된다. 없으면 claude-code다.
# 끄기: 스크립트와 같은 폴더에 `agent-status-hook.off` 파일이 있으면 아무것도 하지 않는다.
#       설정에서 훅을 지워도 이미 떠 있는 세션이 이 스크립트를 계속 부를 수 있어, 꺼짐을 스크립트가 직접 확인한다.
#
# 매핑(상태 / 이벤트):
#   idle     SessionStart, Interrupt, Notification(idle_prompt)
#   working  UserPromptSubmit, PreToolUse, PostToolUse
#   blocked  PermissionRequest, Notification(permission_prompt) → kind=permission
#            Elicitation, Notification(elicitation_dialog | agent_needs_input) → kind=question
#   done     Stop
#   error    StopFailure
#   clear    SessionEnd

# 입력은 꺼짐 확인보다 먼저 끝까지 읽는다. 읽기 전에 끝내면 입력을 보내는 쪽이 닫힌 파이프에 쓰다 EPIPE 오류를 낸다.
input=$(cat 2>/dev/null)

# 경로 없이(같은 폴더에서) 불러도 꺼짐 표지를 보도록 `$0`에 `/`가 없으면 현재 폴더다.
case $0 in
    */*) here=${0%/*} ;;
    *) here=. ;;
esac
if [ -e "$here/agent-status-hook.off" ]; then
    exit 0
fi

# JSON을 jq 없이 다루려고 각 칸의 첫 번째 값만 grep으로 뽑는다. JSON 문자열 안의 따옴표는 항상 \"로 쓰이므로
# `"hook_event_name"` 같은 패턴은 프롬프트·메시지 같은 값 안의 글자와 맞지 않고, 값 앞에 놓인 진짜 키만 맞는다.
field() {
    printf '%s' "$input" | grep -o "\"$1\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" 2>/dev/null | head -n 1 | sed 's/.*:[[:space:]]*"\([^"]*\)"$/\1/'
}
event=$(field hook_event_name)
notification=$(field notification_type)

state=
kind=
case "$event" in
    SessionStart|Interrupt) state=idle ;;
    UserPromptSubmit|PreToolUse|PostToolUse) state=working ;;
    PermissionRequest) state=blocked; kind=permission ;;
    Elicitation) state=blocked; kind=question ;;
    Stop) state=done ;;
    StopFailure) state=error ;;
    SessionEnd) state=clear ;;
    Notification)
        case "$notification" in
            idle_prompt) state=idle ;;
            permission_prompt) state=blocked; kind=permission ;;
            elicitation_dialog|agent_needs_input) state=blocked; kind=question ;;
        esac
        ;;
esac

if [ -n "$state" ]; then
    app=${PADDOCK_STATUS_APP:-claude-code}
    case "$app" in
        claude-code|codex) ;;
        *) app=claude-code ;;
    esac
    body="state=$state:app=$app"
    if [ -n "$kind" ]; then
        body="$body:kind=$kind"
    fi
    # 훅 프로세스는 제어 터미널이 없다(에이전트가 새 세션으로 띄워 /dev/tty가 열리지 않는다). 대신 부모 프로세스를 거슬러 올라가
    # 입력이 터미널(/dev/pts/N)인 첫 조상을 찾아 그 터미널에 쓴다. 에이전트가 터미널 없이 돌면(파이프) 찾지 못하고 아무것도 하지 않는다.
    # 터미널 없는 에이전트(터미널 안의 claude가 도구로 띄운 `claude -p` 등)를 만나면 거기서 멈춘다. 더 위의 터미널은 바깥 에이전트의 것이라
    # 안쪽 에이전트의 Stop·SessionStart가 바깥 행의 상태를 바꾸거나 끝남 알림을 울리면 안 된다.
    terminal=
    pid=$PPID
    depth=0
    while [ -z "$terminal" ] && [ "$depth" -lt 8 ] && [ -n "$pid" ] && [ "$pid" -gt 1 ] 2>/dev/null; do
        candidate=$(readlink "/proc/$pid/fd/0" 2>/dev/null)
        case "$candidate" in
            /dev/pts/*) terminal=$candidate ;;
            *)
                # 에이전트인지: 프로세스 이름(스크립트로 띄운 경우 스크립트 이름)이나 명령줄의 앞 두 칸(node 같은 실행기 뒤 스크립트)으로 본다.
                name=$(cat "/proc/$pid/comm" 2>/dev/null)
                command=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)
                first=${command%% *}
                case "$name:${first##*/}" in
                    claude:*|codex:*|*:claude|*:codex|*:claude-code) break ;;
                esac
                case "$command" in
                    *claude-code/cli.js*|*@openai/codex/*|*/claude/versions/*) break ;;
                esac
                pid=$(sed -n 's/^PPid:[[:space:]]*//p' "/proc/$pid/status" 2>/dev/null)
                depth=$((depth + 1))
                ;;
        esac
    done
    if [ -n "$terminal" ]; then
        { printf '\033]7501;%s\033\\' "$body" > "$terminal"; } 2>/dev/null
    fi
fi
exit 0
