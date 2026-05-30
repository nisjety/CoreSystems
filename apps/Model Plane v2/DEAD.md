# ⛔ This directory is DEAD ⛔

**Status:** decommissioned 2026-05-21.

**Do not:**
- Add new code here.
- Import anything from this directory into other code.
- Deploy these containers in production.
- Use this as a reference for new tools — see `apps/Model Plane/` instead.

**Canonical home:** `apps/Model Plane/` (Go services + Rust inference-core,
single source of truth `proto/model_plane/v1/*.proto`).

**Why this exists at all:** kept for archival / git-history reasons. The
Python "agent-core / capability-core / etc." service shapes were an
exploratory v2 that turned out to duplicate capabilities already plumbed in
v1 (notably `structured_output_schema` on `InferRequest`, which obviates the
Python-side "forced tool-call" coercion).

**If you find yourself reading code in here**, ask: *is there a v1 equivalent
I should be touching instead?* The answer is almost always yes. See
`MODEL_PLANE_V2_ARCHITECTURE.md` for the v2 → v1 migration map.
