# R7-transform-store hand-over
Commits: 217fc548 (markdown compact tables), f39d338c (FilesystemStore::list + list_window shared with memory store, tests/artifacts_fs_list.rs), 14eb0367 (PDF inflate budget 256 MiB, reason too_large, doc_parse_sandbox tests), plus a docs commit for ERROR_REASONS.md.
Uncommitted: docs/RUNBOOKS.md has my appended "Evidence follow-up (R7)" section mixed with other units' hunks; commit it with them.
Left: rebuild the edge image and re-run the evidence run's flate-bomb request (expect 415, not 504). Nested list indentation (x12 at depth 200) is left as is.
Traps: html2md nested tables were exponential (770 MB from 434 B); Cargo.lock is gitignored.

## Round 3 (review fixes)
In progress (uncommitted): pdf.rs rewritten pre-scan (linear forward pass, top-level dict entries, image budget 384 MiB, boundary resets), pdf tests, doc_parse_sandbox limit assertion, markdown_blowup nested-list quadratic test. quarry-transform full test: 153 lib + 16+16+10+11 passed (q2.log). Next: doc_parse_sandbox run, cargo check, RUNBOOKS append (default table output change, nested list quadratic), commit.
Round 3 DONE: commits 4c4e4226 (pdf pre-scan linear, dict entries, image budget, truncated counted, limit assertion), 43b15b46 (nested list quadratic test), 02a47572 (RUNBOOKS section, committed via index blob so another unit's QUARRY_CHALLENGE_ENFORCE hunk stays uncommitted in the working tree). cargo check --workspace --all-targets OK.
Round 4 (review: pre-scan bypasses): edited pdf.rs (comments/#escape/gap/space-after-stream) + tests, doc_parse_sandbox new test; uncommitted, tests not yet run
pdf unit tests 22 passed (incl. new spelling tests); next doc_parse_sandbox + full transform + check + RUNBOOKS + commit
Round 4 DONE: commit 9f08f7e1 (pre-scan comments/#escape/gap/space-after-stream + tests + RUNBOOKS). All tests green (transform 156+16+16+10+11, sandbox 10/10, check OK).
