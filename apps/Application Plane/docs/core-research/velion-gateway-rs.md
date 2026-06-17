# velion-gateway-rs

## Current State

`velion-gateway-rs` is a real Rust onboarding BFF. It is not a stub.

It exposes a `/health` route plus a fairly broad onboarding router that fans out to lower-plane services for:

- session bootstrap
- onboarding state
- organization creation and plan selection
- crawl preview and website ingest
- graph preview
- source discovery and cleanup
- SharePoint discovery warm-up
- integration connect-session and sync start
- plan recommendation

## Entry Points

- Main: `apps/Frontend Plane/velionv3/apps/gateway/src/main.rs`
- Supporting modules: `apps/Frontend Plane/velionv3/apps/gateway/src/*.rs`

## Relationships

- Calls Control Plane services such as `org-core` and `user-core`.
- Calls Data Plane graph index.
- Calls Ingestion Plane and integration endpoints.
- Calls Quarry edge/control endpoints.
- Calls Model Plane plan recommendation at `model-gateway:8080/v1/recommend/plan`.

## Redundancy and Usage Audit

- The current `velionv2` BFF already implements equivalent onboarding proxy logic in `apps/Frontend Plane/velionv2/src/app/api/onboarding/_lib/onboarding-proxy.ts`.
- In-repo references to `velion-gateway-rs` are limited to its own compose wiring and source tree.
- No current `velion` or `velionv2` caller reference points at `velion-gateway-rs` directly.

That makes this service a likely transitional or redundant boundary. It may still be used operationally, but in-repo evidence for active callers is weak.

## Stub, Mock, Placeholder, and Partial Audit

- No explicit placeholder routes were found in the main router.
- The main risk is overlap and architectural duplication, not missing implementation.

## Notes

If the system standardizes on `velionv2` BFF routes, this Rust gateway becomes a prime consolidation candidate.
