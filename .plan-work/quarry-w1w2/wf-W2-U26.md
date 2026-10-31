# W2-U26 hand-over (WS13.4a)

What was done: the FetchResponse captures carrier and all literal sites (see W2-U26-progress.md; commits 3b80faa60 and b015d2206).

Traps found:
- The plan detail names `tests/rerank_behavior.rs`, which does not exist. The plan line numbers for fallback_driver.rs, pipeline.rs and routes.rs are stale; the grep is authoritative.
- A grep of `FetchResponse {` also matches `-> FetchResponse {` (fn return types) and `impl FetchResponse {`. The literal-editing script excludes both; the lookbehind is in the scratch script (scratchpad/W2-U26/add-captures-literals.js).
- The background qtest job in a shell with `&` exits at once with an empty log. Use the run_in_background tool parameter and qwait.sh instead.
- RUNBOOKS.md in the working tree carries another unit's uncommitted content and CRLF endings while HEAD is LF. Commit it only through qcommit-blobs.sh (HEAD plus the section), never from the working-tree copy.
- qcommit-blobs.sh needs qcommit-blobs-add.sh for new files.

Left for others: the U27 reload work in action_runtime.rs, observation.rs and challenge.rs was mid-edit during this unit's compile check; it was not touched.

Next: WS13.3 (U40) builds on `PageCaptures`; WS13.4b (a later batch) fills `captures` on the scrape route.
