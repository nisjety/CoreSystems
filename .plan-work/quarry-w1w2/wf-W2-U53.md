# W2-U53 hand-over (WS13.5)

What was built: see W2-U53-progress.md (commits ab49f5ae3, 82144afb4, 2e541cdfd).

Traps found while building:

- routes.rs, RUNBOOKS.md and auth.rs carried other units' uncommitted hunks at commit time. Commits for those files were made from a patch built from HEAD plus this unit's edits only (qcommit-patch.sh). Repeat that for any later unit touching the same files.
- Several working-tree files in this worktree are LF while others are CRLF. Git stores LF, so the commits are unaffected; the edit helper keeps whatever a file has.
- `actions_mode()` and `capture_mode()` read the environment once (OnceLock). Tests that need a non-off mode must set the variable before the first read or run in a child process.
- The request struct now has `actions` (raw JSON) and `initiated_by`; the stream route refuses `actions` by reading the raw field, the batch route is not checked.
- A request with actions has local_index, page_renderer and title_enricher unset in its PageRunner. Verify each use site before relying on that (depth bullet, high).
- The handler deadline is always 115 s with actions on, ignoring a smaller caller timeout_ms (depth bullet, medium; owner decision).
- The gateway's default client timeout is 25 s; any actions run longer than that is cut at the gateway (depth bullet, high). This is the first thing to fix before the flag is used from the SPA or the Model.

Commands for the next agent:

- Compile: `bash .../scratchpad/tools/qtest.sh 'cargo check -p quarry-runtime --tests'` and `... 'cargo check -p quarry-edge --tests'` (both rc 0 on the working tree at build time).
- Tests to run in the wave's final phase: `cargo test -p quarry-runtime action_runtime`, `cargo test -p quarry-edge actions_tests`, then the integration set in the depth file.
