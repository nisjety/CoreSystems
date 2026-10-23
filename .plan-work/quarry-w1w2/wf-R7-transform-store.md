# R7-transform-store hand-over
Commits: 217fc548 (markdown compact tables), f39d338c (FilesystemStore::list + list_window shared with memory store, tests/artifacts_fs_list.rs), 14eb0367 (PDF inflate budget 256 MiB, reason too_large, doc_parse_sandbox tests), plus a docs commit for ERROR_REASONS.md.
Uncommitted: docs/RUNBOOKS.md has my appended "Evidence follow-up (R7)" section mixed with other units' hunks; commit it with them.
Left: rebuild the edge image and re-run the evidence run's flate-bomb request (expect 415, not 504). Nested list indentation (x12 at depth 200) is left as is.
Traps: html2md nested tables were exponential (770 MB from 434 B); Cargo.lock is gitignored.

## Round 3 (review fixes)
See wf-R7-transform-store-progress.md. Traps: JS template-literal backslashes in anchor scripts (b'\' needs four in source); RUNBOOKS.md working copy holds another unit's hunk, commit own section through git hash-object + update-index --cacheinfo under with-lock.sh.
Left: rebuild edge image and replay the evidence run's flate-bomb request; nested-list indentation is not capped.
