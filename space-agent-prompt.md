# Verevon Space: finish the build

You own the Verevon Space workstream in `C:\dev\CoresSystem`. Your goal is to take Space from "Waves 1 and 2 written and merged" to "every milestone in the plan proven live". Work until that is done, or until you reach a decision only the owner can make.

## Where things stand (as of 2026-11-10)

- Waves 1 and 2 are merged into `main`: branch `space` at `b50d087c0`, branch `space-w2` at `4c6025273`, both on 2026-11-08.
- Wave 1 is built (68 of 70 pieces). It was partly proven live on a private stack on 2026-10-27, and that run found real defects.
- Wave 2 is built (all 98 pieces) and compiles, but **none of its tests have been run**. Some tests were overwritten when the branches were merged.
- Wave 3 and the later epics are only planned.

## Read these first, in this order

1. `docs/plans/space-hybrid-build-plan-2026-10-14.md`: the master plan. Section 4.1 has milestones M0 to M4 and their exit scenarios.
2. `docs/plans/space-w1-report.md`: Wave 1 status, the 12 priority clusters and 12 open owner questions.
3. `docs/plans/space-w1-live-proof.md`: the live proof method and defects F1 to F14.
4. `docs/plans/space-w2-build-ledger.md`: Wave 2 status and the list of tests to re-check after the merge.
5. `docs/plans/space-depth-register-w1.md` and `docs/plans/space-depth-pass-a-and-ui-parity.md`: the issue rows (D1-xxx).
6. `docs/SPACE_AUTHORITY_ADR_2026-08-13.md` and `docs/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md`.
7. Any `CLAUDE.md` or `AGENTS.md` in the repo and in the plane folders you touch.

Before you change anything, confirm the current state from git. Don't trust this summary over the repo.

## Order of work

### Phase 1: make Wave 2 green
- Work on a new branch off `main`, in its own worktree.
- Run the suites Wave 2 touched:
  - the frontend gateway, session-core, model-gateway and execution-core (`cargo test`)
  - user-core, org-core and audit-core (`go test`)
  - convex-core and auth-core (`tsc`)
  - the frontend (`pnpm typecheck` and `pnpm test`)
- Start with the "Merge fixes: kept and replaced" list in the Wave 2 ledger.
- Sort each failure into one of three kinds: a stale test, a real defect, or an environment problem. Fix the real defects and stale tests. Record environment problems with their evidence.

### Phase 2: close the Wave 1 defects
- The real defects from the live proof:
  - F1: the directory SQL needs a `$2::text` cast.
  - F3: rollback order for migration 0072 versus 0044.
  - F6: auth-core can't start on an empty database.
  - F8: 34 type errors block `pnpm build`.
  - F12: same-org outsiders must get byte-equal 404s.
- The unbuilt archive leg, WS04-S7 (D1-352).
- Then the 100 P1 depth rows, cluster by cluster in the report's order: ZDR on writes, memory containment, human posts versus model context, and so on.

### Phase 3: prove M1 and M2 live
- Follow the live-proof method exactly: a private docker-in-docker stack with its own images, networks, volumes and names.
- Re-run the M1 scenarios that failed or were not run: S6 archive, S7 browser cases, S8, and the rest of S9.
- Then run the M2 scenarios for Wave 2 (WS08 to WS16).
- Write the results to `docs/plans/space-w2-live-proof.md` in the same format as the Wave 1 proof.

### Phase 4: Wave 3 and M3
- Build WS17 to WS20 from plan section 3.E: personal agents and memory, the agent self-API, the room canvas, and a room-scoped agent browser.
- Follow the build-ledger style of the earlier waves, then prove M3 live.

### Phase 5: M4 and the later epics (E01 to E12)
- Only after M3 is proven.
- Plan them as their own waves first, and show the owner the plan before you build.

## Rules you must not break

- **Never touch the live stack.** Seed and helper scripts call `docker exec controlplane-postgres ...`. Run on the host, they hit live data. Run every seed and proof step inside the nested daemon only, with `space-proof/*` image tags, never the live tags.
- **Models:** for live turns, use only an Anthropic key with `claude-sonnet-5-5`. Log token use and cost for each run.
- **Git:** never push and never force anything. Commit on your own branch, one slice or fix per commit, and put the slice ID or D1 row in the subject. Merge into `main` only when the owner says so.
- **Gitignored runtime files** (`.env`, keys, internal TLS): read them only to configure the private stack. Never commit them and never print their values.
- **ZDR:** a ZDR org must never get a durable row in the database.
- **Owner decisions:** don't decide anything the plan marks as an owner decision. That covers the 12 questions in section 4 of the Wave 1 report, including:
  - when ZDR switches to enforce, and who writes its copy
  - the production credential registry (SD-27)
  - rate-limit values
  - Norwegian copy review

  Use the plan's recommendation as a provisional default only where the work can't continue without one. Mark that default clearly, and list it in your report.
- **Proof:** a result counts only with evidence: command output, a test name, or a log file in `.plan-work/`. Say plainly what you did not run.

## After each phase

1. Update the matching ledger in `docs/plans/`, giving every slice its status (built, tested, live-proven) and commit.
2. Write a short owner report:
   - what was done
   - what was proven, with evidence
   - what failed and why
   - the provisional defaults you used
   - owner questions, each with your recommendation first
3. If anything in the report blocks the next phase, stop and wait for the owner's answer.

## Done means

Every M1, M2 and M3 scenario in plan section 4.1 passes on a rebuilt private stack, and every P1 depth row is fixed or closed with a reason. All suites the Space work touched are green, or each failure is recorded with its cause. The ledgers and reports match the repo, and the owner has a merge-ready branch.
