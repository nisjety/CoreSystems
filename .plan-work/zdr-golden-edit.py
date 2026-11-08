import hashlib
import json
import os
import re
import shutil

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
GOLDEN = ROOT + "/apps/Model Plane/api/contracts/space-surface.v1.json"
raw = open(GOLDEN, "rb").read()
crlf = b"\r\n" in raw
text = raw.decode("utf-8").replace("\r\n", "\n")
d = json.loads(text)

# The file must round-trip byte for byte, or this script would reformat rows it does not own.
assert json.dumps(d, indent=2, ensure_ascii=False) + "\n" == text, "golden is not canonical indent=2 JSON"
ids = [r["id"] for r in d["rows"]]
assert ids == sorted(ids), "rows are expected to be sorted by id"
by_id = {r["id"]: r for r in d["rows"]}


def bff(method, path, handler, route_class, decision, role, zdr, basis, idem, flag, reason):
    return {
        "id": f"bff:{method}:{path}", "kind": "route", "plane": "frontend", "service": "verevonv3-gateway",
        "method": method, "path": path, "handler": handler, "route_class": route_class,
        "decision_type": decision, "min_role": role, "zdr_class": zdr, "basis": basis,
        "idempotency": idem, "flag": flag, "payload_allow_list": [], "reason": reason,
    }


def mg(method, path, handler, decision, role, zdr, basis, idem, flag, reason):
    return {
        "id": f"mg:{method}:{path}", "kind": "route", "plane": "model", "service": "model-gateway",
        "method": method, "path": path, "handler": handler, "decision_type": decision,
        "min_role": role, "zdr_class": zdr, "basis": basis, "idempotency": idem, "flag": flag,
        "payload_allow_list": [], "reason": reason,
    }


new_rows = [
    bff("POST", "/api/v1/spaces/{space_ref}/threads", "domains/spaces/human_posts.rs::create_post",
        "DurableWrite", "thread-decision", "editor", "refuse", "guard:durable_write_refusal", "idempotency_key",
        "human_posts_api",
        "A member's post is stored free text under their name: refused for a ZDR org before any Control or model call (typed 412)."),
    bff("POST", "/api/v1/spaces/{space_ref}/threads/{thread_id}/messages", "domains/spaces/human_posts.rs::reply_in_thread",
        "DurableWrite", "thread-decision", "editor", "refuse", "guard:durable_write_refusal", "idempotency_key",
        "human_posts_api",
        "A member's reply is stored free text under their name: refused for a ZDR org before any Control or model call (typed 412)."),
    bff("PATCH", "/api/v1/spaces/{space_ref}", "domains/spaces/room_admin.rs::update_room_details_route",
        "Config", "membership", "manager", "n/a", "registry", "last_write_wins", "room_details_ui",
        "Room name and topic are room configuration labels, not conversation content; allowed under ZDR like room creation (K4-14). The room admin tests pin that no Model Plane call is made."),
    bff("PATCH", "/api/v1/spaces/{space_ref}/members/{member_id}", "domains/spaces/room_admin.rs::change_member_role_route",
        "Config", "membership", "manager", "n/a", "registry", "last_write_wins", "membership_writer_api",
        "Role change: ids and roles only, allowed under ZDR (K4-14). Granting or changing manager or owner needs an owner."),
    bff("GET", "/api/v1/spaces/{space_ref}/artifacts", "domains/spaces/room_artifacts.rs::list_room_artifacts",
        "ModelRead", "thread-read-decision", "member", "n/a", "registry", "n/a", "room_artifacts",
        "Read of artifact summaries, never content. A ZDR org gets an empty list and retention.zdr = true; an org-core outage reads as null, not as a posture."),
    bff("GET", "/api/v1/spaces/{space_ref}/events", "domains/spaces/stream.rs::space_events",
        "ModelRead", "thread-read-decision", "member", "poll_only", "posture-decided", "n/a", "realtime_stream",
        "A ZDR org (or an org-core outage) gets one connected frame with mode poll_only and a closing zdr_poll_only, never a push stream; the caller polls."),
    mg("POST", "/v1/threads", "space_routes/posts.rs::create_space_post", "token_posture", "authenticated",
       "refuse", "guard:require_non_zdr_durable_mutation", "idempotency_key", "human_posts_api",
       "A member's post: refuses a ZDR or waived token before it runs (zdr_posture.rs DURABLE_MUTATIONS)."),
    mg("POST", "/v1/threads/:thread_id/messages", "space_routes/posts.rs::append_space_post", "token_posture",
       "authenticated", "refuse", "guard:require_non_zdr_durable_mutation", "idempotency_key", "human_posts_api",
       "A member's reply: refuses a ZDR or waived token before it runs (zdr_posture.rs DURABLE_MUTATIONS)."),
    mg("GET", "/v1/spaces/:space_id/events", "space_sse.rs::space_events", "thread-read-decision", "authenticated",
       "poll_only", "posture-decided", "n/a", "realtime_stream",
       "Relays Session Core's event stream as ids and closed words only; a ZDR caller gets mode poll_only, not a live stream."),
]
for row in new_rows:
    assert row["id"] not in by_id, row["id"]
    d["rows"].append(row)


def reclassify(row_id, **changes):
    row = by_id[row_id]
    for key, value in changes.items():
        row[key] = value


reclassify("bff:PATCH:/api/v1/spaces/{space_ref}/instructions", basis="guard:durable_write_refusal",
           reason="Authored instruction text is stored content: refused for a ZDR org before any Control call (typed 412).")
reclassify("bff:POST:/api/v1/spaces/{space_ref}/conversations/{conversation_id}/agent-action-grants",
           basis="guard:durable_write_refusal",
           reason="Grant create is a durable write (plan 1.3 #8): refused for a ZDR org before any Control decision. A revoke is allowed under every posture.")
reclassify("bff:PATCH:/api/v1/spaces/{space_ref}/threads/{thread_id}/presentation", basis="guard:update_thread_presentation",
           reason="Retitles a thread: the title and preview are refused for a ZDR token in Session Core (typed 412); a pin or persona change stays allowed.")
reclassify("mg:POST:/v1/threads/:thread_id/presentation", zdr_class="refuse", basis="guard:update_thread_presentation",
           reason="Forwards a title, preview or pin to Session Core, which refuses a title or preview for a ZDR token (typed 412); a pin or persona change stays allowed.")
reclassify("mg:PUT:/v1/memories/controls", zdr_class="refuse", basis="guard:set_memory_controls",
           reason="Session Core refuses a ZDR or waived credential before it stores the memory mode (memory_grpc.rs set_memory_controls).")
reclassify("mg:POST:/v1/internal/mail-task/prepare-thread",
           reason="Service-internal bridge that creates an empty user-owned thread (ids only). OPEN: the Control decision it verifies carries no zero_data_retention claim and user-core has no ZDR check on mail tasks, so a ZDR org is not refused here; needs an owner decision.")

d["rows"].sort(key=lambda r: r["id"])
out = json.dumps(d, indent=2, ensure_ascii=False) + "\n"
data = out.encode("utf-8")
if crlf:
    data = out.replace("\n", "\r\n").encode("utf-8")
open(GOLDEN, "wb").write(data)
digest = hashlib.sha256(data).hexdigest()
pin_line = f"{digest} *space-surface.v1.json\n"
pin_bytes = pin_line.encode("utf-8")
old_pin = open(GOLDEN + ".sha256", "rb").read()
if b"\r\n" in old_pin:
    pin_bytes = pin_line.replace("\n", "\r\n").encode("utf-8")
open(GOLDEN + ".sha256", "wb").write(pin_bytes)

# vendored copies: byte-identical to the golden and its pin (zdr-contract-sync.test.sh)
for copy in (
    ROOT + "/apps/Model Plane/rust/services/model-gateway/tests/fixtures/",
    ROOT + "/apps/Frontend Plane/verevonv3/apps/gateway/tests/fixtures/",
):
    shutil.copyfile(GOLDEN, copy + "space-surface.v1.json")
    shutil.copyfile(GOLDEN + ".sha256", copy + "space-surface.v1.json.sha256")

# the pin inside the sync script
sync = ROOT + "/scripts/tests/zdr-contract-sync.test.sh"
s = open(sync, "rb").read().decode("utf-8")
old_hash = re.search(r'space-surface\.v1\.json\|([0-9a-f]{64})', s).group(1)
assert s.count(old_hash) == 1
open(sync, "wb").write(s.replace(old_hash, digest).encode("utf-8"))
print("rows:", len(d["rows"]), "digest:", digest[:16], "was:", old_hash[:16])
