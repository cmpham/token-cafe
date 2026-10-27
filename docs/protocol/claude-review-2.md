**The design gate is not ready.** The core approval fixes really are in the spec. These are the request-digest preimage (§4 L206–218), button arming (L290–297), the per-device auth transcript and HKDF (L85–98), the exact AEAD layout (L101–109), whole-Session upserts (L174–178) and deltas based on the last revision sent (L180–183). But a few of the accepted fixes add new contradictions. The fixed byte caps don't add up. The QA/production profile rule can't be satisfied together with the test plan. And `test-plan.md` was never updated, so its own A04 check ("No unresolved approval safety, framing, identity, or atomicity blockers", `test-plan.md:12`) can't pass.

I ran nothing and didn't hash the files. Line numbers refer to `companion-v2.md` unless a file is named.

## Blockers

**N1 — F1 not resolved: the stated caps overflow the 4096-byte envelope.** The 4096 limit is at L60. A Delta allows 2 whole Sessions plus 16 removes plus a View (L171–173).
- 16 removes × ~130 B is about 2.1 KB.
- A worst-case Session is about 1.2 KB: id, title, display_name and branch at 128/192/192/192, plus a Request with id 128 and title 192.
- A View is about 0.4 KB.
- That totals about 5 KB. `Request.desktop_only_reason` has no bound (L202, `companion.proto:161`), so even a two-session SnapshotPart (L160) has no guaranteed upper size.
- **Fix:** state the cap as an encoded-byte rule ("if the encoding is over 4096, use a snapshot"), lower removes to about 8, and bound every string.

**N2 — The F17 change (all host fragments use write-with-response) breaks the F1 budget at the minimum MTU.** The minimum ATT capacity is still 64, so each fragment carries 28 B (L105–106). Write-with-response allows one write per connection event (L114–116).
- That gives roughly 1–2 KB/s.
- A 64-session snapshot (~32 parts × ~2.5 KB ≈ 80 KB) then takes 40–90 s. That exceeds the 30 s transaction timeout (L125), so the board resyncs forever.
- One 4 KB envelope takes 2–4 s, which blocks heartbeats and actions, since they only interleave at message boundaries (L112). The ≤200 ms convergence target (L422) can't be met.
- The UART simulation (L448–449) doesn't model per-write round trips, so it would never catch this.
- **Fix:** raise the minimum capacity to ≥185 (fail with `ERROR_UNSUPPORTED_MTU` below it), or publish a per-capacity worst-case transfer budget. Add connection-interval latency to the UART shim, and state that the latency targets assume a minimum MTU.

**N3 — F5 is contradictory, and its key-based check is missing.**
- L303–304 makes the build fail if "QA injection and real approval capability" are both enabled. But the firmware can't tell real approvals from synthetic ones; it only has `CAP_APPROVAL_ONCE`. That rule therefore stops the QA image from running the DECIDE tests that `test-plan.md:72` (Q06) requires.
- `test-plan.md:95` (Gate C step 4) runs Q02–Q10 "through the firmware-injected production gesture path". L299 says production builds compile injection out.
- The host relies on the board reporting `input_injection_enabled` about itself (L146–147, L300). Nothing stops a QA image from carrying a production key.
- **Fix:** mark each key in the host key store as `qa` or `production`. The live broker refuses DECIDE from QA-class keys no matter what the board reports. QA firmware may only be provisioned with QA keys. Rewrite Gate C step 4 to use real touch.

**N4 — Gate A paperwork can't close.**
- The B1/B2/C tables in `test-plan.md` have no cases for the remedies that were accepted:
  - F1 worst-case snapshot at capacities 64/185/244
  - F2 golden vectors for the request digest
  - HKDF/AEAD golden vectors
  - F3 triple tap / arming
  - F5 refusing a QA link
  - F8 result arriving before its delta
  - F9 action_id reuse
  - F10/F11 parity and double tap across a swap
  - F13 pipelining
  - F14 blob churn
  - F19 UNCERTAIN surviving a new epoch
  - F22 wrap golden vectors
  - fuzzing with ≥10⁶ executions
- `test-plan.md` A01 requires documented bounds for every field, and N1 shows several are missing.
- `review.md:4-5` says the *original* spec hash is recorded, but L38–39 label the hashes as the current file paths, so it's unclear which version was reviewed (A02).
- The spec has one Gate B (L431); the test plan splits it into B1 and B2.

## High (approval safety, identity, sync)

- **H1 — F19 is only partly fixed.**
  - The UNCERTAIN receipt survives across epochs (L341–343), but `request_id` is only unique within one authority epoch (L53). After a host restart a reused ID could match the old receipt. Key receipts by `(authority_epoch, request_id)` and never match across epochs.
  - L407 says the board "fetches authoritative request state", but no message does that.
  - How long a terminal Request stays in `Session.approval` is undefined.
  - Bound the number of receipts and define the dismiss gesture.
- **H2 — F6: the tool name can still sit off-screen.** L67–68 put the full tool name and title in the body, and L460 puts an escape-explanation preamble there too, with no order between them. Nothing requires the first displayed page to show the tool name. Require the full tool name and title before the preamble on page 1. Separately, `CAP_UTF8_LITERAL` (L141, proto L36) contradicts the mandatory `\u{…}` escaping in L457 and reopens homoglyph spoofing. Exclude it from approval bodies or drop it.
- **H3 — Legacy v1 approval path.** L77–78 and L447 keep the legacy endpoints "for rollback". The spec doesn't say whether v2 production firmware compiles out the v1 frame-fingerprint DECIDE path. If it doesn't, there are two approval paths with different safety guarantees. Require it to be compiled out, or make v1 non-actionable.
- **H4 — One request per session.** `Session.approval` holds a single Request (L201). Claude can raise concurrent hook prompts for parallel tool calls. Define what happens: extra requests make the session DESKTOP_ONLY, or the host serializes them.
- **H5 — F4 auth payloads aren't on the wire spec.**
  - L85–91 don't say how the 16-byte host nonce and the 16-byte host tag (both written to the same characteristic) are told apart. They also don't say whether the board returns its nonce+tag by read or by notify, or how a result is signalled.
  - Hello's `device_id` (L139) isn't required to byte-equal the authenticated `device_id`. Require it, or send ERROR.
- **H6 — F13: deltas after a Resync.** L152–154 and L167 don't say that the board drops in-flight deltas after sending Resync until the next SnapshotCommit. They also don't say the host stops sending deltas when it receives Resync. Without that, deltas still in flight trigger REVISION_GAP again and the resync repeats.
  - The anti-rollback rule at L168 doesn't say it is reset when the authority epoch changes; revision restarts at 1 per epoch (L50).
  - "Equal revision … identical content" contradicts the STATE_INVALID recovery path.

## Medium (bounded memory and schema)

- **M1 — Unbounded fields:** `desktop_only_reason`, `BlobUnavailable.reason` (proto L196, should be an enum), the `capabilities[]` and `required_capabilities[]` counts, `asset_digest` length, and `allowed_decisions` count. Nanopb `.options` can't be written from the spec until these have bounds (L417).
- **M2 — Per-cell source map (L461):** it is unbounded in practice. A 32 KB body of control bytes expands to 128K cells, which is about 512 KB per blob, with up to 5 blobs cached (L59). Store only per-line start offsets, computed lazily.
- **M3 — F22 wrap is still ambiguous (L463–466):** it doesn't say whether lines break hard at 54 cells or at words. It also doesn't say whether a trailing LF creates an empty line. The golden fixtures are tests, not the definition.
- **M4 — F20:** OPEN_REVIEW for UNLINKED_PERMISSION needs a `blob_id` (L273), but Session has no field the board could take it from (L186–188).
- **M5 — F10 allocator:** the spec doesn't define who gets seats when there are more than 8 sessions (seat 255, L57). It also doesn't define which free seat an arrival takes (L363–374). Either define both, or name the desktop function as the normative source.
- **M6 — F8 result timing:** results must follow their deltas (L311), but deltas can be coalesced or held behind a snapshot (L166, L183). The spec doesn't say the result is deferred with them. It also doesn't define `applied_revision` for REJECTED or no-op results.
- **M7 — F14 bandwidth:** only *unselected* task hashes are rate-limited (L262). A selected task changing at 1 Hz with 32 KB blobs saturates the link at low MTU (see N2). The host's 512 KiB-per-link bound (L252) has no cap on the number of links N.
- **M8 — F18:** "critical" outbound messages (L62, L331) aren't defined.

## Low

- The DECIDE row (L278) leaves out `displayed_body_sha256`, which L214 requires. It also lists "armed production input" as a host check, but the host can't verify it.
- The digest rejects empty decision lists (L210), so a DESKTOP_ONLY request's digest is undefined.
- The connect example (L381) puts epochs inside HelloAck, but they are Envelope fields known from HKDF.
- `WAIT_NONE`, `REASON_NONE` and `delivered_decision=0` are valid zeros, which contradicts L42 and `proto:5`. List them explicitly.
- The snapshot digest (L162) concatenates envelopes without length prefixes and adds nothing beyond AEAD with contiguous counters. Length-prefix the entries or drop it.

## Status by finding

| Status | Findings |
|---|---|
| Resolved | F9, F11, F12, F16, F21, F24, F25, F26 |
| Partially resolved | F1 and F17 (N1, N2), F2 (Low), F3 (H2), F4 (H5), F6 (H2), F7 (H4; LRU size), F8 (M6), F10 (M5), F13 (H6), F14 (M7), F15 (Low), F18 (M8), F19 (H1), F20 (M4), F22 (M3), F23 (N2) |
| Regressed or contradictory | F5 (N3) |

Gate A needs N1–N4 and H1–H6 fixed in the spec and test plan, then a re-review against the new file hashes.
