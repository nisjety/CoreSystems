# Chaos tests — §16.4.3

toxiproxy-driven failure-injection suite. Runs quarterly (or pre-release)
to validate degraded-mode behavior against the failure modes we know
will happen in production.

## Stack

`tests/chaos/docker-compose.chaos.yml` brings up:

- The full DPv2 stack (same as `make docker-up`)
- `toxiproxy` (port 8474) interposed in front of NATS, Postgres, Qdrant,
  and Redis on container-local DNS aliases.

Services connect to the proxied endpoints; the tests use toxiproxy's HTTP
API to inject latency / cut connections / corrupt bytes.

## Scenarios

| ID | Failure | Expected behavior |
|----|---------|-------------------|
| C-01 | NATS down for 30s | embedding-engine consumer pauses, resumes when NATS recovers, no message loss |
| C-02 | Qdrant 5s latency injected | retrieval p95 stays under the timeout, no partial-candidate corruption |
| C-03 | Postgres pool exhausted (max_conns=1) | acquire_timeout returns 503 quickly; `dpv2_postgres_pool_saturation` gauge fires alert (§16.2.7) |
| C-04 | Redis disconnected mid-request | cache layer degrades to no-op, retrieval still succeeds |
| C-05 | Bandwidth-limit NATS to 1KB/s | DLQ entries appear after `max_deliver`, replayable via `dlq-replay` (§16.2.4) |

## Running

```
docker compose -f tests/chaos/docker-compose.chaos.yml up -d
cargo test -p retrieval-engine-rs --test chaos -- --ignored --nocapture
```

## Status

Scaffold-only as of wave-3.5 — the toxiproxy compose file and individual
scenarios are tracked under follow-up tasks. The README + open scenarios
unblock §16.4.3 as "design landed, implementation queued".
