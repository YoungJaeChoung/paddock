# Paddock

**Know when a Claude Code or Codex agent stops — in any of your repos.**

Run agents in several repos and one of them is always sitting idle, waiting for you. Paddock is a terminal app that lists every repo's agents in one window and plays a sound when one you aren't watching stops.

**[Download for macOS or Windows](#install)** · Free app, MIT license

![Paddock's sidebar: shop-api's Claude and docs-site's Codex have stopped and show green dots; web-app's Claude is still working](docs/stopped-agents.png)

## Why Paddock

- **Stopped agents find you.** A sound plays and a green dot marks the repo and terminal. No hooks to write, for Claude Code and Codex alike. ([How it decides](docs/settings.md#stop-alerts): the output goes quiet, so a silent build step counts too.)
- **Nothing to set up.** Open a terminal in Paddock, `cd` into a repo and run `claude` or `codex` as usual. The repo joins the list. Your CLIs, logins and skills stay as they are; no worktrees, no wrapper.

![One window: repos and agents on the left, the agent's terminal in the middle, the file it just wrote on the right, and each account's usage at the bottom](docs/screenshot.png)
*Also in one window: the files each agent changed, and every Claude and Codex account's 5-hour and weekly usage.*

**Compared with tmux:** tmux keeps agents running after you close it, and alerts take your own scripts. Paddock alerts out of the box, but agents stop when you quit it (resume with `claude --resume` or `codex resume`).

**Private:** no telemetry. The app only sends the extension searches you type to [Open VSX](https://open-vsx.org), and never reads or stores your credentials; agents are the official CLIs, signed in by you.

## Install

| System | File |
| --- | --- |
| macOS (Apple Silicon) | [Paddock-mac-arm64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-arm64.dmg) |
| macOS (Intel) | [Paddock-mac-x64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-x64.dmg) |
| Windows, including WSL | [Paddock-win-x64.exe](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-win-x64.exe) |

Linux: no installer yet; [build from source](docs/building.md).

The installers are built by the public [Package workflow](https://github.com/YoungJaeChoung/paddock/actions/workflows/package.yml) but aren't code-signed yet. Compare the file with `SHA256SUMS.txt` on the [release page](https://github.com/YoungJaeChoung/paddock/releases/latest) (`shasum -a 256 <file>` on macOS, `Get-FileHash <file>` on Windows), then:

- **macOS 15 or later:** open the app once, then choose **Open Anyway** in System Settings → Privacy & Security. Earlier macOS: Control-click the app and choose **Open**.
- **Windows:** in SmartScreen, choose **More info → Run anyway**.

[Settings and details](docs/settings.md) · [Build from source](docs/building.md) · [MIT License](LICENSE)
