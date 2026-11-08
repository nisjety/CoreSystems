# W5-S6 progress: 077 and 075 decision notes (docs only)

Date: 2026-11-07. Branch `quarryv2-next` (local only).

## 077 decision note: user's own Chrome, parked

- Status: parked. Decision note only. No code and no flag.
- Ruling: owner, 2026-09-28. The table row reads "Chrome control: Hosted, governed browser first. Improve the live view, takeover and approvals. The user's own Chrome and its logins are not driven." Source: `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_PAGE_GAP_PLAN_2026-09-28.md`, section 5.
- Why the code cannot drive it today: `require_isolated_agent_egress` (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/admission.rs:119`, called from `lifecycle.rs:173`) refuses agent runs unless egress is isolated. A user-browser backend is not attested as isolated, so the gate refuses it.
- What reopens it (all three are needed):
  1. A new owner ruling that replaces the 2026-09-28 row.
  2. A multi-backend browser abstraction. This is plan epic E4.5 (remote backends), gated by ADR-10.
  3. A redesigned fail-closed gate that can attest a user-browser backend. The egress check cannot simply be relaxed.

## 075 deletion note: nothing to delete

- Deprecation and sunset headers: covered by the plan's D5 (WS5 slice 8). Not part of this slice.
- Signed images: a grep of `apps/Ingestion Plane/Quarry-v2` and of `apps/Frontend Plane/verevonv3/src` and `apps/Frontend Plane/verevonv3/apps/gateway/src` for "signed image", "image signing", "signed_image" and "sign_image" found no matches on 2026-11-07. Nothing is built, so nothing is deleted.
- A usefulness event, if it is ever built, carries ids and domains only (plan E5.2).

## Not done (W5-S6 scope)

- Stale-doc rows: `FIXES_APPLIED.md` (84, 92), `SCRAPING_ENGINE_GAPS.md` (19, 136-204) and `GAP.md` (`blockAds`, plan line 3461). Not edited. The plan asks for an archive or update row with a Verified banner. That is still open.

## Ran

- Docs only. Citations above were checked by reading the files and by grep on 2026-11-07.
