**Review 3: companion protocol v2 (immutable inputs under `target/protocol-schema/review-3/`)**

**Verdict:** I found no BLOCKERs. Two HIGH findings are missing protocol semantics, not implementation work. Each is a short spec fix. Implementation can start now on crypto, framing, the protobuf schema, the model and the seat allocator. Fix HIGH‑1 and HIGH‑2 before writing the board's code for handling action results and before building the review renderer and its golden fixtures. Gate A (A04) can then pass on a diff check without another full review.

Line numbers refer to `companion-v2.md` unless another file is named. I read the four files, used review 2 as background, and ran nothing.

## Prior findings from review 2

| Finding | Status | Evidence |
|---|---|---|
| N1 envelope caps | Resolved | :62, :200–205. Every string and count is bounded (:47–73). By my estimate a worst-case Session is about 1.45 KB, so a two-session part fits in 4096. |
| N2 minimum MTU and timing | Resolved in the spec | :121, :125–130, :142, :148, :520–522. Real-link timing is still acceptance work. |
| N3 QA versus production keys | Resolved | :90–91, :170–172, :327, :348–357, test-plan:94–98 |
| N4 Gate A paperwork | Resolved | B1/B2 split (:500), R01–R22 and fuzz gate (test-plan:122–161). There is a Low ID collision, listed below. |
| H1 receipts | Resolved | :245–248, :400–405, :474–476 |
| H2 tool name visible | **Partially resolved** | See HIGH‑2. |
| H3 legacy approval path | Resolved | :517–519 |
| H4 concurrent hooks | Resolved | :243–248 |
| H5 auth wire format | Resolved | :96–106, :168 |
| H6 deltas after Resync | Resolved | :193–198, :217–220. The failure path is MED‑1. |

## HIGH

**HIGH‑1: Two-lane priority contradicts the rule that state arrives before its result.**
- Control fragments take priority at every fragment boundary, and ActionResult is on the control lane while Delta and SnapshotCommit are on the bulk lane (:127–130, :144–146).
- So an ActionResult queued after a 4 KB Delta finishes arriving before that Delta does. At ATT capacity 182 the Delta is about 29 fragments.
- That breaks the rule "Host sends state Delta/SnapshotCommit before ActionResult" (:363). "Defer the result … until a state containing its effect has been sent" (:364–366) is also ambiguous: "sent" could mean queued or fully transmitted.
- The board only holds its local visual state for APPLIED results (:379–380). REJECTED intents "are removed immediately" (:381). A REJECTED or no-op result can therefore show stale state while its Delta is still arriving. That is exactly what R11 (test-plan:136) forbids.
- Approval safety still holds, because the host validates the digest and claims the request atomically.
- **Fix:** say the ordering guarantee covers only the order the host queues messages. Require the board to hold every ActionResult, whatever its status, until its applied revision reaches the result's revision, or a newer snapshot commits.

**HIGH‑2: The tool name has no defined place on the wire, so the first-page rule can't be implemented consistently.**
- The spec requires the full tool name and title first, before the escape preamble, and requires DESKTOP_ONLY if they don't fit in 9 lines (:539–541). It also requires both to appear in the body (:76–78).
- `Request` has a title field but no tool field (proto:155–164), and the digest input has no tool either (:250–253).
- The board, the desktop and the golden fixtures therefore can't agree on where the tool name sits or where the preamble goes. The board also can't check the 9-line rule itself.
- **Fix:** either:
  - add a bounded `tool_name` field to `Request` and to the digest input, or
  - define the body's first two lines as the tool name and title.

  Fix the page‑1 order as tool, title, preamble, then the rest. The board must also treat the request as non-actionable if the header overflows 9 lines, not only rely on the host's DESKTOP_ONLY flag.

## MEDIUM (fix in the spec; doesn't hold the gate)

- **MED‑1: Snapshot failure handling is undefined.**
  - What happens when the 60 s snapshot timeout fires is not stated (:142–143), nor what happens on a bad Commit digest or an invalid part.
  - A new Resync makes the board discard its staged snapshot (:194–195), but the host treats a repeated Resync as part of the same snapshot and does not restart it (:219–220). The board then receives parts with no Begin.
  - **Fix:** any snapshot validation failure or timeout terminates the connection. Alternatively, a new SnapshotBegin replaces the staged snapshot and the host restarts on a failure Resync.
- **MED‑2: The negotiated envelope size is one number for two lanes, with no minimum.**
  - Hello and HelloAck carry a single `max_envelope_bytes` (:162–167, proto:43,56), and either side may lower it (:83, :143).
  - That applies ambiguously to the 4096-byte bulk and 1024-byte control lanes. A peer could set it below one Session, making snapshots impossible.
  - **Fix:** v2.0 requires exactly 4096 / 1024 / 32768 (bulk, control, blob) and fails any other value with ERROR_LIMIT.

## LOW

- "One bounded buffer per direction" (:81–82) contradicts the two lanes per direction (:125). Size it as 4096 + 1024.
- The receiver isn't required to reject a payload sent on the wrong lane (:144–146).
- Decisions aren't explicitly disarmed while a Resync is pending (:217, :339–346). This is safe because of host validation, but stating it would help.
- No hook-dispatch timeout leading to UNKNOWN is defined (:398–399).
- Whether uncertain receipts survive a board reboot is not stated (:400–405).
- Test-plan IDs A01–A04 are used both in Gate A (test-plan:9–12) and for the B1 action rows (:41–44).
- Gate C step 4 repeats Q06 decisions in production but doesn't cite the "explicitly authorized synthetic request" required at :356–357.

## Implementation acceptance work, not missing semantics

- **Snapshot time on the real link at capacity 182 (R01).** My rough estimate: about 32 parts × 3 KB, sent as 146-byte write-with-response fragments, takes 10–20 s at 15–30 ms connection intervals. That fits inside 60 s but is not measured.
- **Other open measurements and checks:**
  - the ChaCha20-Poly1305 benchmark (:112–113)
  - the ≤200 ms convergence and ≤75/100 ms touch targets
  - golden vectors (R02)
  - the 10⁶-case fuzz run
  - image size (R22)
  - control-lane latency under bulk load
