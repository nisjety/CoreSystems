import os
import re

GW = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/apps/gateway/src"
CODES = """approval_already_decided approval_authority_unavailable approval_expired approval_id approval_not_addressed_to_you approval_not_found approval_scope_not_permitted decision feature_unavailable files_binding_unavailable files_read_not_authorized files_upstream_unavailable grant_id history_mode idempotency_key_invalid identity_admin_required invalid_before_ms invalid_document invalid_domain invalid_kind invalid_limit invalid_max_chars invalid_reason invalid_refs invalid_reminder_time invalid_run invalid_sidebar_doc invalid_state invalid_status invalid_thread invalid_upload invalid_wake join_role participants pins_unavailable reason reminder_not_found reminders_unavailable room_notices_unavailable room_pins_limit room_pins_unavailable run_control_unavailable scope sidebar_too_large space_reminders_disabled space_run_control_not_authorized space_sidebar_conflict space_sidebar_disabled space_sidebar_unavailable space_status_disabled sso_policy_unavailable sso_required stable_code status_unavailable system_entries_unavailable too_many_reminders turn_origin upload_length_required visibility wake wake_context zdr_feature_not_supported""".split()

files = []
for root, _, names in os.walk(GW):
    for n in names:
        if n.endswith(".rs") and not n.endswith("_test.rs"):
            files.append(os.path.join(root, n))

out = []
for code in CODES:
    hits = []
    for f in files:
        text = open(f, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r'"' + re.escape(code) + r'"', text):
            line_no = text.count("\n", 0, m.start()) + 1
            # the first string literal after the code within 240 chars is usually its message
            tail = text[m.end(): m.end() + 240]
            msg = re.search(r'"([^"]{8,})"', tail)
            hits.append((os.path.relpath(f, GW), line_no, msg.group(1) if msg else ""))
    # keep test-only and fixture hits out by skipping `mod tests`
    shown = hits[:3]
    out.append(f"{code}  hits={len(hits)}")
    for path, line_no, msg in shown:
        out.append(f"    {path}:{line_no}  {msg[:150]}")

open("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/.plan-work/rea-codes.txt", "w", encoding="utf-8").write("\n".join(out) + "\n")
print("written", len(out))
