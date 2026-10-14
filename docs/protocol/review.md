# Claude Code review disposition

Independent reviewer: Claude Code, self-reported Opus 5.5, read-only CLI run.
Original report: `claude-review-1.md`; baseline source ff2ff33. The original spec
SHA256 is recorded below. Review findings are design findings, not claims that
unimplemented v2 code already has these defects. No findings are silently waived.

| Finding | Disposition | Resolution in revised spec/schema |
|---|---|---|
| F1 size/liveness | Accepted | 4KiB envelopes, two-session snapshot parts, atomic commit, liveness on valid fragments, bounded transaction timeout |
| F2 display binding | Accepted | Explicit length-prefixed request hash; board recomputes from verified displayed content; decision carries body hash |
| F3 accidental approval | Accepted | Fresh down after exact LCD completion, local review acknowledgement, 500ms quiet arming, cancel on surface changes |
| F4 device identity | Accepted | Public key lookup ID authenticated in v2 transcript; per-device keys; one host per board, many independent board links |
| F5 QA injection | Accepted | Build-profile capability; production excludes injection; host rejects real approvals from QA boards; fake-adapter-only automation |
| F6 hidden tool/title | Accepted | Full tool/title in scrollable body; normative lossless escaping and layout; no actionable truncation |
| F7 acceptance overclaim | Accepted | DELIVERED_TO_ADAPTER state; explicit current Claude hook limit and missing Codex adapter; bounded pinned outcomes |
| F8 optimistic retirement | Accepted | Terminal-result retirement, APPLIED visual hold until revision arrives, state-before-result order |
| F9 action epoch collision | Accepted | origin_connection_epoch joins device/action IDs in View |
| F10 seat/animation parity | Accepted | Pure host allocator, shared 60px/s paths, explicit arrivals/departures/visual vacancy rules |
| F11 moving double-tap target | Accepted | Touch-down stable-ID hit, current interpolated sprite, OPEN_DESKTOP atomically reselects |
| F12 upsert ambiguity | Accepted | Whole-session replacement; absent approval clears; validate merged model |
| F13 delta baseline | Accepted | Base from last sent revision; Applied only bounds pipeline |
| F14 blob churn | Accepted | 30s sent-history retention, pinned active transfer, 512KiB/link bound, selected-first/coalesced background |
| F15 bootstrap/limits | Accepted | HKDF-derived connection epoch, reciprocal limits/required caps, asset digest |
| F16 crypto ambiguity | Accepted | Exact encrypted-header layout, direction AD, counter +1, full HKDF definition, fatal transmit failure |
| F17 host credits | Accepted with simpler implementation | Initial host writes use WithResponse for portable bounded backpressure; optimize only after measured evidence |
| F18 overflow/replay | Accepted | Board backpressure; host critical overflow disconnect; discard intents on reconnect; enumerate side effects |
| F19 restart uncertainty | Accepted | Preserve uncertain receipt across epochs until user dismissal; absent is not failed |
| F20 request-less review | Accepted | REQUEST/TASK/UNLINKED_PERMISSION variants; only REQUEST can decide |
| F21 renderer fields/IDs | Accepted | Branch/subagent/display_name fields; deterministic hash mapping for oversized source IDs |
| F22 wrap parity | Accepted | Normative escaping, whitespace preservation, source-byte anchors and golden tests |
| F23 panel/task budgets | Accepted | Queued dirty-region DMA required; explicit down/release timing; PSRAM static bounds; decode/render outside BLE callbacks |
| F24 flash rollback | Accepted | Production image-size gate, full flash backup kept private, USB reflash rollback |
| F25 small issues | Accepted / product choice | No automatic screen wake preserves user's ten-minute no-touch rule; ALREADY_DECIDED, appearance mapping, shared view cancellation, navigation rate limit, message-ID no wrap |
| F26 simulation limits | Accepted | Same AEAD/framing over UART with emulated MTUs/faults; no QEMU BLE/physical timing claims |

Reviewer verification of this revision is pending. Gate A is not yet marked passed.
a4ca97f5250b89725c802cabfdea190391f095bb1775108344f39aca62739065  docs/protocol/companion-v2.md
531ee6bea30326a3a37e986a932ad0db62d1a60c79a5d40f3db9b517bfd5c737  protocol/companion.proto

## Follow-up review 2

Report: `claude-review-2.md`. All N1–N4, H1–H6, M1–M8 and Low items accepted
except N4's assertion that the test matrix was never updated: R01–R22 were being
written while the reviewer read it. This was a review-input race, now avoided by
pinning immutable review snapshots. No runtime implementation has started.

- N1/M1: encoded-byte cap is authoritative, at most eight removes, one-session
  snapshot parts when needed, all strings/counts/digests bounded.
- N2: minimum ATT value 182, 60s bounded snapshot, two priority lanes interleaved
  at fragment boundaries, per-write connection-event delay in simulation.
- N3: host key store labels QA/PRODUCTION, QA can exercise only fake adapter;
  real physical approvals require production image and real touches.
- N4: explicit B1/B2 naming, R01–R22 plus added cases and million-case fuzz gate;
  original hash labels below refer to immutable review-input versions.
- H1: epoch-qualified receipts, eight-entry bound and explicit dismissal, snapshot
  recovery, five-second terminal UI retention and 128-outcome host retention.
- H2: full tool/title first, before preamble; oversized first-page identity is
  desktop-only. ESCAPED_TEXT replaces UTF8_LITERAL.
- H3: production v2 excludes legacy approval path; rollback is reflashing v1.
- H4: oldest-first concurrent hook serialization and terminal retention.
- H5: explicit auth type bytes/read states and Hello ID equality.
- H6: pending-resync ignores deltas; host cancels unsent deltas, epoch reset,
  equal-revision requested snapshot repairs corrupted local state.
- M2/M3: lazy per-line mapping, hard wrapping, exact trailing-LF semantics.
- M4: Session.unlinked_review_blob supplies desktop-only guidance content.
- M5: provider-side initial, arrival, overflow and promotion ordering specified.
- M6: results deferred with authoritative state; rejected/no-op revision defined.
- M7: four-link aggregate cap; finish pinned selected text before newest refresh.
- M8: enumerate critical messages versus replaceable/coalesced data.
- Low: explicit DECIDE body hash; host cannot verify physical arming; zero-option
  desktop-only digest defined; valid enum zeros documented; snapshot digest entries
  length-prefixed; bootstrap example uses epochs in Envelope.

## Review 3 and design gate decision

Report: `claude-review-3.md`; exact immutable inputs: `review-3-input.sha256`.
Claude found no blockers and two HIGH items, explicitly allowing Gate A closure
by a targeted diff check once those were fixed, without another full review.

- HIGH-1 accepted/fixed: two-lane priority can reorder results before bulk state.
  Every result status is held until its revision is applied; queue order is not
  claimed to be receive order. Rejected intents also wait for the right state.
- HIGH-2 accepted/fixed: Request.tool_name is bounded, digest-covered and explicitly
  rendered first. Both peers independently enforce the nine-line identity limit.
- MED-1 accepted/fixed: snapshot validation failure/timeout closes connection;
  exactly one resync request, no ambiguous partial transaction restart.
- MED-2 accepted/fixed: v2.0 fixed caps 4096/1024/32768; mismatches reject.
- Low items accepted/fixed: two reassembly buffers, wrong-lane rejection, resync
  disarming, five-second dispatch uncertainty, durable bounded board receipts,
  unique Gate-A case IDs, production QA requires authorized synthetic request.

Primary-agent diff check: both HIGH fixes appear in spec and schema; protocol
schema recompiles. All review findings have dispositions; no unresolved design
blockers. Gate A PASS for implementation. Runtime and physical acceptance remain
unproven until Gates B/C pass. This is not a claim that Claude reviewed the code.
