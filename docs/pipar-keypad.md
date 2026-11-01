# Pipar Flake: setup, keymap, OLED, and recovery

This is the current runbook as of 2026-11-01. The supported connection is **keypad ↔ Mac**, USB preferred, direct Bluetooth fallback. The Mac optionally connects to the Token Café board. No calibration is needed with the known-good VIA bindings below. The earlier **keypad ↔ board ↔ Mac** architecture is documented under [historical direct-board operation](#historical-direct-board-operation); its firmware is not interchangeable with the current build.

## Start here

1. Launch `/Users/cuong/games/token-cafe/release/Token Café.app` for keypad-only operation. Keep the keypad powered on.
2. Connect USB for `USB + OLED ready`. The Mac also establishes its Bluetooth connection; `USB + OLED ready; Bluetooth standby` means both handshakes succeeded.
3. Unplug USB for `Bluetooth + OLED ready`. Reconnect USB to restore USB preference. A stale host heartbeat can take up to five seconds to expire.
4. To use the board too, launch from the repository with `bash tools/run_companion.sh`. It runs that same release app with `--bluetooth` and the existing board registry. Stop the other companion instance first. Wait for `TokenCafe-6336 synced v2`.

The production display uses **semantic-v2 firmware**: build with `bash tools/qemu_v2.sh board-build`, output `firmware/semantic/build/token_cafe_semantic_board.bin`. `tools/firmware.sh display-build` produces the legacy BLE display and is not compatible with this companion. When the app asset digest changes (including after a rebase), rebuild the engine and matching semantic board firmware together; flash only its application at `0x10000` to preserve pairing storage.

A Finder launch currently starts keypad-only mode. `Board: optional` does **not** mean a board is connected. `START TOKEN CAFE…` on the board means it has no active synchronized companion. The production registry defaults in the launcher to `target/esp/v2-pairing/production.json`; `TC_BLE_REGISTRY` can select another registry. Registry/key files are private and must be backed up separately from source. The native app without that launcher uses `~/.token-cafe/boards.json` when explicitly started with `--bluetooth`.

## Connections and ownership

| Route | Current support | Owner of agent selection/actions | OLED source |
| --- | --- | --- | --- |
| Keypad USB ↔ Mac | Preferred | Mac | Mac over vendor HID |
| Keypad Bluetooth ↔ Mac | Fallback; physically confirmed | Mac | Mac over encrypted GATT |
| Mac Bluetooth ↔ board | Optional, independent of keypad transport | Mac authority; board has touch/button interaction | Keypad still receives from Mac |
| Keypad Bluetooth ↔ board ↔ Mac | Historical; removed from current firmware | Earlier board client + Mac authority | Earlier board-to-keypad GATT |

The keypad never decides approval requests. Approve/deny remain on the board touchscreen and the separately documented physical approval button. Mapped controls wake a connected board through an authenticated activity generation; repeated heartbeats do not replay actions. Inputs while no recent keypad host is available are consumed and discarded, not queued for reconnection.

Only one helper may own the keypad. It pins the physical serial, opens the vendor USB interface only, and uses a per-user process lock. Keep one companion running. The keypad's ordinary HID service may still be visible to macOS, but the eleven dedicated controls are intercepted before normal keyboard/volume reports.

## Physical layout and behavior

OLED at the top; eight keys plus one rotary encoder with a press switch:

| Left | Middle | Right |
| --- | --- | --- |
| Scroll up | Scroll down | Knob: turn to browse; press to select |
| Cycle whole café theme | Select, same as knob press | Toggle Claude / Codex |
| Native recording | Claude app | Codex app |

Turning the knob **always browses agents**, including on an approval/details page. It closes that page and moves in stable seat/ID order. Press once to open the selected agent's details; press again to open that exact desktop session. The two top keys scroll the current task/details page. The theme key cycles humans → dragons → Pokémon → mixed. Fixed app shortcuts preserve the selected agent; Voice uses that selection, not whichever app is foreground. Toggle opens Codex when Claude is foreground, otherwise Claude.

Codex recording targets the session composer's **Dictate** button. Control+Shift+V and “Start Voice” can target the pet/global conversation and are deliberately not fallbacks. Claude uses its native recording control; the user confirmed it starts recording. Neither path submits text. `VOICE REQUESTED` means the accessibility press was accepted, not proof of microphone capture. Physical Codex Dictate acceptance remains unconfirmed.

## Known-good VIA bindings: avoid recalibration

The tracked [keymap-reference.json](../firmware/keypad/keymap-reference.json) records all 18 keys and four encoder bindings read back from this device. It contains no macros or bonds. The build default is [pipar_flake.keymap](../firmware/keypad/config/pipar_flake.keymap); persisted VIA settings override compiled defaults.

| Position (row,column) | VIA/QMK code, layer 0 | Token Café command |
| --- | --- | --- |
| Top left (0,0) | `0x0029` Esc | 1: scroll up |
| Top middle (0,1) | `0x002c` Space | 2: scroll down |
| Knob press (0,2) | `0x0068` F13 | 9: select |
| Middle left (1,0) | `0x002a` Backspace | 3: theme |
| Middle middle (1,1) | `0x0052` Up | 4: select |
| Middle right (1,2) | `0x0028` Enter | 5: toggle apps |
| Bottom left (2,0) | `0x0050` Left | 6: native recording |
| Bottom middle (2,1) | `0x0051` Down | 7: Claude |
| Bottom right (2,2) | `0x004f` Right | 8: Codex |
| Turn left | `0x00aa` volume down | 10: previous agent |
| Turn right | `0x00a9` volume up | 11: next agent |

VIA encoder readback uses direction 0 = `0x00a9`, direction 1 = `0x00aa`, on both layers. These are **QMK codes**, not the emitted consumer usages `0xe9`/`0xea`. The device-tree sensor parameters are `<0x00aa 0x00a9>`. Do not swap them based solely on the VIA direction number.

Layer 1 is retained for settings compatibility: top-right `0x5200` returns to layer 0; middle-middle `0x00a8` is mute; middle-right `0x7e00` next Bluetooth profile; bottom-left `0x7e02` reset; bottom-middle `0x7e03` bootloader; bottom-right `0x7e01` clear current Bluetooth profile. Other entries are unassigned. With F13 replacing the stock layer switch, this layer is not normally entered through the knob.

The seller's stock knob press was `0x5201` (`TO(1)`), a local layer change that sends no key event. That caused setup to stall at PRESS THE KNOB. Changing only layer 0 / row 0 / column 2 to F13 fixed it. If already on layer 1, one press returns to layer 0; the next emits F13. A firmware update once decoded this persisted binding as unassigned, so **read back settings after every firmware change**, even when NVS is preserved. Do not mass-reset or recalibrate to fix one wrong binding.

## Hardware and firmware identity

| Item | Verified value |
| --- | --- |
| Application name / USB | `PiparFlake`, manufacturer `koit.dev`, `1d50:6170` |
| Physical serial | `651A6626B5AE5931` |
| Bootloader | nice!nano nRF52840, `239a:00b3`, same serial |
| Build target | `nice_nano_v2`; shields `pipar_flake raw_hid_adapter` |
| Display | SSD1306, 128×32, I²C `0x3c` |
| Matrix | 3×3; eight keys + knob press |
| VIA | protocol 13, firmware version 1, keycode version 8, two layers |
| Board (different device!) | ESP32-S3 `303a:1001`, serial `44:BD:8D:7A:63:54` |

The current firmware is intentionally pinned to this physical keypad in the Swift USB filter, Bluetooth identity check, and firmware identity string. Supporting another keypad requires updating all three together; this is not generic multi-keypad discovery.

Sources already investigated:

- [Seller guide, VIA JSON, and UF2](https://github.com/Hieupham0012/Guide_Pipar_Wireless). Local reference: `target/pipar-reference/{seller.uf2,via.json,provenance.json}`.
- [Seller pipar source](https://github.com/Hieupham0012/pipar) and [pipar_flake source](https://github.com/Hieupham0012/pipar_flake) did not identify the exact source of the published VIA binary.
- [Pipar shield wiring](https://github.com/RasmusKoit/zmk-keyboard-pipar/blob/main/boards/shields/pipar_flake/pipar_flake.dtsi) is copied into the local shield definition.
- [K4zoku/ZMK-VIA](https://github.com/K4zoku/ZMK-VIA), pinned `9f8796671ae5843277e0daaaf711819499ad4a90`, supplies the compatible VIA implementation and custom Pipar keycodes. Compatibility is not proof that it produced the seller binary.
- ZMK `edf5c0814fd3ea202e43aad2d68fd32e882a518c`; raw HID `6a37765dfab6197292e7a9f47305dcf87386d56a`. Full pins: [west-lock.yml](../firmware/keypad/west-lock.yml). Docker digest and VIA pin: [build_keypad.sh](../tools/build_keypad.sh).

Seller UF2 SHA-256: `c599e7d0be2210e49a4d75a4f45140c137871ee73288cdb02e912b6d911d5a4b`. It is a seller-published fallback, **not** a readback backup of this keypad.

## OLED and wire protocol

The OLED has three views, driven by `src/companion/keypad/oled.rs` and drawn locally in `firmware/keypad/module/src/display.c`:

- **Idle** (five seconds without keypad input): Claude icon and active-session count on the left; Codex icon and count on the right. Only working sessions and sessions waiting for input count as active; idle and disconnected sessions do not. Counts include sessions outside the eight board seats.
- **Navigating**: selected session's provider icon/name above two rows of session name (up to 32 characters). Any mapped key or knob input extends this view for five seconds. Turns still browse; selection and scrolling semantics are unchanged. Non-ASCII characters use `?` with the 8×8 font.
- **Pending approval or question** takes precedence: blink the requesting provider's icon on black, 500 ms on / 500 ms off. If both providers have pending approvals, show both icons. Any mapped key press or knob turn silences all current requests and returns to the navigation view; it does not approve anything. Removing the pending request stops its alert. A new request ID/digest can alert again. Unlinked permission waits and questions also alert. Claude desktop `mcp__ccd_turn__wrap_up` cards with a nonempty `ask` are treated as waiting for input even when the provider reports idle; the following tool result and assistant summary preserve the question. A real user reply, resumed tool work, or failed card clears it. A companion restart resets the acknowledged set and may alert for still-pending requests again.

The keypad cancels blinking locally on input, without waiting for a round trip. The Mac tracks request identities so repeated model refreshes and the idle timeout cannot restart an acknowledged alert. Firmware owns blink timing; it does not require alternating Bluetooth frames. Provider artwork is the same monochrome spark/knot used on the board. Normal views use the whole screen; battery percentage remains on offline/legacy text screens.

The transport still supplies four printable 16-character rows atomically. A versioned presentation marker in row 0 selects the layout: `@TC2I` (counts in rows 1/2), `@TC2N` (provider in row 1, name in rows 2/3), or `@TC2A1/2/3` (Claude/Codex/both, alert generation in row 1). Firmware retains plain text rendering for connection/offline messages and older hosts. Install the matching new keypad firmware before running a host that sends these markers; old firmware would display the marker as plain text.

USB reserved reports are 32 bytes: `FE 54 43 4F 01` (`FE TCO`, version 1), command byte, then data. Command 0 probes capabilities; 1 carries a 20-byte display row; input command 2 carries action 1–11 and an 8-bit sequence followed by zero padding. Successful probe/row replies have byte 6 = 1 and byte 7 = 1. Command 3 explicitly clears the active Bluetooth profile over physical USB; normal operation never calls it. It destroys that profile's bond, so use only for intentional repair/migration.

Bluetooth service: `f6cafe10-7f67-4cd9-9b7d-58381d8de601`. Characteristics use first components `f6cafe11` (identity read), `f6cafe12` (OLED write with response), and `f6cafe13` (input notify), with the same suffix. Identity is `TCM2651A6626B5AE5931`. The active bonded profile must have encryption level L2. BLE input is the first eight bytes of the USB input report. Duplicate sequences are discarded per transport.

A display row is `[version=1, sequence, row, 16 ASCII bytes, reserved=0]`. Four ordered rows with one sequence within two seconds publish atomically. USB/BLE have separate frames and activity clocks. Valid host traffic expires after five seconds; stale display content clears after eight seconds. Mac refreshes unchanged frames every two seconds and changes on its 200 ms timer; keypad LVGL refresh is 100 ms. These timers mean OLED updates are not instantaneous. Do not shorten safety timeouts to disguise a delivery problem.

The stock-looking display after a successful flash was traced to a **2,048-byte LVGL heap**, not a failed transfer. The custom screen now reserves **8,192 bytes**, has an opaque background, and uses a dedicated display work queue. The user subsequently confirmed live rendering. Temporary health characteristics and raw USB diagnostics were removed; current firmware has only the Mac service and normal VIA support.

## Build, backup, and application-only flashing

Run from the repository root:

```sh
bash tools/test_keypad.sh       # pure native tests + sanitized display parser
cargo test --workspace --locked
bash tools/build_keypad.sh     # requires Docker; uses pinned dependencies
sh tools/build_macos.sh        # macOS; publishes to main repo release folder
```

On this machine Rust is under `/tmp/token-cafe-cargo/bin` with `CARGO_HOME=/tmp/token-cafe-cargo` and `RUSTUP_HOME=/tmp/token-cafe-rustup`. These are local toolchain locations, not portable requirements. Keypad output: `target/pipar-reference/zmk-via/build/cafe/zephyr/zmk.{hex,bin,uf2}`.

Before flashing: stop the companion helper to avoid competing USB access, save both layers, all encoder mappings, VIA versions, and macro bytes privately. Existing backups include `target/keypad-direct-mac/settings-before.json`, `settings-after.json`, `macros-before.bin`, and `target/pipar-reference/recovery/`. They are ignored local artifacts and can disappear with `target` cleanup; copy required backups to durable private storage. Never put macros or board authentication keys in a public firmware package. The tracked keymap reference is a reusable baseline, not a backup of user macros or bonds.

This managed Mac blocks the bootloader FAT drive with `SUIS premount dissented`; Finder showed no drive or zero bytes. **Use USB serial DFU**, which worked without changing external-media policy. Do not raw-write disks or update the bootloader to work around that restriction.

```sh
python3 -m venv /tmp/token-cafe-keypad-tools
/tmp/token-cafe-keypad-tools/bin/python -m pip install adafruit-nrfutil==0.5.3.post16
mkdir -p target/keypad-release
/tmp/token-cafe-keypad-tools/bin/adafruit-nrfutil dfu genpkg \
  --dev-type 0x0052 \
  --application target/pipar-reference/zmk-via/build/cafe/zephyr/zmk.hex \
  target/keypad-release/pipar-application.zip
/tmp/token-cafe-keypad-tools/bin/python -m serial.tools.list_ports -v
```

Enter the nice!nano bootloader using its reset procedure or the retained VIA bootloader binding; **re-enumerate** and verify `239a:00b3` plus serial `651A6626B5AE5931`. USB port names change. Do not use the ESP32 board's port. Inspect the ZIP manifest: application only, no SoftDevice or bootloader. Verify its application bytes against the built BIN before uploading.

```sh
# Replace VERIFIED_KEYPAD_PORT with the freshly verified bootloader port.
/tmp/token-cafe-keypad-tools/bin/adafruit-nrfutil --verbose dfu serial \
  --package target/keypad-release/pipar-application.zip \
  -p VERIFIED_KEYPAD_PORT -b 115200 --singlebank
```

Require **`Device programmed.`**, then application enumeration with the same serial, then settings readback. The uploader can exit zero after an error, so exit code alone is insufficient. If interrupted, a physical power cycle was needed before retrying. A temporary diagnostic launcher extended host acknowledgement waits from one to ten seconds; it is not part of the maintained toolchain. Do not erase settings as a first response.

Re-read all 18 keys, four encoder bindings and 1,024 macro bytes; compare against the backup. Restore only confirmed differences (notably knob F13). Test USB, unplugged Bluetooth, OLED, shortcuts, and USB reconnect priority. A successful upload alone does not prove display rendering or correct bindings.

The earlier pre-cleanup Mac-only application SHA-256 is `ce60c5d86e1ec2b8a1a533bf00f57bc2410829f3c4aebe64a9efbb028372ac02`; package `target/keypad-mac-bluetooth/pipar-mac-only.zip`. The OLED redesign supersedes that image; its deployment evidence is recorded below. Do not label a new image with an older installed hash.

### Claude desktop question detection — 2026-11-01

The `wrap_up.ask` regression was reproduced on a live desktop session: provider status said idle although “Apply the review fixes now?” remained unanswered. The updated collector reports waiting and preserves the exact question across a successful tool result and closing assistant summary. A user reply, subsequent tool call, or failed question-card tool clears that state. The keypad now alerts for all waiting sessions; question text changes contribute to its alert identity. Regression suite: 219 tests and strict Clippy passed. The app was rebuilt/reloaded at the stable release path. The production semantic board was rebuilt after the master rebase changed the asset digest, and application-only flashing completed with hash verification; no keypad reflash was needed.

### OLED redesign deployment — 2026-11-01

Installed through USB serial DFU at approximately 11:39 (Asia/Ho_Chi_Minh): application SHA-256 `3236c690fc563abb6070a0a4b627c9403697bc85315bc4fbce23a2a37cba90ea`, 277,928 bytes. Package: `target/keypad-oled-redesign/pipar-oled.zip`. The manifest contains application + DFU metadata only; packaged bytes matched the built BIN. Uploader reported `Device programmed.`; post-flash USB enumeration and readback verified 18 keys, four encoder mappings, version fields, and all 1,024 macro bytes unchanged. Backups and transfer log are under `target/keypad-oled-redesign/` (ignored/private).

The matching app was rebuilt at the stable release path and restarted with the existing board registry. Native app title confirmed `USB + OLED ready; Bluetooth standby` and `TokenCafe-6336 synced v2`. Validation: 200 Rust workspace tests, 50 native keypad checks, 18 native desktop checks, sanitized C frame-parser test, and pinned firmware build passed. Physical OLED layout/blink and post-update unplugged Bluetooth acceptance still require user confirmation; transfer success alone is not visual proof.

## Historical direct-board operation

This documents the route that worked earlier in this session. **Current board firmware has no keypad central/scanner, HID parser, calibration UI, keypad bonding, or OLED forwarding. Current keypad firmware consumes the mapped controls into the Mac service and no longer advertises the old OLED service.** Switching Bluetooth settings alone cannot restore this route.

Restoration requires a matched earlier board and keypad build (or explicitly reimplementing the removed modules), plus the Mac companion for session data and desktop actions. The archived notes are [migration history](history/pipar-keypad-migration-2026-11-01.md). Local historical packages live under `target/keypad-oled/`; they are not a maintained rollback release. No committed legacy source revision is recorded here. Verify source/artifact provenance before any rollback; do not assume `HEAD` contains that implementation.

Earlier procedure, with matched firmware only:

1. Power the keypad, choose a free Bluetooth profile, and unplug keypad USB so HID goes to the board. Keep the board connected to the Mac companion.
2. The board scanned names containing Pipar/Flake, paired the encrypted HID link, and displayed KEYPAD SETUP.
3. Learn eight keys, knob press, left turn, right turn. Progress and bindings were saved after each control. Same-action duplicates (knob/select) were allowed; other duplicates showed SAME INPUT AS / KEY ALREADY USED. Optional knob controls could be skipped.
4. Use F13 instead of the stock `TO(1)` knob binding. If a press only changes layer, return to layer 0 and press again. Two controls with the same HID output cannot be distinguished without remapping.
5. Completion briefly appeared to turn the board off because the old setup used a stale idle timestamp. Tapping returned to the café; the timestamp was fixed. Do not interpret that historical symptom as a pairing failure.

Old OLED service `f6cafe00`, value `f6cafe01`, health `f6cafe02`, all with suffix `-7f67-4cd9-9b7d-58381d8de601`. It used the same four-row framing and a `TCO` + version 1 identity. USB showed `USB ACTIVE / UNPLUG FOR CAFE`. That wording is obsolete for current Mac-first firmware. The old health characteristic distinguished successful writes from a stalled LVGL screen; it was diagnostic, not a prerequisite for current operation. The active old board bond was explicitly cleared once during migration to the Mac.

## Troubleshooting and lessons

| Symptom | What we learned / next check |
| --- | --- |
| Board says START TOKEN CAFE | Check the **board** status, not keypad Bluetooth. Relaunch with the provisioned registry and `--bluetooth`; require synced v2. |
| Knob press does nothing | Read layer 0 (0,2): F13 `0x0068`, not TO(1) or unassigned. Check active layer before remapping. |
| Keys affect the Mac during legacy setup | USB was still connected. Only relevant to the historical board route. |
| OLED still looks stock after flashing | Verify application identity and successful DFU; then custom-screen config and LVGL heap. Writes alone do not prove rendering. |
| Bluetooth missing after migration | Verify active profile, current firmware identity, and app Bluetooth permission. Bond clearing is a deliberate repair, not an automatic retry. |
| Input Monitoring permission loop | Obsolete capture design tried to seize the keyboard interface. Current vendor HID path does not require Input Monitoring. Do not repeat that setup. |
| Voice opens the wrong app/session | Select the intended agent first; app shortcut keys preserve selection. Verify provider/session identity and unique native recording control. |
| Native recording fails | Accessibility permission belongs to the signed release app/helper; native app microphone/onboarding dialogs are separate. No global shortcut fallback. |
| INSTANT Keyboard Setup Assistant | Device enumeration identified the USB gaming mouse. Pipar is a separate device; quit the irrelevant layout assistant. |
| Multiple copies / permissions reset | Use only the main-repository release app. Build scripts ad-hoc-sign helpers and enclosing bundle; this does not guarantee persistent grants across changed executables. |
| Delayed knob-to-board navigation | Compare OLED vs board. Publisher prioritizes model updates at message boundaries and coalesces while awaiting acknowledgement. Delivery spikes remain under investigation. |
| Disconnect while reading USB diagnostics | Opening the ESP32 serial port caused a reset despite disabled reset signals. Avoid doing that during stability monitoring. |

## Verification and remaining limitations

User confirmed USB navigation and Claude/Codex shortcuts without a board, then confirmed Bluetooth controls and live OLED after unplugging USB. The optional board was flashed with an application-only hash check and reconnected as `TokenCafe-6336 synced v2`. USB reconnect priority is implemented/tested in pure logic; explicit physical return-to-USB acceptance is still pending.

The scheduling follow-up passed 198 workspace tests. During 10:49–10:51 on November 1, 48 publisher-to-board acknowledgement measurements had median 289 ms and three spikes of 2.25–2.4 s. This is not physical knob-to-LCD latency and does not establish the root cause. No new disconnect accompanied those measurements. Do not claim navigation latency is resolved.

Cleanup validation: `tools/test_keypad.sh` runs 50 native transport checks, 18 native recording matching checks (using checks retained under optimization), and the sanitized frame parser. The full workspace and pinned keypad build also pass. See [architecture review](keypad-architecture-review.md) for exact scope, source boundaries, and outstanding risks.
