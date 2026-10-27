# Companion v2 acceptance test plan

Run synthetic data only. Every result records build SHA/tree diff, command,
transport, assertion, and evidence path. Tests are implementation acceptance
criteria; they do not pass merely because they are documented.

## Gate A — design and review

- G01: All on-wire messages have protobuf definitions and documented bounds.
- G02: Independent Claude Code review saved with exact reviewed artifact hashes.
- G03: Every finding has accepted/fixed or rejected-with-evidence disposition.
- G04: No unresolved approval safety, framing, identity, or atomicity blockers.

## Gate B1 — native protocol and state tests

| ID | Stimulus | Required assertion |
|---|---|---|
| W01 | Rust encode / C decode for every payload | Values and presence match |
| W02 | C encode / Rust decode for every payload | Values and presence match |
| W03 | Unknown optional protobuf field | Ignored without state corruption |
| W04 | Unknown major, required capability, enum/action | Reject without side effect |
| W05 | Wrong epoch, length or duplicate ID in snapshot | Atomic rejection |
| W06 | Every fragmented boundary, including one-byte chunks | One exact envelope |
| W07 | Incomplete/overlap/oversize fragments | Bounded memory, timeout/reset |
| W08 | Alter header, ciphertext, tag, direction or counter | Fail authentication |
| W09 | Replay nonce/counter or counter overflow | Reject and disconnect |
| W10 | MTU minimum, typical and maximum | No oversized ATT values |
| W11 | Malformed protobuf random input under ASan/UBSan | No crash/OOB/mutation |
| S01 | Initial snapshot then valid delta | Correct monotonic applied revision |
| S02 | Revision gap, duplicate delta, stale snapshot | No rollback; resync |
| S03 | Delta includes bad session after valid upsert | No partial mutation |
| S04 | Duplicate occupied seat or wrong provider side | Reject inconsistent model |
| S05 | Session removal while selected/reviewed | Clear invalid focus atomically |
| S06 | 64 sessions, 8 seats, overflow | Defined bounded behavior |
| S07 | Task changes at same ID | Old anchor/content invalidated |
| S08 | Blob hash mismatch, bad UTF-8, partial body | No actionable review |
| S09 | Cache eviction while review open | Pinned question stays available |
| S10 | Two blob transfers interleaved/cancelled | No cross-content publication |
| S11 | Protocol/assets mismatch | Explicit non-actionable error |
| S12 | Queue exhaustion | No silent decision loss or unbounded growth |
| A01 | Every semantic action | Correct resulting view and result |
| A02 | Duplicate action same payload | Cached result; one execution |
| A03 | Duplicate action different payload | Disconnect; no second execution |
| A04 | Old action outside dedup cache | Reject without execution |
| A05 | Two quick local selections, old remote update between | No focus flicker/rollback |
| A06 | Desktop selection races board selection | Host ordering converges |
| A07 | Lost result followed by reconnect | No approval replay |
| A08 | Session closes/moves before action arrives | Resolve stable identity or reject |
| A09 | Navigation unavailable/Linux opener failure | Explicit failure, no false success |
| P01 | Approve/deny supported exact request | Exactly one adapter dispatch |
| P02 | Changed ID/digest/session/body/options | Reject stale/mismatched request |
| P03 | Expired request after button displayed | Reject, disable controls |
| P04 | Two boards send conflicting decisions | One atomic winner |
| P05 | Provider rejects or crashes | FAILED/UNKNOWN; no false acceptance |
| P06 | Host crashes after provider dispatch before reply | No automatic redispatch |
| P07 | Unsupported provider adapter | Desktop-only; never enabled buttons |
| P08 | Too-long/unrepresentable approval body | Desktop-only, no truncation |

## Gate B2 — real firmware in Espressif QEMU

The C receiver, model, gesture recognition and local renderer must be the same
sources linked into the production board target. Only UART/display/touch hardware
adapters differ. A host-side renderer mock is not a replacement for this gate.

| ID | Scenario | Evidence/assertion |
|---|---|---|
| Q01 | Boot, handshake and synthetic scene snapshot | Firmware readiness, revision |
| Q02 | Tap each of eight patrons | Expected stable session ID in emitted action |
| Q03 | Tap selected patron again | Selection clears locally and remotely |
| Q04 | Double tap patron | Exact provider/session target, no second toggle |
| Q05 | Footer double tap | Matching full question or explicit unavailable |
| Q06 | Approval buttons / review double tap | Correct decision or desktop action |
| Q07 | Task and review scrolling | Bounds, legibility, anchor synchronization |
| Q08 | Withhold host replies for 1000ms | Local selection visible before any reply |
| Q09 | Rapid alternating touch burst | Final intent wins, queues bounded |
| Q10 | Patron swap while touching | Displayed patron identity preserved |
| Q11 | Local animation with no state changes | Frames change; no frame network traffic |
| Q12 | Sleep timer and wake touch | First wake consumed; latest model displayed |
| Q13 | Drop connection in review/decision | Controls disabled; no replay |
| Q14 | Restart host/reconnect | Fresh epoch and correct authoritative snapshot |
| Q15 | Worst-case question and session list | No firmware allocation failure |
| Q16 | Screenshot suite | Inspect café, selection, waiting, review, scroll, sleep/wake |

Golden comparisons use synthetic state and deterministic animation times. Compare
semantic regions and RGB565 conversion explicitly. Inspect generated contact sheets
visually; pixel equality alone cannot judge clipping, small text or confusing UX.
QEMU CPU/wall timings are diagnostic only, not Bluetooth or physical latency claims.

## Gate C — physical JC4827W543 after simulation passes

1. Confirm current serial identity/profile, preserve known-working firmware and
   app paths; do not erase provisioning or change unrelated running provider work.
2. Build correct 4MB-flash/8MB-PSRAM board image; flash-tool verify and boot logs.
3. Confirm authenticated v2 connection and no framebuffer streaming.
4. First run a QA-profile image with a QA-class key and isolated fake adapter
   through the injected gesture path. Capture committed framebuffer. Then flash
   production (injection disabled) with its production-class key and repeat Q02–Q10
   using real touch and an explicitly user-authorized synthetic approval request
   (Q06). Never inject decisions into the live approval broker.
5. Measure at least 100 injected interactions with timestamps at input, local
   commit, LCD completion, host receipt and authoritative acknowledgement.
6. Run 1000 rapid mixed non-destructive actions, disconnect/reconnect, host sleep,
   long text, and stale/duplicate synthetic decisions. No real pending approvals
   are approved merely to test transport.
7. Real finger tests verify alignment and capacitive behavior. High-speed camera
   is required for true touch-to-photon; LCD completion and screenshot capture are
   explicitly reported as proxies if camera evidence is unavailable.
8. Initial targets: p95 local feedback <=75ms, cached detail <=100ms, host-event
   convergence <=200ms, local animations 30fps. If missed, investigate rather than
   declare gate passed. Device timing and radio timing are separate distributions.
9. Finish with current live sessions, correct eight-seat mapping, screen sleep/wake,
   no secrets in committed captures/logs, and an explicit rollback procedure.

## Gate D — host compatibility and final report

- Rust tests, Clippy, formatting, feature-disabled build.
- macOS production bundle and navigation, title connection status.
- Linux compile/tests and platform-appropriate navigation fallback.
- Physical Linux BLE only marked passed if exercised on a real Linux adapter.
- Final report lists each gate, case counts, failed cases/fixes, timing distribution,
  capture paths, review disposition, hardware identity and remaining limitations.

## Additional acceptance cases from independent review 1

| ID | Counterexample to exercise | Required assertion |
|---|---|---|
| R01 | Maximum snapshot at simulated capacities 182/244/509 and reject 64 | Atomic transaction; liveness on valid fragments; bounded control latency |
| R02 | C/Rust request identity and crypto vectors | Exact preimage, SHA256, HKDF, ciphertext/tag equality |
| R03 | Stale body pointer paired with current request metadata | Board recomputation rejects decision before transmission |
| R04 | Triple tap footer across all approval-button x positions | Zero decision actions |
| R05 | Desktop opens/replaces review during board contact | Contact cancelled, buttons remain disarmed |
| R06 | Fresh touch before/after 500ms quiet interval and LCD completion | Only fully armed new contact can decide |
| R07 | Swap device IDs/keys of two board emulators | Mutual authentication fails |
| R08 | QA firmware reports injection enabled | Live adapter decisions refused; synthetic adapter works independently |
| R09 | Tool name >32 characters, CRLF/backslash/non-ASCII | Full operation visible, normative escaping and wrapping |
| R10 | Hook dies after socket write; >32 outcomes | No provider-accepted claim; no clear-all loss |
| R11 | Terminal rejection or result-before-delta | Intent resolves without stale focus/flicker |
| R12 | Action IDs reused on new connection | Old View cannot acknowledge new intent |
| R13 | Departure + arrival + swap in same tick | Same allocation and animation semantics on desktop/board |
| R14 | Double tap across seat swap | Original visible stable identity or cancelled action |
| R15 | Upsert removes approval; pipeline Applied is delayed | Approval clears; no false revision gap |
| R16 | Selected task changes once/second during transfer | Pinned text completes, bounded cache and queues |
| R17 | Unsupported required capability / changed asset digest | Explicit non-actionable failure |
| R18 | Fragment notify/write failure, duplicate, gap | Fatal connection reset; no hidden partial model |
| R19 | Host restart while decision dispatch is uncertain | Uncertain receipt survives until dismissal |
| R20 | Footer opens task/unlinked approval review | Full non-actionable view preserved |
| R21 | Input held at wake and repeated waiting updates | Wake consumed; host updates do not reset sleep timer |
| R22 | Production image size/profile flags | Fits actual partition; no QA injection in production |

Revise older table terminology: P01's adapter dispatch is not proof of provider
acceptance. P05 must never display an accepted/executed outcome solely from a hook
socket write. W07 timeout terminates the stream. S10 uses explicit cancellation,
not arbitrary concurrent blob transfers. Q16 uses the same escaping/wrapping
fixtures on both targets. Physical performance remains Gate C, not a QEMU claim.

Additional Gate B cases: byte-encoded delta overflow→snapshot; auth type/state errors;
Hello ID/key-class mismatch; QA-class key with false injection flag still denied;
resync with in-flight deltas; authority revision reset; concurrent hook serialization;
receipt ID reuse across epochs and eight-receipt backpressure; maximum escaped text
layout memory; selected-text churn bandwidth; minimum-MTU WithResponse delay.
Native decoder/framing fuzzing requires at least 1000000 deterministic cases under
ASan/UBSan, with a saved seed and case count.

Review 3 additions: result arrival before bulk state for ALL statuses; explicit
tool_name/title wrapping on first page; invalid snapshot/timeout closes connection;
fixed v2.0 bounds mismatch rejection; wrong-lane rejection; decisions disarmed
during resync; five-second ambiguous dispatch; pending receipt persistence failure
and board reboot recovery.
