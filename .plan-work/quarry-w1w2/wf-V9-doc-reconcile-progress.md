# V9-doc-reconcile progress
- Read all notes and all 45 commit messages of e7c321d5..HEAD (V1 5, V2 14, V3 9, V4 6, WS3.K 6, WS12.S0 5). Live broker verified read-only (image created 2026-10-20T18:44Z, healthy, network members ingestion-postgres + broker, rollback tag present).
- Ran natively: broker go test ok, e2e vet ok, zdr-contract-sync 52 passed + self-test ok, browser-grant-contract 19, Quarry node tests 23.
- Running: workspace cargo test (scratchpad/V9/ws.log), gateway bin tests (V9/gw.log).
- TODO edits: plan ledger rows + live-stack row + errata; RUNBOOKS lines 85, 349, 428, 438, 498 (+ V1 etc); ERROR_REASONS line 3 and 422 section; PROGRESS rows; register section; SECRET_REDACTION.md "7 JSON cases"->10 (outside owned paths, one word).
- COMMITTED a1b37b7d (RUNBOOKS, ERROR_REASONS, PROGRESS, SECRET_REDACTION one word). V9 re-runs: workspace 2414/0/8 rc0; lane 172/4+4/5/4; pg 23; bootstrap 34; sync 52; node 23+19; gateway 758; broker go ok.
- Plan edited (rows, errata, para), uncommitted; next: commit plan, then register, then notes file wf-V9-doc-reconcile.md.
