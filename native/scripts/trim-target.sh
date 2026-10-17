#!/usr/bin/env bash
# 빌드 결과물 폴더(target/<프로필>, target/<대상>/<프로필>) 가운데 상한을 넘은 것을 비운다.
#
# cargo는 기능·RUSTFLAGS·환경 변수가 다른 빌드마다 다른 해시로 결과물을 새로 만들고 옛것을
# 지우지 않는다. 디버그 빌드는 디버깅 정보 파일이 개당 수백 MB라, 설정을 바꿔 가며 빌드하면
# 한 프로필 폴더가 수십 GB까지 쌓인다. cargo에는 오래된 결과물을 고르는 기능이 없어,
# 프로필 폴더 하나가 상한을 넘으면 통째로 비운다 — 다음 빌드가 그 프로필을 처음부터 만들 뿐
# 깨지는 것은 없다. 상한은 TARGET_LIMIT_GB(기본 10)로 바꾼다.
set -euo pipefail

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
limit_gb="${TARGET_LIMIT_GB:-10}"
n_trimmed=0

for profile_dir in "$native_dir"/target/*/ "$native_dir"/target/*/*/; do
    # 프로필 폴더만 .fingerprint를 가진다. 대상 폴더(x86_64-pc-windows-msvc 등)·임시 폴더·그 안의 deps 등은 건너뛴다.
    [ -d "$profile_dir/.fingerprint" ] || continue
    name="${profile_dir#"$native_dir"/target/}"
    size_gb=$(( $(du -sk "$profile_dir" | cut -f1) / 1024 / 1024 ))
    if [ "$size_gb" -ge "$limit_gb" ]; then
        echo "trim-target: target/${name%/} ${size_gb}GB ≥ ${limit_gb}GB, 비웁니다"
        rm -rf "$profile_dir"
        n_trimmed=$((n_trimmed + 1))
    fi
done

[ "$n_trimmed" -eq 0 ] || echo "trim-target: 프로필 폴더 ${n_trimmed}개를 비웠습니다. 다음 빌드는 그 프로필을 처음부터 만듭니다."
