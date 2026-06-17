# data-quality-go Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/data-quality-go`

## Snapshot

`data-quality-go` owns retrieval eval runs, trust scoring, quality gates, lint, and cost summary views for Data Plane v2.

Current evidence highlights:

- Go HTTP service with Postgres backing
- retrieval quality surface is live here
- older docs still mention a Python eval harness, but the current runtime quality service is Go
- route family is internal and analysis-oriented rather than end-user product UI

Non-generated, non-vendored file count from the current tree: about `14`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - Postgres, eval runner, trust scorer, gate checker, linter, cost query, HTTP server
- `internal/eval/*`
- `internal/trust/*`
- `internal/gates/*`
- `internal/lint/*`
- `internal/cost/*`
- `internal/handler/*`

Primary surface:

- `/v1/evals/retrieval`
- `/v1/evals/retrieval/{evalID}`
- `/v1/evals/compare`
- `/v1/quality/trust`
- `/v1/quality/gates`
- `/v1/quality/lint`
- `/v1/cost/summary`

## API And Relationship Map

Current relationships:

- internal operators and evaluation tooling -> `data-quality-go`
- `data-quality-go` -> Postgres
  - eval state, trust, gates, lint, and cost views
- `data-quality-go` -> broader Data Plane runtime
  - quality judgments depend on retrieval and corpus behavior elsewhere in the plane

## Duplicates, Redundancies, And Inactive Surfaces

Documentation redundancy:

- older planning docs still talk about `retrieval-eval-py` as a scaffold target
- the current live runtime quality surface is `data-quality-go`

No source-level `.unused` or `.backup` residue was found in the active tree.

## Stubs, Placeholders, And Missing Connections

No explicit code stubs were found in the active service tree.

The main partial area is documentation and support tooling:

- Python eval harness references still exist
- the service tree for `retrieval-eval-py` is effectively empty

## API Design And Performance Notes

API design:

- quality, trust, lint, and cost belong together because they all evaluate the retrieval estate rather than serve end-user retrieval directly

Performance and operational notes:

- route latency is less important than correctness and reproducibility
- the bigger issue is keeping quality measurement in sync with the real retrieval stack rather than letting side tooling drift

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`

Update or archive, not delete:

- `docs/gap-data.md`
  - still useful, but it should stop implying that the Python eval scaffold is the active quality surface

## Bottom Line

`data-quality-go` appears to be the real quality runtime. The remaining confusion is mostly documentary: old references still point at a Python harness that is not present as a populated service.
