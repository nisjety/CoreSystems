# W2-U11 hand-over (WS15.L1)

Read `W2-U11-progress.md` for commits, verification and deviations; `depth/W2-U11.md` for depth items.

Left for the next person:
- Run the wave test phase: Go tests in services/quarry-control (resources and store/pg), the Rust job_lifecycle test and control_client unit tests, and the existing tests in quarry-edge that build the router (the new routes sit beside /v1/:kind/jobs).
- Deployment evidence (control first, then edge, then orchestrator; HMAC required).
- ZDR-floor 403 test; gateway and SPA surfaces are WS15.L5.

Traps found:
- The edge binary declares its own modules in main.rs as well as lib.rs; a new module must be added to both.
- Git: core.autocrlf is set, and the working-tree files in this repo are LF; the lib-edit helper keeps whichever EOL a file has.
- RUNBOOKS.md carries another unit's uncommitted hunks: commit only your hunk with qcommit-patch.sh.
- Job ids in Rust tests must be valid ULIDs with the job_ prefix.
