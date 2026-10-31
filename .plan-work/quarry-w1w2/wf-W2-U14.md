# wf-W2-U14 hand-over (WS7.F)

What was done: see `W2-U14-progress.md` in this folder. Depth bullets: `depth/W2-U14.md`.

Traps found while building:

- `grep -c $'\r' <file>` returned 0 for CRLF files in this shell. Use `file <path>` to check line endings.
- A background `bash qtest.sh ... &` started with a plain shell `&` produced an empty log (the process was killed when the foreground shell returned). Use the tool's `run_in_background` parameter instead.
- `cargo check -p quarry-core --tests` pulls `quarry-transform`, which another unit was editing (`parse_cache.rs`, `ParseCacheKey` without `Debug`). Re-run after the other unit's commit lands.
- `go vet ./internal/resources/` depends on `internal/store`, which another unit was editing (`DeleteByOrg`, `cron.MinInterval`). Re-run after it compiles.
- `docs/RUNBOOKS.md` carries another unit's uncommitted hunks: the WS7.F section was committed through `qcommit-blobs.sh` (HEAD blob plus the section), and the working copy keeps the other hunks.

Decisions left to the arbiter:

- `careful` and its alias `stealth` declare `proxy.max_tier=pool` (the plan named only `stealth`).
- The 410 code `GONE` is outside the closed error-code sets. A proper variant needs a cross-crate change.
