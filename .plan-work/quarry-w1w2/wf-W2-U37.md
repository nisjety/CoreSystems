# W2-U37 hand-over (WS6.C-e), workflow notes

What was done: the emission half of WS6.C, per `W2-U37-progress.md`. Code in 034f9f124, docs in 76803d35f.

## Traps found

- Shared worktree, moving HEAD: the worktree carried other units' uncommitted hunks in routes.rs, main.rs, lifecycle.rs, step_receipts.rs, runtime lib.rs and RUNBOOKS.md. Commits were made from a mirror (HEAD blobs plus this unit's edits, built by re-running the unit's edit scripts), through a private index (`qcommit-blobs-add.sh`). HEAD moved twice during the build (U38, U39 and U41 commits landed), so the mirror was rebuilt from the current HEAD before committing. Rebuild again if HEAD moves before the next commit.
- HEAD itself did not compile when this unit started: runtime lib.rs lacked `pub mod capture;` (fetch.rs uses it), web_policy_edge.rs had a second copy of a block (PAID_PROVIDER, PaidProvider, paid_provider, record_downgrade, gate_urls), and a step_receipts test literal lacked `policy_mode`. Fixed in 034f9f124 with the worktree copies. Later HEAD commits (U41) added `pub mod org_erasure;` and `profile_backfill;` to runtime lib.rs; the mirror rebuild kept them.
- `qcommit-blobs.sh` cannot add new paths; use `qcommit-blobs-add.sh` for new files.
- CRLF: the worktree files are CRLF. Mirror files are LF (from `git show`). `edit()` in lib-edit.js preserves EOL on the worktree; on blobs it writes LF.
- rustfmt is not clean on HEAD for budget.rs and other files. Only the unit's own new file and the unit's own additions in usage.rs were formatted; HEAD parts were left alone.
- `finish_run` lives in `agent_routes/close.rs` (WS2.B split), not in `lifecycle.rs` as the queue row says. The segment emission is in `close.rs`.
- The `kind` field on `InternalRunPage` must be read by field, not by passing `&req`: `req.signals` is moved by the planner before the cache-hit branch.

## Where to look

- `crates/quarry-edge/src/usage_attrs.rs`: the surface, metadata and gate. The ZDR allowlist is `ZDR_ALLOWLIST`.
- `crates/quarry-edge/src/agent_routes/close.rs`: segment emission (once per segment, by the path that won the removal).
- `crates/quarry-edge/src/budget.rs`: `RunBudget::segment_usage` (the base subtraction for a resumed segment).
- `crates/quarry-runtime/src/usage.rs`: `UsageEvent::keyed`, `PublishTracker` and `flush`.

## Verification commands used

- `cargo check --tests -p quarry-runtime -p quarry-edge` through `qtest.sh` on the export (`U37/export-tree.sh`, then `QROOT` pointed at `U37/export/apps/Ingestion Plane/Quarry-v2`). Result rc 0.
- `go vet ./internal/activities/ ./internal/workflows/` on the export. Result rc 0.
- `go test -run TestRunPageDeclaresJobKind ./internal/activities/` on the export. Result ok (disclosed as an extra run).
