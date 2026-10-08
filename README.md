<div align="center">

# ☕ Token Café

**Your AI coding agents, as regulars at a pixel-art night café.**

Every Claude Code and Codex session running on your machine takes a seat.
They type while they work, nap while you're away, and raise a hand when they need you.

![Rust](https://img.shields.io/badge/Rust-2024-dea584?logo=rust&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-supported-3fb950?logo=linux&logoColor=white)
![macOS](https://img.shields.io/badge/macOS-help_wanted-8b949e?logo=apple&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-help_wanted-8b949e?logo=windows&logoColor=white)

<img src="docs/cafe.png" alt="The café at night: eight desks, patrons typing, thinking, sleeping and raising their hands, and a message window saying CHECKOUT-1C needs your OK" width="720">

</div>

> [!TIP]
> **macOS and Windows contributors welcome!** Token Café runs on Linux today. The window, the renderer and the usage counter are already cross-platform; what's missing is finding the agent sessions on your OS. [Here's what it takes.](#-macos-and-windows-contributors-welcome)

## What you're looking at

Each session is a patron with their own look (skin, hair, shirt, hairstyle, all derived from the session id, so they're the same person every time). What they're doing tells you what your agent is doing:

| Your agent is… | The patron… |
|---|---|
| **Working** | types away; the laptop lid shows Claude's spinning asterisk or Codex's blinking prompt, keys fly off the keyboard, and a `…` thought bubble pops up now and then |
| **Idle** | stretches, fetches a coffee after 2 minutes, and falls asleep on a closed laptop after 10 |
| **Waiting for you** | raises a hand: `!` for a permission prompt, `?` for a question. The nameplate blinks and the message window calls them out by name, cycling through everyone waiting |

New sessions walk in from the door, finished ones walk out, and a ninth waits outside until a seat frees up.

<div align="center">
<img src="docs/arrivals.gif" alt="Three new patrons walk in from the door and take their seats while one leaves" width="640">
</div>

Up on the wall, **the tab** keeps a running total for the calendar month: tokens used and what they'd cost at the vendors' list prices, Claude and Codex together. (If you're on a subscription, that's a measure of how much you've used, not a bill.) When nobody needs you, the message window shows who's working, who's on a break, and the time.

<div align="center">
<img src="docs/night.png" alt="A quiet night at 03:12: one patron working, two asleep, one with a coffee" width="640">
<br><sub>3 a.m.: one agent still working, the rest have dozed off.</sub>
</div>

## Getting started

You need [Rust](https://rustup.rs) 1.88 or newer and a C compiler (SQLite is built in), on a Wayland or X11 desktop. From a clone of this repository:

```sh
cargo build --release
./target/release/token-cafe
```

| Command | |
|---|---|
| `token-cafe` | the live café |
| `token-cafe --demo` | a full house showing every pose, no agents needed |
| `token-cafe --print` | list the live sessions and this month's usage in the terminal, then exit |
| `token-cafe --scale N` | open at N × 320×240 (default 3) |

| Key | |
|---|---|
| `1` – `6` | resize the window to that many times 320×240 |
| `f` / `F11` | fullscreen (nice on a spare monitor) |
| `Esc` | leave fullscreen |
| `q` | quit |

The café is drawn at 320×240 and scaled by the largest whole number that fits, so the pixels stay crisp at any window size; the rest is letterboxed. Only what changes on screen is repainted, so a live café costs about 1% of a CPU core.

> [!NOTE]
> On a tiling window manager the window gets tiled like any other. Its Wayland app id and X11 class are both `token-cafe`, so you can write a window rule to float or pin it.

## How it knows

Everything is read locally; nothing leaves your machine. Token Café only reads files, and writes one cache file.

| | Where it looks |
|---|---|
| **Claude Code** sessions | `~/.claude/sessions/<pid>.json`: one per interactive session, with its status and what it's waiting for. A session counts while its process is alive |
| **Codex** sessions | running `codex` processes → the rollout log each one has open (or its newest thread in `~/.codex/state_5.sqlite`) → whether the last task started or finished |
| **The tab** | every transcript under `~/.claude/projects/` and `~/.codex/sessions/` written this month; each API response carries its token counts |
| **Cache** | `~/.cache/token-cafe/usage.json`, so only changed transcripts are read again |

`CLAUDE_CONFIG_DIR` and `CODEX_HOME` are honoured. Codex doesn't log its approval prompts, so Codex patrons never raise a hand.

## Under the hood

```
design/cafe.js          the art and the scene's behaviour, as pixel strings and plain JS
tools/gen_assets.mjs    turns it into src/assets.rs and src/bg.rgb
src/scene.rs            the café: seats, walking, poses, particles, message window, the tab
src/gfx.rs              rectangles, shadows, palette sprites, a 5×7 font
src/collect/            which sessions are live (procfs.rs is the only Linux-specific file)
src/usage.rs            this month's tokens and list-price dollars
src/app.rs              the window: winit + softbuffer, whole-number scaling, damage tracking
examples/screenshots.rs renders the images in this README
```

Changing the art means editing `design/cafe.js`, then:

```sh
node tools/gen_assets.mjs                          # regenerate the sprites and the room
cargo run --release --example screenshots          # refresh docs/*.png and docs/arrivals.gif
```

## 🍎🪟 macOS and Windows contributors welcome

The café itself already runs anywhere: the renderer is plain Rust, and the window uses [winit](https://github.com/rust-windowing/winit) and [softbuffer](https://github.com/rust-windowing/softbuffer). What's Linux-only is how the live sessions are found, plus a few small assumptions. If you're on a Mac or a Windows machine, here's the list:

- [ ] **Make it compile.** `src/app.rs` uses `WindowAttributesExtWayland` to set the app id; that needs a `cfg` gate for non-Linux targets.
- [ ] **Process information.** `src/collect/procfs.rs` reads `/proc` for a process's start time, executable, arguments, working directory and open files. A macOS (`libproc`) or Windows version, or a switch to the [`sysinfo`](https://crates.io/crates/sysinfo) crate, would cover most of it.
- [ ] **Telling live Claude sessions from stale ones.** Session files carry a `procStart` that is Linux clock ticks; on other systems, check what Claude Code writes there, or compare the process's start time with the file's `startedAt`.
- [ ] **Finding each Codex session's log.** On Linux it's the rollout file the process has open. Without that, the existing fallback (the newest thread for the process's working directory in `state_5.sqlite`) may already be enough. The executable is `codex.exe` on Windows.
- [ ] **Paths.** `$HOME` and `$XDG_CACHE_HOME` are used directly; Windows wants `%USERPROFILE%` and a proper cache directory (the [`dirs`](https://crates.io/crates/dirs) crate).
- [ ] **Check the session files exist where expected.** `~/.claude/sessions/`, `~/.codex/sessions/` and `~/.codex/state_5.sqlite` have only been seen on Linux so far.

`token-cafe --print` is the quickest way to see what the collector finds on your machine, and `--demo` to see the window without any agents. Pull requests and issues with what you find are very welcome, even partial ones.

## Credits

The café started life on a 2.8″ ESP32-S3 desk display, as a little JRPG-style net café that sat next to the keyboard. This app brings the same patrons to your desktop.
