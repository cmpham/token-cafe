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

## Rollback

Stop this checkout's app and restore the private pre-v2 full flash backup with
esptool at address 0, using the actual 4MB flash size. Restore the saved desktop
executable and launch the legacy path with its original pairing key. Do not erase
NVS or print the backup. This development run preserved both artifacts under
`target/esp/backups/pre-v2/`. Legacy mode is explicit; failed semantic authentication
never falls back to legacy approval behavior.
