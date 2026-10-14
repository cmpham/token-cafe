# Semantic companion setup and recovery

The desktop and firmware must use the same renderer asset digest and v2 schema.
Use a USB data cable for provisioning/flashing. Normal operation uses Bluetooth;
USB or an external battery supplies power only. The board stores its provisioned
secret in firmware and unresolved decision receipts in NVS. Keep full flash
backups private: they contain secrets. There is no OTA rollback on this 4MB board.

## Repeatable development sequence

1. Read `companion-v2.md`, `review.md`, and `test-plan.md`.
2. Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
3. With the repository's configured ESP Rust/IDF environment, run
   `bash tools/qemu_v2.sh build` then `bash tools/qemu_v2.sh test`.
4. Preserve the board flash and current desktop executable before replacing either.
5. Provision a separate QA identity:
   `python3 tools/provision_semantic.py --class qa --id tc-6336-qa --name TokenCafe-6336-QA`.
6. Run `bash tools/qemu_v2.sh board-qa-flash /dev/cu.usbmodem2101`,
   `cargo build --release --example protocol_board_qa`, then use the configured
   ESP-IDF Python to run `tools/test_protocol_v2_board.py --port /dev/cu.usbmodem2101 --count 100`.
   This host is synthetic: it has no approval broker or desktop opener.
7. After passing QA, provision production:
   `python3 tools/provision_semantic.py --class production --id tc-6336 --name TokenCafe-6336`.
   Run `bash tools/qemu_v2.sh board-flash /dev/cu.usbmodem2101`.
8. Build the matching app (`sh tools/build_macos.sh` on macOS, `cargo build --release`
   on Linux), then `bash tools/run_companion.sh`. Default is small and unpinned on Mac.

Generated registries and keys are under ignored `target/esp/v2-pairing/` with private
permissions. `firmware/semantic/main/pairing.h` is private and ignored. Provisioning
reuses an existing identity's key, and rewrites that trust-class registry to contain
that one device. To manage several boards, explicitly maintain a registry containing
one to four unique device entries; do not provision over your only registry copy.
`TC_BLE_REGISTRY` overrides the default `~/.token-cafe/boards.json`. The local launcher
selects this checkout's production registry. The running companion does not require USB.

Each registry entry contains `device_id`, advertised `name`, `key_path`, and
`trust` (`production` or `qa`). Discovery names are hints: the exact identity and
key proof authenticate the link. QA keys cannot authorize live provider decisions.
Production rejects peers advertising test injection. The production build compiles
out USB injected-touch and framebuffer-capture commands. Never print or commit keys.

## Diagnosis and evidence

Window title reports the BLE connection/synchronization state. A connected radio
is not enough: the host waits for an applied model revision before reporting sync.
The QA suite writes `results.json`, `latency.json`, `actions.jsonl`, serial/host logs,
and six screen captures. LCD completion timestamps measure software presentation,
not physical photon response. RAM screenshots cannot detect wiring or panel color faults.
The latest test evidence and remaining gaps are in `validation.md`.

Linux and macOS share the protocol and renderer. Desktop-session opening is currently
macOS-only; Linux sends `navigation_available=false`. Physical Linux BLE, multiple
simultaneous boards, battery behavior and RF interference need hardware-specific QA.

## LCD orientation and color settings

The JC4827W543 uses the shared NV3041A adapter in `firmware/ble/main/board.c`,
including when built through `firmware/semantic`. Preserve this combination:

| Setting | Required value | Verification |
| --- | --- | --- |
| Orientation | MADCTL (`0x36`) = `0xc0` | Boot `LCD: REG 0b = c0 c0 c0 c0` |
| Color polarity | Inversion ON (`0x21`) | Boot `LCD: REG 09 = 60 13 24 00`; inversion bit `0x20` is set in the third byte |
| Touch axes | `x = 479 - raw_x`, `y = 271 - raw_y` | Physically tap patrons in all four corner cells |

Despite the command name, inversion ON produces normal colors on this panel.
Inversion OFF (`0x20`, status third byte `0x04`) produced negative colors during
the October 14 hardware check. Do not infer the correct polarity from the word
"inversion" or copy the previous inversion-OFF instructions.

Apply MADCTL and inversion as standard commands **after** the initialization
table closes the vendor page with `0xff = 0x00`. Keep `panel_orientation()` as
the common startup, wake, and periodic restoration path; do not wrap its commands
in vendor-page selection. Keep the initialization table's MADCTL consistent.
Changing orientation also requires checking the GT911 mapping and gesture tests;
a color-only correction must not change orientation or touch coordinates.

For a recurrence:

1. Record the running app's ELF hash from serial boot output and compare it with
   the intended build. Preserve the existing flash before replacing firmware.
2. Read the boot LCD diagnostics at 115200 baud. The ID should be `30 41 a1`;
   compare orientation and status with the table above. The probe reads the
   bidirectional SDA line at 1 MHz. Build success and flash hash verification
   alone do not establish that the LCD accepted its settings.
3. Build and flash the semantic companion with `tools/qemu_v2.sh board-build`
   and `board-flash PORT`; use the actual connected port. Do not replace a v2
   companion with the legacy `tools/firmware.sh display-flash` image.
4. Confirm upright text, normal dark-cafe colors, and physical corner-cell touch
   selection. Repeat after reset and backlight sleep/wake. RAM screenshots,
   QEMU, and injected touch cannot verify physical panel polarity or alignment.

Keep physical confirmation separate from register readback in `validation.md`.

## Rollback

Stop this checkout's app and restore the private pre-v2 full flash backup with
esptool at address 0, using the actual 4MB flash size. Restore the saved desktop
executable and launch the legacy path with its original pairing key. Do not erase
NVS or print the backup. This development run preserved both artifacts under
`target/esp/backups/pre-v2/`. Legacy mode is explicit; failed semantic authentication
never falls back to legacy approval behavior.
