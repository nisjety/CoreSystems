# Q2-ws6e progress (2026-10-20)
- DONE 8b350923 step 1 proto+pb.go+vendored proto+contract test (buf regen identical, buf breaking clean, no new lint, node 19 pass; added per-field comments).
- NEXT: step 2 broker (go vet/test native, integration under lock), step 3 Quarry (qtest), step 4 model-gateway (mp-test.sh), step 5 docs.
Scratch: scratchpad/Q2/
- DONE 561b6d48 step 2 broker (go vet/test native; integration under lock PASS incl. TestPg_Budget*, migration rerun)
- DONE 3dea6da5 Quarry (7 claim tests, runtime 739, edge all ok), df6460aa main.rs boot hunk. NEXT: model-gateway (mp-test.sh), docs (RUNBOOKS mine-only hunk, STALE register, plan ledger)
- ALL DONE; see wf-Q2-ws6e.md. Last commit 81948638.

## Review round 4 fixes (2026-10-20, second pass of this unit)
Findings addressed (7): (1) retry 409 -> broker replay lookup + gateway probe; (2) parent-claim ceiling test; (3) tool-input route test; (4) PG negative tests on claimed row; (5) boot refusal test spawning the binary; (6) per-run claim documented + pinned; (7) presented run grant clipped to org ceiling.
Edited, all UNCOMMITTED until commits below land: broker grant/{budget,budget_test,grant,postgres,postgres_integration_test}.go; model-gateway src/{browser_run,state}.rs; quarry-edge tests/{grant_budget_boot(new),grant_budget_claim}.rs; RUNBOOKS (mine-only patch scratchpad/Q2fix/rb/rb.patch).
Results so far: broker go vet/test ok; tagged PG suite ok (real tree) and mutant (inline CHECKs removed) FAILS the schema test; quarry grant_budget_claim 8 ok + grant_budget_boot 2 ok; model-gateway browser_run 56 ok (49 before). Mutation run: scratchpad/Q2fix/mut.log.

## Round 5: DONE, commit 8410abea (see wf-Q2-ws6e.md)
