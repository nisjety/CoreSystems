# E2E pipeline tests — §16.4.1

End-to-end suite driving the full DPv2 pipeline against the local
docker-compose stack:

```
documents-api → index-engine → embedding-engine → graph-index → retrieval-engine
                       │             │                  │              │
                       └─── NATS ────┘                  │              │
                                                        └─── Qdrant ───┘
                                                                       │
                                                                  Postgres
```

Each test scenario:

1. POSTs documents via `documents-api` `/v1/documents/bulk`.
2. Waits for `embedding_status = 'done'` rows on `knowledge_units` (poll
   loop with a 60s budget).
3. POSTs a retrieval query to `retrieval-engine` `/v1/retrieve` and asserts
   the planted documents come back with expected scores.
4. Verifies the trace persisted in `retrieval_runs` includes a non-empty
   `mode_mix_applied` (§16.1.1) and any expected `zdr_actions_applied`
   (§16.1.3).

## Running

```
make docker-up                          # full stack
DATAPLANE_E2E_BASE=http://localhost:8004 \
  cargo test -p retrieval-engine-rs --test pipeline_e2e -- --ignored --nocapture
```

The `--ignored` gate keeps this off the default `cargo test` path; CI
opts in via a dedicated workflow step.

## Scenarios

- `happy_path` — plant + retrieve + assert score ordering.
- `zdr_reject` — plant + mark one doc restricted + retrieve + assert it's
  filtered + `zdr_actions_applied` records the filter.
- `large_bulk` — 200 docs in one BulkIngest, verify all reach `done`.
- `cache_invalidation` — plant + retrieve + update + retrieve again, must
  return the new content within 1s (validates §16.2.2 org_version path).

## Open items

- Currently the suite ships as a scaffold — real assertions land alongside
  the cache_invalidation work (§16.2.2 wiring on the read side).
- Chaos variants live under `tests/chaos/` (§16.4.3).
