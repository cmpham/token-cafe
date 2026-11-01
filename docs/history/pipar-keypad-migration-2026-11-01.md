# Historical Pipar migration notes

These notes record earlier implementations and point-in-time results. They are not current setup instructions. Bluetooth-to-board support has been removed. See [current setup](../pipar-keypad.md).

# Pipar Flake keypad

The primary path is now **Pipar USB ↔ Mac companion ↔ optional board**. The Mac owns selection, key actions, and OLED content; a board is not required. Direct Bluetooth-to-Mac pairing is not yet provisioned or physically validated. The earlier Bluetooth-to-board path is retained as a fallback.

## Direct Mac migration (2026-10-31)

The Mac companion contains a dedicated `keypad-hid` helper that matches only this Pipar's vendor USB interface, VID/PID, and serial. Key presses and knob turns arrive as explicit Token Café commands on the same vendor interface as the OLED. The helper does not open, seize, or monitor keyboard/consumer interfaces and does not request Input Monitoring. Accessibility permission is separately used for native recording controls. A process lock prevents competing copies from controlling the device. Closing the companion releases the vendor interface.

The local keypad controller uses the same shared navigation and authority code as the board. It has no approval-decision capability. Agent browsing, selection/details, exact-session desktop handoff, native recording, app shortcuts, and theme changes run without a board. The top keys scroll the companion's café/details page. A connected board follows the shared view; a new authenticated heartbeat input generation wakes its screen without replaying a key action or repeatedly extending its idle timer.

OLED data travels through a reserved VIA Raw HID command (`FE TCO 01`, command 0 for capability probe, 1 for a 20-byte display row). The firmware routes only this signature before the ordinary VIA parser. Replies identify successful acceptance; unknown firmware is never sent display frames. USB takes precedence over board BLE OLED writes. Frame assembly, refresh, and stale-screen clearing retain the existing bounded four-row protocol. VIA keymaps and macros remain unchanged. A capability flag identifies dedicated USB input support. Input reports are exactly 32 bytes (`FE TCO 01 02`, command 1–11, sequence byte, zero padding). The host validates reports and drops duplicate sequences. Firmware intercepts the eleven mapped USB controls before ordinary HID reports, so they do not leak Esc/arrows/volume into desktop applications. Inputs without a recent host heartbeat are discarded. The Bluetooth route remains ordinary HID.

Build with `tools/build_macos.sh`, which places `Token Café.app` in the main repository's `release` folder, shared by all worktrees and outside Cargo's disposable `target` directory. The path on this machine is `/Users/cuong/games/token-cafe/release/Token Café.app`. Select this release app when granting Accessibility access for native recording. The bundle path and identifier stay fixed; this locally ad-hoc-signed build does not guarantee that macOS will retain permission after a changed executable is rebuilt. Launch the release app normally for standalone use. `tools/run_companion.sh` uses the same release app and also works without a board registry; when a registry exists, it connects boards optionally. The title shows separate keypad and board status. Keep the keypad connected by USB for this mode.

Validation: the workspace suite passed **197 tests**, including local navigation/recording dispatch without a board and authenticated board wake without action replay. The extended local test also covers select → desktop handoff. The native HID mapping passed 13 pure checks; the recording helper passed 18 pure checks. The display parser passed AddressSanitizer/UndefinedBehaviorSanitizer. Both firmware builds and the Mac bundle build succeeded. The board application flash verified its hash and retained pairing settings.

**Installed:** the application-only keypad firmware was uploaded through the verified nice!nano serial interface (`239a:00b3`, serial `651A6626B5AE5931`). The uploader reported `Device programmed.` after 16.7 seconds, and the keypad returned as `PiparFlake` (`1d50:6170`, same serial). All 18 key bindings, four encoder mappings, VIA protocol/layer count, and 1,024 macro bytes matched the private pre-flash backup. The new USB OLED capability probe returned success. Application SHA-256: `87a2d0bd39558860eadea3b1629a9fe7cab787e3c2d42582658d1f1172dc53e5`. Package: `target/keypad-direct-mac/pipar-direct-mac-dfu.zip`; upload log: `target/keypad-direct-mac/keypad-flash.log`.

The reloaded companion is synced with `TokenCafe-6336`; direct input is currently waiting for the user to enable macOS Input Monitoring. Physical Mac-input/OLED and board-disconnected acceptance remain unverified. Build/test/runtime logs are `target/keypad-direct-mac-{build,desktop-build,board-build,board-flash,tests,runtime}.log`.

## Layout

With the keypad OLED at the top:

| Left | Centre | Right |
| --- | --- | --- |
| Scroll up | Scroll down | Knob: turn to browse; press to select |
| Cycle café theme | Select (same as knob press) | Toggle Claude / Codex |
| Native voice | Claude app | Codex app |

Knob turns **always browse agents**, including while an approval or details page is open. Turning closes that page and highlights the previous/next agent in stable seat/ID order. Press the knob or middle-centre key to open the selected agent's request/details; press either again to open that exact desktop session. The two top keys scroll the currently displayed request/details or café task text without changing the agent. App shortcuts preserve the café's selection and review.

The theme key cycles humans → dragons → Pokémon → mixed → humans across the whole café, synchronized with the desktop. A custom per-seat roster cycles to humans first. The toggle key opens Codex when Claude is foreground; otherwise it opens Claude. The two bottom app shortcuts retain their previous destinations.

The voice key opens the host-resolved selected session and activates that app's native voice control. Codex uses its session composer’s **Dictate** button; this installed Claude Code version uses its own **Press and hold to record** control. Codex dictation records speech into the selected session’s composer. It does not start the pet/global voice chat. It never uses macOS dictation or submits text. Token Café needs macOS Accessibility permission to activate the control; a physical voice-key press asks macOS to show its normal permission prompt if permission is missing. The helper enables Electron’s documented `AXManualAccessibility` interface after macOS authorization, processes the main run loop while waiting for app activation, verifies the app and session route/heading, requires one enabled matching control, and fails closed if it cannot identify it. OLED feedback is `OPENING VOICE`, `VOICE REQUESTED` (control accepted the press), or `CHECK VOICE APP` (permission, availability, or routing failed). Starting actual microphone capture remains a physical acceptance check; first-use microphone/onboarding dialogs belong to the desktop app.

The legacy board route reinterprets its saved physical bindings without recalibration. Unplug keypad USB only when intentionally using that legacy Bluetooth-to-board mode; direct Mac mode requires USB and the new keypad firmware. On macOS, the Codex shortcut opens Codex when installed in `/Applications`, otherwise the unified ChatGPT app. Session handoff uses the existing host-resolved session link.

Every mapped key press, knob press, or knob turn wakes the board screen immediately and restarts its ten-minute idle timer. The same press performs its normal action when connected; it is not consumed just to wake the screen. This also wakes the disconnected/startup screen, without queuing offline actions. The board applies the backlight state in the same input-loop iteration. Regression coverage checks all eleven controls after actual idle sleep, both connected and disconnected, and verifies the full renewed idle interval.

Approve/deny remain on the touchscreen (and the existing separate physical button retains its documented behavior). No keypad command maps to a decision. Offline input is discarded; it is not replayed on reconnection.

## Keymap update validation (2026-10-31)

The new map is installed on the board and the companion was reloaded with the existing production registry. The application-only board flash verified its hash and preserved NVS. The running companion reports `TokenCafe-6336 synced v2`. The keypad application and VIA bindings were not changed.

The workspace suite passed (192 tests), followed by the added voice-feedback regression (70 core tests total) and a focused café-task scrolling check. Coverage includes unconditional knob browsing on approval pages, top-key scrolling in both page types, identical select keys, full roster cycling and desktop reconciliation, exact selected-session voice dispatch, missing-target/permission rejection, retry deduplication, and no approval decisions. The native Swift helper and desktop/board release builds succeeded. Native microphone activation and the new physical key functions still require user acceptance; UI inspection of Codex is restricted in the computer-use tool, so its voice activation was not live-tested by the agent.

Build/flash logs: `target/keymap-desktop-build.log`, `target/keymap-board-build.log`, `target/keymap-board-flash.log`. Runtime voice errors are in `target/keyboard-launch.log` and appear as `CHECK VOICE APP` on the OLED. The keymap helper is packaged as `Contents/MacOS/keypad-desktop` in the desktop app.

## Legacy board pairing and layout setup

1. Power the keypad on and select a free Bluetooth profile using its existing keymap. Leave it close to the board. Unplug its USB cable from the Mac during setup: USB mode can send the keys to macOS instead of the board. Keep the Token Café board powered and its Mac companion running.
2. An unconfigured board scans for advertised names containing `Pipar` or `Flake`. If multiple matching devices are visible, it waits; turn off the extra device.
3. The board secures and bonds the HID connection. Both keyboard and consumer-control reports are supported, including standard six-key and ZMK NKRO layouts.
4. Follow **KEYPAD SETUP** on the board. Press and release each named key, then press the knob and turn it one click each way. Allow the label to change between inputs. The board learns the keypad's existing outputs; no keyboard firmware flash is needed when all controls emit distinct HID usages.
5. If the knob press emits the same input as the centre/Select key, setup accepts it because both have the same action. Other conflicts identify the existing control with **SAME INPUT AS**. Tap **SKIP THIS KNOB CONTROL** for a knob control that cannot be learned. The eight primary keys remain required.
6. Each completed or skipped step is saved in NVS, so setup resumes after a restart. Commands activate only after all steps have been learned or skipped. Bond and bindings survive ordinary app-only firmware updates.

A key that only switches a local keyboard layer or Bluetooth profile emits no HID input and cannot be learned. Two controls with identical outputs also cannot be distinguished: give them distinct keyboard/consumer keycodes in the keyboard keymap to assign different actions. Skipping a duplicate knob control preserves the original key's action; pressing the knob will still trigger that same action. Multi-key macros are not supported as control bindings. Modifiers with a single non-modifier key are supported.

During layout setup, hidden touchscreen/physical-button actions are suppressed. The board does not reinterpret setup keystrokes as live commands. The keypad OLED continues to show its own firmware's battery/profile information.
Setup keeps the display awake using the current activity timestamp. Completing setup starts a fresh ten-minute idle interval, even when calibration took longer than ten minutes.

The earlier browse/read knob mode was replaced by unconditional agent browsing at the user's request. Regression coverage now includes turns before and after explicitly opening an approval and after synchronized review updates; only the top keys scroll.

### Default knob press switches layers

The manufacturer's default Flake keymap assigns normal-layer row 0, column 2 (the knob press) to `TO(1)`, with `TO(0)` at the same position on the function layer. Those are local keyboard actions and emit no host key press. The tested PiparFlake USB device (VID `1d50`, PID `6170`, VIA protocol 13) had these exact bindings.

For Token Café, change **only layer 0, row 0, column 2** from `0x5201` (`TO(1)`) to `0x0068` (`F13`) through VIA. Back up both layers first and read them back afterward to verify every other binding remains unchanged. This gives the knob a distinct input; it no longer enters the function layer. Leave the function-layer `TO(0)` binding intact so a keypad already on that layer can return to normal with one press. Then unplug keypad USB and complete board setup over Bluetooth. The first press may only return to the normal layer; the next emits F13.

On the physical keypad configured on 2026-10-31, both layers were backed up locally to `target/pipar-keymap-before.json`; the knob change was read back and all 17 other bindings matched the backup. After USB was unplugged, the user confirmed that pressing the knob advanced the board to the left-rotation prompt. Both rotation steps subsequently completed. A setup activity-timestamp bug caused immediate sleep at completion; the user confirmed tapping restored the café. The timestamp fix passed the core suite (61 tests) and was flashed with verification while preserving NVS. The user then confirmed both physical app shortcuts: bottom-middle brought Claude forward and bottom-right brought Codex forward. The original knob binding can be restored through VIA without resetting the keyboard or Bluetooth bonds.

## Legacy Bluetooth OLED integration (installed; live OLED verified)

The implementation is in `firmware/keypad/`, with a pinned build script at `tools/build_keypad.sh`. It preserves VIA and uses a separate encrypted GATT service for OLED updates, avoiding changes to VIA's message dispatch. `firmware/shared/keypad_display.h` defines the shared protocol. The engine exports four 16-character ASCII rows with the selected agent, status, and current press hint; knob turns always browse. The keypad adds its own battery percentage to row one; USB mode displays `USB ACTIVE` and `UNPLUG FOR CAFE`.

Service UUID: `f6cafe00-7f67-4cd9-9b7d-58381d8de601`; characteristic UUID: `f6cafe01-7f67-4cd9-9b7d-58381d8de601`. An encrypted read must return `TCO` followed by byte `1` before the board sends anything. Each acknowledged write is exactly 20 bytes: version, frame sequence, row index, 16 printable ASCII bytes, and a zero reserved byte. Four ordered rows within two seconds publish one complete screen. The board coalesces changes, sends at most five frames per second, and refreshes every two seconds; eight seconds without a complete update clears stale information. BLE errors disable OLED sending without disconnecting working HID controls. The display renders on its dedicated queue, not in Bluetooth callbacks; screen updates never write flash.

Build with `bash tools/build_keypad.sh`. The script uses a pinned Docker image, pinned VIA revision, and `firmware/keypad/west-lock.yml` for resolved dependencies. Output: `target/pipar-reference/zmk-via/build/cafe/zephyr/zmk.uf2`; the prepared deployment copy is `target/keypad-oled/token-cafe-pipar.uf2`. The `nice_nano_v2` build uses the seller's published shield wiring, the existing two-layer keymap with F13, and the backed-up VIA encoder codes (parameters are QMK codes, not ZMK HID usages). The keypad exposes the matching nice!nano nRF52840 bootloader. Serial deployment checks VID/PID and the exact device serial before uploading.

Validation on 2026-10-31: 65 companion-core tests passed, including OLED browse/read/offline behavior. The portable frame parser passed AddressSanitizer/UndefinedBehaviorSanitizer checks for incomplete, out-of-order, expired, malformed, and wrapping-sequence frames. Both complete firmware builds passed. Keypad use: 275,784 bytes flash, 53,988 bytes RAM. Board firmware was flashed with hash verification, and the running companion reported `TokenCafe-6336 synced v2` afterward.

The keypad was successfully flashed over **USB serial DFU** on 2026-10-31. The official nice!nano bootloader supports this separate firmware-update interface; no mounted disk is required. `adafruit-nrfutil` 0.5.3.post16 reported `Device programmed` after 16.7 seconds. The keypad restarted as `PiparFlake` (VID `1d50`, PID `6170`, same serial `651A6626B5AE5931`). The package contained only the 275,784-byte application, address range `0x26000..0x69547`; its binary payload was checked byte-for-byte against the built HEX, BIN, and UF2. It contains no SoftDevice or bootloader image. Application SHA-256: `8e79b439975c76b207cfa4f86a1a58ebdd48b18822a255c297983b9430d473a4`. Log: `target/keypad-oled-serial-flash.log`.

Post-flash VIA readback matched protocol, firmware version, keycode version, both encoder mappings, all 1,024 macro bytes, and 17 of 18 keys. The persisted knob binding decoded as unassigned after the build change; restoring only layer 0 / row 0 / column 2 to F13 (`0x0068`) made all 18 keys match the backup. This illustrates why settings must be read back after changing firmware even when the settings partition is preserved. The later rendering fix and hardware evidence are recorded below; the USB screen and live Bluetooth agent display are now user-confirmed; encoder direction/detents and browse/read behavior still require acceptance.

The earlier UF2-drive attempt was blocked by this Mac's managed external-media policy: the valid 33.7 MB FAT16 `NICENANO` drive failed mounting with `SUIS premount dissented`, and managed `mount-controls.harddisk-external` is `deny`. That restriction applies to the disk route. Serial DFU worked through the normal CDC interface without raw disk access, manual mounting, or any policy change. Original application readback through `CURRENT.UF2` remains unavailable.

To reproduce the application-only serial upload after building:

```sh
python3 -m venv /tmp/token-cafe-keypad-tools
/tmp/token-cafe-keypad-tools/bin/python -m pip install adafruit-nrfutil==0.5.3.post16
/tmp/token-cafe-keypad-tools/bin/adafruit-nrfutil dfu genpkg \
  --dev-type 0x0052 \
  --application target/pipar-reference/zmk-via/build/cafe/zephyr/zmk.hex \
  target/keypad-oled/token-cafe-pipar-dfu.zip
/tmp/token-cafe-keypad-tools/bin/python -m serial.tools.list_ports -v
```

Enter the keypad's bootloader, then verify its exact USB identity and application package before writing. The observed port was `/dev/cu.usbmodem11101`; enumerate again rather than assuming that path remains stable. Do not target the Token Café ESP32 board's serial port.

```sh
/tmp/token-cafe-keypad-tools/bin/adafruit-nrfutil --verbose dfu serial \
  --package target/keypad-oled/token-cafe-pipar-dfu.zip \
  -p /dev/cu.usbmodem11101 -b 115200 --singlebank
```

Official references: [nice!nano interfaces](https://nicekeyboards.com/docs/nice-nano/), [Adafruit serial uploader](https://github.com/adafruit/Adafruit_nRF52_nrfutil), and [bootloader source](https://github.com/adafruit/Adafruit_nRF52_Bootloader).

The portable package is `target/keypad-oled/token-cafe-pipar-oled.zip`. It contains the custom UF2, application-only serial DFU package, checksums, installation notes, and seller-published recovery image labeled as an unverified fallback (not a backup of the installed application). Existing keymap and macro backups remain local; macros are in the private `target/pipar-reference/recovery/` directory and are excluded from the package. On a machine that permits the recovery drive, `INFO_UF2.TXT` and `CURRENT.UF2` can additionally be saved privately.

### OLED rendering fix and hardware diagnostics

The user's photo initially showed the stock ZMK battery/profile/`default` layer screen even after the custom application was installed. The board confirmed `OLED v1 connected` and acknowledged display writes. A read-only health characteristic (`f6cafe02-7f67-4cd9-9b7d-58381d8de601`) isolated rendering from transport: driver ready, initialization error zero, UI initialized, but only one refresh. This custom screen inherited LVGL's 2,048-byte heap; ZMK only supplies its larger heap default for the built-in screen. Raising `CONFIG_LV_Z_MEM_POOL_SIZE` to 8,192 and explicitly making the screen background opaque restored recurring refreshes. The same hardware health read then reported **86 refreshes**, with no driver error. The user then confirmed that the USB status screen works and live agent information appears after unplugging USB. This verifies physical rendering and the board-to-keypad Bluetooth display path. Encoder direction/detents and the previously reported browse-versus-scroll interaction remain separate acceptance checks.

The health characteristic returns 12 encrypted-read bytes: `TCD`, version 1, driver-ready flag, driver initialization error, UI-initialized flag, reserved zero, then a little-endian 32-bit refresh count. It contains no agent names or session content. The board logs it once per connection; older keypads without this characteristic continue to work.

The fixed application was uploaded successfully using serial DFU. Final readback matched all 18 key bindings, four encoder mappings, 1,024 macro bytes, and VIA versions. The interrupted diagnostic transfer was recovered after a physical power cycle; the keypad is back in its application, not stuck in recovery. Temporary USB diagnostics were removed after the Bluetooth health read worked. The final build uses 276,032 bytes of flash and 60,164 bytes of RAM.

Evidence: `target/keypad-oled-health-flash.log`, `target/keypad-oled-health-board-flash.log`, `target/keypad-oled-health-board-diagnostic.log` (one refresh before fix), `target/keypad-oled-render-fix-board-diagnostic.log` (86 refreshes afterward), and `target/pipar-reference/usb-settings-after-oled.json`.

Uploader caution: `adafruit-nrfutil` can return exit code zero after an upload error. Require `Device programmed.` in the log and confirm application enumeration. A temporary launcher increased its one-second acknowledgement timeout to ten seconds for host USB scheduling pauses; this changes only host waiting time, not the wire protocol. A partially interrupted transfer may require a physical power cycle before retrying. No bootloader update, mass erase, or disk-policy change was performed.

### Earlier design and source investigation

The manufacturer's Flake shield specifies an SSD1306 128×32 display. Use it as a compact remote-control status screen, with four lines in a 6×8 font (at most 21 characters per line). Proposed examples:

```text
BROWSE  3/8       85%
anduin-helpcenter-3c
NEEDS INPUT
TURN:AGENT PUSH:READ
```

```text
READ    3/8       85%
anduin-helpcenter-3c
NEEDS INPUT
TURN:PAGE  PUSH:MAC
```

The board must supply the agent, status, and actual input mode; the keypad must not infer mode from its last key press or from a pending approval. Battery comes from the keypad. Agent names truncate to one line. Brief shortcut feedback can show `OPEN CLAUDE` or `OPEN CODEX`, but must not claim the desktop app opened without a host acknowledgement. An unavailable desktop target replaces `PUSH:MAC` with `NO DESKTOP TARGET`. On loss of the board link, show `CAFE DISCONNECTED` and clear stale agent details. USB mode should explicitly say `USB ACTIVE` / `UNPLUG FOR CAFE` so the user knows which device receives input.

Live status needs a supported board-to-keypad data path and custom keypad display firmware. The installed keypad exposes VIA protocol 13; the manufacturer's published ZMK source has not yet been matched to that installed build. Standard VIA keymap commands do not define a general text-display API. USB identification, a matching firmware source/target, and a recoverable original image are required before deploying a replacement. No OLED firmware changes have been installed. The previously reported browse-to-scroll issue also remains awaiting physical diagnosis; this display must expose the actual mode rather than mask that issue.

References: [manufacturer shield definition](https://github.com/RasmusKoit/zmk-keyboard-pipar/blob/main/boards/shields/pipar_flake/pipar_flake.dtsi), [ZMK custom display configuration](https://zmk.dev/docs/config/displays).

### Seller firmware identified on 2026-10-31

The seller publishes [the VIA-enabled UF2 and matching JSON](https://github.com/Hieupham0012/Guide_Pipar_Wireless). The JSON matches the connected device's `1d50:6170` USB IDs and 3×3 matrix. A read-only USB inspection reports VIA protocol 13, firmware version 1, keycode version 8, and two layers. All current bindings match the previous backup except the intentional knob change to F13; both layers' encoder bindings are volume down/up.

Local inspection artifacts are in `target/pipar-reference/`: `seller.uf2`, `via.json`, `usb-settings-backup.json`, and `provenance.json`. The downloaded UF2 is 608,768 bytes, nRF52840 family `ada52840`, application range `0x26000..0x70500`, SHA-256 `c599e7d0be2210e49a4d75a4f45140c137871ee73288cdb02e912b6d911d5a4b`. Its strings include Zephyr 3.5.0 and `via_compat`, `via_encoder`, and `via_macro`. Matching USB IDs do not prove the downloaded image is byte-identical to the installed firmware.

The seller's [pipar source repository](https://github.com/Hieupham0012/pipar) uses upstream ZMK v0.3; [pipar_flake](https://github.com/Hieupham0012/pipar_flake) uses upstream main. Neither inspected source tree includes the VIA implementation present in the published binary. The VIA JSON exposes key remapping but no OLED controls. The exact source/configuration that produced `pipar_flake_zmk.uf2` remains unidentified, but the subsequent independent VIA source discovery below provides an alternative implementation route. No keypad firmware or settings were changed during this inspection.

### Independent VIA source and OLED transport route

Further GitHub code search found [K4zoku/ZMK-VIA](https://github.com/K4zoku/ZMK-VIA), inspected at commit `9f8796671ae5843277e0daaaf711819499ad4a90`. Its `config/src/via_zmk.c` reports the same protocol 13, firmware version 1, and keycodes version 8 as the connected keypad. Its `tests/fixtures/providers/pipar/pipar_custom_keycodes.c` implements the exact observed `0x7e00..0x7e03` Bluetooth next, Bluetooth clear, reset, and bootloader mappings. Its `via_compat`, `via_encoder`, and `via_macro` names also occur in the seller binary. These are strong compatibility clues, not proof of an identical build.

The module pins ZMK `edf5c0814fd3ea202e43aad2d68fd32e882a518c` and [zzeneg/zmk-raw-hid](https://github.com/zzeneg/zmk-raw-hid) `6a37765dfab6197292e7a9f47305dcf87386d56a`, with tracked patches for VIA functionality and transport-specific responses. Raw HID supports USB and Bluetooth; its BLE output report requires an encrypted connection. The VIA native C test harness passed locally with AddressSanitizer and UndefinedBehaviorSanitizer. No ARM firmware build or hardware compatibility test has yet been performed for this stack.

Recommended integration:

1. Build a reproducible Pipar configuration using the published shield wiring, this pinned VIA stack, and the backed-up keymap/encoder bindings, including F13 on knob press. Verify the controller/bootloader and recovery image before deployment.
2. Add a custom 128×32 ZMK status screen and a versioned Café status command on Raw HID. The VIA listener currently consumes every 32-byte request, including unknown ones; explicitly route Café messages before its VIA dispatch rather than adding a listener whose order could make it ineffective. Preserve ordinary VIA traffic and replies.
3. Extend the board's existing keypad BLE client to identify the vendor output report and send bounded status updates. It currently subscribes to input reports and skips output reports. Negotiate a sufficient ATT MTU for a 32-byte report or implement an appropriate acknowledged long write; do not assume the default ATT payload can hold it. Probe Café display support before sending updates to a stock keypad.
4. Use board-owned selection and knob mode, sequence complete screen updates, coalesce changes, and clear stale state after disconnect/timeout. Render on the display work queue, not the Bluetooth callback. Do not persist transient screen updates to flash.
5. Validate USB/VIA, all eight keys, both turn directions, F13, Bluetooth reconnect, OLED sleep/wake, and the unresolved browse-to-scroll behavior before accepting the new firmware.

Other routes evaluated: [zmk-nice-oled](https://github.com/mctechnology17/zmk-nice-oled) demonstrates host-fed display widgets but primarily targets taller displays and would need a compact Café-specific layout; [zmk-dongle-display](https://github.com/englmaxi/zmk-dongle-display) supports 128×32 with reduced widgets but does not provide Café session data. A custom encrypted GATT characteristic is another transport option if keeping display traffic separate from VIA is preferable. None of these modules unlock arbitrary text on the existing binary without a firmware change. The missing seller VIA source is therefore no longer a prerequisite for developing a replacement that retains VIA functionality, although that compatibility still needs a full build and device verification.

## Build and verification

Use the existing ESP-IDF setup and production pairing header, then:

```sh
bash tools/qemu_v2.sh board-build
cargo test -p companion-core
cargo test --lib
clang -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined \
  firmware/semantic/test_keyboard_hid.c firmware/semantic/main/keyboard_hid.c \
  -o /tmp/token-cafe-keyboard-hid-test
/tmp/token-cafe-keyboard-hid-test
clang -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined \
  firmware/semantic/test_keyboard_layout.c firmware/semantic/main/keyboard_layout.c \
  -o /tmp/token-cafe-keyboard-layout-test
/tmp/token-cafe-keyboard-layout-test
sh tools/build_macos.sh
```

Install the matching desktop and firmware builds together. New app-launch action enum values 8 and 9 are bounded, authenticated navigation requests; older hosts reject them. Launch targets are fixed on the Mac, never supplied as commands or URLs by the keyboard. The C and Rust wire enum definitions are kept in sync.

Before physical deployment, save the complete original flash privately for rollback, preserving its pairing secrets. Normal flashing writes the application/boot partitions without erasing NVS. Serial `KEYPAD` messages report discovery, security, setup completion, and command numbers; they do not log typed key values.

Opening the board's USB diagnostic port can reset the ESP32-S3. Leave serial monitors closed during calibration and ordinary connection checks; use the companion's connection status instead.

The follow-up press-to-desktop behavior passed 64 core tests, including exact-session dispatch, duplicate-request suppression, unavailable desktop targets, and no approval decisions. The matching board firmware was flashed with hash verification; physical handoff verification is pending.

### Native voice lookup follow-up

After Accessibility was granted, the first two physical voice attempts reached the helper but timed out after about 8.2 seconds. The helper now enables Electron’s external accessibility interface and services the main run loop during activation waits. Failure diagnostics distinguish app, window, session, and voice-control lookup and report only counts/provider, without session text. The helper was rebuilt and replaced atomically; physical acceptance is pending. Reference: [Electron accessibility](https://www.electronjs.org/docs/latest/tutorial/accessibility).

The follow-up physical attempts reached Codex with Accessibility available and a populated UI tree, but stopped at session verification. Codex's installed source sets the document/native-window title, which the initial helper did not accept. Matching now includes those titles and child labels of headings, normalizes whitespace, handles the collector's 80-character display-name limit, and refuses title fallback for duplicate live titles. A recognized mismatching document route cannot be overridden by a title. Nine pure Swift lookup checks and the Rust duplicate-title regression passed. The companion/helper were rebuilt and reloaded, and `TokenCafe-6336 synced v2` was verified. The physical retest confirmed Claude native recording starts; the runtime log recorded an accepted Claude voice action. Codex reached the correct session but found no matching voice control. The user confirmed its voice button works when clicked manually.

Voice targets the agent selected **on the board**, not whichever desktop app is currently foreground. The fixed Claude/Codex app keys intentionally preserve the board's selection, so use the knob to change the voice target. Logs `target/keypad-voice-before-title-fix.log` and `target/keypad-voice-fix-build.log` record this diagnosis and build.

The next helper revision accepts normalized accessible labels exposed through AXValue or static-text children and traverses deeper native/web accessibility containers (bounded to 80 levels and 6,000 nodes). It retains exact start-only labels, unique enabled-control matching, session verification, and the foreground-app recheck. Thirteen pure Swift checks passed, and the helper was replaced without restarting the companion or board. Codex physical acceptance remains pending.

The expanded accessibility lookup did not resolve Codex reliably: one physical request still found zero start labels (1,116 nodes), although other requests reported AXPress acceptance. Acceptance alone does not verify session-scoped voice. The user tested the installed Control+Shift+V shortcut and reported that it starts the pet/agent voice rather than the current session. This shortcut must not be used as a fallback for the selected-session voice key; no shortcut fallback was implemented. Packaged source also registers a global new-thread fallback for this command.

### Correct session recording control

The user identified the working Codex button precisely: **Dictate**, beside the message box. The installed app source confirms `composer.dictation.aria` and `composer.dictation.tooltip` both resolve to `Dictate`. The helper now targets only that label for Codex; `Start Voice` and `Start voice chat` are explicitly excluded to avoid pet/global voice. Claude’s working native recording mapping is preserved. Eighteen pure Swift checks passed, including rejection of both wrong Codex voice labels and stop-dictation controls. The release helper was replaced atomically; no board or companion reload is required. Physical Codex dictation verification is pending.

### macOS Input Monitoring identity repair (2026-11-01)

The initial build retained Cargo/Swift linker signatures: the app had a compiler-generated identity, an unbound Info.plist, and no resource seal. The build now explicitly ad-hoc-signs both helpers with dedicated identifiers, then signs and strictly verifies the enclosing `local.token-cafe.desktop` bundle. The release path remains unchanged. This is local signing, not Developer ID signing or notarization; permission persistence across changed builds is not guaranteed. macOS must authorize the corrected app identity. Terminal launches were also attributed to the launching app in TCC diagnostics; use Finder to launch the release app for permission verification. Earlier enabled executable entries did not establish successful input capture. Physical key acceptance remains pending.

### Dedicated vendor USB input (2026-11-01)

TCC diagnostics eventually confirmed Input Monitoring was allowed for `local.token-cafe.desktop`, but opening the exclusive keyboard interface still failed. The previous generic status incorrectly described every open failure as a missing permission. Dedicated USB commands now replace that capture path entirely. No keyboard-device access or privileged daemon is used. The native parser passed 35 checks (all eleven commands and malformed/acknowledgement rejection), the standalone controller regression passed, and both firmware and release-app builds succeeded. Physical input acceptance remains pending.

Installed through serial DFU (`Device programmed.`, 16.7 seconds). Application SHA-256: `1142e773e5506a29c9366661baa597f8ea024e33b1f9e0e0006e7f161cb0b8f3`; 276,536 bytes. Readback matched all 18 keys, four encoder mappings, VIA metadata, and all 1,024 macro bytes. The new capability probe acknowledged both OLED and dedicated USB input. The normally launched release app reports **USB + OLED ready — Board: optional**, verifying the USB handshake in standalone mode. Physical navigation/shortcut acceptance is still pending. Logs: `target/keypad-vendor-input/flash.log`, `target/keypad-vendor-input-{build,desktop-build,tests}.log`.

User acceptance on 2026-11-01: with the keypad connected directly to the Mac by USB and the release companion in standalone mode (`Board: optional`), the user confirmed knob navigation changes the selected agent and both bottom-row app shortcuts work (Claude and Codex). This verifies the direct USB input path and those actions without a board connection. Native recording, theme/selection/scroll keys, and optional-board wake/synchronization remain separate physical acceptance checks.
