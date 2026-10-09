# Paddock — run Claude Code and Codex in many repos from one window

Running Claude Code or Codex in several repos at once means tabbing through terminals to find the one that finished or is waiting for you. Paddock is a desktop terminal for macOS and Windows that lists every repo you are running an agent in, and plays a sound and marks a green dot when an agent you aren't watching stops.

![Paddock with three repos: shop-api and docs-site show green dots because their agents stopped (Quiet), web-app's agent is still printing (Output), and the file it just created, Cart.tsx, is open beside the terminal](docs/screenshot.png)

[Download](#download) · [How it works](#how-it-works) · [Privacy](#privacy-and-platforms)

## How it works

In a Paddock terminal, `cd` into a repo and run `claude` or `codex` the way you would anywhere else. Paddock puts that repo in the sidebar with its agent terminals: no project setup, no wrapper around the CLI. Open more terminals in the same repo with **＋** to run several agents there.

Agents are the official `claude` and `codex` CLIs running in ordinary terminals, signed in by you. Paddock never reads or stores your credentials.

| Compared with | What changes with Paddock |
| --- | --- |
| Terminal tabs, tmux | Agents are grouped by repo in one list, a stopped agent is marked for you, and the repo's changed files open in the same window. |
| Agent managers that create a worktree per task | Paddock creates no worktrees or branches. It shows the repos and terminals you already use. |
| An agent's own desktop app | Claude Code and Codex sit side by side, with your shell and files next to them. |

## What you get

- **Stopped agents find you.** When an agent you aren't watching has worked for a few seconds and then stops, a short sound plays and a green dot marks its repo and terminal. Open it and the dot clears. Paddock watches the terminal's output, so the dot means "stopped": read the last lines to see whether it finished or is waiting for your answer. Next to each agent, `Output` means it printed recently and `Quiet` means it hasn't.
- **Changes in the same window.** The repo's file tree marks modified files `M` and new files `U`; click one to open it beside the terminal. Source control shows the diff.
- **Usage limits in view.** Keep several Claude and Codex accounts, each signed in through its own CLI. After an account's first reply, its 5-hour and weekly usage shows in the status bar, and a new terminal on another account is one click away.
- **Your layout comes back.** Quit and reopen: repos, terminals, tab order and splits return. Agents stop when you quit Paddock; pick the conversation up again with `claude --resume` or `codex resume`.

## Privacy and platforms

- **No telemetry.** The only thing the app sends is the extension search you type, to [Open VSX](https://open-vsx.org). Commands you run and extensions you install can reach other services on their own.
- **macOS and Windows.** Apple Silicon and Intel Macs; Windows with PowerShell, Command Prompt, Git Bash and WSL shells. Linux has no installer yet; it runs from source.
- **Unsigned for now.** On macOS, Control-click the app and choose **Open**. On Windows, choose **More info → Run anyway** in SmartScreen.

## Why Paddock

Free, and it keeps your setup: the same CLIs, logins, skills and shell you use today. Version 0.1 — what is verified and what isn't yet is listed under [Current scope](#current-scope).

## Download

Get the installer for your system from [Releases](../../releases):

| System | File |
| --- | --- |
| macOS (Apple Silicon) | `Paddock-mac-arm64.dmg` |
| macOS (Intel) | `Paddock-mac-x64.dmg` |
| Windows | `Paddock-win-x64.exe` |

## Settings worth knowing

- **Agent permission prompts** (`paddock.agents.permissions`): how Claude and Codex started from **Accounts** handle permission prompts. `ask` (default) asks before each tool use. `allowSkip` lets Claude switch to skipping prompts inside the session with Shift+Tab. `skip` starts Claude with `--dangerously-skip-permissions` and Codex with `--dangerously-bypass-approvals-and-sandbox`, so agents change files and run commands without asking — use it only in folders you can restore. Agents you start by typing `claude` or `codex` follow their own settings.
- **Done sound** (`paddock.agentDoneSound`): turn the stop sound off.

Open settings with `Ctrl+,` (macOS `Cmd+,`) and search for `paddock`.

## Current scope

Verified: local terminals, text editing and saving, extension install from Open VSX or a `.vsix` file, and extension editors in file tabs. Not yet: code-signed installers, automatic updates, and screens to update or remove extensions. Not every VS Code extension is compatible. The license will be settled before the first public release.

## Build from source

Node.js 22 or later and a native build toolchain: Xcode Command Line Tools on macOS (`xcode-select --install`); Visual Studio Build Tools ("Desktop development with C++") and Python 3 on Windows. From the repository root:

```bash
npm run setup
npm run build
npm start                       # or: npm start -- /absolute/path/to/project
```

Installers are built on their own system because the terminal's native modules are compiled per OS. Output goes to `desktop/dist/`.

| System | Command | Output |
| --- | --- | --- |
| macOS | `make package-mac` | `Paddock-mac-<arm64 or x64>.dmg` |
| Windows | `make package-win` (or `npm run setup` then `npm run package:win`) | `Paddock-win-x64.exe` |

The **Package** GitHub Actions workflow builds all three installers. A `package/…` branch attaches them to the run as artifacts; `make release` tags `v<version>` from `desktop/package.json` and publishes them to a GitHub Release. Installed builds and development runs share `~/.paddock/extensions` and `~/.paddock/config`.

Product code lives in `desktop/`.
