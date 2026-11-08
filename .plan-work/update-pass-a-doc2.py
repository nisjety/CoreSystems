import os

os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567")
p = "docs/plans/space-depth-pass-a-and-ui-parity.md"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")


def sub(old, new):
    global b
    assert b.count(old) == 1, (b.count(old), old[:70])
    b = b.replace(old, new)


sub("(items 2 and 3 fixed, U2 done)", "(items 2, 3 and 4 fixed, U2 done)")

old_start = b.index("| 4 | Cluster 1, ZDR on Space writes")
old_end = b.index("\n", old_start)
new_row = ("| 4 | DONE in part (see section 2a). Cluster 1, ZDR on Space writes: each row was checked in code; three of the eight were wrong or stale, "
           "two were real gaps and are fixed, the rest are owner decisions | section 2a | M | Privacy. |")
b = b[:old_start] + new_row + b[old_end:]

section = """
### 2a. Cluster 1 (ZDR on Space writes), row by row

| Row | What the code showed | Result |
|---|---|---|
| D1-401 `POST /v1/threads/:id/presentation` | Session Core refuses a title or preview for a ZDR token (typed 412); a pin or persona change stays allowed, and a test pins both. The model-gateway handler is not the store. | Not a gap. Registry row reclassified `refuse`, basis `guard:update_thread_presentation`. |
| D1-401 `PUT /v1/memories/controls` | Session Core's `set_memory_controls` refuses a ZDR or waived credential (`memory_grpc.rs`). | Not a gap. Row reclassified `refuse`, basis `guard:set_memory_controls`. |
| D1-401 `POST /v1/internal/mail-task/prepare-thread` | A service route that creates an empty user-owned thread. The Control decision it verifies has no `zero_data_retention` claim and user-core's mail-task code has no ZDR handling, so a ZDR org is not refused. | OPEN, owner decision: should Control refuse mail tasks for a ZDR org? Row text now says so. |
| D1-403 BFF durable writes | `PATCH /spaces/{ref}/instructions` stored free text in Convex with no ZDR check, and `POST .../agent-action-grants` (create) had none either, though the registry classed both `refuse`. | FIXED. A shared `zdr::durable_write_refusal` (typed 412, or 503 `zdr_decision_unavailable` when org-core cannot answer) now runs before any Control or Convex call; human posts use the same helper. Grant revoke stays allowed. Tests: refusal comes before the Control role read and the Convex write, an org-core outage is its own typed 503, reads still work, a ZDR create never asks Control for a decision while a revoke does. |
| D1-638, D1-639, D1-174 missing rows | The two human-post routes, the room details and member-role routes, artifacts, the event stream and the model-gateway events route had no registry row. | FIXED. Nine rows added (6 BFF, 3 model-gateway), golden re-vendored and re-pinned, and both "known gap" lists in the lints are gone, so a new unclassified route now fails outright. |
| D1-151 room Ask sends its idempotency key for a ZDR org | `send_receipt_id` returns none for a ZDR turn, a ZDR turn creates no thread, and a test covers it. | Not a gap. |
| D1-390 observe mode | `SESSION_CORE_SPACE_ZDR=observe` is the default: a ZDR-flagged Space decision proceeds on the legacy create and append, and only a counter notes it. The gateway guards posts and the model-gateway runs ZDR turns without a thread, so this is reachable only by a caller that presents a ZDR-flagged decision directly. | OPEN, owner decision SD-12 (flip to enforce after a clean counter week). Not changed. |
| D1-402 seven BFF rows with `min_role: unverified` | Metadata accuracy only; no behaviour depends on it. | Left open, low value. |
"""
anchor = "\nTwo small things to fold in:"
assert b.count(anchor) == 1
b = b.replace(anchor, section + anchor)

if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("doc updated")
