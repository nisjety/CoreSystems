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
   password (counts only; all must be 0) and removes every container, volume,
   network and image it created (trap, so also on failure). The last lines
   report `E2E_CLEANUP leftovers ...` (all 0).

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
| f | wrong audience, expired, wrong issuer, forged signature, no token: Unauthenticated; `grpc.health.v1` stays public; read-only and write-only services get PermissionDenied where the scope is missing; another person, or the same user id in another organization, gets NotFound for an existing grant with the same message as for a missing one |
| e | Postgres stopped: `/readyz` 503, `/healthz` 200, Validate/Acquire/Derive/Revoke of durable grants are Unavailable and never answer valid (a ZDR grant, held in memory, still validates); Postgres started: `/readyz` 200 and the earlier grant validates again |

Only grant ids and counts are printed, never a host list or a token.

## Notes

* No BrowserBroker method is service-only (`authz.Authorize` lets every person
  through), so "a person calls a service-only method" has nothing to refuse.
* The broker has no budget ceiling of its own: a claim can only tighten the
  parent session grant's claim (`grant.tightenBudget`). Spend itself is tracked
  by the consumer, so the broker leg asserts that the stored claim and its stamp
  never change on a lookup.
* `e2e/idp` has its own `go test -tags e2e ./e2e/idp`, which needs no Docker and
  checks the stub's tokens against the broker's real verifier and authz rules.
* The build tag `e2e` keeps all of this out of `go test ./...` and `go vet ./...`.
