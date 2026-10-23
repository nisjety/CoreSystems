# W2-U39 hand-over (WS8.B)

What was done: the human-browse slice (open-web policy, human receipts, human grant admission, budget exemption, checkpoint policy mode), dark behind QUARRY_EDGE__HUMAN_BROWSE (off). Details: W2-U39-progress.md. Open items: depth/W2-U39.md.

Traps found:
- `git commit` of a file with CRLF in the working tree and LF in HEAD rewrites the whole file. Check `git diff --numstat` before committing a doc; for RUNBOOKS.md the commit 600671bb did this and dropped five committed sections (fixed by 2f6b026f). Use qcommit-patch with trimmed hunks for shared docs.
- `cmd &` in the Bash tool returns at once. The qtest result is in the log file; wait with qwait.sh on that file.
- Inside a double-quoted `node -e "..."`, backticks run as shell command substitution. Write the script to a file with the Write tool.
- `AgentAction` has no PartialEq (use `matches!`). `ArtifactKind` is a ULID newtype and cannot hold a marker string.
- The quarry-edge bin test target depends on main.rs and routes.rs edits of another unit; `cargo test -p quarry-edge --lib --no-run` and the named integration targets compile without it.
- `run_limits::human_browse_enabled()` reads the variable at each start (no cache), so a test that sets it must not run in parallel with a test that expects it off; the file tests/human_browse.rs turns it on for all its tests.
