#!/usr/bin/env bash
# WSL에서 Windows용 release 실행 파일 세 개(paddock-app·paddock-daemon·paddock-pen-mcp)를 cargo-xwin으로 만든다.
# 셰이더는 Windows SDK의 fxc.exe로 미리 컴파일해 실행 파일에 넣는다 — 결과는 Windows CI의 release 빌드와 같은 꼴이다.
#
# 왜 따로 두나: gpui-pre-windows의 build.rs는 빌드하는 머신이 Windows일 때만 fxc로 셰이더를 컴파일한다.
# WSL(Linux)에서 크로스 빌드하면 그 단계가 빠지고, release 코드가 include!하는 OUT_DIR/shaders_bytes.rs가 없어
# 컴파일이 멈춘다(dev-win 프로필은 이를 피하려고 실행 중 컴파일로 돌린다 — 그 실행 파일은 이 PC에서만 돈다).
# 이 스크립트는 build.rs가 Windows에서 만들었을 파일을 같은 내용으로 그 OUT_DIR에 만든다.
#
#   cargo 빌드(1회차) → gpui-*-windows 빌드 스크립트 실행, OUT_DIR 생김
#        │             셰이더 파일이 없으면 그 크레이트 컴파일이 실패한다
#        ▼
#   fxc.exe(Windows, WSL 상호 운용으로 실행) → OUT_DIR/<모듈>_vs.h·_ps.h → OUT_DIR/shaders_bytes.rs
#        ▼
#   cargo 빌드(2회차) → 셰이더가 들어간 release 실행 파일
#
# 셰이더 파일이 이미 있으면 다시 만들지 않는다 — 다시 쓰면 그 크레이트부터 전부 다시 컴파일된다.
# fxc.exe는 GPUI_FXC_PATH(gpui의 build.rs와 같은 이름)로 지정하거나, 없으면 설치된 Windows SDK의 최신 x64 판을 쓴다.
# 결과는 native/target/x86_64-pc-windows-msvc/release/에 생긴다.
set -euo pipefail

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
target=x86_64-pc-windows-msvc
release_dir="$native_dir/target/$target/release"
messages="$(mktemp)"
trap 'rm -f "$messages"' EXIT

# gpui-pre-windows build.rs의 모듈 목록과 같다: shaders.hlsl의 여덟 모듈과 color_text_raster.hlsl의 글자 그리기 모듈.
SHADER_MODULES="quad shadow path_rasterization path_sprite underline monochrome_sprite subpixel_sprite polychrome_sprite"

build() {
    cd "$native_dir" && XWIN_ACCEPT_LICENSE=1 cargo xwin build --release --target "$target" \
        -p paddock-app -p paddock-daemon -p paddock-pen-mcp --message-format=json-render-diagnostics
}

find_fxc() {
    local found=""
    if [ -n "${GPUI_FXC_PATH:-}" ] && [ -f "$GPUI_FXC_PATH" ]; then
        found="$GPUI_FXC_PATH"
    else
        found="$(ls -d "/mnt/c/Program Files (x86)/Windows Kits/10/bin"/10.*/x64/fxc.exe 2>/dev/null | sort -V | tail -1)"
    fi
    [ -n "$found" ] || { echo "fxc.exe not found: install the Windows SDK or set GPUI_FXC_PATH" >&2; exit 1; }
    echo "$found"
}

# fxc 헤더(`const BYTE 이름[] = { … };`)를 Rust 상수(`const 이름: &[u8] = &[ … ];`)로 바꿔 덧붙인다 — build.rs와 같은 변환이다.
append_binding() {
    local header="$1"
    local const_name="$2"
    local binding="$3"
    awk -v name="$const_name" '
        { text = text $0 "\n" }
        END {
            rest = substr(text, index(text, "const BYTE"))
            rest = substr(rest, index(rest, "=") + 1)
            gsub(/^[ \t\r\n]+|[ \t\r\n]+$/, "", rest)
            gsub(/\{/, "[", rest)
            gsub(/\}/, "]", rest)
            printf "const %s: &[u8] = &%s\n", name, rest
        }' "$header" >>"$binding"
}

# 한 모듈의 정점·픽셀 셰이더를 컴파일해 바인딩 파일에 덧붙인다(build.rs의 compile_shader_for_module과 같은 인자).
compile_module() {
    local fxc="$1"
    local module="$2"
    local shader="$3"
    local out_dir="$4"
    local binding="$5"
    local stage profile suffix const_name header output
    for stage in vertex fragment; do
        if [ "$stage" = vertex ]; then profile=vs_4_1 suffix=vs; else profile=ps_4_1 suffix=ps; fi
        const_name="$(echo "${module}_${stage}_BYTES" | tr '[:lower:]' '[:upper:]')"
        header="$out_dir/${module}_${suffix}.h"
        # fxc는 성공해도 원본 셰이더의 경고를 수십 줄 낸다 — 실패했을 때만 보인다.
        if ! output="$("$fxc" /nologo /T "$profile" /E "${module}_${stage}" /Fh "$(wslpath -w "$header")" /Vn "$const_name" \
            /O3 "$(wslpath -w "$shader")" 2>&1)"; then
            echo "$output" >&2
            echo "build-release-win: fxc failed for ${module}_${stage}" >&2
            exit 1
        fi
        append_binding "$header" "$const_name" "$binding"
    done
}

# 빌드 스크립트 출력 폴더 하나에 셰이더 바인딩을 만든다. 이미 있으면 건너뛴다.
# 반쯤 쓴 파일이 빌드에 쓰이지 않도록 임시 이름으로 쓴 뒤 옮긴다.
write_shaders() {
    local crate="$1"
    local version="$2"
    local out_dir="$3"
    local binding="$out_dir/shaders_bytes.rs"
    [ -f "$binding" ] && return 0
    local source_dir
    source_dir="$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"$crate-$version"/src | head -1)"
    local fxc
    fxc="$(find_fxc)"
    local partial="$binding.partial"
    rm -f "$partial"
    local module
    for module in $SHADER_MODULES; do
        compile_module "$fxc" "$module" "$source_dir/shaders.hlsl" "$out_dir" "$partial"
    done
    compile_module "$fxc" emoji_rasterization "$source_dir/color_text_raster.hlsl" "$out_dir" "$partial"
    mv "$partial" "$binding"
    echo "build-release-win: $crate $version shaders compiled with $(basename "$fxc")"
}

n_written=0
if ! build >"$messages"; then
    # 빌드 스크립트 실행 기록에서 gpui-*-windows의 OUT_DIR을 찾는다. package_id 꼴: …#gpui-pre-windows@0.3.9
    while IFS=$'\t' read -r crate version out_dir; do
        write_shaders "$crate" "$version" "$out_dir"
        n_written=$((n_written + 1))
    done < <(sed -n 's/.*"reason":"build-script-executed".*#\(gpui-[a-z]*-windows\)@\([^"]*\)".*"out_dir":"\([^"]*\)".*/\1\t\2\t\3/p' "$messages")
    [ "$n_written" -gt 0 ] || { echo "build-release-win: the build failed before any GPUI shader step" >&2; exit 1; }
    build >"$messages"
fi

for exe in paddock-app paddock-daemon paddock-pen-mcp; do
    ls -l "$release_dir/$exe.exe"
done
