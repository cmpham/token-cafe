# Linux desktop support — 2026-11-02

The core desktop visualizer supports Linux X11/Wayland and macOS. Pipar keypad transport/OLED updates, focused-app native dictation, and desktop-session focusing are macOS-only. Linux physical board/Bluetooth and touch-device operation have not been verified.

## Defects found and corrected

The review of master `64335f9` found that the default Linux build succeeded and all 235 Rust tests passed, but strict Clippy failed on 14 unused library items from the macOS keypad integration. Its Ubuntu CI job reproduced the failure and skipped the subsequent X11 smoke test. The README also omitted native Linux dependencies and did not include keypad/dictation in its platform matrix. Linux's title misleadingly requested a USB keypad connection even though its keypad startup path was a no-op.

The change gates the native keypad runtime and its quota worker to macOS. Portable keypad routing, OLED presentation and account/parser tests still compile and run on Linux. Standalone read-only CLI quota diagnosis remains available in Bluetooth-enabled builds. Unsupported platforms report `Keypad: macOS only` rather than requesting USB; a Linux regression test exercises that status without opening hardware or provider apps.

The README now lists Debian/Ubuntu build dependencies, the optional `--no-default-features` desktop build, and the platform boundaries. CI runs workspace tests both with and without companion support, plus the portable OLED parser and macOS native helper tests. Matrix jobs finish independently so one failure does not cancel the other platform's results.

## Local validation of the change

| Check | Linux ARM64, Rust 1.88.0 | macOS ARM64 |
| --- | --- | --- |
| Workspace/all-target tests | 236 passed | 239 passed |
| Tests without default features | 192 passed | 196 passed |
| Strict workspace/all-target Clippy | Passed | Passed |
| Default desktop build | Passed | Covered by tests/Clippy; bundle build runs in CI |
| Keypad/native helpers | Portable Rust tests retained | 50 keypad, 16 recording, 19 quota storage checks passed; sanitized C OLED parser passed |

Linux validation used a disposable Debian Bookworm container with tracked source mounted read-only, a separate dependency/build cache, and no host credentials, devices or host D-Bus. Dependencies were fetched in a separate phase; test/runtime checks ran without network access. The Linux toolchain matches the README's minimum Rust version.

Additional Linux runtime checks passed:

- Multiple Claude profile discovery, environment overrides, symlink deduplication, transcript isolation and closed-process removal, using the existing `tools/test_claude_accounts.py` credential-free fixture.
- Actual `/proc` discovery of synthetic live Claude/Codex processes: busy/waiting/idle/completed states, open rollout detection, stale process-start rejection and removal after exit. No real provider login was used.
- X11 under Xvfb: window class, pixel presentation, keyboard roster change, selection/scroll/Escape/pin-toggle no-crash checks and clean keyboard quit. Fictional-session screenshots were captured.
- Wayland under Weston headless: native surface commits, `token-cafe` app ID, five-second render loop and the explicit `Pin via compositor` title. No native pinning support is claimed.

Formatting and diff whitespace checks passed. Evidence is local and ignored under `target/linux-support-audit/` in the primary checkout; `pr-results/` contains this change's Linux logs and captures. The earlier incomplete offline-cache attempt is excluded from validation.

These checks do not establish physical touch/Bluetooth support, optical display behavior, long-running stability, every Linux distribution/window manager, or a new CPU/performance claim. The installed macOS application and devices were not replaced for this change.
