# Controller mode

Visual implementation contract: [Controller design system v2](controller-design-system.md).

Controller mode is a touch-first macro keyboard for supervising local Claude Code and Codex sessions. The existing café remains available. Both modes use the same 480×272 framebuffer on desktop and ESP32-S3.

## Screen

The controller shows six sessions per page in a two-column, three-row grid. Tiles are 234×56 physical pixels, with a 4-pixel gap. Each tile shows a session title, provider and branch. The gold outline is the board cursor; it does not claim the desktop is focused. All detected sessions participate, including café overflow.

Tap a tile once to request that exact desktop session. The overview remains visible. Sessions without a known target say “Details only.” Desktop navigation is currently macOS-only. The desktop uses its existing identified session URL targets; no project-path guesses are allowed.

The header switches back to Café and filters All / Claude / Codex / Needs you. Press M in the desktop window to switch modes. Controller uses antialiased DejaVu Sans; the café retains its pixel font. The café artwork contains no Controller label or hidden touch target. Mode and filter replicate through the Mac authority to connected devices. Filter changes preselect an eligible visible session without opening it.

The footer keeps global attention/working counts and explicit page navigation. Page buttons clamp at the first/last page. Details displays status and bounded task text; it closes after 30 seconds. The selected session remains in Needs you after resolution until the cursor moves. Selection follows session IDs, rather than screen coordinates or desktop focus.

## Status icons

- Loading ring: active work. This does not distinguish reasoning from tool execution.
- Blue dot: Codex reports unread output, or newly observed public assistant output after the initial observation baseline when provider unread state is unavailable. It is not inferred from idle or completion. Explicit activation acknowledges fallback indicators; Codex-reported unread state remains authoritative until Codex marks the session read.
- Warning/exclamation: waiting for permission.
- Question mark: waiting for input.
- Disconnected mark: provider reports disconnected; never inferred to be successful completion.
- No icon: idle without new observed output.

Assistant evidence is bounded and excludes private reasoning and tool results. Initial history does not create unread indicators. Codex desktop unread state is read from its local persisted read-state list (version 1), including at startup; viewing the session in Codex clears the indicator on the next poll. Multiple account identities or local-host buckets, invalid data and unknown versions fall back to observed-output tracking. Claude uses observed-output tracking. Long or identical excerpts may be indistinguishable when no new timestamp is available.

## Keypad

| Position | Left | Middle | Right |
| --- | --- | --- | --- |
| Top | Previous eligible session + open | Next eligible session + open | Knob: turn to browse; press to open |
| Middle | Details / Back | Open highlighted session | Cycle All / Claude / Codex / Needs you |
| Bottom, unchanged | Dictate in focused app/session | Open Claude | Open Codex |

Top keys advance from the board cursor in filtered reading order, wrap, and skip details-only sessions. Knob traversal includes details-only sessions and changes only the cursor. Open always acts on the highlighted session. An empty list is inert. The bottom row remains independent of board selection/filter. Café retains its prior pinned-sidebar, theme and app-toggle mappings.

The 128×32 OLED uses exactly the same display design in Café and Controller modes: existing provider icons, active counts, weekly allowances (including both Claude accounts), attention alerts, and the five-second session-name navigation overlay. There is no separate Controller OLED display or mode/filter/sort marker. Mode and filter remain visible on the main desktop/board screen. No keypad firmware, VIA keymap or bond changes are required.

## Dispatch and reconnects

Within a batch, the keypad sends only the latest controller desktop/app navigation request. Controller URL dispatches execute serially, avoiding detached openers that finish in reverse order. Already handed-off OS activations cannot be revoked. “Open requested” means dispatch acceptance, not verified exact-session focus. Failures must not be represented as success. The board consumes disconnected input without replaying it after reconnect; its existing retained-frame policy applies.

## Validation and deployment

Run workspace tests, clippy, formatting, portable no-std checks, keypad native/parser tests, desktop build and firmware build. Render the real native fixture using `cargo run --example screenshots -- --controller`.

Physical acceptance remains separate: confirm adjacent touch targets, long session identities, each keypad control, rapid app/session switching, actual desktop targets, OLED legibility, mode/filter visibility, reconnects, and both café and controller behavior. Installing new board code requires rebuilding and flashing its engine firmware. Do not treat a successful build as a device update.

## Implementation validation — 2026-11-04

- 269 workspace tests passed, covering controller rendering/navigation, unread state, status ordering, and the 60-minute idle boundary.
- Workspace clippy with warnings denied and formatting checks passed.
- QEMU protocol, attention, reconnect and 60-minute sleep/wake checks passed.
- Shared companion core compiled without default/std features.
- Sanitized keypad display/parser and Swift routing/recording fixture checks passed.
- macOS app bundle rebuilt and its signatures verified.
- ESP32-S3 engine and complete semantic-board firmware built; app partition retains approximately 20% free space.
- Native 480×272 controller screenshot rendered and visually inspected: [controller.png](controller.png).

Build outputs: `release/Token Café.app` and `firmware/semantic/build/token_cafe_semantic_board.bin`. The app and board were subsequently updated as recorded below. Physical touch/display acceptance remains separate from transfer verification. No keypad firmware or VIA keymap change is needed.

Controller typography uses the bundled DejaVu Sans alpha atlas at 11px, 13px and 16px, shared by desktop and board. Regenerate it with `python3 tools/gen_controller_font.py` (Pillow required only for generation). Font notices are in `assets/fonts/LICENSE-DejaVu.txt`. The café has no Controller text or touch target; switch with M.

## Status ordering

Tap **Stable order / Status order** in the Controller footer, or press **S** in the desktop Controller window. Status order groups sessions as **Needs you → New output → Working → Idle**, followed by disconnected sessions. Waiting takes precedence over unread; unread takes precedence over working. Ties retain deterministic session-ID order. Provider filters apply before ordering. The cursor follows its session ID through reordering and stays visible on its new page. Knob and top-key navigation follow this order. Mode/filter/sort replicate between desktop, board and keypad; the OLED retains the existing Café layout. Bottom-row actions are unaffected.

If status changes move a tile during a board touch, the existing identity check cancels the gesture rather than opening the replacement tile. Status ordering is optional; stable order remains the default.

The physical board display turns off after **60 minutes** without touch/keypad activity. Host updates and unchanged heartbeats do not extend this interval.

## Board installation — 2026-11-04

Installed the production semantic firmware on TokenCafe-6336, including normal-font Controller mode, status ordering, provider unread indicators and the 60-minute display timeout. A private complete 4 MiB flash backup was taken first. The application write passed flash-hash verification, and NVS was verified unchanged byte for byte before boot. The board booted with the expected LCD register settings and reconnected after restarting the matching desktop app; the app reported `synced v2`. Controller mode and status ordering were restored. Physical touch/display acceptance remains separate from transfer and synchronization verification.
