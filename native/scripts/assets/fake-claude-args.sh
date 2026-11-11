#!/bin/bash
# 시험 자료(래퍼): 임시 홈 $1의 bin 폴더에 받은 인자를 그대로 한 줄(`ARGS|<인자…>`)로 찍고 끝나는 가짜 claude.exe를 만든다.
# 실제 ~/.paddock는 건드리지 않는다. claude.exe는 Windows PowerShell의 Add-Type으로 C# 콘솔 프로그램을 컴파일해 만든다.
H=$1
mkdir -p $H/bin
cat > $H/bin/fake-claude.cs <<'CS'
public static class FakeClaude { public static int Main(string[] args) { System.Console.WriteLine("ARGS|" + string.Join(" ", args)); return 0; } }
CS
SRC=$(wslpath -w $H/bin/fake-claude.cs); OUT=$(wslpath -w $H/bin/claude.exe)
powershell.exe -NoProfile -Command "Add-Type -TypeDefinition (Get-Content -Raw '$SRC') -OutputAssembly '$OUT' -OutputType ConsoleApplication" > /dev/null 2>&1
ls $H/bin/claude.exe > /dev/null
