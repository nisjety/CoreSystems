# space-finish Phase 1 working notes (2026-11-10)

Branch `space-finish` off main 031e7965d, worktree C:\dev\space-finish. Cargo targets C:\dev\space-finish-target-{mp,gw}.
Logs: .plan-work/space-finish/logs/<id>.log (+ .exit).

## Git state confirmed
- main 031e7965d contains space (272cf7abc, via b50d087c0) and space-w2 (8d46f6db4, via 4c6025273). No unmerged commits on either.
- Later merges into main touched Space-adjacent files: 6a1ae082e (mp-ep14-s1-gap: reverted chat files), 292ded7ad/08c321ee1 (quarryv2-next: lost a brace in gateway browser.rs).
- The owner already fixed both on mp-w345 (afcee0802, 2026-11-10 08:39); cherry-picked here as W2-P1-01.

## Suite results (round 1)
| suite | result | class | evidence |
|---|---|---|---|
| gateway cargo test | compile FAIL: browser.rs unclosed delimiter 11941 | real defect (merge loss, not Space) | p1-gw-cargo.log |
| pnpm typecheck | 51 errors: 21 quarry-client.ts, 24 settled-fold.test.ts, 6 ChatMessages.rewind.test.tsx | 30 = real defect (merge 6a1ae082e); 21 = env (pnpm file: dep copied before SDK dist existed) | p1-fe-typecheck.log |
| pnpm typecheck after W2-P1-01 + SDK dist in the pnpm virtual store | 0 errors | | p1-fe-typecheck2.log |
| user-core go test | all ok | | p1-go.log |
| org-core go test | all ok | | p1-go.log |
| audit-core go test | cmd/server FAIL TestModelAuditProducerACLs... | stale test (nats.conf gained quarry-trace-producer) | p1-go.log, fixed W2-P1-02, p1-audit-fix.log ok |
| convex-core tsc --noEmit | 0 errors | | p1-convex-tsc.log |
| auth-core tsc --noEmit | 0 errors | | p1-auth-tsc.log |
| SPA node --test (pnpm test, first half) | 57/58; action-surface-contract dispatcher equality FAIL | pre-existing drift (D1-046/D1-601), 96 registry ids without a handlers.rs arm, 8 arms without a registry id; Space ids not among them | p1-fe-test.log |

## Environment notes
- Docker Desktop engine pipe is down (WSL distro docker-desktop Stopped; backend log: "lingering processes detected at startup"). Needs the owner to restart Docker Desktop. Blocks: the pre-commit gitleaks fallback (docker), every live proof, DB-tier tests on a throwaway Postgres.
- Workaround for the secret gate: install gitleaks locally (go install) so the hook uses the binary path. Never --no-verify.

## F6 (auth-core empty DB)
- Root cause found: `migrations/init_better_auth.sql` was deleted from main in 932b30d26 (2026-09-23, "consolidate Verevon product readiness"), but `docker-entrypoint.sh` still lists it first and skips missing files (`[ -f "$f" ] || continue`). On an empty DB gdpr_hard_delete.sql then runs first and fails on "user".
- The live ledger (captured 2026-10-27) has init_better_auth.sql with checksum = CRLF hash of blob b4e1d53c0 (the file as last committed at 42d7fed7e and re-added on d76598ecd, not on main).
- gdpr-outbox-erasure.postgres.spec.ts also reads migrations/init_better_auth.sql, so that spec cannot run on main either.
- Fix: restore the file byte-identical (blob b4e1d53c0) so the ledger checksum still matches on CRLF checkouts, and make the entrypoint fail closed if a bootstrap file is missing (it is listed explicitly, so its absence is a packaging error, not an optional file). Add a deployment-contract assertion.

## F12 (same-org non-member 403 vs 404)
- Control classifies non-member / unknown Space / non-org-member identically as 403 space_membership_required (anti-enumeration at Control).
- The BFF breaks that: `space_lifecycle_by_ref` reads the Convex org index first and answers 404 space_not_found for a ref not in the org, while a real room's non-member reaches Control and gets 403. Affects context, threads, artifacts (and any route with the same prelude).
- Plan M1-S6 requires byte-equal 404 bodies. The depth-pass doc marked it "needs owner decision (are org-visible rooms discoverable?)". The plan text (3.A WS04: "other-org Space is 404 byte-equal to unknown") and M1-S6 criterion already state the rule for outsiders; the open question is only whether an org_public room (WS04-S9 directory) changes it. Directory rooms are listed through the directory route, so their refs are already known to org members; byte-equality of the read routes does not reveal more.
- Proposed fix: in the read routes, a Control membership refusal whose reason is space_membership_required is answered with exactly the unknown-room 404 body. Other reasons (role floor, entitlement, audience) only reach a member, so they stay typed.
