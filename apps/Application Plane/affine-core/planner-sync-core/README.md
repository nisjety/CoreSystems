# planner-sync-core

`planner-sync-core` is Aqencia's first-party collaborative transport service for planner workspaces.

It currently owns:

- websocket transport for Yjs document sync
- planner room naming based on workspace and document ids
- auth-backed websocket admission
- workspace authorization via user-core session context and affine-core resolution
- document-level admission against Aqencia planner metadata
- first-party collaboration lifecycle events on the shared Aqencia bus

Current collaboration policy:

- `private` documents are owner-only
- `shared` and `collection` documents require a non-`viewer` org role for sync transport
- denied upgrade attempts emit `aqencia.application.planner.transport.denied`

The underlying Yjs transport mechanics remain in place, but the product and service boundary is now Aqencia-owned.
