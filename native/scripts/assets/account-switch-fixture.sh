#!/bin/bash
# 시험 자료(계정 바꾸기, Windows 네이티브): 임시 홈 $1에 등록 Claude 계정 A·B와, 대화를 흉내 내는 가짜 claude.exe를 만든다. 실제 ~/.paddock·실제 계정 폴더는 건드리지 않는다.
#   - 가짜 claude.exe(C# 콘솔 프로그램, Windows PowerShell의 Add-Type으로 컴파일): 시작하면 `STARTED <대화 ID>`(`--resume <경로>`를 받았으면 `RESUMED <경로>`)와
#     `CFG <계정 폴더>`를 찍고, 계정 폴더(CLAUDE_CONFIG_DIR)에 `sessions/<pid>.json`(pid·대화 ID·interactive·폴더·시작 시각)과 `projects/<폴더 이름>/<ID>.jsonl`을 남긴다.
#     표준 입력으로 `delete-transcript`를 받으면 자기 대화 파일을 지운다. 대화 ID는 기본 aaaaaaaa-1111-4111-8111-000000000001이다.
#   - 앱이 설정 폴더 bin에 놓는 claude 래퍼가 PATH 맨 앞이라 계정 터미널은 래퍼를 거쳐 가짜 claude.exe를 부르고, 래퍼가 붙이는 `--settings <파일>`은 가짜가 건너뛴다.
H=$1
IDA=11111111-aaaa-4aaa-8aaa-111111111111
IDB=22222222-bbbb-4bbb-8bbb-222222222222
C=$H/.paddock/config
mkdir -p $C $H/.paddock/native $H/bin
printf '{\n  "version": 1,\n  "profiles": [\n    {"id": "%s", "provider": "claude", "label": "A", "runtime": "native"},\n    {"id": "%s", "provider": "claude", "label": "B", "runtime": "native"}\n  ]\n}\n' $IDA $IDB > $C/account-profiles.json
printf '%s' '{ "terminal.integrated.defaultProfile.windows": "PowerShell", "paddock.wsl.claudeLauncher": "off" }' > $H/.paddock/native/settings.json
cat > $H/bin/fake-claude.cs <<'CS'
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
public static class FakeClaude
{
    public static int Main(string[] args)
    {
        string resume = null;
        for (int i = 0; i < args.Length; i++)
        {
            if (args[i] == "--resume" && i + 1 < args.Length) { resume = args[++i]; }
            else if (args[i] == "--settings") { i++; }
        }
        string cfg = Environment.GetEnvironmentVariable("CLAUDE_CONFIG_DIR");
        if (string.IsNullOrEmpty(cfg)) { cfg = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".claude"); }
        string cwd = Directory.GetCurrentDirectory();
        StringBuilder project = new StringBuilder();
        foreach (char c in cwd) { project.Append(char.IsLetterOrDigit(c) && c < 128 ? c : '-'); }
        string sid;
        string transcript;
        if (resume != null)
        {
            sid = Path.GetFileNameWithoutExtension(resume);
            transcript = resume;
        }
        else
        {
            sid = Environment.GetEnvironmentVariable("FAKE_SESSION");
            if (string.IsNullOrEmpty(sid)) { sid = "aaaaaaaa-1111-4111-8111-000000000001"; }
            string folder = Path.Combine(cfg, "projects", project.ToString());
            Directory.CreateDirectory(folder);
            transcript = Path.Combine(folder, sid + ".jsonl");
            File.WriteAllText(transcript, "{}\n");
        }
        Directory.CreateDirectory(Path.Combine(cfg, "sessions"));
        int pid = Process.GetCurrentProcess().Id;
        long now = (long)(DateTime.UtcNow - new DateTime(1970, 1, 1)).TotalMilliseconds;
        File.WriteAllText(Path.Combine(cfg, "sessions", pid + ".json"),
            "{\"pid\":" + pid + ",\"sessionId\":\"" + sid + "\",\"kind\":\"interactive\",\"cwd\":\"" + cwd.Replace("\\", "\\\\") + "\",\"startedAt\":" + now + "}");
        // 데몬이 대화를 읽을 수 있게 기록 파일을 다 쓴 뒤에 화면에 찍는다(데몬은 출력이 있을 때 다시 조사한다).
        if (resume != null) { Console.WriteLine("RESUMED " + resume); } else { Console.WriteLine("STARTED " + sid); }
        Console.WriteLine("CFG " + cfg);
        string line;
        while ((line = Console.ReadLine()) != null)
        {
            if (line.Trim() == "delete-transcript") { File.Delete(transcript); Console.WriteLine("TRANSCRIPT-DELETED"); }
        }
        return 0;
    }
}
CS
SRC=$(wslpath -w $H/bin/fake-claude.cs); OUT=$(wslpath -w $H/bin/claude.exe)
powershell.exe -NoProfile -Command "Add-Type -TypeDefinition (Get-Content -Raw '$SRC') -OutputAssembly '$OUT' -OutputType ConsoleApplication" > /dev/null 2>&1
ls $H/bin/claude.exe > /dev/null
