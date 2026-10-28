# wf-D2-A-compose-flags progress (continuation pointer, 2026-10-28)

Unit D2-A-compose-flags: decision 3.12 DATAPLANE-OPTION-B and decision 3.8 ADR-6 (the compose line), owner-accepted 2026-10-28, list A.
Worktree: C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b, branch quarryv2-next, local commits only.

Continue from D2-A-compose-flags-progress.md (the full state) and from `git log` / `git status` on the owned paths. Owned paths:
- apps/Ingestion Plane/docker-compose.yml
- apps/Ingestion Plane/Quarry-v2/deploy/compose/docker-compose.yml, README.md, .env.example
- apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs (CRLF)
- apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs (QB-T17 todo text only)
- apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md, docs/ONBOARDING_LIVE_CRAWL.md (compose-default statements only)
- .plan-work/quarry-w1w2/D2-A-compose-flags-progress.md, depth/D2-A-compose-flags.md, wf-D2-A-compose-flags.md

Not ours and not to be committed: .plan-work/next-slices.md, rb.patch, apps/Model Plane/go/go.work.sum, the EOL-only and mod-order strays in quarry-edge lib.rs, main.rs, tests/support/fixture_site.rs, quarry-runtime step_receipts.rs.

Status (2026-10-28): done and committed locally (8a8bf5c1b, 675903067, 88fb4afe3, c457cdc51; notes commit follows). Nothing is left to continue in this unit except the stack session's recreate-and-check (decision 3.12 step 4) and the open items in D2-A-compose-flags-progress.md section "Left open". If you resume, check `git log --oneline -6` and `git status` on the owned paths first.
