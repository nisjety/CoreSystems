# wf-W2-sweep hand-over

What was done: see the commits named in the final answer and `depth/W2-sweep.md`.

Summary: of the ~60 files in `git status`, 17 were tracked-modified. Complete work of two units was committed: WS7.F Rust half (U14) and WS17.P0 (U30: edge guard, routes, tests, OpenAPI, RUNBOOKS section, gateway profile list). Four shared files were stale against HEAD and were re-synced (see depth). Five files are strays and are left. The untracked notes under quarry-w1w2/ (W2 and W3 units' progress, depth and hand-over files) were committed as notes.

Traps found:

- A private-index blob commit (qcommit-blobs*.sh) does not touch the working copy; a later whole-file write from an older copy then silently drops committed hunks from the working tree. Check `git diff HEAD --stat` for deletions before a test run.
- `qcommit-blobs.sh` cannot add a new file; use `qcommit-blobs-add.sh` (the first call exited 6 and committed nothing).
- A `( ... ) &` background job in the Bash tool dies with the call; use `run_in_background` on the call itself.
- `cargo check --workspace --all-targets` stops at the first failing crate unless `--keep-going` is given.
