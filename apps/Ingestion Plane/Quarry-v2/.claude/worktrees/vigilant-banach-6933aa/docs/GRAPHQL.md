# GraphQL Overlay — Design

Cycle 27 / cluster #11.

## Status: design landed, implementation deferred

Cluster #11's acceptance is "GraphQL works without breaking REST;
complexity limits + auth boundaries enforced; subscriptions reuse
canonical event model". The wire contracts that GraphQL programs
against — every resource shape in `quarry-core::resources`, the
`JobHistoryEvent` envelope in `quarry-core::job_history`, the
pagination contract in `quarry-core::pagination` — are all shipped.
GraphQL itself is a thin overlay; we're documenting the shape and
deferring the `async-graphql` integration to cycle 30 because:

1. Subscriptions reuse the cluster #7 `JobHistoryEvent` envelope —
   the consumer-side replay logic on the SSE path (cycle 25.A) needs
   to settle first.
2. The Rust ecosystem has two viable libraries (`async-graphql`,
   `juniper`); a benchmark suite (cycle 28) needs to inform the
   choice.
3. Authentication MUST flow through the same JWT path; cluster
   #auth+tenancy is solid in P0, so this is straightforward but
   warrants a dedicated cycle.

## Designed surface

### Schema

```graphql
schema {
  query: Query
  mutation: Mutation
  subscription: Subscription
}

type Query {
  # List queries — paginated via the same opaque cursor as REST.
  jobs(filter: ListFilter!): JobPage!
  schedules(filter: ListFilter!): SchedulePage!
  stores(filter: ListFilter!): StorePage!
  snapshots(filter: ListFilter!): SnapshotPage!
  sessions(filter: ListFilter!): SessionPage!
  artifacts(filter: ListFilter!): ArtifactPage!
  benchmarks(filter: ListFilter!): BenchmarkPage!

  # Single-resource fetch.
  job(id: ID!): Job
  schedule(id: ID!): Schedule

  # Team-level aggregates.
  teamCreditUsage(period: String): TeamCreditUsage!
  teamTokenUsage(period: String): TeamTokenUsage!
  teamConcurrency: TeamConcurrency!
  teamQueueStatus: TeamQueueStatus!
}

type Mutation {
  # Mutation lands last (gap-quarry §10.1 #11 explicit ordering).
  pauseSchedule(id: ID!): Schedule!
  unpauseSchedule(id: ID!): Schedule!
  triggerSchedule(id: ID!): Schedule!
  backfillSchedule(id: ID!, input: BackfillInput!): Schedule!
  deleteSchedule(id: ID!): Boolean!
  replayJobEvents(runId: ID!): JobHistoryEventConnection!
}

type Subscription {
  # Reuses cluster #7's canonical event envelope.
  jobProgress(runId: ID!): JobHistoryEvent!
  changeDetected: ChangeRecord!
  scheduleStatus(scheduleId: ID!): Schedule!
}
```

Every type maps 1:1 onto an existing Rust type in `quarry-core`:

| GraphQL type     | Rust origin                              |
| ---------------- | ---------------------------------------- |
| `Job`            | `resources::JobSummary`                  |
| `Schedule`       | `resources::ScheduleSummary`             |
| `Source`         | `resources::Source`                      |
| `Snapshot`       | `resources::Snapshot`                    |
| `Artifact`       | `resources::ArtifactSummary`             |
| `Benchmark`      | `benchmark::BenchmarkSummary`            |
| `JobHistoryEvent`| `job_history::JobHistoryEvent`           |
| `ChangeRecord`   | `change_history::ChangeRecord`           |
| `ListFilter`     | `pagination::ListFilter`                 |

So the schema introspection is `derive`-driven via `async-graphql`'s
`#[derive(SimpleObject)]` — no hand-written resolvers for shape
mapping.

## Complexity limits

Operators set `QUARRY_EDGE__GRAPHQL_MAX_COMPLEXITY=2000` (or similar
budget). Each list query costs `limit`, each subscription
`1+open_connections × cost_per_message`. Queries above budget return
a 400-shaped GraphQL error.

## Auth integration

The existing `require_auth` middleware (P0 / cluster #auth+tenancy)
wraps the GraphQL route. The verified `Claims` extension flows into
the GraphQL context; resolvers pull `claims.org_id` to filter every
query.

Schema introspection does NOT bypass auth — the GraphQL playground
will not show the schema to unauthenticated callers in production.

## Subscriptions over SSE

To avoid a second wire protocol, subscriptions use Server-Sent
Events over HTTP (GraphQL-over-SSE pattern). One long-lived
connection per subscription; resume-via-`seq` works the same as
the cluster #7 reconnect contract.

## Pending implementation (cycle 30+)

- Add `async-graphql` + `async-graphql-axum` deps to workspace
- Build schema in `crates/quarry-edge/src/graphql/schema.rs`
- Wire `POST /graphql` + `GET /graphql/sse` routes
- Playground (`/graphql/playground`) gated by `QUARRY_EDGE__GRAPHQL_PLAYGROUND=true`
- Complexity calculator + budget enforcement
- Schema introspection test (every type from `quarry-core` reachable)
- Per-query auth test (unauthenticated → 401)
- Subscription resume test (mid-run reconnect picks up at last seq)
