// Paddock가 git.path 기본값으로 쓰는 git 분배기.
//
// 왜 있는가: Windows 앱의 Git 보기(VS Code 내장 git 확장)는 WSL 폴더(\\wsl.localhost\<배포판>\…)의 저장소도 Windows git으로 연다.
// 그러면 WSL 쪽 git이 커밋·gc로 index·pack 파일을 바꾸는 순간 Windows git이 9P 파일 시스템 위에서 파일 크기를 못 읽어
// "fatal: mmap: could not determine filesize"로 죽는다. 작업 폴더가 WSL이면 그 배포판 안의 git을 실행해 이 경쟁을 없앤다.
//
// 동작: 현재 폴더가 \\wsl.localhost\<배포판>\… 또는 \\wsl$\<배포판>\… 이면 `wsl.exe -d <배포판> --cd <현재 폴더> -e git …`을 실행한다.
// 아니면 진짜 Windows git에 인자 그대로 넘긴다. 어느 쪽이든 표준 입출력 핸들을 자식 프로세스에 직접 물려 주고(바이너리 안전),
// 종료 코드를 그대로 돌려준다. 이 프로세스가 강제 종료되면 자식도 함께 끝난다(작업 개체).
//
// 예외: 확장이 GIT_EDITOR·GIT_SEQUENCE_EDITOR로 Windows 노드 스크립트를 넘기는 호출(빈 입력란 커밋 등)은 WSL이 실행할 수 없어 Windows git으로 돌린다.
// 편집기 대화는 드물어 9P 경쟁 위험이 작다. `true` 같은 단순 명령(rebase 계속)은 WSL로 그대로 넘긴다.
// askpass 계열은 넘기지 않는다(WSL에서 동작할 수 없고, WSL 저장소의 인증은 WSL의 credential helper가 맡는다).
//
// 환경 변수
//   PADDOCK_WINDOWS_GIT        분배 대상 Windows git 경로. 없으면 기본 설치 위치(확장과 같은 순서)와 PATH(자기 폴더 제외)에서 찾는다.
//   PADDOCK_GIT_SHIM_TRACE     1이면 실행할 명령줄을 stderr에 찍고 실행한다. dry면 찍고 실행하지 않는다.
//
// 빌드: scripts/build-git-shim.cjs (csc.exe, C# 5). paddock-shell/bin/git.exe로 나온다.

using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace Paddock
{
    /// <summary>경로 변환과 분배 판정. 프로세스 실행과 분리한 순수 함수 모음.</summary>
    internal static class GitPaths
    {
        /// <summary>WSL 폴더를 Windows에서 가리키는 호스트 이름 두 가지.</summary>
        private static readonly string[] WslHosts = new string[] { "wsl.localhost", "wsl$" };

        private static bool IsSeparator(char c)
        {
            return c == '\\' || c == '/';
        }

        /// <summary>`\\wsl.localhost\Ubuntu\home\me` 같은 경로에서 배포판 이름과 Linux 경로를 꺼낸다. WSL 경로가 아니면 false.</summary>
        public static bool TryParseWslPath(
            string path,
            out string host,
            out string distro,
            out string linuxPath)
        {
            host = null;
            distro = null;
            linuxPath = null;
            bool matched = false;
            if (path != null && path.Length > 2 && IsSeparator(path[0]) && IsSeparator(path[1]))
            {
                int hostEnd = 2;
                while (hostEnd < path.Length && !IsSeparator(path[hostEnd]))
                {
                    hostEnd++;
                }
                string hostName = path.Substring(2, hostEnd - 2);
                bool isWslHost = false;
                foreach (string candidate in WslHosts)
                {
                    if (string.Equals(hostName, candidate, StringComparison.OrdinalIgnoreCase))
                    {
                        isWslHost = true;
                    }
                }
                if (isWslHost && hostEnd < path.Length)
                {
                    int distroEnd = hostEnd + 1;
                    while (distroEnd < path.Length && !IsSeparator(path[distroEnd]))
                    {
                        distroEnd++;
                    }
                    string name = path.Substring(hostEnd + 1, distroEnd - hostEnd - 1);
                    if (name.Length > 0)
                    {
                        host = hostName;
                        distro = name;
                        string rest = path.Substring(distroEnd).Replace('\\', '/');
                        linuxPath = rest.Length == 0 ? "/" : rest;
                        matched = true;
                    }
                }
            }
            return matched;
        }

        private static bool IsDrivePath(string text)
        {
            return text.Length >= 3
                && ((text[0] >= 'A' && text[0] <= 'Z') || (text[0] >= 'a' && text[0] <= 'z'))
                && text[1] == ':'
                && IsSeparator(text[2]);
        }

        /// <summary>인자(또는 `--옵션=값`의 값)가 Windows 경로면 Linux 경로로 바꾼다. 아니면 그대로.</summary>
        /// <remarks>
        /// | 입력                                  | distro(현재 폴더) | 결과                    |
        /// |---------------------------------------|-------------------|-------------------------|
        /// | \\wsl.localhost\Ubuntu\home\me\a.txt  | Ubuntu            | /home/me/a.txt          |
        /// | C:\Users\me\x.patch                   | Ubuntu            | /mnt/c/Users/me/x.patch |
        /// | --git-dir=C:/repo/.git                | Ubuntu            | --git-dir=/mnt/c/repo/.git |
        /// | \\wsl.localhost\Debian\a              | Ubuntu            | 그대로(다른 배포판)     |
        /// | status, -z, HEAD:C:x                  | Ubuntu            | 그대로                  |
        /// </remarks>
        public static string ToLinuxArgument(
            string argument,
            string distro)
        {
            string prefix = "";
            string value = argument;
            if (!IsDrivePath(value) && !value.StartsWith("\\\\") && !value.StartsWith("//"))
            {
                int equals = argument.IndexOf('=');
                if (equals > 0)
                {
                    prefix = argument.Substring(0, equals + 1);
                    value = argument.Substring(equals + 1);
                }
            }
            string converted = value;
            string pathHost;
            string pathDistro;
            string linuxPath;
            if (IsDrivePath(value))
            {
                converted = "/mnt/" + char.ToLowerInvariant(value[0]) + value.Substring(2).Replace('\\', '/');
            }
            else if (TryParseWslPath(value, out pathHost, out pathDistro, out linuxPath)
                && string.Equals(pathDistro, distro, StringComparison.OrdinalIgnoreCase))
            {
                converted = linuxPath;
            }
            else
            {
                prefix = "";
                converted = argument;
            }
            return prefix + converted;
        }

        /// <summary>git 출력에 든 Linux 절대 경로를 Windows git이 같은 저장소에서 내는 꼴로 되돌린다.</summary>
        /// <remarks>
        /// | 입력                | wslRoot                | 결과                                 |
        /// |---------------------|------------------------|--------------------------------------|
        /// | /home/me/repo       | //wsl.localhost/Ubuntu | //wsl.localhost/Ubuntu/home/me/repo  |
        /// | /home/me/repo       | //wsl$/Ubuntu          | //wsl$/Ubuntu/home/me/repo           |
        /// | /mnt/c/Users/me/r   | //wsl.localhost/Ubuntu | C:/Users/me/r                        |
        /// | .git                | //wsl.localhost/Ubuntu | .git (절대 경로가 아니면 그대로)     |
        /// wslRoot는 현재 폴더가 들어온 표기(`wsl.localhost` 또는 `wsl$`)를 그대로 따른다. Windows git이 같은 폴더에서 내는 꼴과 같게 하려는 것이다.
        /// </remarks>
        public static string ToWindowsPath(
            string linuxPath,
            string wslRoot)
        {
            string converted = linuxPath;
            if (linuxPath.StartsWith("/"))
            {
                bool isDrive = linuxPath.StartsWith("/mnt/")
                    && linuxPath.Length >= 6
                    && char.IsLetter(linuxPath[5])
                    && (linuxPath.Length == 6 || linuxPath[6] == '/');
                if (isDrive)
                {
                    string rest = linuxPath.Substring(6);
                    converted = char.ToUpperInvariant(linuxPath[5]) + ":" + (rest.Length == 0 ? "/" : rest);
                }
                else
                {
                    converted = wslRoot + linuxPath;
                }
            }
            return converted;
        }

        /// <summary>줄 단위 출력에서 경로로 된 줄을 Windows 꼴로 바꾼다. `worktree <경로>`처럼 접두가 붙은 줄도 다룬다.</summary>
        public static string ToWindowsOutput(
            string text,
            string wslRoot)
        {
            string[] lines = text.Split('\n');
            for (int i = 0; i < lines.Length; i++)
            {
                string line = lines[i];
                bool hasReturn = line.EndsWith("\r");
                string body = hasReturn ? line.Substring(0, line.Length - 1) : line;
                if (body.StartsWith("worktree /"))
                {
                    body = "worktree " + ToWindowsPath(body.Substring("worktree ".Length), wslRoot);
                }
                else
                {
                    body = ToWindowsPath(body, wslRoot);
                }
                lines[i] = hasReturn ? body + "\r" : body;
            }
            return string.Join("\n", lines);
        }

        /// <summary>GIT_EDITOR·GIT_SEQUENCE_EDITOR 값이 WSL 안에서도 같은 뜻인지. 비었거나 경로·공백 없는 단순 명령(true, cat, `:`)이면 참.</summary>
        /// <remarks>
        /// 확장은 rebase 계속에 `GIT_EDITOR=true`를, 빈 입력란 커밋에 확장 폴더의 Windows 노드 스크립트(`C:\…\git-editor.sh`)를 넘긴다.
        /// 앞은 WSL에서도 같으니 그대로 넘기고, 뒤는 WSL이 실행할 수 없으므로 그 호출만 Windows git으로 돌린다.
        /// | 값                      | 결과 |
        /// |-------------------------|------|
        /// | (없음)·true·:·cat       | 참   |
        /// | C:\x\git-editor.sh      | 거짓 |
        /// | node /x/editor.js       | 거짓 |
        /// </remarks>
        public static bool IsPortableEditor(string value)
        {
            bool portable = true;
            if (!string.IsNullOrEmpty(value) && value != ":")
            {
                foreach (char c in value)
                {
                    if (!(char.IsLetterOrDigit(c) || c == '_' || c == '-' || c == '.'))
                    {
                        portable = false;
                    }
                }
            }
            return portable;
        }

        /// <summary>편집기를 열 수 있는 서브커맨드. 이 명령만 편집기 변수가 실제로 쓰인다.</summary>
        private static readonly string[] EditorCommands = new string[]
        {
            "commit", "merge", "rebase", "tag", "cherry-pick", "revert", "am", "notes", "pull",
        };

        /// <summary>서브커맨드가 편집기를 열 수 있는 명령인지(커밋·병합·리베이스 등).</summary>
        /// <remarks>
        /// 확장은 편집기 변수(GIT_EDITOR에 확장 폴더의 Windows 스크립트)를 status·diff를 포함한 모든 git 호출에 붙인다.
        /// 편집기 변수만 보고 Windows git으로 돌리면 WSL 폴더의 모든 호출이 9P 위 Windows git으로 가서, 큰 저장소의 status가 수십 배 느려지고
        /// 분배기가 막으려던 파일 크기 경쟁("short read while indexing")도 그대로 생긴다. 그래서 편집기를 열 수 있는 명령만 Windows git으로 돌린다.
        /// | 인자                         | 결과 |
        /// |------------------------------|------|
        /// | status -z -uall              | 거짓 |
        /// | -c x=y commit --quiet        | 참   |
        /// | rebase --continue            | 참   |
        /// </remarks>
        public static bool OpensEditor(string[] args)
        {
            int at = FindSubcommand(args);
            bool opens = at >= 0 && Array.IndexOf(EditorCommands, args[at]) >= 0;
            return opens;
        }

        /// <summary>전역 옵션(-C·-c 등)을 건너뛰고 서브커맨드 위치를 찾는다. 없으면 -1.</summary>
        public static int FindSubcommand(string[] args)
        {
            int found = -1;
            int i = 0;
            while (found < 0 && i < args.Length)
            {
                string arg = args[i];
                if (arg == "-C" || arg == "-c" || arg == "--git-dir" || arg == "--work-tree" || arg == "--namespace" || arg == "--exec-path")
                {
                    i += 2;
                }
                else if (arg.StartsWith("-"))
                {
                    i += 1;
                }
                else
                {
                    found = i;
                }
            }
            return found;
        }

        /// <summary>출력에 Linux 절대 경로가 실리는 명령인지. 그런 명령만 stdout을 받아 경로를 되돌린다.</summary>
        public static bool PrintsPaths(string[] args)
        {
            int at = FindSubcommand(args);
            bool prints = false;
            if (at >= 0 && args[at] == "rev-parse")
            {
                foreach (string arg in args)
                {
                    if (arg == "--show-toplevel" || arg == "--git-dir" || arg == "--absolute-git-dir" || arg == "--git-common-dir"
                        || arg == "--show-superproject-working-tree" || arg == "--git-path" || arg == "--show-prefix-absolute")
                    {
                        prints = true;
                    }
                }
            }
            else if (at >= 0 && at + 1 < args.Length && args[at] == "worktree" && args[at + 1] == "list")
            {
                prints = true;
            }
            return prints;
        }

        /// <summary>CreateProcess용 명령줄 한 인자를 MS C 런타임 규칙으로 감싼다(CommandLineToArgvW와 왕복 동치).</summary>
        public static string QuoteArgument(string argument)
        {
            string quoted = argument;
            bool plain = argument.Length > 0 && argument.IndexOfAny(new char[] { ' ', '\t', '\n', '\v', '"' }) < 0;
            if (!plain)
            {
                StringBuilder builder = new StringBuilder("\"");
                int n_backslashes = 0;
                foreach (char c in argument)
                {
                    if (c == '\\')
                    {
                        n_backslashes++;
                    }
                    else if (c == '"')
                    {
                        builder.Append('\\', n_backslashes * 2 + 1);
                        builder.Append('"');
                        n_backslashes = 0;
                    }
                    else
                    {
                        builder.Append('\\', n_backslashes);
                        builder.Append(c);
                        n_backslashes = 0;
                    }
                }
                builder.Append('\\', n_backslashes * 2);
                builder.Append('"');
                quoted = builder.ToString();
            }
            return quoted;
        }

        public static string BuildCommandLine(
            string program,
            IEnumerable<string> args)
        {
            StringBuilder builder = new StringBuilder(QuoteArgument(program));
            foreach (string arg in args)
            {
                builder.Append(' ').Append(QuoteArgument(arg));
            }
            return builder.ToString();
        }
    }

    /// <summary>Windows 프로세스 생성·핸들 호출. 표준 입출력 핸들을 자식에게 그대로 물려 주려고 CreateProcess를 직접 부른다.</summary>
    internal static class Native
    {
        public const uint STARTF_USESTDHANDLES = 0x100;
        public const uint CREATE_UNICODE_ENVIRONMENT = 0x400;
        public const uint CREATE_NO_WINDOW = 0x08000000;
        public const uint HANDLE_FLAG_INHERIT = 1;
        public const uint INFINITE = 0xFFFFFFFF;
        public const int STD_INPUT_HANDLE = -10;
        public const int STD_OUTPUT_HANDLE = -11;
        public const int STD_ERROR_HANDLE = -12;
        public const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000;
        public const int JobObjectExtendedLimitInformation = 9;

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        public struct STARTUPINFO
        {
            public int cb;
            public string lpReserved;
            public string lpDesktop;
            public string lpTitle;
            public int dwX;
            public int dwY;
            public int dwXSize;
            public int dwYSize;
            public int dwXCountChars;
            public int dwYCountChars;
            public int dwFillAttribute;
            public uint dwFlags;
            public short wShowWindow;
            public short cbReserved2;
            public IntPtr lpReserved2;
            public IntPtr hStdInput;
            public IntPtr hStdOutput;
            public IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct PROCESS_INFORMATION
        {
            public IntPtr hProcess;
            public IntPtr hThread;
            public int dwProcessId;
            public int dwThreadId;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct SECURITY_ATTRIBUTES
        {
            public int nLength;
            public IntPtr lpSecurityDescriptor;
            [MarshalAs(UnmanagedType.Bool)]
            public bool bInheritHandle;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_BASIC_LIMIT_INFORMATION
        {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint PriorityClass;
            public uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct IO_COUNTERS
        {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
        {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CreateProcessW(
            string lpApplicationName,
            StringBuilder lpCommandLine,
            IntPtr lpProcessAttributes,
            IntPtr lpThreadAttributes,
            [MarshalAs(UnmanagedType.Bool)] bool bInheritHandles,
            uint dwCreationFlags,
            IntPtr lpEnvironment,
            string lpCurrentDirectory,
            ref STARTUPINFO lpStartupInfo,
            out PROCESS_INFORMATION lpProcessInformation);

        [DllImport("kernel32.dll")]
        public static extern IntPtr GetStdHandle(int nStdHandle);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetHandleInformation(
            IntPtr hObject,
            uint dwMask,
            uint dwFlags);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CreatePipe(
            out IntPtr hReadPipe,
            out IntPtr hWritePipe,
            ref SECURITY_ATTRIBUTES lpPipeAttributes,
            uint nSize);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool ReadFile(
            IntPtr hFile,
            byte[] lpBuffer,
            uint nNumberOfBytesToRead,
            out uint lpNumberOfBytesRead,
            IntPtr lpOverlapped);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool WriteFile(
            IntPtr hFile,
            byte[] lpBuffer,
            uint nNumberOfBytesToWrite,
            out uint lpNumberOfBytesWritten,
            IntPtr lpOverlapped);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern uint WaitForSingleObject(
            IntPtr hHandle,
            uint dwMilliseconds);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetExitCodeProcess(
            IntPtr hProcess,
            out uint lpExitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CloseHandle(IntPtr hObject);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        public static extern IntPtr CreateJobObjectW(
            IntPtr lpJobAttributes,
            string lpName);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetInformationJobObject(
            IntPtr hJob,
            int jobObjectInfoClass,
            ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION lpJobObjectInfo,
            uint cbJobObjectInfoLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool AssignProcessToJobObject(
            IntPtr hJob,
            IntPtr hProcess);
    }

    internal static class Program
    {
        private const int EXIT_NOT_FOUND = 127;

        /// <summary>WSLENV에 있어도 WSL로 넘기지 않는 변수. Windows 경로의 스크립트·Windows 프로세스 간 IPC를 가정한다. 편집기 변수는 값이 단순 명령일 때만 넘긴다(IsPortableEditor).</summary>
        private static readonly string[] NotForwarded = new string[]
        {
            "GIT_ASKPASS", "SSH_ASKPASS", "ELECTRON_RUN_AS_NODE",
        };

        /// <summary>실행할 프로그램과 인자. 분배 결과.</summary>
        private sealed class Plan
        {
            public string Program;
            public List<string> Args = new List<string>();
            public string WslDistro;
            public string WslRoot;
        }

        /// <summary>Windows git 위치. 환경 변수 → 기본 설치 위치 → PATH(자기 폴더 제외) 순. 못 찾으면 null.</summary>
        /// <remarks>기본 설치 위치의 순서는 내장 git 확장이 git을 찾는 순서와 같다(같은 git을 고르려는 것).</remarks>
        private static string FindWindowsGit()
        {
            string found = null;
            string configured = Environment.GetEnvironmentVariable("PADDOCK_WINDOWS_GIT");
            string selfDirectory = Path.GetDirectoryName(Path.GetFullPath(System.Reflection.Assembly.GetExecutingAssembly().Location));
            if (!string.IsNullOrEmpty(configured) && File.Exists(configured)
                && !string.Equals(Path.GetDirectoryName(Path.GetFullPath(configured)), selfDirectory, StringComparison.OrdinalIgnoreCase))
            {
                found = configured;
            }
            string localAppData = Environment.GetEnvironmentVariable("LOCALAPPDATA");
            string[] roots = new string[]
            {
                Environment.GetEnvironmentVariable("ProgramW6432"),
                Environment.GetEnvironmentVariable("ProgramFiles(x86)"),
                Environment.GetEnvironmentVariable("ProgramFiles"),
                string.IsNullOrEmpty(localAppData) ? null : Path.Combine(localAppData, "Programs"),
            };
            foreach (string root in roots)
            {
                if (found == null && !string.IsNullOrEmpty(root))
                {
                    string candidate = Path.Combine(root, "Git", "cmd", "git.exe");
                    if (File.Exists(candidate))
                    {
                        found = candidate;
                    }
                }
            }
            string pathVariable = Environment.GetEnvironmentVariable("PATH") ?? "";
            foreach (string entry in pathVariable.Split(Path.PathSeparator))
            {
                if (found == null && entry.Trim().Length > 0)
                {
                    try
                    {
                        string candidate = Path.Combine(entry.Trim().Trim('"'), "git.exe");
                        bool isSelf = string.Equals(Path.GetDirectoryName(Path.GetFullPath(candidate)), selfDirectory, StringComparison.OrdinalIgnoreCase);
                        if (!isSelf && File.Exists(candidate))
                        {
                            found = candidate;
                        }
                    }
                    catch (ArgumentException)
                    {
                    }
                }
            }
            return found;
        }

        /// <summary>현재 폴더와 인자로 실행 계획을 세운다. WSL 폴더면 wsl.exe, 아니면 Windows git.</summary>
        private static Plan BuildPlan(
            string currentDirectory,
            string[] args)
        {
            Plan plan = new Plan();
            string host;
            string distro;
            string linuxDirectory;
            // 배포판 공유가 열리지 않으면(등록 해제·이름 불일치) wsl.exe가 UTF-16 오류와 종료 코드 -1을 내므로 Windows git으로 돌린다.
            // 편집기를 열 수 있는 명령(커밋 등)이고 편집기 변수가 WSL에서 못 쓰는 값이면(확장의 Windows 노드 스크립트) 그 호출만 Windows git으로 돌린다.
            // 편집기 대화는 드물어 9P 경쟁 위험이 작다. status·diff처럼 편집기를 열지 않는 명령은 편집기 변수와 상관없이 WSL git이 실행한다(OpensEditor).
            bool hasPortableEditors = GitPaths.IsPortableEditor(Environment.GetEnvironmentVariable("GIT_EDITOR"))
                && GitPaths.IsPortableEditor(Environment.GetEnvironmentVariable("GIT_SEQUENCE_EDITOR"));
            if (GitPaths.TryParseWslPath(currentDirectory, out host, out distro, out linuxDirectory)
                && Directory.Exists("\\\\" + host + "\\" + distro)
                && (hasPortableEditors || !GitPaths.OpensEditor(args)))
            {
                plan.WslDistro = distro;
                plan.WslRoot = "//" + host + "/" + distro;
                if (!hasPortableEditors)
                {
                    // 편집기를 열지 않는 명령이지만, 만에 하나 편집기를 찾으면 WSL 기본 편집기가 입력을 기다리며 멈추지 않도록 바로 실패하는 명령으로 바꿔 넘긴다.
                    Environment.SetEnvironmentVariable("GIT_EDITOR", "false");
                    Environment.SetEnvironmentVariable("GIT_SEQUENCE_EDITOR", "false");
                }
                plan.Program = Path.Combine(Environment.GetEnvironmentVariable("SystemRoot") ?? "C:\\Windows", "System32", "wsl.exe");
                plan.Args.Add("-d");
                plan.Args.Add(distro);
                plan.Args.Add("--cd");
                plan.Args.Add(currentDirectory);
                plan.Args.Add("-e");
                plan.Args.Add("git");
                foreach (string arg in args)
                {
                    plan.Args.Add(GitPaths.ToLinuxArgument(arg, distro));
                }
            }
            else
            {
                plan.Program = FindWindowsGit();
                plan.Args.AddRange(args);
            }
            return plan;
        }

        /// <summary>wsl.exe가 WSL 안 git에 넘겨 줄 환경 변수를 WSLENV에 등록한다. askpass 계열은 Windows 경로·IPC를 가정해 WSL에서 동작하지 않으므로 넘기지 않는다.</summary>
        private static void ExportEnvironmentToWsl()
        {
            string[] names = new string[]
            {
                "GIT_OPTIONAL_LOCKS", "GIT_PAGER", "GIT_HTTP_USER_AGENT", "LC_ALL", "LANG", "VSCODE_GIT_COMMAND", "GIT_EDITOR", "GIT_SEQUENCE_EDITOR",
            };
            string[] pathNames = new string[]
            {
                "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY",
            };
            if (Environment.GetEnvironmentVariable("GIT_TERMINAL_PROMPT") == null)
            {
                Environment.SetEnvironmentVariable("GIT_TERMINAL_PROMPT", "0");
            }
            List<string> entries = new List<string>();
            string existing = Environment.GetEnvironmentVariable("WSLENV");
            if (!string.IsNullOrEmpty(existing))
            {
                // 사용자가 이미 둔 목록은 유지하되 askpass·편집기 계열은 뺀다(WSL에서 실행할 수 없는 Windows 스크립트를 가리킨다).
                foreach (string entry in existing.Split(':'))
                {
                    string name = entry.Split('/')[0];
                    if (entry.Length > 0 && Array.IndexOf(NotForwarded, name) < 0 && !name.StartsWith("VSCODE_GIT_"))
                    {
                        entries.Add(entry);
                    }
                }
            }
            entries.Add("GIT_TERMINAL_PROMPT");
            foreach (string name in names)
            {
                if (Environment.GetEnvironmentVariable(name) != null)
                {
                    entries.Add(name);
                }
            }
            foreach (string name in pathNames)
            {
                if (Environment.GetEnvironmentVariable(name) != null)
                {
                    entries.Add(name + "/p");
                }
            }
            Environment.SetEnvironmentVariable("WSLENV", string.Join(":", entries.ToArray()));
        }

        private static void MakeInheritable(IntPtr handle)
        {
            if (handle != IntPtr.Zero && handle != new IntPtr(-1))
            {
                Native.SetHandleInformation(handle, Native.HANDLE_FLAG_INHERIT, Native.HANDLE_FLAG_INHERIT);
            }
        }

        private static byte[] ReadAll(IntPtr pipe)
        {
            MemoryStream buffer = new MemoryStream();
            byte[] chunk = new byte[65536];
            uint n_read;
            while (Native.ReadFile(pipe, chunk, (uint)chunk.Length, out n_read, IntPtr.Zero) && n_read > 0)
            {
                buffer.Write(chunk, 0, (int)n_read);
            }
            return buffer.ToArray();
        }

        private static void WriteAll(
            IntPtr handle,
            byte[] data)
        {
            int offset = 0;
            while (offset < data.Length)
            {
                byte[] part = new byte[Math.Min(65536, data.Length - offset)];
                Array.Copy(data, offset, part, 0, part.Length);
                uint n_written;
                if (!Native.WriteFile(handle, part, (uint)part.Length, out n_written, IntPtr.Zero) || n_written == 0)
                {
                    break;
                }
                offset += (int)n_written;
            }
        }

        /// <summary>자식을 실행해 종료 코드를 돌려준다. capturePaths가 거짓이면 표준 입출력을 그대로 물려 준다.</summary>
        private static int Run(
            Plan plan,
            bool capturePaths)
        {
            Native.STARTUPINFO startup = new Native.STARTUPINFO();
            startup.cb = Marshal.SizeOf(typeof(Native.STARTUPINFO));
            startup.dwFlags = Native.STARTF_USESTDHANDLES;
            startup.hStdInput = Native.GetStdHandle(Native.STD_INPUT_HANDLE);
            startup.hStdOutput = Native.GetStdHandle(Native.STD_OUTPUT_HANDLE);
            startup.hStdError = Native.GetStdHandle(Native.STD_ERROR_HANDLE);
            IntPtr realStdout = startup.hStdOutput;
            IntPtr pipeRead = IntPtr.Zero;
            IntPtr pipeWrite = IntPtr.Zero;
            if (capturePaths)
            {
                Native.SECURITY_ATTRIBUTES attributes = new Native.SECURITY_ATTRIBUTES();
                attributes.nLength = Marshal.SizeOf(typeof(Native.SECURITY_ATTRIBUTES));
                attributes.bInheritHandle = true;
                Native.CreatePipe(out pipeRead, out pipeWrite, ref attributes, 0);
                Native.SetHandleInformation(pipeRead, Native.HANDLE_FLAG_INHERIT, 0);
                startup.hStdOutput = pipeWrite;
            }
            MakeInheritable(startup.hStdInput);
            MakeInheritable(startup.hStdOutput);
            MakeInheritable(startup.hStdError);

            IntPtr job = Native.CreateJobObjectW(IntPtr.Zero, null);
            Native.JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits = new Native.JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
            limits.BasicLimitInformation.LimitFlags = Native.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            Native.SetInformationJobObject(job, Native.JobObjectExtendedLimitInformation, ref limits, (uint)Marshal.SizeOf(typeof(Native.JOBOBJECT_EXTENDED_LIMIT_INFORMATION)));

            StringBuilder commandLine = new StringBuilder(GitPaths.BuildCommandLine(plan.Program, plan.Args));
            Native.PROCESS_INFORMATION info;
            int exitCode = 1;
            // 현재 폴더 인자를 null로 두면 자식이 UNC 현재 폴더를 그대로 이어받는다.
            bool started = Native.CreateProcessW(plan.Program, commandLine, IntPtr.Zero, IntPtr.Zero, true, Native.CREATE_UNICODE_ENVIRONMENT, IntPtr.Zero, null, ref startup, out info);
            if (capturePaths)
            {
                Native.CloseHandle(pipeWrite);
            }
            if (started)
            {
                Native.AssignProcessToJobObject(job, info.hProcess);
                if (capturePaths)
                {
                    byte[] raw = ReadAll(pipeRead);
                    string text = new UTF8Encoding(false).GetString(raw);
                    WriteAll(realStdout, new UTF8Encoding(false).GetBytes(GitPaths.ToWindowsOutput(text, plan.WslRoot)));
                }
                Native.WaitForSingleObject(info.hProcess, Native.INFINITE);
                uint code;
                Native.GetExitCodeProcess(info.hProcess, out code);
                exitCode = unchecked((int)code);
                Native.CloseHandle(info.hProcess);
                Native.CloseHandle(info.hThread);
            }
            else
            {
                Console.Error.WriteLine("paddock-git: cannot start " + plan.Program + " (error " + Marshal.GetLastWin32Error() + ")");
                exitCode = EXIT_NOT_FOUND;
            }
            if (capturePaths)
            {
                Native.CloseHandle(pipeRead);
            }
            Native.CloseHandle(job);
            return exitCode;
        }

        private static int Main(string[] args)
        {
            int exitCode;
            string currentDirectory = Directory.GetCurrentDirectory();
            Plan plan = BuildPlan(currentDirectory, args);
            string trace = Environment.GetEnvironmentVariable("PADDOCK_GIT_SHIM_TRACE");
            if (plan.Program == null)
            {
                Console.Error.WriteLine("paddock-git: Windows git not found. Install Git for Windows or set PADDOCK_WINDOWS_GIT.");
                exitCode = EXIT_NOT_FOUND;
            }
            else
            {
                if (plan.WslDistro != null)
                {
                    ExportEnvironmentToWsl();
                }
                if (!string.IsNullOrEmpty(trace))
                {
                    Console.Error.WriteLine("paddock-git: " + GitPaths.BuildCommandLine(plan.Program, plan.Args));
                }
                if (trace == "dry")
                {
                    exitCode = 0;
                }
                else
                {
                    bool capturePaths = plan.WslDistro != null && GitPaths.PrintsPaths(args);
                    exitCode = Run(plan, capturePaths);
                    if (plan.WslDistro != null && exitCode == -1)
                    {
                        // git은 -1(0xFFFFFFFF)로 끝나지 않는다. wsl.exe 자체가 실패한 경우다.
                        Console.Error.WriteLine("paddock-git: wsl.exe failed to run git in '" + plan.WslDistro + "' (exit -1)");
                        exitCode = 1;
                    }
                }
            }
            return exitCode;
        }
    }
}
