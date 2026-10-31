# wf-W2-U30 progress (WS17.P0, batch 5)

Status: code and tests written; compile check in progress (qtest queued behind the docker lock). Nothing committed yet.
Owned: profile_guard.rs (precheck, principal refusal, ZDR action map, import scope), profile_routes.rs (precheck in every handler, redacted load, restore summary, 4 MiB import const, unit tests), auth.rs (dev stub carries the import scope; one line), main.rs (boot warning, production with the ZDR flag off), routes.rs (profile_import router, 4 MiB cap; the 1 MB cap moved onto the surface router), tests/profile_guard.rs (new), tests/profile_roundtrip.rs (redacted assertions), tests/zdr_refusal_modes.rs (flag-on read and probe now 403; observe set gains profiles/probe), docs/openapi.yaml (profile paths and schemas, 3.1 style), docs/RUNBOOKS.md (one appended section), gateway ingestions/profiles.rs (object rows accepted).
Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md "Slice WS17.P0 detail". Queue: W2-queue.md section 3, BATCH 5, U30.

Deviations from the plan text (also in depth/W2-U30.md):
- origins carries IndexedDB origins only (snapshot has no origin on local or session storage); local and session storage are counts.
- saved_at, rev, snapshot_version not returned (not exposed by the store trait).
- cookie view has `session`, no `same_site` (not stored).
- no store file changed (queue row named cached/postgres profile stores; the plan detail names none).
- 413 is the HTTP layer's plain body.
- the dev-bypass stub carries the import scope (decision for the owner).

Commit plan (each via qcommit.sh with explicit paths, or qcommit-blobs-add.sh for the three shared files that carry other units' hunks: routes.rs, openapi.yaml, RUNBOOKS.md; blobs built from HEAD by U30/blobs.js):
1. feat(profile): precheck, redacted reads, import scope and ZDR gate (WS17.P0): profile_guard.rs, profile_routes.rs, auth.rs, main.rs, tests (profile_guard.rs, profile_roundtrip.rs, zdr_refusal_modes.rs), routes.rs (blob).
2. fix(gateway): object rows in the profile list (WS17.P0): gateway ingestions/profiles.rs.
3. docs(quarry): openapi profile paths and schemas (WS17.P0): openapi.yaml (blob).
4. docs(quarry): RUNBOOKS section for WS17.P0 (W2-U30): RUNBOOKS.md (blob).
5. docs(plan-work): W2-U30 progress and depth notes.

Verification so far: none run. Compile check requested: `cargo check -p quarry-edge --tests --features postgres-queue` (qtest). Gateway: not yet compile-checked (native `cargo check --manifest-path apps/gateway/Cargo.toml --tests` owed). YAML not parsed.
