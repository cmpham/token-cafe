# Token Café contributor guide

Token Café shows live Claude Code and Codex sessions as patrons of a pixel-art café. Linux and macOS are supported; Windows process discovery is not implemented. The Mac is the authority for the optional Pipar Flake keypad and ESP32-S3 display.

## Build and validate

```sh
cargo build --release --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
bash tools/test_keypad.sh  # sanitized C OLED parser; on macOS, Swift keypad/recording checks
sh tools/build_macos.sh   # macOS app + native helpers, local ad-hoc signing
```

Use workspace tests: plain `cargo test` skips workspace member crates. Run checks appropriate to the change. Native tests do not interact with running apps or prove physical recording/firmware behavior.

On this managed Mac, the installed Rust toolchain uses `CARGO_HOME=/tmp/token-cafe-cargo`, `RUSTUP_HOME=/tmp/token-cafe-rustup`, and `/tmp/token-cafe-cargo/bin` on `PATH`; these are local paths, not project requirements.

Every worktree publishes to the primary repository's `release/Token Café.app`. Launch that stable bundle for Finder, Dock and macOS permissions; do not launch a competing copy from a temporary build directory. Rebuilds are not notarized distribution releases and macOS grants may need rechecking. `bash tools/run_companion.sh` launches the same app with the provisioned board registry when present. Run one app/keypad helper at a time. Keep the primary checkout on `master` for this repository; Android work belongs on its separate branch.

## Running and controls

- `target/release/token-cafe`: live; `--demo`: fictional sessions; `--print`: live sessions and monthly totals; `--usage-json`: recorded usage breakdown.
- `--weekly-limits`: read-only keypad quota diagnosis (Bluetooth build); status/percentages only, no app window or device ownership.
- `--session-info [--watch]`: bounded local task/activity evidence for verified live sessions; output can contain private requests and paths.
- `--scale N`: initial size (macOS default 1, elsewhere 3); `--always-on-top`: start pinned. Keys 1–6 resize, F/F11 fullscreen, T pin, Q quit.
- Click a patron or bottom summary to focus; click the selected patron/empty desk to clear. Tab, Left/Right and the main banner include overflow sessions. Up/Down, wheel and scrollbar page task text. Escape exits fullscreen or clears focus.
- P cycles rosters; C chooses a character seat; [ / ] change its character. U opens monthly usage; A changes app/recorded-organization breakdown.
- macOS: identified iTerm Claude CLI single-click reveals the existing pane; other supported desktop sessions open on double-click. Never guess a terminal or session by project path.

## Current behavior to preserve

- Eight seats, Claude on the left and Codex on the right. Active/waiting patrons have seating priority over idle patrons on their own side. Extra sessions wait outside. Selection follows session identity through swaps and refreshes.
- One gold outline marks focus, including during walking. Attention uses badges/footer, not a second focus style. No timer rotates the selected patron or footer. New waiting prompts pulse for 30 seconds, then keep a reminder until resolved; acknowledgement does not approve.
- Idle patrons stretch/sip, fetch coffee after two minutes, sleep after ten. Session state age survives app restarts. Initial sessions seat immediately; subsequent arrivals/departures walk.
- Keypad knob browses **only the eight visible patrons**, in seat order, including when leaving a details page. Press once for details and to focus the identified desktop session. Details return to the café after 30 seconds without navigation input. The OLED keeps navigation visible for five seconds after input.
- Bottom-left mic starts native recording in the **currently focused Codex or Claude window/session**, independent of café selection. It must not activate an app, open a session, use a global voice shortcut, or submit text. Switching app/window/exposed session context cancels a pending request.
- `voice-focused` uses the uniquely identified recording control, validates its hit-test, and posts one click. A rejected hit-test sends no event and may retry within the same request; never retry a posted click. Codex confirms **Stop dictation**; Claude confirms click dispatch only. One-press behavior was physically confirmed for both apps. Requires macOS Accessibility permission.
- Idle OLED shows active (working/waiting) counts, including overflow, and remaining weekly allowance. Separate Claude CLI/Desktop account identities use account UUID + organization UUID, never email; two unlabeled percentages appear CLI above Desktop. Missing/stale data displays `--`. Poll quotas every five minutes, re-read existing credentials every minute, expire successful data after ten minutes. Honor provider backoff and stop retrying rejected bearers until their owner replaces them. Keep both known Claude account slots through temporary failures. Poll Codex independently. `@TC3I` is single-account; `@TC4I` is dual-account. Matching keypad firmware is required.

## Source map

| Location | Responsibility |
| --- | --- |
| `design/cafe.js`, `tools/gen_assets.mjs` | Sprite/background source and generated `crates/cafe-ui/src/assets.rs`, `bg.rgb` |
| `crates/cafe-ui` | Shared renderer, 480×272 logical canvas, desktop detail rendering, scene/input/attention |
| `src/app.rs`, `src/main.rs` | winit/softbuffer window, integer scaling, damage tracking, collectors |
| `src/collect/` | Linux procfs/macOS libproc + sysctl discovery; bounded task evidence in `info.rs` |
| `src/usage.rs` | Monthly local transcript accounting, API list-price estimates, versioned cache |
| `crates/companion-core` | Bounded Protobuf state/actions, authentication, reconciliation, gestures and approval guards |
| `src/companion/keypad/` | Mac-owned keypad routing, OLED presentation and weekly quota polling |
| `native/keypad/`, `native/desktop/`, `native/quota/` | macOS keypad bridge, focused recording and read-only Desktop quota credentials |
| `firmware/keypad/` | Pipar nRF52840 firmware and OLED rendering |
| `firmware/engine`, `firmware/semantic`, `firmware/v2` | Shared board engine, physical BLE/LCD/touch/NVS adapter, QEMU adapter |
| `examples/screenshots.rs` | Reproducible README screenshots/GIFs with fictional sessions |

## Collection and privacy rules

Claude live PID/status records must match process identity; desktop metadata enriches a live Code-tab session and never establishes liveness. Ordinary Claude Chat/Cowork/cloud history is not collected. Codex desktop gets one patron per open root rollout, excludes archived/subagent-only logs, and deduplicates CLI references. Saved sidebar history alone is not live. Root task events determine state; old subagent activity cannot keep a completed root busy.

Respect `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, and `TOKEN_CAFE_CLAUDE_CONFIG_DIRS`; keep profile transcripts isolated and deduplicate path aliases. Claude identity uses account and organization UUIDs. Current CLI login is a hint, not historical billing proof. Preserve unknown/unassigned attribution rather than guessing. Monthly dollars are API list-price estimates, not subscription quotas.

Task evidence is bounded and incremental. Preserve timestamps/sources, `history_partial`, and the distinction between observed tool calls, returned results and explicit failures. Do not expose raw tool arguments, outputs or reasoning in the café.

The weekly quota poller is separate from local collection: it reads existing Claude credentials (CLI Keychain on macOS; Desktop's encrypted OAuth cache via its Safe Storage Keychain item, with verified launch credentials as fallback), makes read-only quota/profile requests, and uses Codex's `account/rateLimits/read`. Never log/save tokens, refresh credentials, change logins, or include secrets in process arguments. Normal Desktop quota polls must not show Keychain dialogs; the explicit `keypad-quota authorize` command handles the macOS grant. Verify cached account/org against the API profile before reading usage, and fail closed on ambiguous organizations. Keep registries, pairing keys, macros and real session captures out of Git. Tests and previews use fictional names.

## Hardware changes

Read [keypad setup/recovery](docs/pipar-keypad.md), [architecture](docs/keypad-architecture-review.md), and [board operations](docs/protocol/operations.md) before device work. Preserve the simulation-before-hardware gate; fixtures must never dispatch real approvals.

The keypad connects directly to the Mac, USB preferred with Bluetooth fallback. The optional board independently connects to the Mac; the old keypad-to-board route is historical. Only one helper owns the keypad. Keypad controls never approve/deny requests.

Renderer digest changes require rebuilding the shared engine and matching semantic board firmware. `bash tools/qemu_v2.sh board-build` builds the current physical board; legacy `tools/firmware.sh display-build` is incompatible with semantic-v2. Preserve pairing NVS during application-only flashing. Mac-only recording changes need no device firmware update.

Keypad firmware: `bash tools/build_keypad.sh` uses pinned dependencies/Docker. Verify device identity before flashing; the nRF52840 keypad and ESP32 board are different devices. Back up/read back both layers, encoder bindings and macros. A zero uploader exit status is insufficient: require `Device programmed.`, normal device enumeration and unchanged settings. Report transfer, transport and physical behavior separately.

## Documentation and media

[README](README.md) is the user overview. Current renderer previews: [café](docs/cafe.png), [mixed roster](docs/cafe-mixed.gif), [selection](docs/focus.gif), [attention](docs/attention.gif). These are rendered fixtures, not physical hardware recordings.

After editing art, run `node tools/gen_assets.mjs`; do not hand-edit generated assets. Regenerate media from current code:

```sh
cargo run --release --locked --example screenshots
cargo run --release --locked --example screenshots -- --patrons
cargo run --release --locked --example screenshots -- --focus
cargo run --release --locked --example screenshots -- --attention
```

Inspect generated PNGs and representative GIF frames before publishing. Keep real session names out of tracked media. See [visual QA](docs/qa/REVIEW.md), [protocol](docs/protocol/companion-v2.md), and [hardware recreation](docs/hardware/recreate-approval-controller.md) for detailed evidence and procedures.

For measured renderer/host changes, run `cargo run --release --locked --example ui_performance` with fictional fixtures and compare allocation counts before/after. The [2026-11-01 cleanup review](docs/qa/performance-cleanup-2026-11-01.md) records scope, retained compatibility paths, memory tradeoffs and simulator validation.

The [2026-11-02 quota recovery](docs/qa/weekly-quota-recovery-2026-11-02.md) records expired-credential handling, Desktop Keychain authorization, native crypto/identity tests and the paired app/board reload.
