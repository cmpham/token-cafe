# Focused-app pinned navigation — 2026-11-02

The top-left and top-middle keypad buttons now request previous/next pinned
sessions in the foreground Claude or Codex window. They wrap in sidebar order;
when the current session is unpinned, down chooses the first pin and up the last.
The knob still selects the eight visible café patrons. The top keys do not change
café selection, page task text, acknowledge café alerts, or reset its detail timer.

The Mac intercepts existing physical commands 1/2 before the shared client. Other
commands retain their existing protocol routing. Native desktop actions share one
pending slot: a busy action rejects another rather than queuing stale navigation.
There is no firmware, renderer, or board protocol change.

The native helper accepts only foreground Claude/Codex windows, locates the
Pinned sidebar section through bounded container traversal, and filters explicit
session-row markers. It does not enumerate the transcript/composer. Claude's
buttons lack session URLs. All sidebar session rows participate; local Code
metadata is only an optional current-session hint, supplemented by exact page
title matches. A cursor stores only hashes of ordered accessibility row identities and current
page context plus the process ID and index. It is saved after a confirmed route
change and reused only while all identity checks match.
Codex uses the exact current page title, not hover/highlight classes, to identify its current pin. Read-only metadata is
bounded by depth, entry count and file size; symlinks are skipped. No titles,
identifiers, file contents or credentials are logged by this helper.

Navigation rechecks foreground app/window, sidebar order, identities and selection
before a hit-tested click; it checks foreground focus again immediately before
posting the click. It never types a shortcut or submits a prompt. Missing,
ambiguous, disabled, over-limit or covered controls cancel the action. A single
already-focused pin is a no-op. These integrations depend on the installed apps'
English Pinned sidebar and current accessibility/CSS row markers; changed layouts
fail closed. Keep the sidebar visible and Pinned expanded. Missing or ambiguous
current-session hints start at the first/last row; they do not remove any pins.

## Verification

- macOS workspace: 243 tests passed, including host routing with an empty café
  and an unrelated selected café patron. Both top keys leave the replicated
  model and selection unchanged and emit no shared protocol action.
- Native optimized suites passed: USB/Bluetooth parser, recording controls,
  pin direction/wrap, empty/ambiguous selection, session URL boundaries,
  session-vs-project row markers, sidebar order, duplicate titles, metadata
  hints, nested metadata files, symlinks and malformed JSON. The OLED C
  parser ran under address/undefined-behavior sanitizers.
- Strict workspace Clippy and formatting passed. Native desktop code compiled
  with optimization and warnings treated as errors. The signed Mac bundle was
  rebuilt at the stable release path.
- Earlier bounded read-only inspection identified the installed Claude/Codex
  sidebar shapes and current-session markers. The subsequent live down/up check
  could not run because the Mac was at the login screen. Automated fixtures and
  accessibility discovery are not physical-button acceptance.
- Token Café and its keypad bridge were reloaded; USB/OLED readiness and Bluetooth
  standby were observed. No board or keypad firmware flash was needed.

## Follow-up after physical test

The user reported Claude cycling only some pins and Codex doing nothing. Bounded
inspection reproduced both discovery defects: only 3/6 Claude pins matched the
old saved-metadata filter, and Codex's 10 pins were under an `AXList` container
that traversal skipped. The fix removes metadata-based membership filtering and
includes accessibility lists. Regression cases cover all six pins with only three
metadata records, both directions, cursor invalidation and the list container.
The helper requests scroll-to-visible before hit-testing and confirms a changed
route/current-row marker after activation. A posted click is never retried.

Optimized native/transport/quota tests and C sanitizers passed again. Live probes
did not confirm actual navigation. Codex visual inspection was denied by tool
policy, so no alternative Codex UI inspection was attempted afterward. Physical
acceptance of the updated helper remains pending; build success is not evidence
that both desktop apps switch correctly. Earlier workspace/Clippy results above
are from the first implementation; the follow-up changes only Swift/docs.

Local evidence is under ignored `target/pinned-navigation/`.


## Second physical test

The user reported Claude could not reach the top pin and was slow; Codex movement
appeared random. The prior “navigation-confirmed” result was insufficient: Codex
highlight classes also represent hover/highlight, not only the active session.
The revised helper ignores those classes for selection and confirmation, uses
exact page identity, orders rows by vertical position, and keeps a hashed cursor
keyed to stable accessibility node identities and page context. Claude metadata
is consulted only when neither the cursor nor page identity identifies a row.
Visible targets avoid unconditional scrolling/waits; post-click confirmation
reads page context instead of rediscovering the entire sidebar. Diagnostic logs
contain only row counts/indices, source and milliseconds, never titles or URLs.
These changes still require a new physical test, particularly Claude's top row.

The revised signed stable app was rebuilt and its keypad bridge restarted. Native optimized tests and sanitizers passed. The new physical check is pending.

## Third physical test: Codex accepted, Claude top-pin stall

The user confirmed Codex navigation works and Claude Up reaches the top pin, but
Claude Down often stalls. Runtime logs show a repeatable transition: successful
`from=1 to=0` (or `5 to=0`), followed by `from=-1 to=0 source=metadata` and no
navigation confirmation. The cursor was no longer valid after opening the top
pin; Down kept clicking that same row. Other Claude cursor steps took roughly
170–250 ms inside the navigation function; metadata fallback added about 300 ms.

Claude now uses session URLs without mutable page titles and ordered row titles
without ephemeral AX node IDs for cursor validation. A changed route must remain
stable for 120 ms before saving. Codex's confirmed identity path is unchanged.
A regression fixture checks that a loading-title update preserves the top-row
cursor, that Down chooses the second row, and that a manual route change still
invalidates the cursor. Native tests and C sanitizers passed. Physical acceptance
of this Claude-specific correction remains pending.

## Fourth test: clean labels and complete live Claude cycles

Further user testing still found intermittent Claude navigation. Logs showed the
same unknown-current-row fallback despite the cursor changes. Bounded sidebar
inspection established the missing cold-selection cases: Claude page titles end
in ` - Claude Code`, and three pinned buttons include fork/parent-session text
that is not part of the actual title. The row's nested “More options for …” menu
provides the clean title. The helper now reads that label within the row wrapper
and recognizes the Claude Code page-title suffix. Codex behavior is unchanged.

A live native-helper test exercised every one of the six Claude pins in both
directions: **12/12 steps passed**, including top/bottom wrapping. Each selected
row was independently checked against the current page title, not just the saved
cursor. The test restored the starting pin and original foreground app. Observed
navigation-function confirmation times were **284–356 ms** (excluding helper
startup). Evidence: `target/pinned-navigation/claude-cycle/result.log` (ignored).
This verifies desktop navigation; hardware-button acceptance remains distinct.
