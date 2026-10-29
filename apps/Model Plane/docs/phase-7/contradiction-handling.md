# Contradiction handling

## Product role

Contradiction handling defines how new knowledge claims interact with existing
claims in graph memory and wiki memory. Every derived claim is evaluated against
prior claims using a fixed set of **contradiction verbs**, producing an explicit
relationship rather than silently overwriting. This preserves provenance and
keeps the knowledge plane additive.

## Transport

- **Verbs**: `reinforce`, `weaken`, `qualify`, `contradict`, `create`.
  - `reinforce` — new claim agrees with and strengthens an existing claim.
  - `weaken` — new claim reduces confidence in an existing claim.
  - `qualify` — new claim narrows or conditions an existing claim.
  - `contradict` — new claim directly opposes an existing claim; both are
    retained with the contradiction edge.
  - `create` — new claim has no prior counterpart.
- **Evaluation**: performed by Rust synthesizers during graph/wiki
  materialization; result is a typed edge between claims with confidence and
  provenance labels.
- **Query**: `GET /v1/knowledge/graph/*` and `GET /v1/knowledge/wiki/*` expose
  contradiction edges and qualified claims. HTTP is GET-only; non-GET returns
  405 with `Allow: GET`.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Verb evaluation (claim-to-claim comparison) | `inference-core` |
| Contradiction-edge materialization | `inference-core` |
| Confidence and provenance labelling | `inference-core` |
| Catalog, policy, retention, scope, scheduling metadata | `capability-core` |

## `capability-core` responsibilities

- **Catalog** — register verb evaluators as capabilities with explicit
  versions; expose the fixed verb set.
- **Policy** — enforce scope on contradiction queries; gate reinforcement /
  weakening thresholds per org policy.
- **Metadata** — durable records of verb-evaluation runs and the claims they
  touched, keyed by source event IDs.
- **Scheduling hints** — when to re-evaluate verb edges (e.g., after new
  corroborating events), delegated to Rust workers.

## Reference inputs

- `graphify` — typed relationship extraction.
- `GraphRAG` — claim-level graph retrieval.

References only; no code is copied.

## Out of scope

- Silent overwrite of prior claims (contradictions are retained, not deleted).
- Free-form user-authored contradiction edges.
- Verb-evaluator implementation details (owned by `inference-core`).
