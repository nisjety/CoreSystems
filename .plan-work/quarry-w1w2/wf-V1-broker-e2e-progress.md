# V1-broker-e2e progress
- Files (mine, uncommitted): apps/Model Plane/go/services/browser-broker/e2e/{run.sh,Dockerfile.idp,harness_test.go,scenarios_test.go,idp/idp.go,idp/idp_test.go,cmd/idp/main.go}
- Done: stub IdP verified against authctx.NewVerifier (go test -tags e2e ./e2e/idp ok); vet + go test ./... green.
- Next: run e2e/run.sh under with-lock (E2E_LOCK_CMD), fix findings, README, commit in steps.
- Run: E2E_LOCK_CMD="bash $S/tools/with-lock.sh docker 2700 --" bash e2e/run.sh   (S = scratchpad)
- DONE: committed fe46954c, c25865fc. Notes written to wf-V1-broker-e2e.md.
- ROUND 4 DONE: committed 19125dc5 (fix: ErrApprovalParentInvalid -> FailedPrecondition, was Internal) and 37712250 (run.sh gates: log scan + scenario count + image rmi; scenario f cross-org; new scenario g, 83 assertions). Final run: E2E_RESULT rc=0 scenarios=7/7, assertions a20 b14 c72 d38 f38 g83 e20 = 285.

- ROUND 6 DONE: d8437358 (README: mutation check). Fault injections F1 (scoped() org check) and F2 (RevokeScoped org check) each made run.sh fail 5/7 on the f and g cross-org assertions; both reverted, tree clean.
