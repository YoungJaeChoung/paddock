# Paddock

**Every repo's Claude Code and Codex agents in one window, with a sound the moment one stops.**

Run agents in several repos and you only learn one has finished when you tab through terminals to find it, and it sat idle until then. Paddock is a terminal app that lists every repo's agents in one window and tells you. When a Claude account runs low, switch accounts in the same tab and the conversation continues.

**[Download for macOS or Windows](#install)** · Free app, MIT license

[![One window: repos and agents on the left, the agent's terminal in the middle, the file it just wrote on the right, and each account's usage at the bottom](docs/paddock-demo.gif)](https://youtu.be/f1G04Br991g)

*Click the demo to watch it with sound. The demo and screenshots show example projects, sample usage numbers and scripted agents; the demo's alert sound and music were added in editing.*

## Why Paddock

### Every agent in one list, with a sound when one stops

When an agent you aren't watching stops, **a sound plays and a green dot marks its repo and terminal**. Run `claude` or `codex` in a Paddock terminal as usual and its repo joins the sidebar. ([How a stop is detected](docs/settings.md#stop-alerts): the output goes quiet, so a silent build counts too.)

![Paddock's sidebar: shop-api's Claude and docs-site's Codex have stopped and show green dots; web-app's Claude is still working](docs/stopped-agents.png)

### Open the files they changed

Changed files are **marked M or U in the file tree and open beside the terminal**, so you check the work and type the next instruction without leaving the window.

![Changed files marked M and U in the file tree, with Cart.tsx open beside the agent's terminal](docs/files-changed.png)

### Switch Claude accounts and keep the conversation

Keep several Claude and Codex accounts, each signed in through its own CLI. **Each account's 5-hour and weekly usage** shows in the status bar once an agent on it has replied, as its CLI reports it. When a Claude account runs low, **pick another account from the account button at the top right and the same tab continues the same conversation under it**, in the same folder and layout. The conversation so far is reloaded; a request Claude was still answering when you switched has to be sent again. A Codex account opens a new terminal in the same folder; `codex resume` reopens the old conversation.

![Top: Claude · Personal is at 91% of its 5-hour usage and the Accounts menu offers Claude · Work to continue this conversation. Bottom: the same tab now runs under Work with the same conversation and 12% used](docs/accounts-continue.png)

![The status bar shows each Claude and Codex account's 5-hour and weekly usage](docs/accounts-usage.png)

**Memory** in the status bar warns once it passes 90%, naming the terminal using the most, so you can stop it before WSL or Windows runs out. It only warns; nothing is stopped for you. Turn on [Memory notice](docs/settings.md#telling-agents-when-memory-is-low) and Claude Code agents are told too, so they hold off on heavy builds.

No project setup: your logins and skills stay as they are.

**vs. other tools:** no worktrees to manage, Claude Code and Codex side by side, macOS and Windows alike.

**vs. tmux:** Paddock alerts without scripts or key bindings; tmux keeps agents running after you close it, Paddock's stop when you quit (resume with `claude --resume` or `codex resume`).

**Private:** Paddock collects nothing: no telemetry, no account, no server of its own, so your code and conversations are never used to train an AI. Its only outside connections are [Open VSX](https://open-vsx.org), to look up extensions, and Google, to download a spell-check dictionary; neither is sent your code, terminals or conversations. It never stores or sends your credentials; to tell accounts apart it only reads the account id from each CLI's own login file. Agents are the official CLIs, signed in by you; what they send follows Claude's or Codex's own terms.

## Install

| System | File |
| --- | --- |
| macOS (Apple Silicon) | [Paddock-mac-arm64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-arm64.dmg) |
| macOS (Intel) | [Paddock-mac-x64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-x64.dmg) |
| Windows, including WSL | [Paddock-win-x64.exe](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-win-x64.exe) |

Linux: no installer yet; [build from source](docs/building.md).

The installers are built by the public [Package workflow](https://github.com/YoungJaeChoung/paddock/actions/workflows/package.yml) but aren't code-signed yet, so your system warns on first run:

- **macOS 15 or later:** open the app once, then choose **Open Anyway** in System Settings → Privacy & Security. Earlier macOS: Control-click the app and choose **Open**.
- **Windows:** in SmartScreen, choose **More info → Run anyway**.

[Settings and details](docs/settings.md) · [Build from source](docs/building.md) · [MIT License](LICENSE)
