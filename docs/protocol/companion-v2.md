# Token Café companion protocol v2

Status: design gate passed after three Claude reviews and dispositions below; implementation and acceptance gates tracked in `validation.md`.
Scope: macOS/Linux desktop authority and ESP32-S3 companion, 480×272 pixel UI.
The normative wire schema is `protocol/companion.proto`. This document defines
semantics that protobuf alone cannot enforce. No production board is upgraded
until the simulation gate passes. Preserve the current working firmware for rollback.

## 1. Architecture and invariants

The desktop owns provider sessions, seat assignment, request lifecycle and the
ordering of shared UI actions. Each board owns a cached model, local rendering,
gesture recognition and immediate reversible interaction feedback. Neither side
streams frames during normal v2 operation. Assets, fonts, hit boxes and animation
rules are versioned build artifacts. Screenshot capture is a diagnostic operation.

Use protobuf with Nanopb (C firmware) and prost (Rust desktop), transported over
authenticated BLE GATT. The simulation uses the identical messages and state
machine over a private UART socket. QEMU does not certify BLE or panel timing.

Invariants:

1. Session identity is a stable provider-qualified ID, never a seat index or PID.
2. A snapshot or delta is validated completely before atomic publication.
3. Provider state changes never wait for animation; selection follows identity
   through seat swaps. The host assigns final seats; board animates locally.
4. Buttons never imply provider acceptance until the corresponding result exists.
5. Decisions bind an immutable request ID and full content digest. Unavailable,
   incomplete, replaced, expired or unrepresentable requests are not actionable.
6. Reconnection destroys pending approval intent. No offline approval queue.
7. Only one desktop authority controls a board per authenticated connection.
8. A board never executes commands, opens arbitrary URLs, or interprets provider
   text as protocol instructions. Host resolves navigation by known session ID.
9. Ordinary scene input and rendering do not wait for a network reply.
10. The desktop and board use the same pixel artwork and interaction patterns.

## 2. Identity, versions and limits

All integer values are unsigned unless the schema explicitly says otherwise.
Protocol major is 2; minor is additive. Unknown fields are ignored by protobuf;
unknown required capabilities, major versions, actions and enum values fail closed.
Do not reuse field numbers. Explicit zero/UNSPECIFIED enums are invalid in
required fields. Explicit valid zero exceptions: WAIT_NONE, REASON_NONE, an
unset delivered_decision before delivery, optional missing request/review fields,
zero task/review anchors and action acknowledgements before any action. Absence and zero must be distinguished for patchable scalars.

| Name | Definition / bound |
|---|---|
| authority_epoch | 16 random bytes on desktop process start; changes after restart |
| connection_epoch | 16 bytes derived by HKDF from fresh authenticated nonces |
| device_id | Provisioned opaque ID, bound to the authenticated key |
| revision | u64, starts at 1 per authority epoch; no wrap permitted |
| action_id | u64 increasing per connection, starts at 1; no wrap permitted |
| session_id | UTF-8 string, 1–128 bytes, provider-qualified; oversized source IDs map to provider + `sha256:` + lowercase hex digest |
| request_id | UTF-8 opaque string, 1–128 bytes, unique within authority epoch |
| blob_id / request_digest | SHA-256, exactly 32 bytes |
| title / tool_name | UTF-8, at most 192 bytes each; never used as an identity |
| max sessions | 64 known sessions, exactly 8 physical seats |
| seat | 0–7, or 255 for outside; duplicate occupied seats forbidden |
| text blob | At most 32768 UTF-8 bytes per blob |
| cache | Four complete blobs plus one in-progress blob; LRU, pin open review |
| envelope | Encoded byte cap: bulk 4096, control 1024, including all envelope fields |
| blob chunk | At most 512 bytes; no text normalization on transfer |
| queue | 32 actions on board (backpressure); 32 critical outbound messages on host (disconnect on overflow) |
| acknowledgement cache | Last 64 completed actions for current connection |
| capabilities / required capabilities | At most 8 distinct known values each |
| asset_digest / snapshot digest | Exactly 32 bytes |
| allowed_decisions | At most 2 distinct values; empty for desktop-only |
| desktop_only_reason / error detail | UTF-8 <=192 bytes |
| BlobUnavailable.reason | Bounded enum, never arbitrary text |
| device / origin_device_id | 1–64 ASCII bytes (origin empty before actions) |
| host board links | At most 4; 512KiB history per link, 2MiB aggregate |
| uncertain receipts / outcomes | Board 8 receipts; host 128 terminal outcomes; in-flight entries pinned |

A request larger than the text bound is marked DESKTOP_ONLY. Render every byte
faithfully using the escaping algorithm below. Never truncate actionable commands.
The full tool name and full title must also occur in the scrollable body, so a
short header cannot conceal the operation. Any unsupported display sequence
disables decisions instead of replacing it silently.
Task text may have an explicit truncated flag and desktop navigation affordance.
Transport limits are checked before allocation. Reassembly has two bounded buffers
per direction (4096 bulk + 1024 control); idle incomplete messages expire after 2 seconds and terminate the link. Peers must match the fixed v2.0 limits; incompatible limits fail negotiation.

## 3. BLE framing and security

Retain the service UUID and legacy endpoints for explicit rollback. v2 has
separate RX/TX/auth characteristics with suffixes 5/6/7. No silent v1 fallback.
One board accepts one host connection; a host may have N independent board links.
Host key configuration maps provisioned opaque device_id to a distinct 32-byte key
and a trusted key class (PRODUCTION or QA). Limit to four simultaneous board links.
Read the public device_id before choosing a key; it is only a key lookup hint until
authenticated. Never try unrelated keys or select an ambiguous device automatically.
CoreBluetooth UUID and advertised name are not authenticated identities.

Auth v2 values have an explicit type byte. Read in IDLE: type=0, u8 ID length,
ID bytes. Host write: type=1, host_nonce[16]. Board enters CHALLENGE; next read:
type=2, board_nonce[16], board_tag[16]. Host write: type=3, host_tag[16].
Board verifies then enters AUTHENTICATED; next read: type=4, status=0,
ATT_value_capacity:u16LE. Wrong length/type/state fails and disconnects. No auth
notifications are used. Type=1 is permitted only in IDLE; reauthentication requires
a fresh connection. Board creates a fresh nonce and truncated HMAC for each attempt. Define T = ASCII `tc-auth-v2` || u16LE length
of device_id || device_id || host_nonce || board_nonce. Board tag is HMAC(key,
ASCII `board` || T)[0:16]; host response is HMAC(key, ASCII `host` || T)[0:16].
Constant-time verification is required. Both nonces change for every attempt.
An auth error disconnects and clears secrets; no application data is accepted yet.

HKDF-SHA256: salt = SHA256(T); IKM = provisioned key. Separate HKDF expand labels
are ASCII `tc-v2/host-key` (32 bytes), `tc-v2/board-key` (32),
`tc-v2/host-nonce` (4), `tc-v2/board-nonce` (4), `tc-v2/epoch` (16).
The epoch output is connection_epoch, known before Hello. Keys/prefixes are never
reused across handshakes. ChaCha20-Poly1305 protects each fragment. ESP-IDF must
enable MBEDTLS_CHACHAPOLY_C; benchmark this before selecting final packet limits.
BLE link encryption can additionally protect metadata; do not assume bonding UX.

Wire fragment = counter u64LE (clear) || ciphertext || 16-byte AEAD tag.
AEAD nonce = direction-specific prefix (4 bytes) || counter u64LE.
Associated data = ASCII `tc-v2` || direction byte (1 host→board, 2 board→host)
|| counter u64LE. Plaintext = message_id u32LE || total_length u32LE ||
offset u32LE || envelope fragment. Overhead is exactly 36 bytes. Require ATT
value capacity >=182 (ATT_MTU>=185); capacity is ATT_MTU-3, observed independently by each side.
Counter starts at 1 and must equal previous+1. Duplicate, missing, out-of-order,
invalid-tag or overflow counters terminate the connection. The message ID lane encoding and no-wrap limit are defined below.

Fragments must be contiguous within a lane with fixed total length within its cap. Two bounded reassembly lanes per direction: message_id high bit is lane
(0 bulk, 1 control), low 31 bits are per-lane IDs starting at 1, no wrap. Bulk
envelopes <=4096, control <=1024. Give control fragments priority at EVERY fragment
boundary while preserving contiguous offsets within each lane. Crypto counter is
shared across lanes in actual transmit order. Thus a snapshot cannot block an
action/heartbeat behind a whole bulk envelope. Message ID monotonicity is per lane. Transmit failure terminates the connection (no silent drop).
For initial implementation use GATT write-with-response for host fragments: this
provides portable bounded backpressure without relying on CoreBluetooth's hidden
write-without-response credits. Notifications use a bounded worker queue and
NimBLE notify completion/backpressure; failed delivery closes the link. Rendering
and decoding never execute inside NimBLE access callbacks. The callback copies
only a bounded fragment into the worker queue. Notify/write completion is never
an approval outcome. More aggressive credits are a separately benchmarked change.

Heartbeat every 1 second of otherwise idle time. Liveness refreshes on **every
AEAD-valid fragment**, not only completed envelopes. After 3 seconds without one,
disable decisions and disconnect. Partial-message idle timeout is 2 seconds.
Handshake timeout is 10 seconds. Snapshot transaction total timeout is 60 seconds,
independent of liveness. v2.0 requires exact receive limits: bulk 4096, control 1024 and blob 32768.
Hello/Ack max_envelope_bytes denotes bulk; control cap is fixed by major/minor.
Any different advertised bound is ERROR_LIMIT, not a partially working connection.
Control lane carries Action, ActionResult, Applied, Heartbeat, Resync, Error,
BlobGet/Cancel/Unavailable and Hello/Ack. Reject any payload on the wrong lane.
Bulk lane carries snapshot messages,
Delta and BlobChunk. No envelope may exceed its lane cap after encoding; emit a
bounded ERROR_LIMIT or use the snapshot alternative, never silently truncate.
Performance tests emulate negotiated connection-event delay per WithResponse write
as well as MTU. At lower MTU reject before sync. These limits bound memory and
progress; the 200ms convergence target must still be measured on the actual link.

## 4. Envelope and payload catalogue

Every envelope has `major=2`, `connection_epoch`, `authority_epoch`, and a `oneof`
payload. Hello has empty authority_epoch; HelloAck supplies the authority epoch. Both already know connection_epoch from HKDF. Subsequent
messages must match both epochs exactly. Wire integers are protobuf varints,
fixed byte arrays are protobuf bytes. JSON below is readable diagnostic notation,
not a second on-wire encoding. Binary digests are shown as hex only in examples.

### Connection messages

- Hello {device_id, minor, asset_digest, max_envelope_bytes, max_blob_bytes,
  capabilities[], required_capabilities[], input_injection_enabled}. Capabilities include STATE_SYNC, LOCAL_UI, APPROVAL_ONCE,
  DENIAL, NAVIGATION, ESCAPED_TEXT, DIAGNOSTICS. Approval bodies always use the
  mandatory escaping below; no capability can enable literal Unicode approvals.
- HelloAck {minor, asset_digest, capabilities[], required_capabilities[],
  heartbeat_ms=1000, session_limit=64, blob_limit=32768, max_envelope_bytes=4096}.
  Hello device_id must byte-equal the authenticated ID. Negotiation is intersection, not union. Missing required capabilities fail.
  asset_digest is SHA256 of generated artwork, font, layout, hit boxes and motion
  constants. Mismatch permits diagnostics/update guidance only. Host capability
  intersection removes real decisions when input_injection_enabled=true OR the
  authenticated key class is QA. Key class is host configuration, not self-report.
- Error {code, related_action_id?, detail}. Detail is bounded 192-byte diagnostic
  text; errors cannot themselves authorize state changes.
- Heartbeat {applied_revision, monotonic_ms}. Monotonic clock is sender-local;
  never compare timestamps from different machines for expiry/security decisions.
- Resync {applied_revision, reason}. Reasons: INITIAL, REVISION_GAP, STATE_INVALID,
  HOST_RESTART. Host answers a new snapshot. Pending intents survive an in-connection resync only if their targets remain
  valid; all intents are discarded on a new connection.

### Authoritative model

A snapshot is an atomic transaction, never one large protobuf:
- SnapshotBegin {revision, session_count, view, usage, overflow_count}.
- SnapshotPart {revision, index, sessions[]} with at most **two** full Sessions.
- SnapshotCommit {revision, parts_count, sha256}.
Digest is SHA256 of the concatenation of u32LE encoded length followed by exact
decrypted Envelope bytes for SnapshotBegin and each SnapshotPart, in order
(excluding Commit). Both sides
hash wire bytes; neither re-encodes them. Parts have contiguous indexes starting 0;
count must match Begin; all epochs/revisions must agree. Stage in PSRAM; publish
only after full validation and digest verification. No delta interleaves a snapshot;
host coalesces later state and sends a delta after Commit. A fresh Resync discards
staging. A snapshot with revision below the current revision cannot roll back state;
equal revision is allowed only for a requested resync and atomically replaces
local state even after STATE_INVALID. The authoritative host content is trusted;
this explicitly repairs damaged local state. A new authenticated authority epoch
resets revision tracking to zero while preserving uncertain receipts separately.

Delta {base_revision, revision, upsert_sessions[], remove_session_ids[],
optional view, optional usage, optional overflow_count}. Maximum two upserts and
8 removes per envelope. If the ENCODED envelope exceeds 4096 bytes even within these count limits,
use a new snapshot. String/count maxima are not a guarantee that a combined delta
fits. Encode into a bounded buffer before enqueueing; error means no partial send.
SnapshotPart may contain one rather than two sessions to fit the same cap. In particular,
never split a seat swap across deltas. Each upsert is a **whole Session replacement**;
absent approval clears it. Only top-level optional fields mean unchanged. Validate
IDs, provider-side seats, uniqueness and referenced view against the merged model
before publication. A valid update removing the selected session also supplies a
cleared/replaced view in the same transaction.

Revision increases; base must equal applied revision. Host bases the next delta
on the last revision **sent** on this reliable ordered connection, including a
just-sent snapshot Commit. Applied acknowledges processing/flow control only.
Allow at most eight unacknowledged revisions; then coalesce unsent updates.
Board sends Applied {revision} after atomic model publication, not LCD completion.
After sending Resync the board ignores all deltas until a valid SnapshotCommit.
Host receiving Resync cancels unsent deltas and starts a fresh snapshot at its
latest revision; already-sent deltas are harmlessly ignored. The board sends one Resync and disarms decisions while waiting. Repeated Resync
before SnapshotBegin is coalesced. After Begin, invalid part/count/ID/digest or
60-second transaction timeout terminates the connection and discards staging;
never send another Resync that could leave host/board in different transactions.

Session {id, provider, title, status, waiting_reason, seat, appearance_seed,
  state_age_ms, task_blob, task_truncated, navigation_available, approval, branch,
  active_subagents, display_name, unlinked_review_blob}.

- provider: CLAUDE or CODEX.
- status: WORKING, IDLE, WAITING, DISCONNECTED. WAITING includes permission and
  questions. Source evidence controls status; timeout is not evidence of approval.
- waiting_reason: NONE, PERMISSION, QUESTION, UNKNOWN.
- appearance_seed: fixed32 FNV-1a of original provider-qualified ID, shared with
  desktop artwork. If wire ID is hashed for length, this preserves existing looks.
- branch: optional UTF-8 <=192 bytes; active_subagents: optional u32 (presence
  distinguishes unknown from zero); display_name: UTF-8 <=192 bytes, same short
  nameplate input as desktop. title is full session title.
- state_age_ms: sampled state age; refreshed at state transitions and at most every 30 seconds during an unchanged state. The board advances existing patron animations locally; a newly connected board can start up to 30 seconds behind the desktop idle-animation phase. Age-only collector ticks do not trigger wire updates.
- unlinked_review_blob: optional BlobRef for generated desktop-only permission
  guidance, enabling UNLINKED_PERMISSION without inventing a request.
- task_blob: BlobRef {sha256, byte_length}; empty optional ref means unavailable.
- approval: optional Request {id, digest, title, tool_name, body_blob, state,
  allowed_decisions[], desktop_only_reason}. States: PENDING, SUBMITTING,
  DELIVERED_TO_ADAPTER, EXPIRED, CANCELLED, FAILED, UNKNOWN. The delivered
  decision is included separately; no state asserts provider execution.
  Multiple pending hooks for one session are serialized oldest-first by creation
  sequence (request ID as tie-breaker). Do not replace the displayed pending request
  when a later one arrives. A terminal request stays visible for five seconds unless
  the user closes its review; then expose the next request. Host retains its outcome
  for at least ten minutes in a 128-entry LRU; reject new dispatch as BUSY if that
  bound is full with unexpired entries rather than losing uncertain outcomes.

Request digest = SHA256 of the following byte sequence, independent of protobuf
serialization: ASCII `tc-req-v2` || LP(request_id) || LP(session_id) || provider:u8
|| LP(full_title) || LP(tool_name) || body_sha256:32 || body_length:u32LE || decision_count:u8
|| sorted distinct decision values:u8[]. LP is u16LE byte length plus exact UTF-8
bytes (no normalization). IDs/titles obey prior limits. Empty/default values and
unknown decisions are invalid. Zero allowed decisions is valid only for a
DESKTOP_ONLY request and still has a defined digest. Rust/C golden vectors match this preimage.

Board must recompute this digest from the full title, tool_name and verified body bytes associated
with the actual completed review rendering. DECIDE includes displayed_body_sha256
and this recomputed digest. Host validates both against its current immutable
request, verifies current provider availability, and atomically claims the request.
A digest echo from metadata alone is not sufficient. Full body is scrollable; a
locally verified body does not prove the human read every page.

DELIVERED_TO_ADAPTER means only that the response was delivered to the hook
transport. Current Claude hooks do not acknowledge Claude acceptance/execution.
UI says SENT TO CLAUDE, never command approved/executed. Write failure or uncertain
provider outcome is UNKNOWN. FAILED and EXPIRED are terminal/non-actionable.
Codex currently lacks a decision adapter: its requests are DESKTOP_ONLY. This
protocol does not invent provider support. Keep a bounded per-request outcome LRU;
never clear all outcomes at capacity or evict in-flight dispatches.

View {selected_session_id, page, task_anchor, review_request_id, review_anchor,
  generation, origin_device_id, origin_connection_epoch, applied_action_id,
  review_kind, review_blob}.
- page: CAFE or REVIEW. Empty selection means automatic scene focus.
- review_kind: REQUEST, TASK or UNLINKED_PERMISSION. TASK/UNLINKED_PERMISSION
  are always non-actionable and may have empty request ID. review_blob identifies
  the exact task/information body; missing content shows unavailable, not stale text.
- Anchors are UTF-8 byte offsets into their identified blob; validate codepoint
  boundaries and clamp at render time. They are not pixel offsets or page counts.
- Changing selected session resets task anchor; changing request resets review
  anchor. REVIEW requires a current matching session; REQUEST additionally requires its
  exact current request. Shared navigation can move either display. Any remote
  view generation cancels the current touch gesture before new hit testing.
- generation increments for shared view changes; origin/applied_action_id permits
  matching intents across device AND connection epochs; action_id alone is insufficient. Model revision is separate.
- Sleep/backlight is device-local and not part of the shared view.

Usage {tokens_display, cost_display}; at most 24 ASCII bytes each, produced by the
same formatter as desktop. This is informational, not financial computation.

### Text transfer

BlobGet {sha256}; serve hashes referenced by the current model or any snapshot/
  delta sent to that connection in the last 30 seconds. Pin accepted transfers
  until completion/cancellation. Bound retained bodies to 512KiB per host link;
  prioritize selected task and open review, evict unselected history first.
BlobChunk {sha256, total_length, offset, data, final}. Transfer is ordered; validate
bounds, exact length, UTF-8 and SHA-256 before publishing. No partially received
blob may enable approval. Require exactly contiguous offsets, consistent length and one active blob
transfer per link. Unexpected/repeated/overlapping chunks abort that transfer. Hash mismatch evicts the buffer and retries
once, then reports error. Closing a review can cancel its pending BlobGet.
BlobUnavailable {sha256, reason}; show unavailable/open desktop, never substitute
another session's cached text. Session removal unpins its blobs, but does not rewrite an in-flight immutable
  review. Prefetch only selected content and open reviews; coalesce unsent unselected
  task hash updates to at most one per five seconds. Selected text transfers are
non-preemptible by newer task versions: finish the pinned body, then fetch only
the newest replacement after a minimum five-second interval. Status and actions
remain immediate. This bounds bandwidth while avoiding churn starvation. Approval
bodies are immutable, fetched with higher priority than task content. Cancellation uses BlobCancel
  {sha256}; complete already-sent chunks may arrive and are safely discarded.

### Board actions and host results

Action {id, observed_revision, kind, session_id, request_id?, request_digest?,
  blob_id?, anchor?, decision?, displayed_body_sha256?}.

| kind | Required payload | Immediate local behavior | Host validation / effect |
|---|---|---|---|
| SET_SELECTION | session_id (empty clears) | Move hand and show cached task | Session exists; update shared view |
| OPEN_REVIEW | session_id, review_kind, request_id when REQUEST, blob_id | Open matching cached question/loading state | Same current content/request, set REVIEW; non-request reviews cannot decide |
| CLOSE_REVIEW | no target | Return to café | Set CAFE, preserve valid selection |
| SET_TASK_ANCHOR | session_id, blob_id, anchor | Scroll cached task | Same task blob, valid UTF-8 boundary |
| SET_REVIEW_ANCHOR | session_id, request_id, blob_id, anchor | Scroll question | Same request/body, valid boundary |
| OPEN_DESKTOP | session_id | Show opening feedback, return to café | Atomically select session and close review; resolve trusted target, rate-limit to one launch/second per device; report platform failure |
| DECIDE | session_id, request_id, recomputed request_digest, displayed_body_sha256, decision | Disable buttons, show SENDING | Current pending request, recomputed digest/body hash, production-class authenticated key, allowed decision, active authenticated connection |

Decision values are APPROVE_ONCE and DENY only. No allow-always or scope expansion.
Resolve touch-down against the displayed/interpolated patron sprite rectangles,
front-most first; store stable ID until release. Cancel when the bound patron
vanishes or a remote view replaces the interaction surface. A first tap selects/
toggles immediately. A second tap within 450ms must hit the SAME stable patron at
its current visual location (12px slop); OPEN_DESKTOP atomically reselects it even
if the first tap cleared selection. No second toggle. Never reuse old coordinates.
Double-tapping footer opens review. Double-tapping review text opens desktop and
closes review; buttons do not participate in double-tap text gestures.

Decision buttons arm only after: verified complete body and recomputed digest;
LCD completion for this exact request/view; local opening of the review (or an
explicit tap on the body to acknowledge a remotely opened review); all fingers up;
500ms of no touch since the last release/open/request change. Touch-down must occur
AFTER all these conditions, and release must still match request/view and button.
Any remote view/request change or sleep/disconnect disarms and cancels contacts.
Thus an opening double tap/triple tap cannot activate a button underneath it.
Every decision immediately disarms; no repeated tap may create another decision.

Production builds compile out injection and report input_injection_enabled=false.
QA firmware reports true; host refuses **real provider** decisions from that link.
Synthetic approval tests use a separately instantiated fake adapter and test-only
provisioned key, with no connection to the live broker. Provisioning refuses QA firmware with a
PRODUCTION-class key. Production build disables injection; QA may advertise
APPROVAL_ONCE for a fake adapter, but the live host rejects decisions by its own
trusted key class regardless of the reported flag. No firmware-only capability
can elevate a QA key. Trusted firmware/provisioning is assumed; this is not
remote attestation. Never disable the live-host check for testing. Physical final approval tests require real touch
and an explicitly authorized synthetic request.

ActionResult {action_id, status, applied_revision, view_generation, reason}.
Statuses: APPLIED, PENDING, REJECTED, UNKNOWN. Reasons include SESSION_GONE,
STALE_REQUEST, STALE_CONTENT, UNSUPPORTED, NOT_CONNECTED, BUSY, INVALID,
EXPIRED, NAVIGATION_FAILED, PROVIDER_FAILED, OUTCOME_UNKNOWN, ALREADY_DECIDED.
Host queues state Delta/SnapshotCommit before ActionResult for its revision.
The priority control lane may deliver the result before bulk state. Board therefore
holds EVERY ActionResult (including REJECTED, UNKNOWN and PENDING) until its applied
revision reaches result.applied_revision, or a newer snapshot commits. If state is
deferred behind snapshot/coalescing, queue the result only with the containing state. REJECTED and no-op results carry the
current authority revision; send that state first if this link has not seen it.
PENDING may follow the SUBMITTING state before a terminal result exists.
PENDING is receipt/dispatch, not success. A later APPLIED/REJECTED/UNKNOWN completes
it. Provider request state is independently broadcast to every connected board.

Strictly process actions in order per connection. A duplicate retained action ID
with identical payload returns cached result, including pending. Same ID with a
different payload terminates the connection. IDs below the high-water mark but
outside the 64-result cache are rejected without executing. Keep in-flight
requests pinned in the cache. A new connection cannot retry an old action ID.

Local reversible intents are replayed over authoritative state until resolved.
Any terminal result retires the matching intent, including REJECTED/UNKNOWN. Do not retire or display a result while its revision is still in flight. Once
that revision is applied, remove terminal intents and reapply later intents over
the new authority state. This applies to rejection as well as success and prevents
priority-lane delivery from causing a visual rollback.
An older incoming view update must not visually undo a newer unacknowledged local
selection. Once acknowledged, a later authoritative action from another device
wins. Coalesce only unsent selection/anchor intents; never coalesce across an
OPEN_REVIEW, OPEN_DESKTOP or DECIDE ordering boundary. At 32 queued actions,
block new network-dependent input and show BUSY while keeping the link.
Critical means ActionResult, Error, and model updates representing a decision
transition. Host outbound critical queue overflow disconnects explicitly; replaceable
unsent state, heartbeat and task blobs coalesce instead of filling that queue. SET_SELECTION,
SET_TASK_ANCHOR, SET_REVIEW_ANCHOR, OPEN_REVIEW and CLOSE_REVIEW are reversible.
OPEN_DESKTOP and DECIDE are side effects and never replay. On reconnect discard
all intents; the user may reissue reversible input after fresh synchronization.
No intent is automatically assigned a new ID across connections. Do not automatically retry DECIDE after an uncertain result/disconnect.

Desktop and board buttons use the same authoritative dispatch function; provider
adapters are not invoked directly by rendering or input code. Dispatch transitions
PENDING→SUBMITTING atomically before releasing the authority lock, preventing two
boards from executing different decisions. A dispatch without a definite transport outcome within three seconds becomes
UNKNOWN, retaining its receipt and preventing retry. Successful hook writes
produce DELIVERED_TO_ADAPTER with the decision; definite
pre-dispatch failure is FAILED; ambiguous dispatch outcome is UNKNOWN. A board
keeps an UNCERTAIN receipt keyed by (authority_epoch, request_id) across
host/connection epochs until explicitly dismissed; never match a receipt to a
request from another authority epoch. Before emitting DECIDE, persist its epoch/request-qualified pending receipt in
board NVS; failure to persist blocks the action. Receipts survive board reboot,
becoming UNCERTAIN until a same-epoch authoritative result is known or explicitly
dismissed. At most eight receipts; if full, block new decisions until one is dismissed. Dismiss with the explicit CHECKED ON DESKTOP
button in the uncertain review, never by generic close/navigation;
absence of a request after restart never implies that its action did not happen.

## 5. State transitions

Connection: DISCONNECTED → AUTHENTICATING → NEGOTIATING → SYNCING → READY.
Authentication/version/asset failure → ERROR (non-actionable). Timeout or radio
loss from any connected state → DISCONNECTED. READY requires a validated snapshot;
opening approvals additionally requires the complete verified body blob.

Board review: CLOSED → LOADING → READY → SENDING → COMPLETED.
LOADING→UNAVAILABLE for missing/unsupported content. READY/SENDING→STALE if request
changes, expires or disappears. SENDING→UNCERTAIN on disconnect or ambiguous
provider result. Neither STALE nor UNCERTAIN shows enabled decision buttons.

Board display: AWAKE → ASLEEP after 600000ms without physical touch; host updates
do not reset this timer. New waiting requests do not wake it automatically: this
preserves the requested no-touch sleep behavior; desktop alerts remain available. First physical touch wakes and is consumed (no selection
or approval). Incoming state remains synchronized while asleep; render latest state
when waking. QA-injected touches use the same gesture path and are test-only.

Seat order: Claude seats [0,1,4,5], Codex [2,3,6,7], each numbered 1–4 row-major
within its side (first second-row seat is number 3). Working and waiting patrons
are active. Host promotes an active patron into the earliest lower-numbered idle
seat on its side, with the idle occupant swapping. For initial placement, partition
by provider; order active before idle then stable source arrival sequence (ID as
tie-breaker), allocate smallest free seat. Retain existing assignments on refresh.
New arrivals take the smallest free seat on their provider side in stable arrival
order; beyond four per provider remain at seat=255. When a seat frees, take the
oldest outside patron on that side, active before idle, then apply promotion.
No cross-provider borrowing. Selection may refer to an outside session. Extract a pure host seat allocator independent of render/animation state and use
its assignments on desktop and board. Generate shared paths and speed (60px/s,
matching current desktop) rather than a contradictory fixed 600ms duration.
Arrival: door→aisle→assigned chair. Departure: reverse path to door. Swap: existing
two offset aisle paths. Allocation updates immediately; a newcomer waits visually
at the door until the departing sprite clears its target chair. Both renderers
apply the same occupancy animation rule; no allocator waits for animation. Hit testing binds the patron visible
at the touch location, not the final seat while it is still moving. Incoming
newer seat changes rebase animation from the current visual position.

## 6. Example exchanges

Connect:
```
board -> Hello(device_id="qa-board", major=2, asset_digest=ASSETS, input_injection_enabled=true)
host  -> Envelope(authority_epoch=A, connection_epoch=C, hello_ack={...})
host  -> SnapshotBegin(revision=1, session_count=2, view={page:CAFE})
host  -> SnapshotPart(revision=1, index=0, sessions=[...])
host  -> SnapshotCommit(revision=1, parts_count=1, sha256=TRANSACTION_DIGEST)
board -> Applied(revision=1)
board -> BlobGet(task_hash)
host  -> BlobChunk(task_hash, offset=0, data=..., final=true)
```
Tap then scroll:
```
board locally selects claude:demo immediately
board -> Action(id=1, observed_revision=1, SET_SELECTION, session_id="claude:demo")
host  -> Delta(base_revision=1, revision=2, view={selected_session_id:"claude:demo", ...})
host  -> ActionResult(id=1, APPLIED, applied_revision=2)
board -> Applied(2)
```
Approval race:
```
board A -> DECIDE(id=7, request_id=R, digest=D, APPROVE_ONCE)
host atomically marks R SUBMITTING; replies PENDING
board B -> DECIDE(id=9, request_id=R, digest=D, DENY)
host -> board B REJECTED(ALREADY_DECIDED)
hook transport receives A decision
host broadcasts R DELIVERED_TO_ADAPTER; replies APPLIED to board A
```
Disconnect after dispatch: board shows UNCERTAIN, reconnects with a fresh epoch,
receives request state through the fresh snapshot (no separate query), retains
its uncertain receipt even if that request is absent, and never resends DECIDE.

## 7. Rendering and performance

Generate board artwork from the same source as desktop. Keep fonts, character
appearance, focus hand, task footer and approval layout consistent. Render and
animate locally; network traffic must remain zero for unchanged model animations.
The physical adapter uses changed-column LCD strips with polling DMA; it is
measured independently of protocol. A bounded 32-packet receive queue keeps BLE
write acknowledgements independent of rendering. One serialized model/render
task drains at most eight incoming fragments per iteration; touch has a separate
bounded queue. Large Nanopb structures/staging live in PSRAM, never task stacks.
Use explicit .options bounds and no PB_ENABLE_MALLOC.
Do not claim 60fps without physical validation. Preserve the current 480×272 aspect.

Targets: p95 physical touch-down→press-feedback LCD completion <=75ms;
release→cached-detail LCD completion <=100ms;
cross-device convergence <=200ms from an event entering the host model, local
animation 30fps. These exclude provider polling and launching another desktop app.
Record touch-down, release, local model commit, LCD completion, host receipt, result,
authoritative revision and board application separately. No cross-clock subtraction
without a measured clock offset; round-trip timing uses a single clock.

## 8. Verification gates and migration

Gate A: Claude Code critical review, findings disposition, revised schema/spec.
Gate B consists of B1 native protocol checks and B2 QEMU interaction/visual checks.
Required: native sanitizer tests, protobuf interoperability, state/action fault matrix,
linked production image size within the actual 4MB partition map,
QEMU boot, injected gestures and screenshot checks against local rendering.
Gate C: only after Gate B passes, build production board profile, preserve rollback
image/config using host-side esptool read_flash backup (never commit pairing
secrets), flash and run real BLE/LCD/touch QA. This board uses a single app
partition: rollback means reflashing the saved image over USB, not on-device OTA.
Gate D: macOS/Linux host checks, report exact supported features and untested items.

Minimum cases: all actions/statuses, same patron toggle, correct desktop target,
seat swaps during touches, all eight hit regions, long text, Unicode, missing text,
duplicate/out-of-order/stale actions, invalid delta atomicity, corrupt blobs,
reconnect at every approval stage, two-board conflicting decisions, host restart,
unsupported capabilities, MTU fragmentation, queue exhaustion, disconnected board,
sleep/wake, withheld host replies with immediate local feedback, and rapid input.

Production v2 compiles out the legacy frame/input approval path. Legacy rollback
means reflashing the saved v1 image, not running two approval paths concurrently.
Legacy tools remain in the repository for explicit diagnostics/rollback only;
no silent fallback can hide a failed v2 handshake. QEMU uses the SAME AEAD/framing code over UART with emulated ATT capacities
182/244/509 plus dropped/reordered-fragment fault injection; capacity 64 must
fail UNSUPPORTED_MTU. Model per-connection-event write response delay. Its RNG is test-only.
QEMU screenshot comparison proves renderer output in memory, not physical
touch-to-photon, GT911 behavior, NV3041A timing, BLE flow control or RF latency.

## 9. Text escaping, wrapping and anchors

Keep raw UTF-8 bytes for hashing. For display: LF starts a new line; backslash
is shown as `\\`; CR is shown as `\r`; TAB as `\t`; other ASCII control bytes
as `\xNN` (two lowercase hex digits); non-ASCII scalar values as `\u{hex}`
(lowercase, no leading zeros). ASCII graphic characters and spaces remain literal.
No CR stripping, Unicode normalization, hidden control handling or case folding
is permitted for approval bodies. Explain escapes in the scrollable review preamble.
Keep only lazy per-line source-byte starts (bounded by raw byte length plus one),
not a per-cell map for all cached bodies. At most one current layout is materialized.

Review wraps HARD at 54 expanded cells (no word wrapping), without dropping
whitespace. Every LF emits a line, including a final empty line after trailing LF.
Render tool_name then full title, then escape preamble, then the verified body.
Host includes tool and title in the scrollable body as well; the explicit Request
fields define identity for header/layout validation. Board independently checks
that the first tool/title block fits within nine hard-wrapped 54-cell lines; if
it does not fit, disable decisions locally even if the host advertises them. A truncated header does
not replace that full first-page content. Nine lines visible, page step eight lines. Task text
uses the same byte mapping and whitespace-preserving wrap at 32 columns, two lines
visible, page step two. Anchors are the source-byte offset of the first cell in a
line; if an escape spans wrapped lines, both may share an anchor and the renderer
chooses the first such line. Clamp an anchor into the line containing it. Reject
non-UTF8-boundary anchors on host; if text is not cached, board temporarily shows
loading and applies the validated anchor once content arrives. Shared golden
fixtures specify whitespace, CRLF, backslashes, non-ASCII and 32768-byte limits.

## References

- https://github.com/nanopb/nanopb
- https://github.com/tokio-rs/prost
- https://www.bluetooth.com/specifications/specs/core-specification-6-2/
- https://www.rfc-editor.org/rfc/rfc8439 (AEAD)
- https://www.rfc-editor.org/rfc/rfc5869 (HKDF)
