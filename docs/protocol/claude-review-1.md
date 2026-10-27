# Independent review: Token Café companion protocol v2 (`docs/protocol/companion-v2.md`)

**Reviewer:** Claude Code (Opus 5.5) • **Baseline:** `cmpham` @ `ff2ff33` • **Mode:** read-only. I edited no files, ran no builds, accessed no provider sessions and acted on no approvals.
**Inputs read:** `companion-v2.md`, `validation.md`, `src/companion.rs`, `src/approval.rs`, `src/review.rs`, `src/scene.rs` (seating, hit testing, review input), `src/app.rs` (decision path), `firmware/ble/main/{main.c,board.c,double_tap.h,touch_gesture.h}`, `firmware/ble/sdkconfig*.defaults`.

## Verdict

The architecture is sound: the desktop owns the model, the board renders locally, actions are tied to identity, and there's no offline approval queue. But the spec is **not ready for Gate B implementation**. Six problems block it:

- Its liveness and size limits can't both hold on BLE.
- The approval-digest design doesn't prove what the board displayed.
- The review opens directly under the user's finger, so a tap meant for something else can approve.
- It assumes multiple boards, but there's no multi-key identity model for them.
- Desktop/board parity contradicts the current scene code.
- Several state-machine edges have no defined resolution, which leaves intents stuck forever or allows wrong-target actions.

Line numbers below refer to `companion-v2.md` unless a file is named.

---

## Blockers (must be resolved before Gate B)

| # | Blocker | Finding |
|---|---|---|
| B1 | The 64 KB envelope, one-message-at-a-time sending and the 3 s liveness timeout contradict each other at legal MTUs | F1 |
| B2 | The request digest is "canonical protobuf" that the board can't reproduce, so DECIDE proves nothing about what was displayed | F2 |
| B3 | The review opens under the finger; a third tap lands on APPROVE ONCE | F3 |
| B4 | No pre-auth device identity or per-device key, yet the two-board race is a required case | F4 |
| B5 | Production firmware has no rule against touch injection (QA USB) | F5 |
| B6 | Seat/animation parity contradicts `scene.rs` (600 ms swap vs ~3 s walk; seat allocation depends on animation) | F10 |

---

## Findings

### F1 — BLOCKER — Envelope size vs liveness vs head-of-line blocking
**Where:** §2 L60 (envelope ≤ 65536), L62; §3 L93–98 (min ATT capacity 64, effective data = capacity − 36, "Send one message at a time per direction", priority only "at message boundaries"), L105–108 (1 s heartbeat, 3 s without "a valid authenticated incoming **envelope**" → disconnect); §2 L68–69 (2 s reassembly expiry).

**Counterexample:**
- At the minimum legal capacity of 64 bytes, each fragment carries 28 bytes. A 50 KB Snapshot is about 1,800 fragments. Even at 4 fragments per 15 ms connection event, that takes about 7 s.
- During that time no complete envelope arrives, and the Heartbeat can't be inserted mid-message (L97–98). The receiver therefore declares the link dead at 3 s and reconnects. The reconnect sends another full Snapshot, so the cycle repeats indefinitely.
- 64 sessions × (id 128 + title 192 + Request id 128 + title 192 + digests and refs) ≈ 45–55 KB, so this envelope is reachable.
- "Send progress/heartbeat even while transferring long text" (L108) can't be satisfied for a single large Snapshot envelope.

**Remedy:**
- Define liveness as "any AEAD-valid fragment".
- Cap the envelope at about 8 KB, and make Snapshot a multi-message transaction (`SnapshotBegin{revision,count}` / `SnapshotPart` / `SnapshotCommit{digest}`) that is published atomically at commit.
- Send only the seated sessions plus sessions with requests plus the selected session (≤ ~16), with `overflow_count` for the rest, instead of 64.
- Raise the minimum capacity to ≥ 185, or compute a worst-case transfer budget per MTU and fail with UNSUPPORTED_MTU below it.
- Turn the L69 reassembly expiry into a link error. On a reliable, strictly sequenced stream an abandoned message means desync, not something to drop silently.

### F2 — BLOCKER — The request digest can't be checked on the board, so it proves nothing about what the user saw
**Where:** §4 L158–161 ("canonical protobuf RequestIdentity"), invariant 5 (L28–29), DECIDE row L203.

**Problem:**
- Protobuf has no canonical encoding. prost and nanopb differ on packed vs unpacked repeated enums, default-value omission and unknown-field retention.
- The spec never says the board computes the digest, so the natural implementation echoes `approval.digest` from the model.
- In that case a board bug, such as a stale body blob still pinned from a previous request or a wrong cache-key lookup, sends a "matching" digest for content that was never displayed.
- v1 had a real display binding: `src/app.rs:441` accepts a decision only when the board's presented-frame fingerprint equals the host's render. v2 removes it and puts nothing in its place.

**Remedy:**
- Specify RequestIdentity as a hand-defined, length-prefixed byte string, e.g. `"tc-req-v2" ‖ u16 len ‖ request_id ‖ … ‖ sha256(body) ‖ u32 body_len ‖ sorted decision bytes`, with golden test vectors shared by Rust and C.
- Require the board to recompute the digest from the bytes it actually rendered (title + verified body blob), and to refuse DECIDE if the result differs from the model's digest.
- Add `displayed_body_sha256` to DECIDE.

### F3 — BLOCKER — A tap meant for opening the review can approve
**Where:** §4 L207–210 (double-tap the footer to open the review; "Buttons accept single taps only"), §5 L246.

**Code facts:**
- Footer hit band is `y ≥ 212` (`src/assets.rs:6`, `scene.rs:690`, `x < 436`).
- Review buttons are at `y 220–264`: DENY at `x 12–156`, APPROVE ONCE at `x 168–432` (`review.rs:47`, `scene.rs:676–681`).
- After a double tap, `double_tap.h:13` clears `pending`, so the next tap is a fresh single tap.

**Counterexample:**
1. The user triple-taps the footer at (300, 240).
2. Tap 1 does nothing (`scene.rs:845–851`).
3. Tap 2 opens the review.
4. Tap 3 is a single tap at (300, 240), inside APPROVE ONCE, and sends DECIDE(APPROVE_ONCE).

The user never saw the body. The same thing happens when the desktop opens REVIEW through the shared View (L165–172) while the user is tapping the board.

**Remedy:** Make decision buttons *armed* only when all of these hold:
- the touch-down happened after the review frame finished displaying (LCD completion);
- at least 500 ms have passed since the review opened or the request changed;
- the touch-down is not part of the gesture or tap sequence that opened the review.

Also:
- Never arm buttons on a review the board didn't open itself, or exclude `page=REVIEW` from the shared view.
- Optionally move the buttons out of the footer band.
- Add this as a required acceptance case.

### F4 — BLOCKER — No device identity before authentication; per-device keys are missing
**Where:** §2 L49 (device_id "bound to the authenticated key"), §3 L74–78, §4 L120, invariant 7 (L31), example L285–291 and gate case L322 ("two-board conflicting decisions").

**Code facts:**
- There is a single `pairing_key.h` (`main.c:5`) and a single host key (`companion.rs:37`).
- The host refuses more than one match (`companion.rs:172`).
- `BT_NIMBLE_MAX_CONNECTIONS=1`.
- device_id travels in Hello, which comes *after* authentication.

**Problem:** To authenticate, the host must pick a key, but it learns device_id only afterwards. If two boards share one key, board A can claim to be board B, so "bound to key" means nothing. On macOS, btleplug exposes a CoreBluetooth UUID, not a MAC address, so the advertised name `TokenCafe-XXXX` is the only identity, and it isn't authenticated.

**Remedy:**
- Expose a non-secret device_id in the auth characteristic read (or the advertisement), include it in both HMAC transcripts and in the HKDF info, and look up a per-device key in a host key store.
- Define what invariant 7 means precisely (one host per board; N boards per host).
- If multi-board isn't in scope for v2, remove the two-board case from the gates and state the single-board limit.

### F5 — BLOCKER/HIGH — Touch injection can reach DECIDE
**Where:** §5 L254 ("QA-injected touches use the same gesture path and are test-only"); §8 L316.

**Code fact:** with `CONFIG_TC_QA_USB`, `qa_task` turns USB lines into touch samples (`main.c:252–258`, `board.c:117–127`).

**Counterexample:** A QA-profile image flashed for Gate C lets anything on USB (any local process that can open the serial port) inject a tap at APPROVE ONCE.

**Remedy:**
- Hello must carry a build-profile flag (`input_injection_enabled`).
- The host must refuse DECIDE (and present it as non-actionable) from any board reporting injection.
- The production profile must fail to compile with QA input and APPROVAL_ONCE both enabled.
- Add a gate check that the flashed image reports `injection=false`.

### F6 — HIGH — The tool name is truncated, and it is the security-relevant field
**Where:** §2 L56 (title ≤ 192 B), L65–66 ("Never truncate actionable commands"); §7 L297–298.

**Code facts:**
- The request title is `ALLOW {tool}?` (`approval.rs:98`).
- The body contains only `tool_input`, cwd and session (`approval.rs:99–103`); the tool name isn't in it.
- The review shows `title.chars().take(32)` (`review.rs:30`).

**Counterexample:** `ALLOW mcp__github__delete_repository?` is displayed as `ALLOW mcp__github__delete_reposi`. Any tool whose name differs only after 26 characters is indistinguishable.

**Remedy:**
- Put the tool name in the body, or make the title wrap without truncation.
- Mark a request DESKTOP_ONLY if any digest-covered field can't be displayed in full.
- Also define the escaping algorithm normatively: `approval::lines` escapes non-ASCII as `\u{…}` (`approval.rs:150–172`), which contradicts L65 ("characters our font cannot faithfully display → DESKTOP_ONLY").
- `str::lines` silently drops a `\r` before `\n`; decide whether that counts as faithful.
- Consider requiring the user to scroll to the end of the body before APPROVE ONCE arms.

### F7 — HIGH — ACCEPTED overstates what the adapter knows
**Where:** §4 L162, L236–237; example L289–290.

**Code fact:** "Delivered" means `writeln!(stream)` succeeded (`approval.rs:137`), i.e. the kernel buffered the bytes for the hook process.

**Counterexample:** The hook process is killed, or Claude times out the hook, after the write succeeds but before Claude reads stdout. The host broadcasts ACCEPTED, yet the provider never applied the decision.

There are two more gaps:
- The outcomes map is wiped once it reaches 32 entries (`approval.rs:140–142`), so outcomes can be lost and become UNKNOWN.
- Codex has no approval channel at all (project CLAUDE.md), and the spec doesn't say so.

**Remedy:**
- Rename ACCEPTED to `DELIVERED_TO_ADAPTER`, or define a hook-side acknowledgement and document exactly what each adapter can observe.
- Make the outcome store bounded LRU, never clear-all.
- State that `provider=CODEX` requests are never actionable.
- Make FAILED and EXPIRED explicitly terminal and non-actionable.

### F8 — HIGH — Rejected optimistic intents are never retired; ActionResult arrives before Delta
**Where:** §4 L172, L212–217, L225–228; example L278–281.

**Counterexample A (stuck intent):**
1. The board sends SET_SELECTION(X), id=5.
2. X disappears; the host replies REJECTED(SESSION_GONE).
3. Nothing in the View carries `applied_action_id=5`, and L172 only lets a matching View retire the intent.
4. The intent is replayed forever, keeping a dead selection, and blocks all later authoritative selections (L226).

**Counterexample B (flicker):**
1. In the example, `ActionResult(APPLIED, applied_revision=2)` arrives before `Delta(rev 2)`.
2. If the board retires the intent on the result, it renders revision 1's view, then revision 2's.
3. The selection visibly snaps back and forth.

**Remedy:**
- Retire an intent on any terminal ActionResult, but keep rendering it until `applied_revision_local ≥ result.applied_revision`.
- Require the host to send the Delta before the ActionResult for the same revision.
- Specify this ordering as normative.

### F9 — HIGH — `applied_action_id` collides across connections
**Where:** §2 L51 (action_id restarts at 1 per connection); §4 L165–166, L172.

**Counterexample:**
1. Board A, connection 1: action 3 sets the View with `origin=A, applied_action_id=3`.
2. The board reconnects (same authority epoch, new connection) and sends new action 3, SET_SELECTION(Y).
3. The cached or retransmitted Snapshot View says `origin=A, applied=3` with selection X.
4. The board retires the new intent and shows X.

**Remedy:** Add `origin_connection_epoch` to the View, or make action IDs unique per (device, authority epoch).

### F10 — BLOCKER — Seat/animation parity contradicts the existing scene
**Where:** invariant 3 (L25–26), invariant 10 (L35), §5 L256–262.

**Code facts:**
- Swaps walk a path at `WALK_SPEED = 60 px/s` (`scene.rs:119`, `271–284`). Seat 1→0 is about 180 px, roughly 3 s, not 600 ms.
- `seat_free` treats a *leaving* patron as occupying its seat until it is gone (`scene.rs:496`). Seat allocation therefore waits on animation, which contradicts "host assigns final seats" plus "never wait for animation".
- `promote_active` only moves patrons that are `Mode::Seated` (`scene.rs:609, 624`).
- Arrivals and departures aren't covered by §5 at all.

**Counterexample:** Patron P leaves seat 2 and newcomer N appears in the same tick. On the board the host assigns N to seat 2 immediately, so N walks in while P is still walking out of seat 2. On the desktop, N waits outside until P is gone. The two displays now disagree.

**Remedy:**
- Move seat allocation into one host-side pure function whose inputs exclude animation state, and use it for the desktop too.
- Define arrival, departure and swap paths plus a speed (or a fixed duration) as generated, shared constants.
- Specify how a newcomer seated into a seat that is still being vacated is animated (for example, it walks in only after the leaver clears the doorway).
- Pick one swap timing and change the desktop to match.

### F11 — HIGH — Double tap during motion binds coordinates, not identity; toggle semantics diverge
**Where:** §4 L206–209; §5 L260–262.

**Code facts:**
- The second tap reuses the *first tap's coordinates* (`double_tap.h:13`), not its identity.
- `patron_at` ignores patrons that are `Swapping` and hit-tests by *final seat* (an arriving patron is hittable at its seat while still at the door) (`scene.rs:804–816`).
- Taps are emitted on *release* (`touch_gesture.h:24–28`).

**Counterexample A:**
1. The user taps A in seat 1.
2. A swap A↔B starts within 450 ms.
3. The second tap is resolved at seat 1's coordinates and opens the desktop for **B**.

**Counterexample B:**
1. The user taps an already-selected patron. Tap 1 toggles it off and sends SET_SELECTION("").
2. Tap 2 opens the desktop. The spec (L208) leaves the selection cleared.
3. The desktop's `activate_patron` re-selects (`scene.rs:823–835`), so the two sides end differently.

**Remedy:**
- Resolve the hit at touch-down against the frame on screen, store the session_id, and make the double tap require the same session_id (with a tolerance radius around its *current* sprite bounding box).
- Hit-test sprite bounding boxes at the interpolated position, front-most first, and change the desktop to match.
- Define the end state explicitly ("OPEN_DESKTOP implies selected=id"), and make OPEN_DESKTOP carry that selection atomically.

### F12 — HIGH — Delta upsert semantics are undefined (patch vs replace)
**Where:** §2 L43 ("Absence and zero must be distinguished for patchable scalars"); §4 L137–138, L144–156.

**Counterexample:** The host clears a request by upserting a Session with `approval` absent. Under patch semantics, absent means unchanged, so the board keeps showing an actionable PENDING request for a provider prompt that no longer exists. F2 fails closed at the host, but the board UI is wrong.

**Remedy:**
- `upsert_sessions` replaces the whole Session; there are no partial patches inside a Session.
- Only top-level `view`, `usage` and `overflow_count` are optional-means-unchanged.
- Validate seat uniqueness and the provider-side constraint (Claude ∈ {0,1,4,5}, Codex ∈ {2,3,6,7}) on the *merged* model before publishing.

### F13 — HIGH — Coalescing base vs Applied
**Where:** §4 L139–141 ("coalesce … from the last board-**acknowledged** state" vs "delta base must equal board's applied revision").

**Counterexample:**
1. The host sends Delta 1→2. The board applies it, but Applied(2) is still in flight.
2. The host coalesces 3 and 4 "from last acknowledged" (1) and sends Delta 1→4.
3. The board's state is 2, so it sends REVISION_GAP and forces a full Snapshot.
4. Under steady churn this repeats, and F1 makes each Snapshot expensive.

**Remedy:** Base deltas on the last revision *sent* on this connection (the link is ordered and reliable). Use Applied only for flow control, never as the base.

### F14 — HIGH — Blob churn livelock and bandwidth
**Where:** §4 L181–188 ("only hashes referenced by the current model can be requested").

**Counterexample:** A working session's task text changes about every second (`--session-info` updates at 1 Hz).
1. The board requests H1 at revision 5.
2. By the time the host handles the request, the model is at revision 6 with H2.
3. The host returns BlobUnavailable, the board requests H2, and the cycle repeats.

Each try can be up to 32 KB, competing with control traffic under F1.

**Remedy:**
- Validate BlobGet against the board's *applied* revision, or keep blobs referenced in the last N revisions or T seconds.
- Rate-limit task-blob hash changes (e.g. ≤ 0.2 Hz) for unselected sessions.
- Fetch on demand only for the selected session or an open review.
- Also: "Transfer is ordered" (L182) contradicts "overlapping nonidentical chunks" (L184–185). With an ordered, AEAD-sequenced stream, any non-contiguous chunk is a protocol error.

### F15 — HIGH — Hello/epoch bootstrapping is inconsistent
**Where:** §2 L48; §4 L112–114; example L268–269.

**Problems:**
- L112 says every envelope has `connection_epoch`, but L113 has Hello without `authority_epoch`, and HelloAck "assigns" both (L269). The spec doesn't say what epoch Hello carries.
- Hello has `max_envelope_bytes`, but HelloAck has no host receive limit, and L60's "max fragment payload negotiated" has no field.
- L41 says unknown *required* capabilities fail closed, but capabilities carry no required/optional marker.

**Remedy:**
- Derive `connection_epoch` from HKDF over the handshake transcript, so both sides know it before Hello.
- Hello and HelloAck each declare their own receive limits.
- Add `required_capabilities[]`.
- Make `asset_version` a SHA-256 of the generated asset bundle plus the hit-box and layout tables, not a human-chosen string.

### F16 — MEDIUM — AEAD framing is ambiguous; counter gaps hide drops
**Where:** §3 L84–86 vs L91–92.

**Problems:**
- L85 calls the framing header "associated data", while L91 calls it the "fragment plaintext header", meaning it's inside the ciphertext. The two describe different wire layouts.
- "Strictly increasing" allows gaps.
- In the current firmware, a failed notification consumes the counter and silently drops the rest of the message (`main.c:108–116`).

**Remedy:**
- Fix the layout: `counter(8, clear) ‖ AEAD(header(12) ‖ payload)`, with AD = `direction ‖ counter`.
- Require counter = previous + 1.
- Treat a failed notify or write as fatal to the link (or retry the identical ciphertext).
- Specify the HKDF salt and info, including device_id, both nonces, both roles and the protocol major version.
- The ESP32-S3 has AES hardware but no ChaCha hardware. That's fine at these rates, but `MBEDTLS_CHACHAPOLY_C` must be enabled and the per-fragment cost measured.

### F17 — MEDIUM — Write-without-response flow control is unspecified for btleplug/CoreBluetooth
**Where:** §3 L100–102.

**Fact:** btleplug doesn't expose `canSendWriteWithoutResponse`. The current host works around this with a with-response write every 8th fragment (`companion.rs:279`). With strict counters, a single silently dropped write terminates the link.

**Remedy:** Define application-level credits (the board grants N fragments in a notification), or keep the documented every-K with-response barrier. Measure on macOS and Linux (BlueZ) with drops counted.

### F18 — MEDIUM — Queue overflow semantics contradict each other
**Where:** §2 L62 ("overflow disconnects") vs §4 L229–231 ("stop accepting … show reconnecting").

**Remedy:**
- Choose one. Recommended: the board blocks *new* network-dependent input at 32 while keeping the link, and the host disconnects only on *its* outbound overflow.
- Enumerate the reversible actions (SET_SELECTION, SET_*_ANCHOR, CLOSE_REVIEW?) and irreversible ones (OPEN_DESKTOP, DECIDE), and say which are re-issued with new IDs after reconnect. L132 and L225 currently conflict on this.
- The dedup cache (L219–223) is nearly useless in-connection, because BLE is reliable and cross-connection retry is forbidden. Keep it, but don't count it as an at-most-once mechanism.

### F19 — MEDIUM — Host restart while a request is in SUBMITTING
**Where:** §4 L231; §6 L292–293.

**Counterexample:**
1. The host crashes after writing to the hook, before broadcasting.
2. The board reconnects to a new authority epoch, which has no record of R.
3. "Fetch authoritative request state" yields *absent*, which the board may render as STALE ("nothing happened").

**Remedy:** UNCERTAIN survives a change of authority epoch. The board shows "outcome unknown — check desktop" until the user dismisses it, and never derives "not applied" from absence.

### F20 — MEDIUM — Request-less review is dropped
**Where:** §4 L198 (OPEN_REVIEW requires request_id), L170–171.

**Code fact:** today the footer double tap opens a "QUESTION / TASK" or "APPROVAL IN DESKTOP" review for waits that have no linked request (`scene.rs:699–714`). The spec loses this, or leaves `review_request_id` undefined for it.

**Remedy:** Allow `review_kind = TASK | UNLINKED_PERMISSION | REQUEST` with a blob reference. Keep it non-actionable.

### F21 — MEDIUM — The Session schema lacks fields the renderer uses
**Where:** §4 L144–145.

**Code fact:** speech bubbles use `branch` and `active_subagents` (`scene.rs:922–928`), and the nameplate uses a short name (`shorten`, `PLATE_LEN`).

**Remedy:**
- Add `branch` (bounded), `active_subagents` and a display name, or declare that the board omits them (which breaks invariant 10).
- `session_id ≤ 128` conflicts with the broker accepting session IDs under 200 characters plus the `claude:` prefix (`approval.rs:87, 97`). Define the mapping for oversized IDs (e.g. hash-qualified).

### F22 — MEDIUM — Anchor/page parity needs a normative wrap algorithm
**Where:** §4 L168–169, L200–201.

**Problem:** Byte anchors become pages through `task_pages` (TASK_LINE_LEN 32, two-line pages, `scene.rs:439`) and `lines(…, 54)`, with 9-line review pages (`review.rs:19, 33`). Rust and C will drift unless the algorithm is specified.

**Remedy:** Specify both algorithms, including escape expansion, and ship shared golden vectors (text → line breaks → anchor ↔ page).

### F23 — MEDIUM — Performance targets vs the panel path
**Where:** §7 L299–305.

**Code fact:** the panel is QSPI at 20 MHz using `spi_device_polling_transmit` (`board.c:56–61, 166`). A full frame of 261 KB takes at least 26 ms of blocking time before PSRAM byte-swapping. Touch is polled every 20 ms, and a tap is emitted only on release (`board.c:140`, `touch_gesture.h`).

**Problems:** At 30 fps with full-frame pushes the budget is almost entirely spent on the transfer. "p95 touch feedback ≤ 75 ms" is unmeasurable until the start event is defined.

**Remedy:**
- Make dirty-rectangle rendering with DMA queueing (not polling) mandatory.
- Define the latency start as touch-down for press feedback and release for the tap action.
- Move rendering off the NimBLE host task.
- Decode envelopes in a worker, not inside `access_rx`, which currently feeds the parser inside the NimBLE callback (`main.c:137–157`) on an 8 KB stack.
- Nanopb static structs for a 64-session Snapshot (~45 KB), times two for staging, must live in PSRAM with `.options` max sizes and no `PB_ENABLE_MALLOC`.

### F24 — MEDIUM — Flash, profile and rollback assumptions
**Where:** header L7; §8 L316.

**Fact:** the JC4827 profile is 4 MB flash (`sdkconfig.jc4827.defaults:2`), and the base defaults use `PARTITION_TABLE_SINGLE_APP_LARGE` (no OTA slot). "Preserve rollback image" therefore means a host-side backup plus reflashing, not on-device rollback. Generated 480×272 RGB565 background art plus sprites, NimBLE, mbedTLS and nanopb must fit the app partition.

**Remedy:** State the rollback procedure (an esptool `read_flash` backup of the full image, excluding committed secrets) and add an app-size budget check to Gate B.

### F25 — LOW
- **L254, sleep:** a new PENDING approval doesn't wake the screen, so an approval can wait unseen. Make a deliberate product decision (e.g. a brief backlight pulse with input still consumed).
- **L288:** board B's rejection reason should be BUSY or ALREADY_DECIDED, not STALE_REQUEST. The request is unchanged; it is SUBMITTING.
- **L151:** `appearance_seed` is redundant with `fnv1a(id)`. The board should verify it, or the field should be dropped, to avoid a silent look mismatch.
- **Shared view, L165–174:** desktop keyboard navigation (Tab/arrows) on a shared view moves the board under the user's finger. Document this, or keep `page` and anchors device-local.
- **OPEN_DESKTOP:** rate-limit it on the host; an authenticated but buggy board shouldn't be able to launch apps repeatedly.
- **Fragment `message_id` u32:** add a no-wrap rule or wrap semantics.

### F26 — MEDIUM — QEMU gate scope
**Where:** §1 L17–19; §8 L313–314, L327–328.

**Facts:**
- The QEMU project (`firmware/`) uses `esp_lcd_qemu_rgb`, not the QSPI NV3041A, and has no NimBLE or GT911.
- A UART socket has no ATT MTU, write-without-response loss or credit behaviour.
- QEMU timing isn't cycle-accurate, and its RNG isn't a real entropy source.

**Remedy:**
- Run the *same* fragmentation and AEAD layer over the UART with an emulated capacity (64/185/244) and injected drops and reordering.
- Keep the protocol core platform-free C, built natively for ASan/UBSan and fuzzing.
- State explicitly that QEMU certifies none of these: latency, BLE flow control, touch, panel.

---

## Acceptance tests to add to Gate B/C (each must fail on the counterexample above)

1. **F1:** Worst-case Snapshot at capacities 64, 185 and 244 over emulated BLE timing. No liveness timeout, and the Heartbeat interleaves.
2. **F2:** Rust and C golden vectors for the RequestIdentity bytes, HKDF and AEAD. A board whose rendered body differs from the model's digest refuses DECIDE.
3. **F3:** Triple tap on the footer at every x in [168, 432] → zero DECIDE. A desktop-initiated REVIEW → buttons stay disarmed until a fresh touch-down after LCD completion plus 500 ms.
4. **F4:** Two boards with distinct keys. Swapping their keys or device_ids → authentication fails. The two-board race produces exactly one provider write.
5. **F5:** A QA-profile board reports injection, and the host rejects its DECIDE.
6. **F7:** Kill the hook after the socket write → state is UNKNOWN or DELIVERED, never "provider accepted". Thirty-three outcomes → none lost.
7. **F8/F9:** A rejected SET_SELECTION retires its intent. An ActionResult before its Delta causes no flicker (frame diff). action_id reuse across reconnect doesn't retire a new intent.
8. **F10/F11:** Leave, arrive and swap in one tick → desktop and board render pixel-identically at injected time t. A double tap across a swap start → OPEN_DESKTOP targets the first-tap identity, or nothing. Hit-test sprites mid-walk.
9. **F12/F13:** An upsert without `approval` clears it. Pipelined deltas without Applied never cause a REVISION_GAP.
10. **F14:** Task text churning at 1 Hz → the selected task text loads within 2 s, and the bandwidth cap holds.
11. **F16/F17:** Counter gap, duplicate and replay → the link terminates. A dropped write-without-response on macOS is detected, not silently corrupted.
12. **F19:** Host crash in SUBMITTING → the board keeps UNCERTAIN across the new authority epoch.
13. **F6/F22:** An MCP tool name over 32 characters is fully visible or DESKTOP_ONLY. Escape and wrap golden vectors match between Rust and C.
14. **Fuzzing:** Reassembly, the nanopb decoder and the model validator under ASan/UBSan, with ≥ 10⁶ executions and no allocation before the bounds check.
15. **Physical (Gate C):** Touch-down to LCD completion p95, release to action p95, and heartbeat timing under a maximum-size transfer, all on one clock.

## Parts of the design that hold up

- Identity-bound actions.
- No offline approval queue and no automatic DECIDE retry.
- A single authoritative dispatch with atomic PENDING→SUBMITTING, which matches `Broker::decide`'s remove-then-send in `approval.rs:37–39`.
- Sleep-wake touches are consumed.
- Fail-closed enums.
- No silent fallback to the legacy transport.

These should be kept. They aren't enough on their own until the blockers above are resolved.
