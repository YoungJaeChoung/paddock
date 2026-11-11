#!/usr/bin/env bash
# build-mac-app.sh가 만든 Paddock Native.app을 ~/Applications에 설치한다(있으면 교체). bash 3.2(macOS 기본)에서 돈다.
#
#   install-mac-app.sh --check   설치할 자리의 앱이 실행 중인지만 확인한다(실행 중이면 종료 코드 3).
#   install-mac-app.sh           확인한 뒤 묶음을 교체한다.
#
# 실행 중인 앱은 덮어쓰지 않는다 — 멈추고 알린다. 실행 중인 데몬은 끝내지 않는다: 프로토콜이 같으면 새 앱이 이어 쓴다.
# 교체는 묶음 폴더 통째로 한다. 새 묶음을 같은 폴더의 임시 이름에 완성해 두고, 옛 묶음을 옆으로 옮긴 뒤(rename)
# 새 묶음을 제자리로 옮긴다(rename). 파일을 제자리에서 덮어쓰지 않는 이유: macOS는 실행 중인 파일이 바뀌면
# 코드 서명이 맞지 않아 그 프로세스를 죽인다. 옮기거나 지운 파일은 실행 중 프로세스가 원래 내용 그대로 계속 쓴다.
# 설치 폴더는 PADDOCK_MAC_INSTALL_DIR로 바꿀 수 있다(기본 ~/Applications).
set -euo pipefail

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
source_bundle="$native_dir/target/mac/Paddock Native.app"
install_dir="${PADDOCK_MAC_INSTALL_DIR:-$HOME/Applications}"

# 설치 폴더를 심볼릭 링크를 푼 실제 경로로 바꾼다. 실행 중인 앱의 실제 경로와 비교하려면 같은 꼴이어야 한다.
# 폴더가 아직 없으면(첫 설치) 그대로 둔다.
if [ -d "$install_dir" ]; then
    install_dir="$(cd "$install_dir" && pwd -P)"
fi
dest="$install_dir/Paddock Native.app"
app_exe="$dest/Contents/MacOS/paddock-app"

# 설치할 자리의 앱 실행 파일로 도는 프로세스의 번호를 줄마다 하나씩 낸다. 다른 폴더의 앱(개발 빌드, 시험 사본)은 세지 않는다.
# ps의 comm 열은 프로세스를 띄울 때 쓴 경로(상대 경로·심볼릭 링크 포함)라 비교에 쓸 수 없다. 이름이 paddock-app인 프로세스만
# 골라, 커널이 아는 실제 실행 파일 경로(lsof의 txt 항목 중 첫 번째)를 `app_exe`와 비교한다.
running_app_pids() {
    local pid image
    for pid in $(ps -axo pid=,comm= | awk '{ pid = $1; sub(/^ *[0-9]+ /, "", $0); n = split($0, parts, "/"); if (parts[n] == "paddock-app") print pid }'); do
        image="$(lsof -a -p "$pid" -d txt -Fn 2>/dev/null | sed -n 's/^n//p' | head -1)"
        if [ "$image" = "$app_exe" ]; then
            echo "$pid"
        fi
    done
}

refuse_if_running() {
    local pids
    pids="$(running_app_pids)"
    if [ -n "$pids" ]; then
        echo "install-mac-app: Paddock Native is running (pid $(echo "$pids" | tr '\n' ' ')). Quit it, then run this again." >&2
        echo "install-mac-app: running terminals are kept by the daemon; the new app picks them up after it starts." >&2
        exit 3
    fi
}

refuse_if_running
if [ "${1:-}" = "--check" ]; then
    exit 0
fi

[ -d "$source_bundle" ] || { echo "install-mac-app: $source_bundle not found; run 'make native-mac' first" >&2; exit 1; }

mkdir -p "$install_dir"
install_dir="$(cd "$install_dir" && pwd -P)"
dest="$install_dir/Paddock Native.app"
staging="$install_dir/.Paddock Native.app.new.$$"
retired="$install_dir/.Paddock Native.app.old.$$"

# 끝날 때(실패·Ctrl-C 포함) 임시 묶음을 지우고, 옛 묶음을 옮긴 뒤 새 묶음을 놓기 전에 멈췄으면 옛 묶음을 제자리로 돌려놓는다.
restore_on_exit() {
    if [ ! -e "$dest" ] && [ -e "$retired" ]; then
        mv "$retired" "$dest"
        echo "install-mac-app: the install did not finish; the previous app was put back" >&2
    fi
    rm -rf "$staging"
}
trap restore_on_exit EXIT
trap 'exit 130' INT TERM

# 새 묶음을 설치 폴더 안(같은 볼륨)에 완성해 둔다. ditto는 서명 봉인에 필요한 속성과 권한을 그대로 옮긴다.
rm -rf "$staging"
ditto "$source_bundle" "$staging"
codesign --verify --deep --strict "$staging"

# 옛 묶음을 옆으로 옮기고 새 묶음을 제자리로 옮긴다. 두 번의 rename 사이에 잠깐 앱이 없는 순간이 있지만 어느 파일도 반쯤 쓰이지 않는다.
if [ -e "$dest" ]; then
    mv "$dest" "$retired"
fi
mv "$staging" "$dest"
# 옛 묶음은 지운다. 그 안의 데몬이 아직 돌고 있어도 실행 중 프로세스는 지워진 파일을 계속 쓴다.
rm -rf "$retired"

echo "install-mac-app: installed $dest"
du -sh "$dest"
