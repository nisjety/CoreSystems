# browser-broker end-to-end harness

Runs the broker **container** (built from the current tree) against a throwaway
Postgres and a stub identity provider, over real gRPC, with a real
`docker restart` and a real database outage. It needs Docker and Go on the host;
nothing in it touches a running stack, an `.env` ledger or a real credential.

```bash
cd "apps/Model Plane/go/services/browser-broker"
bash e2e/run.sh
```

On a machine where several agents or people share one Docker engine, serialise
it with your lock, for example
`E2E_LOCK_CMD="bash <tools>/with-lock.sh docker 2700 --" bash e2e/run.sh`.
`E2E_SKIP_BUILD=1` reuses the image `browser-broker-e2e:test`; `E2E_KEEP=1`
leaves the containers up for debugging (remove them by hand: everything is
named `e2e-*-<suffix>`).

## What `run.sh` does

1. `docker build -t browser-broker-e2e:test -f services/browser-broker/Dockerfile .`
   from `apps/Model Plane/go` (the working tree, uncommitted edits included; the
   image's `SOURCE_REVISION` label says `-dirty` then).
2. Generates an RSA key **at run time** (`e2e/cmd/idp keygen`) into a temp
   directory: the private half stays on the host, the public JWKS is baked into
   a `FROM scratch` image with a tiny Go JWKS server (`e2e/cmd/idp serve`). No
   key is checked in.
3. Creates a private network, the stub IdP (`e2e-idp-*`), `postgres:16-alpine`
   (`e2e-pg-*`, random password, named volume) and the broker (`e2e-broker-*`)
   with `DATABASE_URL` at that Postgres, `AUTH_CORE_JWKS_URL` at the stub and
   `AUTH_CORE_ISSUER` matching the tokens. The broker fetches the JWKS once at
   boot (`authctx.NewVerifier`), which is why the stub keeps running for the
   whole run: every `docker restart` re-fetches it. The entrypoint applies the
   real migrations.
4. Runs `go test -tags e2e ./e2e/` on the host. Ports are published to
   `127.0.0.1` at random and looked up again after each restart.
5. Scans the broker's log for host names, bearer tokens and the database
   password (counts only). This is a gate: any non-zero count fails the run,
   and a control that plants one of each in a copy of the log must fire, so a
   broken pattern cannot pass for a clean log.
6. Checks that the run really ran: every scenario below must have printed its
   `E2E_SCENARIO_OK` line (go test exits 0 when the test skips, so its exit
   status alone proves nothing) and no test may be skipped.
7. Removes every container, volume, network and image it created (trap, so also
   on failure; the broker image `browser-broker-e2e:test` only when this run
   built it). The last lines report `E2E_CLEANUP leftovers ...` (containers,
   networks, volumes and images, all 0) and
   `E2E_RESULT rc=... scenarios=<ok>/<expected>`.

## Scenarios

Tokens are minted by `e2e/idp` with the issuer, audience (`browser-broker`) and
scopes the broker's `internal/authz` requires: a person is `principal_type=user`
(every method open, pinned to their own grants); a service is
`principal_type=service` and needs `browser:write` for Acquire/Revoke/Derive and
`browser:read` for Validate/Health.

| id | what it asserts |
|---|---|
| a | `/readyz` 200; a non-ZDR session grant; `docker restart`; the same grant still validates with the same owner, hosts and expiry; its row is in Postgres |
| b | a ZDR session grant (request flag, and token claim) writes no row; after a restart it is NotFound |
| c | Derive: hosts inside the parent's (dot boundary), expiry <= parent's, replay returns the same grant, a replay with other hosts is AlreadyExists, the ninth live run under one session is ResourceExhausted (a replay at the cap is not new), a service derives from a user's session, ZDR parent gives ZDR child that never reaches Postgres, revoking a parent revokes the subtree (before a restart and after one) |
| d | WS6.E: a claim above the parent's (any dimension) is refused and leaves nothing; a tighter claim inherits omitted dimensions; a replay without a claim returns the stored claim and the original stamp (not re-stamped); a different claim on replay is AlreadyExists; the claim survives a restart; ZDR claims stay in memory |
| g | sensitive approvals (the other derived-grant path): never wider than the parent (a host outside it, a look-alike host, one bad host among good ones: PermissionDenied and no row), expiry capped at the parent's (also under a 60 s run grant), one action and one frame, a budget claim or a second action refused, an approval under an approval or under another session's key is FailedPrecondition (not Internal), no run grant derives from an approval or a run grant, a ZDR principal under a persistent parent is FailedPrecondition, ZDR parent gives a ZDR memory-only approval; another person, the same user id in another organization and a service of another organization get NotFound (same message as a missing parent) for acquire, revoke and validate; revoking an approval leaves the parent, revoking the parent revokes its approvals, a live approval survives a restart |
| f | wrong audience, expired, wrong issuer, forged signature, no token: Unauthenticated; `grpc.health.v1` stays public; read-only and write-only services get PermissionDenied where the scope is missing; another person, or the same user id in another organization, gets NotFound for an existing grant with the same message as for a missing one; a person and a browser:write service of another organization get the same NotFound for derive, revoke and validate of that grant, with the derive message equal to the one for a missing parent, and nothing is written |
| e | Postgres stopped: `/readyz` 503, `/healthz` 200, Validate/Acquire/Derive/Revoke of durable grants are Unavailable and never answer valid (a ZDR grant, held in memory, still validates); Postgres started: `/readyz` 200 and the earlier grant validates again |

Only grant ids and counts are printed, never a host list or a token.

## Notes

* Erratum: the message of commit c25865fc says "three runs of bash e2e/run.sh".
  That is wrong. Two full runs reached the scenarios and passed; the first run
  failed in run.sh on an MSYS path before any scenario started (a harness bug,
  since fixed). The assertion counts that commit gives (192) were reproduced by
  an independent run.

* No BrowserBroker method is service-only (`authz.Authorize` lets every person
  through), so "a person calls a service-only method" has nothing to refuse.
* The broker has no budget ceiling of its own: a claim can only tighten the
  parent session grant's claim (`grant.tightenBudget`). Spend itself is tracked
  by the consumer, so the broker leg asserts that the stored claim and its stamp
  never change on a lookup.
* `e2e/idp` has its own `go test -tags e2e ./e2e/idp`, which needs no Docker and
  checks the stub's tokens against the broker's real verifier and authz rules.
* The build tag `e2e` keeps all of this out of `go test ./...` and `go vet ./...`.

## Mutation check of the cross-organization assertions (2026-10-21)

The cross-organization assertions of scenarios f and g were checked by
single-fault injection: one change to `internal/grant/postgres.go`, one full
`run.sh`, then the change was reverted by hand (the tree is clean).

* The org comparison removed from `scoped()` (used by Derive, the approval path
  and GetScoped): the run failed (`scenarios=5/7`); f failed on the derive and
  validate of another organization's grant (code OK, want NotFound, and the
  "left no row" count), g failed on the cross-organization approval and on the
  subtree-revoked row check.
* The org comparison removed from `RevokeScoped` only (the SQL `org_id = $2`
  filter still keeps the data safe, so the revoke returned OK instead of
  NotFound and changed nothing): f and g failed on the four cross-organization
  revoke assertions.

The unmutated tree passes 7/7 (see the commits above). Re-run the mutation by
hand if you change the scoping code.
