# Wire-surface plan — gRPC + GraphQL across the platform

> Audit + decision log: which services serve gRPC, which serve HTTP/REST, where
> GraphQL is a better fit, and the order of operations to close the gaps.

## 1. Audit (current state)

| Plane | Service | HTTP | gRPC | Proto exists | Notes |
|-------|---------|------|------|--------------|-------|
| **Data Plane v2** | data-orchestrator-go | ✅ | ❌ | ❌ | internal NATS dispatcher; HTTP admin |
| | data-quality-go | ✅ | ❌ | ❌ | quality checks; HTTP admin |
| | documents-api-go | ✅ | ❌ | ✅ `documents_v2` | **paper proto** — service exposes HTTP only |
| | embedding-engine-rs | ✅ | ✅ | ❌ | gRPC primary; tonic |
| | **graph-index-rs** | ✅ | ✅ | ✅ `graph_v1` | gRPC server now **shipped** (this PR) |
| | index-engine-rs | ✅ | ❌ | ❌ | internal indexer |
| | quickwit-adapter-rs | ✅ | ❌ | ❌ | adapter only |
| | retrieval-engine-rs | ✅ | ✅ | ✅ `retrieval_v2` | only fully-wired pair today |
| | retrieval-eval-py | ❌ | ❌ | ❌ | offline lab |
| | **wiki-store-go** | ✅ | ❌ | ✅ `wiki_v1` | **paper proto** — service exposes HTTP only |
| **Control Plane** | auth-core | ✅ | ❌ | partial | NestJS; HTTP is the right shape |
| | audit-core / billing-core / org-core / session-core | ✅ | ❌ | ❌ | HTTP REST for verevon proxies |
| **Application Plane** | affine-core / convex-core / notification-core | ✅ | ❌ | ❌ | HTTP / Convex / WebSocket |

## 2. Decision matrix — which wire fits

| Pattern | Best fit | Why |
|---------|----------|-----|
| Model Plane → Data Plane hot path (retrieval, embedding, graph, wiki, documents) | **gRPC** | strongly-typed, streaming, low-latency, service-to-service |
| verevon → Control Plane (auth, org, billing) | **HTTP REST** (status quo) | browser-driven, well-suited to fetch + cookies; gRPC-web adds plumbing for no win |
| verevon → ad-hoc orchestration (read paths) | gRPC where typed (already in place); HTTP proxies for plain reads | mixed today; keep as-is |
| **Graph traversal queries** (entity → relationships → claims → contradictions, with selectable depth) | **GraphQL** | flexible field selection + deep nested traversal is exactly its strength; gRPC forces N round-trips or fat responses |
| **Wiki rendering** (page + version + sources + backlinks in one fetch) | **GraphQL** | classic "fetch what the page needs" pattern; today the verevon view does multiple HTTP calls |
| Run / agent CRUD, plan/todo/approval transitions | gRPC | strongly-typed transitions, server-driven validation |

## 3. Work plan

### Done in this round

- **graph-index-rs** — tonic gRPC server alongside axum HTTP on `:50053`,
  implementing all 6 `graph_v1.GraphService` RPCs (`GetEntity`,
  `ListEntitiesByType`, `GetRelationships`, `GetClaims`, `ExpandGraph`,
  `GetContradictions`). Same `GraphStore` backs both wires; 2 mapping tests pass.

### Next (each its own focused PR)

#### A. Proto layout cleanup (prerequisite to wiki + documents gRPC)
The shared `gen/go/` uses `paths=source_relative`; flat `wiki_v1.pb.go` +
`graph_v1.pb.go` (package `v1`) collide with the `v2` files in one dir, breaking
Go builds. **Fix**: move `proto/{graph_v1,wiki_v1}.proto` into per-package
subdirs (`proto/graph/v1/`, `proto/wiki/v1/`) and update `gen-clients.sh`
imports + the graph-index-rs `build.rs` paths. Once that lands, the gen script
can include all five protos and each service can either import shared stubs or
own their proto compile.

#### B. wiki-store-go gRPC
Implement `WikiService` (10 RPCs) on `:50054` alongside chi HTTP. Repo
already has the backing methods (`GetPage`, `GetPageByPath`, `ListVersions`,
`CreatePage`, `CreateVersion`, `SubmitProposal`, `GetBacklinks`, etc.). Two
proto-vs-model gaps to reconcile while doing this: `WikiSourceLog` and
`WikiMaintenanceLog` proto messages don't match the `model.SourceLog` /
`model.MaintenanceLog` fields. Decide whether to widen the model or narrow the
proto.

#### C. documents-api-go gRPC
Same pattern as wiki: tonic-less Go service, existing chi HTTP, proto contract
`documents_v2` already declared. Wire alongside HTTP on the matching port.

#### D. GraphQL gateway for graph + wiki traversal
Add a GraphQL endpoint (single port across both services, or per-service)
backed by the same stores. Concrete schema sketch:

```graphql
type Entity {
  entityId: ID!
  orgId: String!
  type: String!
  text: String!
  confidence: Float!
  relationships(type: String): [Relationship!]!
  claims(status: String): [Claim!]!
}

type Relationship { relId: ID!, type: String!, a: Entity!, b: Entity! }
type Claim { claimId: ID!, text: String!, status: String!, contradictedBy: [Claim!]! }

type WikiPage {
  pageId: ID!
  path: String!
  currentVersion: WikiPageVersion!
  versions(limit: Int): [WikiPageVersion!]!
  backlinks: [WikiPage!]!
  sources(versionId: String): WikiSourceLog
}

type Query {
  entity(orgId: String!, entityId: ID!): Entity
  expand(orgId: String!, seedIds: [ID!]!, maxHops: Int): [Entity!]!
  wikiPage(orgId: String!, pageId: ID!): WikiPage
  wikiByPath(orgId: String!, path: String!): WikiPage
}
```

This is where verevon would render an entity card with relationships + claims +
contradictions in **one** round trip instead of 4 — and a wiki page with its
version + sources + backlinks in **one** instead of 3+. gRPC stays underneath
for the Model Plane hot path; GraphQL sits on top for client convenience.

#### E. Control Plane stays HTTP
No gRPC value-add — verevon talks REST, and that's correct. Skip.

## 4. Rule of thumb going forward

- **New service-to-service contract?** gRPC.
- **New client-facing flexible query?** GraphQL on top of the gRPC.
- **New browser-driven REST resource?** HTTP REST.
- **Don't author paper protos.** If a `.proto` ships, a gRPC server ships
  alongside it in the same change.
