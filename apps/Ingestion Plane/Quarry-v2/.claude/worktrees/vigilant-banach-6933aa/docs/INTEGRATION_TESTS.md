# Integration Tests — End-to-End Harness

D6 / cluster #14 + cluster #5.

## Goal

Verify Quarry's edge → control → Temporal path end-to-end:
1. Edge signs a `POST /v1/schedules` request with HMAC.
2. Control verifies the signature, persists the schedule.
3. Temporal SDK call registers the schedule.
4. Operator queries via `/v1/schedules` to confirm round-trip.

## Existing infrastructure

The monorepo already runs Temporal under two profiles in
`docker-compose.yml`:

- `mp-temporal` (port-mapped, used by Model Plane)
- `org-core-temporal` (used by Control Plane org-core)

Quarry-control reuses `org-core-temporal` by default
(`QUARRY_TEMPORAL_HOSTPORT=org-core-temporal:7233`). The namespace
defaults to `quarry`; the org-core-temporal container's
auto-setup creates the namespace on first start.

## Harness pattern

The simplest workflow is `make e2e` (or its equivalent):

```bash
# 1. Bring up the stack
docker compose up -d \
    aquatiq-postgres-local \
    aquatiq-redis-local \
    org-core-temporal \
    quarry-nats \
    quarry-control \
    quarry-edge

# 2. Wait for health (use the existing healthchecks)
docker compose wait quarry-edge

# 3. Run the e2e test binary
cd services/quarry-control && go test -tags=e2e ./e2e/...
```

The build tag `e2e` gates tests that need a live stack so default
`go test ./...` stays fast.

## Test pattern (sketch)

```go
//go:build e2e

package e2e_test

import (
    "context"
    "testing"
    "time"

    "github.com/triodelab/quarry-v2/services/quarry-control/internal/temporal"
)

func TestE2E_ScheduleCreatePauseTrigger(t *testing.T) {
    ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
    defer cancel()

    edgeURL := mustGetEnv(t, "QUARRY_EDGE_URL")
    jwt := mustGetEnv(t, "QUARRY_TEST_JWT")

    // 1. Create a schedule via the edge (HMAC-signed by edge automatically)
    sched := createSchedule(t, edgeURL, jwt, temporal.CreateOptions{
        Name: "e2e-test",
        Kind: "crawl",
        Cron: "*/5 * * * *",
    })

    // 2. Pause it
    pauseSchedule(t, edgeURL, jwt, sched.ScheduleID)

    // 3. Verify Temporal namespace shows the schedule
    tc := newTemporalClient(t) // dials org-core-temporal:7233
    handle := tc.ScheduleClient().GetHandle(ctx, "quarry-org_test-"+sched.ScheduleID)
    desc, err := handle.Describe(ctx)
    require.NoError(t, err)
    require.Equal(t, temporalclient.ScheduleStatePaused, desc.State.Paused)
}
```

## Why no `make e2e` exists yet

The e2e binary needs the live Temporal SDK linked (cycle 24), and
the test harness depends on the orchestrator workflow definitions
existing on the Temporal side. Both are scoped to cycle 24 — this
doc captures the contract so the work is straightforward when it
lands.

## Smoke covered today

| Layer                                | Today                                 |
| ------------------------------------ | -------------------------------------- |
| Edge → control HMAC roundtrip        | ✅ Rust unit tests + Go middleware tests |
| Edge schedule create → control DB    | ✅ if compose is up (manual smoke)      |
| Control → Temporal schedule register | ⬜ pending SDKClient (cycle 24)         |
| Operator pause via edge → Temporal   | ⬜ pending SDKClient                    |
| Backfill via edge → Temporal         | ⬜ pending SDKClient                    |

## Manual smoke recipe

Until the Go test binary lands, operators can verify the path
manually:

```bash
# 1. Stack up
docker compose up -d quarry-control quarry-edge
# 2. Mint a dev JWT (or use the dev bypass)
export JWT="..."
# 3. Create a schedule
curl -X POST http://localhost:8082/v1/schedules \
    -H "Authorization: Bearer $JWT" \
    -H "Content-Type: application/json" \
    -d '{"name":"smoke","kind":"crawl","cron":"0 3 * * *","overlap_policy":"skip","config":{"url":"https://example.com"}}'
# 4. Verify the edge → control forward landed
docker logs quarry-control | grep schedule
```

The HMAC headers + Idempotency-Key are stamped automatically by the
edge; no client involvement.

## What full coverage will look like (cycle 24)

- New `services/quarry-control/e2e/` package, built with `-tags=e2e`.
- Reuses the running `quarry-edge` + `quarry-control` containers.
- Spawns `temporalcli` for assertions where the SDK is overkill.
- ~5 tests: create+list, create+pause+trigger, backfill window,
  duplicate idempotency-key short-circuit, HMAC tamper rejection.
