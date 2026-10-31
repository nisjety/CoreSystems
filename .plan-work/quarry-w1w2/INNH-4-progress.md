# INNH-4 progress

Lane b run of 2026-10-31. Continue from the tree: every unit below leaves a checkable change.

Design decisions (from INNH-4-status.md, refined while building):
- Flag `QUARRY_EDGE__CHANGE_TEXT_DIFF`, default off (unset in the composes). Off: the recorder behaves exactly as before (artifact_id None, one-line raw note, format `text`, unfilled record) and `GET /v1/change/diff` answers 501.
- New artifact kind `change_diff` (extension `change_diff.json`) for the stored JSON diff.
- The store is reached through a small trait in quarry-edge (`ChangeStore`, `change_diff.rs`) so `record_core` and the read route are tested against an in-memory double without Postgres; the SQL is covered by Postgres-gated tests that skip without DATABASE_URL.
- `markdown_artifact_id` on run_page is general (not change-specific): item 12 reuses it.
- The read route refuses a ZDR reader outright (owner decision noted in the status file).

Units:
- [x] U1 core + transform: ArtifactKind::ChangeDiff, wire types (`change_history.rs`), chunker character-boundary fix, `diff_paragraphs` linear lookup and a `moved` that ignores shifts caused by inserted or dropped paragraphs
- [x] U2 runtime store: `load_baseline`, `load_diff`, `find_diff` (+ 2 Postgres-gated tests)
- [x] U3 run_page `markdown_artifact_id` (Rust `InternalRunPageResult`, Go `RunPageResult`, `CheckChange` forwards it; 3 Go tests)
- [x] U4 edge: `change_diff.rs` (flag, gate, `ChangeStore`, diff, read logic), `record_core` wiring, webhook `new_baseline` and `diff_id`, `GET /v1/change/diff`
- [x] U5 docs (openapi, openapi-internal, CONTRACTS, CHANGE_TRACKING, RUNBOOKS), ZDR matrix row `change_diff_read` (+ golden), route manifest, route-count pin, docs-contract test
- [x] U6 tests green in the lane container and on the host (counts in INNH-4-status.md)
- [x] U7 rustfmt of the new files (host rustfmt; the files HEAD had clean stay clean), status file, commits
