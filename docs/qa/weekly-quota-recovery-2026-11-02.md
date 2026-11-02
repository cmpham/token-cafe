# Keypad weekly quota recovery — 2026-11-02

## Cause and change

The Claude CLI Keychain credential had expired. Verified Claude Desktop Code processes also retained expired OAuth tokens from process launch, even though Desktop had refreshed its own login. The previous poller retried these bearers, discarded successful values on any failure, and hid the second Claude row when its percentage was unavailable. Sequential Claude requests could also delay Codex polling.

The Mac now prefers Desktop's existing encrypted `oauth:tokenCacheV2`, read through a dedicated signed `keypad-quota` helper and the existing `Claude Safe Storage` Keychain item. The user explicitly authorized read-only quota access and approved the macOS Keychain dialog. The user refreshed the separate CLI login through Claude's own `/usage` command.

The helper accepts only the macOS v10 Safe Storage format, bounded input, the current account, production Anthropic API host, and Code OAuth scopes. A verified live Code process supplies an organization selector when available. Ambiguous organizations fail closed. The provider profile must match the cached account/org before reading usage. Email never determines identity. No refresh credentials leave the helper; access tokens remain in memory/private pipes and are never logged, saved, renewed or changed. Normal polls cannot display a password dialog. The explicit `authorize` command prints access status only.

Quota requests run independently for CLI, Desktop and Codex. Existing Claude credentials are re-read every minute, successful quotas poll every five minutes, and values expire after their original ten-minute freshness interval. HTTP Retry-After is honored. A rejected bearer is not retried until its owner supplies a replacement. Transient failures retain still-fresh successful data. Distinct known Claude identities keep two unlabeled percentage slots through temporary quota failure; identical identities appear once using the fresher response. Account/token changes clear old quota data before querying the new identity.

The `--weekly-limits` command diagnoses the same sources without opening a window or claiming keypad ownership. It prints status and percentages only; run the stable bundle to find its sibling quota helper.

## Verification

- 239 workspace Rust tests passed, including expiration, rejected-token suppression, credential replacement, rate-limit backoff, profile identity mismatch, duplicate accounts, fresh response selection and two-slot OLED fallback.
- 196 Rust tests without default features passed.
- Strict workspace/all-target Clippy, formatting and diff whitespace checks passed.
- Native checks: 50 keypad transport/routing, 16 focused recording, 19 quota storage/selection checks, plus the OLED C frame parser under ASan/UBSan.
- Quota crypto fixture was generated independently with OpenSSL and a fictional password. Native tests do not access Keychain or running provider apps.
- Signed macOS bundle rebuilt and verified at `release/Token Café.app`.
- Live read-only checks returned CLI **95%**, Desktop **91%**, Codex **26%** remaining. Restarted app logs independently reported all three values; no credentials or account identifiers were logged.

## Reload and board compatibility

The app was restarted from its stable bundle with the existing production registry. Token Café's window title confirmed `USB + OLED ready; Bluetooth standby` and `TokenCafe-6336 synced v2`.

The physical board still had the renderer preceding the already-committed performance cleanup. Before reloading master, a private full 4 MiB board backup was taken and checked against the application metadata. The matching semantic board firmware was rebuilt and flashed **application-only at 0x10000**. Flash hash verification passed. Post-flash readback verified the first 64 KiB, including bootloader, partition table and pairing/NVS, were byte-for-byte unchanged. The shared renderer's full QEMU regression suite had passed during the cleanup review; this quota change does not modify that renderer or board input/decision code.

Private readbacks, firmware/backup hashes and deployment logs are under ignored `target/deploy-quota-recovery/`. Never publish them. Keypad firmware was unchanged and was not flashed. Transport readiness and host quota values were verified; no physical OLED photograph or new LCD/touch soak test was taken.
