# Control Plane wire contracts

A golden file pins a request one Control Plane service sends to another, so a
header, path or scope mismatch fails a test instead of failing silently in
production. Each golden is written by the **real client** and replayed through
the **real server** (real middleware, real route table, real compose credential
block). A golden is never hand-edited.

## Where a golden lives

**Not in this directory.** A golden that sits here is read from outside the build
context of every service that consumes it, so a test run from a service's own
Docker context cannot find it, and a test that skips when its file is missing
then passes by not running. So each consuming tree holds its own copy:

- the **canonical** copy is in the tree of the code that produces the request
  (for Auth Core: `auth-core/src/auth/fixtures/`), with a `.sha256` pin;
- every **consumer** vendors a byte-identical copy inside its own tree (for
  org-core: `org-core/internal/http/testdata/`), with the same pin;
- both pins are the sha256 of the file with CRLF normalised to LF;
- the producer's spec compares the copies and **fails** when they differ, and the
  repo-level drift check (`scripts/tests/zdr-contract-sync.test.sh`) pins both.

Files that are the *system under test* rather than a golden (the principal policy
`config/plane-service-principals.json`, `docker-compose.yml`) cannot be vendored.
Specs that read them skip when they are absent, **except under `REQUIRE_CONTRACT`**
(see the end of this file), where the absence fails.

## auth-core-org-core-retention-posture.contract.json

Auth Core's interactive retention resolver asks org-core for an org's ZDR
posture when it mints a token:

    GET /internal/orgs/:orgId/retention-posture
    x-service-id: auth-core
    x-service-token: <ORG_CORE_SERVICE_TOKEN>
    -> 200 { "zdrIntent": boolean, "entitled": boolean }
    -> 404 { "error": "organization not found" }

Scope: `org:retention:read:any`, granted to the `auth-core` principal only (in
`docker-compose.yml`, `ORG_CORE_SERVICE_CREDENTIALS`). The same read answers the
directory check for a `tenancy: "directory"` principal (a 404 means "no such
organization"; see `config/README.md`).

| Side | Copy | Test | What it proves |
| --- | --- | --- | --- |
| Auth Core (TypeScript) | `auth-core/src/auth/fixtures/` (canonical) | `interactive-retention-policy.org-core-contract.spec.ts` | the resolver sends exactly the golden request, with service headers; the copy is pinned; org-core's copy is identical |
| org-core (Go) | `org-core/internal/http/testdata/` (vendored) | `internal/http/retention_posture_contract_test.go` | the copy matches its pin; org-core's real middleware and routes admit the golden request for Auth Core and refuse every other principal |

Why it exists: the resolver once sent no service headers. org-core answered 401,
the resolver read that as an outage and resolved every org to `zdr: false`, so no
org's ZDR setting ever reached a token, and nothing failed because the resolver's
own spec mocked `fetch`.

### Regenerate after an intentional client change

    cd "apps/Control Plane/auth-core"
    UPDATE_CONTRACT_GOLDEN=1 npx jest interactive-retention-policy.org-core-contract

That writes the canonical copy and the org-core copy, each with its pin, in one
step. Then update the digest in `scripts/tests/zdr-contract-sync.test.sh` and run

    cd ../org-core
    REQUIRE_CONTRACT=1 go test ./internal/http/ -run 'Retention|Compose'

### REQUIRE_CONTRACT

Set it in any job that runs from a full checkout. Without it, a spec that needs a
file outside its service's tree (the policy, the compose file, another service's
vendored copy) skips when the file is absent; with it, the absence fails the
suite, so CI cannot pass by not finding the contract.
