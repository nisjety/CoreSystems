# wf-C2 round 5 hand-over (2026-10-23)

Commits: e36429a1, 377f7194, af4a9c99 (on top of 9ff290dd, 90634d87, 0ef06f3b, 7849ff1b). See C2-ws4i-ws4j-progress.md and the RUNBOOKS section "ZDR run tombstone: round 5 corrections and the erasure marker".

Traps: (1) run-pg.sh passes its args unquoted-split, so quotes inside -run regex become literal; pass -run A|B|C bare. (2) time.AddDate wraps into the future at 1<<55 and math.MaxInt days: keep the clamp. (3) finish_run is split into untracked agent_routes/close.rs by another unit: the tombstone emit_run_end call must move with it. (4) A bin compile error from another unit's mid-edit (driver.shutdown) can hit a mutation copy: retry.
Still OFF; counsel gates enabling. Not done: counsel, stack evidence run, await-instead-of-spawn mutant.
