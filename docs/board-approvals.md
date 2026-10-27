# Board permission review

## Supported behavior

Tap a waiting patron, then tap VIEW in its bottom preview to open the full question. The review screen has a 44px bottom action row and a right-side page rail. Text preserves command case, punctuation and line breaks; non-ASCII/control characters are shown as explicit Unicode escapes. Tap the upper/lower rail to page. Double-tap the question body, or tap Desktop, to hand off to that session and close the review. Café returns without answering.

Live Claude `PermissionRequest` hooks expose **Approve once** and **Deny**. These return `allow`/`deny` through the exact hook invocation, without changing permission rules or tool arguments. Requests expire after 590 seconds, after a provider disconnect, or when handed to the desktop. Failure/timeout leaves Claude's native permission flow in control. A result says “Decision returned to hook”; it does not claim the requested action executed successfully.

Unlinked conversational questions and other requests have a read-only review and desktop handoff. The opt-in Codex relay below supports bounded choices and approvals for clients that launch through it. No decision is inferred from transcript text. Codex desktop approvals are **not implemented**: this machine's running app-server processes do not expose an attachable control socket. Starting a separate app-server would not answer their existing requests. Existing Claude prompts that predate hook installation also stay in the desktop app. Claude sandbox network prompts do not fire PermissionRequest hooks.

## Install / remove

Run `python3 tools/approval/install.py` to merge the hook into `~/.claude/settings.json`. The installer backs up the file, preserves other hooks/settings, and avoids duplicates. Use the same Python interpreter with `--remove` to remove only this hook. Installation is local to this machine. The installer copies the bridge to `~/.token-cafe/claude_hook.py`, so it survives moving or deleting this checkout. Reinstall after updating the bridge code. It migrates old Token Café checkout registrations, preserves unrelated handlers, and honors `CLAUDE_CONFIG_DIR` for the settings location. Run `python3 tools/approval/install.py --check` to check registration, disabled hooks, and installed script freshness. This checks installation, not whether a live provider has invoked the hook. The app listens on `~/.token-cafe/approval.sock` in a private directory (0700), with socket mode 0600. Only one app instance owns the bridge. macOS and Linux use the same Unix-socket hook path; desktop URL navigation remains macOS-only.

Official hook contract: https://code.claude.com/docs/en/hooks#permissionrequest

## Transport and request lifecycle

The BLE handshake derives a per-connection context from host/board nonces and the paired secret. New firmware advertises authenticated fragments: each direction adds a monotonic counter and a 128-bit HMAC-SHA256 tag covering its role, both nonces, counter and fragment. Invalid tags, gaps, replay and counter exhaustion fail closed. This adds integrity/replay protection, **not encryption**. Old firmware remains a view/navigation-only client and cannot submit decisions.

Each touch records the first 64 bits of SHA256 over the displayed RGB565 frame. A changed or partially presented frame during contact invalidates the touch for decisions. The host requires that fingerprint to match the current immutable review, including its random request marker. A request replacement invalidates the open review rather than replacing its action. Repeated touches cannot enqueue a second decision. Input from a stale screen, legacy firmware, an unauthenticated connection, or after request removal cannot approve. Opening the desktop releases the hook so the native prompt can resume.

The current firmware requires an ATT payload above 20 bytes for authenticated fragments. The connected JC4827W543 negotiated a larger payload successfully. Raw screenshot and input injection commands exist only in QA firmware; restore production after tests.

## Verification (2026-10-14)

- 69 Rust tests passed with Bluetooth; 61 without it. Unit/integration coverage: hook allow/deny/handoff, disconnected clients, duplicate/stale decisions, literal text, MAC corruption/replay/direction/connection separation, existing application regression tests.
- Builds with and without the Bluetooth feature; macOS native bundle; clippy with warnings denied. Linux-specific runtime behavior was not exercised on a Linux machine.
- Ten hardware tests in `tools/test_board_approvals.py`: preview, expanded review, paging, approve, repeated input, deny, expired request, exact desktop target, back to café, and the changed double-tap-footer behavior. Each compares all 261120 RGB565 bytes of the physical board capture with the host and expected scene.
- Native macOS visual test and a live board-to-BLE-to-app-to-hook round trip: a clearly labeled synthetic request returned exactly the documented allow JSON. No real pending user action was approved during testing.
- Existing full hardware regression: 48/49 passed on the initial run; the old double-footer expectation was updated for the intended full-screen behavior and passed on the final targeted run. Summary: `target/board-approval-regression/summary.json`. No transport errors; measured tap-to-LCD median 850 ms, p95/max 990 ms (11 samples).
- New captured screens and per-case results: `target/board-approvals/` (generated locally).

### Real provider verification (2026-10-14)

Verified with the installed Claude 2.1.295 runtime and a separate Claude Desktop session named **Token Cafe approval integration QA** (desktop ID `48e0176e-ac09-49a5-9dae-9a7836f3f562`). The user's working session was not interrupted.

- Native Token Cafe approval of a real CLI Write request created exactly the requested QA marker.
- Physical board injected Deny prevented a real CLI Write; Claude confirmed denial and the file was absent.
- In Claude Desktop Manual mode, an outside-workspace Write produced the actual desktop permission dialog and a linked board request. The board displayed the full arguments. Injected **Approve once** over BLE dismissed the desktop prompt and created `/tmp/token-cafe-provider-qa/desktop-approved.txt` with `TOKEN_CAFE_DESKTOP_QA`.
- A second Desktop request was denied on the board. Claude explicitly reported denial by the permission hook, and `desktop-denied.txt` remained absent.
- The previous completed review stayed immutable when the next request arrived. Returning to Cafe and reopening loaded the new request.
- Board captures: `target/board-approval-live/desktop-provider-approval.png` and `desktop-provider-deny.png`.
- 71 Rust tests passed after fixing stale footer selection; macOS bundle rebuilt. Production firmware restored after capture/injection tests.

The hook writes a private `~/.token-cafe/approval-hook-status.json` diagnostic containing only event and timestamp (no request contents). Events distinguish receipt, waiting, decision delivery, desktop handoff/expiry, and bridge errors. The wait budget is now 590 seconds (hook socket 595, Claude hook timeout 600) to allow time to read a long request. A request already pending before installation cannot be attached retrospectively; fresh Desktop requests were verified without restarting Claude Desktop.

At the time of this Claude verification, Codex response integration was missing; these Claude tests do not establish Codex support.

## Recovery from a missing approval bridge (2026-10-16)

The installed hook referenced a deleted Codex worktree. Claude could still display
its native permission dialog, but no request reached the running broker, so the
board could only offer desktop handoff. The hook status file still contained an
old successful event; its timestamp must be checked before interpreting it as
current health.

The installer now installs a private, durable script outside Git worktrees and
replaces recognized stale registrations, including ones using an older Python
interpreter. It backs up settings and preserves other hooks and permission rules.
It does not disable sandboxing or grant persistent tool permissions.

For a missing request:

1. Run `python3 tools/approval/install.py --check`; reinstall if it needs repair.
2. Confirm the companion is running and the board reports `synced v2`.
3. Trigger a **fresh** permission request. A prompt whose hook already failed
   cannot be reattached; answer it on desktop or cancel/retry it yourself.
4. Check the timestamp/event in `~/.token-cafe/approval-hook-status.json`.
   `waiting_for_board` proves the hook connected to the broker;
   `decision_returned` means it returned a decision through that invocation.
   Neither proves the tool executed successfully.

Regression command: `python3 tools/approval/test_bridge.py`. Isolated tests cover
migration from a deleted worktree, idempotent reinstall, custom config directories,
preservation/removal, disabled hooks, exact allow/deny replies, desktop handoff,
and an unavailable broker leaving the native prompt authoritative.

Live repair verification: a synthetic `TEST ONLY` PermissionRequest reached the
production companion and physical board. The user tapped Approve once on the
board; the installed hook returned exactly `hookSpecificOutput` with
`hookEventName: PermissionRequest` and `decision.behavior: allow`. No tool was
executed by this fixture. Three isolated Python bridge tests and three Rust
broker tests passed. Actual execution of the screenshot's command was not tested
or authorized by this repair.

## Codex relay integration (2026-10-16)

The opt-in `tools/approval/codex_relay.py` relays an owning client's app-server
stdio connection. Configure that client's server launch command as:

```sh
python3 /absolute/path/to/token-cafe/tools/approval/codex_relay.py -- codex app-server --listen stdio://
```

The rebuilt Token Cafe companion must be running and the board must use matching
firmware. This command is a server transport for an app-server client, not an
interactive terminal client. No model turn starts and no permission is granted
just by launching it. Remove the wrapper to stop offering board requests.

**Existing Codex Desktop sessions remain unsupported.** Read-only inspection
found no attachable control socket on the running Desktop processes. Starting
another server cannot answer their requests. No Desktop session was restarted or
answered during this work. Rollout files are detection-only, never a response
channel. Official contract: [Codex app-server](https://learn.chatgpt.com/docs/app-server).

Supported through the relay:

- Command approval with the full command: **Approve once** or **Deny**.
- File-change approval after the full changes have arrived in `item/started`.
- One to three questions with two to four unique, single-choice options each.
  The review shows the question and numbered labels, with descriptions available
  by paging. Select an option, then tap **Submit option N**. Nothing is selected
  by default. Selecting does not submit and resets the presentation/quiet guard.
  All questions must be answered before one complete RPC reply is sent.

Free text, Other, secret input, multi-select, oversized content, persistent grants,
scoped network/extra-permission grants, incomplete diffs, and unavailable brokers
remain in the owning client. The original request is always forwarded to that
client. The first board or native answer wins. Native answers, resolved requests,
turn changes, and disconnection cancel the associated board socket. A new relay
connection never replays old replies. A board delivery receipt confirms delivery
to the adapter, not tool execution or eventual model success.

The relay binds each pending answer to its connection, JSON-RPC request ID,
thread and turn. Each board question gets a separate random broker request ID.
Choice responses preserve the exact question IDs and option labels. The protocol
adds decision values 3–6 for options 1–4, permits at most four decisions, and hashes
that ordered list plus the complete question text. Only Codex requests can use
contiguous choice codes. Existing stale-digest, immutable-review, touch, and
receipt protections also apply to choices. Upgrade companion and firmware together;
the renderer asset-digest handshake rejects mismatched versions.

### Reproducible verification

```sh
python3 tools/approval/test_bridge.py
python3 tools/approval/test_codex_relay.py
cargo test --workspace --all-targets
bash tools/qemu_v2.sh build
bash tools/qemu_v2.sh test
```

`tools/test_codex_qemu.py` drives synthetic app-server requests through the real
relay, a private Rust broker/Store/Authority, encrypted QEMU transport, and the
Xtensa firmware renderer/touch handler, then checks the exact RPC reply. It does
not execute a model turn or a command. Generated captures and results are under
`target/protocol-v2-qa/codex-*`. Physical-panel/touch and live Codex-provider
verification remain separate from these emulator tests. This change has not been
flashed onto the physical board.

Verification results for this implementation: 145 workspace tests and 49
Bluetooth-disabled tests passed; the three Claude bridge tests and six Codex
relay tests passed. The full `qemu_v2.sh test` suite passed, including all eleven
Codex cases (allow, deny, file allow, four choices, multi-question, stale digest /
reconnect / turn cancellation, resolved cancellation, and native answer).
Clippy passed with the existing `cloned_ref_to_slice_refs` warnings in unrelated
scene tests allowed; the unmodified strict lint command still reports those.
The multi-question run found and fixed a missing-body review transition that
could otherwise retain the previous question. Its focused regression also passes.

## Desktop answers retire board approvals (2026-10-16)

Claude Desktop can finish its permission dialog while the parallel PermissionRequest
hook is still waiting. Socket liveness alone is not evidence of a pending dialog.
The installed hook now watches that session's observed permission-wait transition
and the result of an unambiguously matched tool invocation in its transcript. It
sends a resolution notice over the same broker socket and returns no permission
decision. Transcript errors are not interpreted as denials: they can also be
execution failures or interruptions. A known success says `APPROVED ON DESKTOP`;
otherwise the board says `REQUEST RESOLVED`. Explicit denial notices are rendered
as `DENIED ON DESKTOP`, but the current Claude lifecycle watcher cannot reliably
identify every native denial. These notices only retire controls; they never
authorize execution.

The broker records terminal outcomes even if socket setup or writing fails after
the provider disconnects. It rejects late board decisions. Resolved reviews are
read-only and retained for 30 seconds while their session remains present. The
normal collector status stops the patron's attention indication when Claude
continues. Fresh requests get their own request identity and remain actionable.
Reinstall with `python3 tools/approval/install.py` after updating the script;
update the companion and firmware together for matching renderer digests.

Validation: 146 workspace tests; five Python hook tests; full QEMU regression
suite, including `test_desktop_resolution_qemu.py`. The new test runs the actual
hook against synthetic session files/transcripts and the real broker/Store, then
makes the first board touch only after the native result. It covers success,
error/denial with unknown outcome, and continuation before a tool result. In all
three cases the stale buttons cannot emit a decision. This is isolated emulator
verification, not a claim that a real Claude Desktop permission was answered.

Deployment: the durable hook was reinstalled and passed `--check`; the matching
macOS companion was rebuilt and restarted. A private 4 MiB flash backup was saved
before flashing the production semantic image to `/dev/cu.usbmodem2101`. Esptool
verified the written hashes, and the companion window reported
`TokenCafe-6336 synced v2` after reconnecting. Physical visual/touch confirmation
of a fresh real Claude Desktop approval remains pending user verification.
