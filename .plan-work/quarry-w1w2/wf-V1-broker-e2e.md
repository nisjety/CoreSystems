# V1-broker-e2e hand-over
Commits: fe46954c (harness, stub IdP, run.sh, README), c25865fc (scenarios a-f).
Run: cd "apps/Model Plane/go/services/browser-broker" && bash e2e/run.sh  (E2E_LOCK_CMD=... to serialise docker).
Result: 6/6 scenarios pass, 192 assertions (a20 b14 c72 d38 f28 e20), two green full runs (run2, run3; run1 failed on a
MSYS path in my script, fixed). Broker log: 0 host names, 0 tokens, 0 password. Cleanup: 0 leftovers.
ERRATUM to c25865fc message: it says "three runs"; it was two successful runs (a third, earlier, failed in run.sh itself before any scenario).
Traps: (1) MSYS_NO_PATHCONV=1 makes native git/go/docker-build get /tmp paths they cannot resolve: use cygpath -m (WIN helper).
(2) broker fetches the JWKS once at boot: the stub IdP must outlive every docker restart. (3) random published ports change on docker restart: re-resolve with docker port.
(4) the git revision label read "unknown" in run1/2 for trap 1's reason.
Findings: no broker defect. No method is service-only; no broker-level budget ceiling (tighten-only vs parent). Validate needs browser:read, so a write-only service cannot validate (by design, asserted).
Not done: negative control against a deliberately broken broker; scenario counts are asserted by the harness, not pinned.

## Round 4 (findings fixed)
- Commits: 19125dc5 (broker fix), 37712250 (e2e). c25865fc "three runs" erratum is now also in e2e/README.md and the 37712250 body.
- Defect found by scenario g: an approval under an approval / another session key returned Internal; now FailedPrecondition (ErrApprovalParentInvalid).
- run.sh is now a gate (log scan + planted-line control, 7 E2E_SCENARIO_OK lines, no SKIP) and removes browser-broker-e2e:test when it built it.
- Traps: E2E_SKIP_BUILD=1 fails after a normal run (the image is gone by design); per-owner cap of 20 live grants applies to a user id across scenarios, use a fresh user per scenario (g uses user-g, user-g2); PG store maps approval-owner to the actor, so a same-org service cannot mint an approval under a user's grant (fail closed, asserted).
- Not exercised: the scan failure path with a real leaking broker.

## Round 6
Mutation check of cross-org assertions done (see e2e/README.md, commit d8437358).
