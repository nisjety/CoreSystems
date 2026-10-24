# Hand-over, W2-U67 (WS8.F)

Read first: `W2-U67-progress.md` (commits, what was run, what is not built) and `depth/W2-U67.md` (the depth register).

Where the code is:

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/tab_ledger.rs` (ledger, attach registry, budget, lane check; pure).
- `.../agent_routes/attach.rs` (attach, release, tab control, the delegate step, the person's take-over gate).
- `.../agent_routes/step.rs` (the delegate dispatch and the take-over hook), `router.rs` (three routes), `types.rs` (`StepBody.grant_id`).
- `.../quarry-runtime/migrations/0018_step_receipt_tab_attach.sql` and the receipt/postgres changes.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/attach_cobrowse.rs` (refusal paths).

The next batch should start with the sweeper release, `list_tabs` for the delegate and the ZDR matrix rows (all listed as high or medium in the depth register), then the enabled-path driver double.
