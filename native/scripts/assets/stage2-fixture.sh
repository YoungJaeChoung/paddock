#!/bin/bash
# 시험 자료(2단계 총측정 장면, memory-stage2·3·4가 쓴다): 임시 홈 $1 아래 proj-a(네이티브 폴더: 항목 1,000개 + 200 KB .ts + README.md + 1080p 이미지 + 3쪽 PDF + 일치 2,500줄 bulk.txt)와 bin 폴더,
# 배포판 안 /tmp/pdk-stage2/proj(WSL 폴더: 파일 200개, 터미널 안에서 승격).
H=$1; P=$H/proj-a; A=/home/youngjae/paddock-evidence/native/stage2/chunk18/assets
S=/home/youngjae/paddock-workspace/paddock/native/scripts/assets
mkdir -p $P/img $H/bin
for i in $(seq 1 984); do echo x > $P/file-$i.txt; done
for i in $(seq 1 10); do mkdir -p $P/dir-$i; done
for n in $(seq 1 2500); do echo "needle $n: const value = compute(foo, bar) + baz; // trailing comment for line $n"; done > $P/bulk.txt
for n in $(seq 1 2400); do echo "export function fn$n(a: number, b: string): string { return b + a + 'line $n'; }"; done > $P/code-200k.ts
cp $A/a.png $P/img/
cp $A/f-1080p.png $P/
cp $S/sample-3p.pdf $P/
printf '# Stage2 README\n\nA paragraph with **bold**, *italic*, `code` and a list:\n\n- one\n- two\n\n![local](img/a.png)\n\n## Table\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nDONE\n' > $P/README.md
W=/tmp/pdk-stage2/proj
rm -rf /tmp/pdk-stage2; mkdir -p $W/src
for i in $(seq 1 200); do echo x > $W/f$i.txt; done
echo one > $W/src/a.js
( cd $W && git init -q -b main && git config user.name Tester && git config user.email t@example.com && git add -A && git commit -q -m base && echo two >> f1.txt )
ls $P | wc -l; ls -la $P/code-200k.ts
