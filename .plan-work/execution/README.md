# Execution queue: Model Plane chat build

> **Building strategy from 2026-10-23 (owner's order): one wave at a time.** Build a wave's slices to the plan row's code and unit tests, commit, and log everything that is depth (live proofs, rebuilds, DB harness runs, hostile reviews, edge cases, owner decisions) in `docs/plans/model-plane-depth-register.md` instead of chasing it. When a wave's buildable slices are all committed, stop, prioritise the depth with the owner and test the wave; only then start the next. Waves: 1 = WS01-WS12, 2 = WS13-WS27. Tools: `tools/wave-args.py WAVE SIZE BATCH_ID` picks the next slices (disjoint files, dependencies met, nothing with uncommitted changes) and `tools/run-wave.js` runs them (implement, one verify, one fix round for blocking defects only). `tools/run-batch.js` is the older flow with hostile review and fix loops and is used only in a depth phase. **Testing:** inside a slice only its own unit tests run; one light test pass (compile, unit suites, lint, static proofs) runs in the background when the wave's last buildable slice is committed, and a red result is logged as a finding, never a stop; anything that needs docker waits until the owner has prioritised the depth (see the Testing section of the register).

## How to resume

1. Open `batches.json` and take the lowest batch that is not finished.
2. Run every slice in that batch in parallel, one implementer agent per slice. Use the model in the slice's `model` field: opus 5.5, sonnet 5.5 (high) or haiku 5.5 (max).
3. Each agent edits only the paths in its record's `files[]` and `new_files[]` in `queue.json`. Inside the worktree `epic-shaw-4d613b` on branch `model-plane`:
   - it runs its `test_commands` (on Windows, `--lib` for Rust);
   - it never runs docker and never touches a credential;
   - it never pushes or opens a PR;
   - it never touches the live tree `C:/dev/CoresSystem`.
4. If the agent needs a line in a `lead_shared_files` entry (the ledger, the ownership matrices, ZDR.md, MODEL_PLANE_STATUS.md, docker-compose.yml switch lines, .env.example), it hands the exact lines to the lead instead of editing the file.
5. After the batch, the lead:
   - applies those shared lines;
   - commits each slice in Conventional Commit style, preserving CRLF;
   - runs the `lead_docker_after` rebuilds and live proofs;
   - sets `status` to `done` in `queue.json` (or records `remaining` if a slice is only partly done).
6. Batch 1 also carries a lead-only item: the WS01-S1 audit-alert delivery proof.

`order` in `queue.json` gives the full sequence for all 290 records, not just the first 12 batches. `later_batches_preview` in `batches.json` shows batches 13 to 20. Past batch 12, take the next slices by `order`. Check that every `depends_on` entry is done and that `files[]` stays disjoint within the batch. The hot files are sse.rs, agent.rs, runtime_loop/mod.rs, http_routes.rs, tool_loop.rs, chat-client.ts, use-chat-controller.ts, DashboardComposer.tsx, ChatMessages.tsx, mp-contracts lib.rs and every .proto.

A `blocked-external` record does not stop its code: the buildable half is its own `<id>.code` record with status `todo`. Only the external proof half waits. `optional-deferred` records are reopened when their stated condition changes; for example, the WS09 conditional slices are re-checked after WS09-S1. The questions for the owner are in `decisions-applied.md`.

## Counts

There are 262 plan slices across WS01 to WS27:

| Status | Slices |
|---|---|
| done | 11 |
| partial | 5 |
| todo | 198 |
| blocked-external | 27 |
| optional-deferred | 21 |

The 5 partial slices are WS01-S1 (lead-only proof), WS03-S8 (S8-B), WS03-S9 (live-proof runner), WS12-S1 (CI remnant) and WS14-S2 (S2b).

`queue.json` holds 290 records: the 262 slices plus 27 `.code` halves (todo) and one `.ext` owner-setting record (WS12-S1.ext). By record, that is 225 todo and 28 blocked-external.

Model per record: opus 81, sonnet 180, haiku 29.

Taken in order, the queue needs 54 batches of at most five parallel slices; 12 of them are listed in `batches.json`.
