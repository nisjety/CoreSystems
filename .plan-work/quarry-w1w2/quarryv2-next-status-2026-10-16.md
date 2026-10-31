# quarryv2-next status (worktree vigorous-perlman-e7ae5b), 2026-10-16
Branch cut from quarryv2 bed81d16. Not pushed, no PR. quarryv2 (gracious-gagarin) untouched; fast-forward it to quarryv2-next when ready.

Commits (oldest first): WS3.E stealth deletion, WS4.B cache forensics/purge scripts, WS2.E fleet console flag (also carries the WS2.B rename, see its message), WS2.B agent_routes split,
WS2.C 6efc33cc control authority, WS2.D 38449e06 run registry + sweeper, WS3.B 9e4c9b37 live Chromium CI lane, WS4.D 69c24711 ZDR log/meter/provider-error leaks.

Verified: WS3.B live run 10 passed/0 failed/3 ignored (rust:1 + chromium). WS4.D: capture test fails without the pipeline.rs change, passes with it; runtime lib 673, edge lib 289, zdr_claim_floor 22, zdr_log_capture 4; gateway cargo check ok.
Not done / not verifiable: WS2.C Postgres receipt round trip + agent_run_control integration test; WS2.D edge Retry-After header, resumable hint on 404 run_not_live, RSS at 8/16/32 contexts, sweeper integration test; WS4.D rebuilt-edge docker-logs check; SPA typecheck/vitest on host.
Left in plan: WS1.A2 (blocked on X-1), WS4.B purge execution (after WS4.A on every replica), WS5.B, positions 4-11, WS4.E/F (structural log filter + lint), WS3.B CI job is continue-on-error until it has green runs.
