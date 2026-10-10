# Paddock

**Every repo's Claude Code and Codex agents in one window, with a sound the moment one stops.**

Run agents in several repos and you only learn one has finished when you tab through terminals to find it, and it sat idle until then. Paddock is a terminal app that lists every repo's agents in one window and tells you.

**[Download for macOS or Windows](#install)** · Free app, MIT license

![One window: repos and agents on the left, the agent's terminal in the middle, the file it just wrote on the right, and each account's usage at the bottom](docs/screenshot.png)

## Why Paddock

### Every agent in one list, with a sound when one stops

When an agent you aren't watching stops, **a sound plays and a green dot marks its repo and terminal**, so none sits idle unnoticed. Open a terminal in Paddock, `cd` into a repo and run `claude` or `codex` as usual; the repo joins the sidebar with its agent terminals. ([How it decides](docs/settings.md#stop-alerts): the output goes quiet, so a silent build step counts too.)

![Paddock's sidebar: shop-api's Claude and docs-site's Codex have stopped and show green dots; web-app's Claude is still working](docs/stopped-agents.png)

### Open the files they changed

Changed files are **marked M or U in the file tree and open beside the terminal**, so you check the work and type the next instruction without leaving the window.

![Changed files marked M and U in the file tree, with Cart.tsx open beside the agent's terminal](docs/files-changed.png)

### Several accounts in one place

Keep several Claude and Codex accounts, each signed in through its own CLI. **Each account's 5-hour and weekly usage** shows in the status bar once an agent on it has replied, as its CLI reports it; when one runs low, start the next agent from another.

![The status bar shows each Claude and Codex account's 5-hour and weekly usage; the Accounts menu opens an agent on another account](docs/accounts-usage.png)

Nothing to set up: your CLIs, logins and skills stay as they are. No worktrees, no wrapper.

**Compared with tmux:** Paddock alerts out of the box and lists every repo's agents without scripts or key bindings. tmux keeps agents running after you close it, while Paddock's agents stop when you quit (resume with `claude --resume` or `codex resume`).

**Private:** no telemetry. The app only sends the extension searches you type to [Open VSX](https://open-vsx.org), and never reads or stores your credentials; agents are the official CLIs, signed in by you.

## Install

| System | File |
| --- | --- |
| macOS (Apple Silicon) | [Paddock-mac-arm64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-arm64.dmg) |
| macOS (Intel) | [Paddock-mac-x64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-x64.dmg) |
| Windows, including WSL | [Paddock-win-x64.exe](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-win-x64.exe) |

Linux: no installer yet; [build from source](docs/building.md).

The installers are built by the public [Package workflow](https://github.com/YoungJaeChoung/paddock/actions/workflows/package.yml) but aren't code-signed yet. Compare the file with the SHA-256 shown for it on the [release page](https://github.com/YoungJaeChoung/paddock/releases/latest) (`shasum -a 256 <file>` on macOS, `Get-FileHash <file>` on Windows), then:

- **macOS 15 or later:** open the app once, then choose **Open Anyway** in System Settings → Privacy & Security. Earlier macOS: Control-click the app and choose **Open**.
- **Windows:** in SmartScreen, choose **More info → Run anyway**.

[Settings and details](docs/settings.md) · [Build from source](docs/building.md) · [MIT License](LICENSE)
