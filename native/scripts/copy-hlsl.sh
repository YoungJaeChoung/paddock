#!/usr/bin/env bash
# GPUI Windows 백엔드의 HLSL 셰이더 원본을 Windows 드라이브로 복사한다.
#
# WSL에서 cargo-xwin으로 만든 Windows 빌드는 셰이더를 미리 컴파일하지 못한다(fxc는
# Windows 전용) — 그래서 dev-win 프로필은 gpui가 실행 때 컴파일하게 한다. 이때 gpui는
# 빌드한 머신의 절대 경로에서 .hlsl을 읽고, Windows는 그 경로를 C:<같은 경로>로 푼다.
# 이 스크립트는 그 자리에 파일을 똑같이 둔다.
set -euo pipefail

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
registry_dir="${CARGO_HOME:-$HOME/.cargo}/registry/src"
n_copied=0

for crate in gpui-pre-windows gpui-fast-windows; do
    version="$(awk -v name="$crate" '
        $0 == "name = \"" name "\"" { found = 1; next }
        found && /^version = / { gsub(/"/, "", $3); print $3; exit }
    ' "$native_dir/Cargo.lock")"
    [ -n "$version" ] || continue
    for source_dir in "$registry_dir"/*/"$crate-$version"/src; do
        [ -d "$source_dir" ] || continue
        target_dir="/mnt/c${source_dir}"
        mkdir -p "$target_dir"
        cp "$source_dir"/*.hlsl "$target_dir"/
        n_copied=$((n_copied + $(ls "$source_dir"/*.hlsl | wc -l)))
    done
done

echo "copied $n_copied shader files"
