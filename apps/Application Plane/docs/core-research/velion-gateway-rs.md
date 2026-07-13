# velion-gateway-rs

> **2026-07-13 update.** The active implementation is Frontend Plane Velion v3's gateway. Social metrics/catalog routes exist. The navbar support caller now targets canonical `POST /api/v1/notification-requests` with a non-PII deterministic idempotency key, and information UI contracts label observation provenance. Those source changes are not deployed. Notification tenant/recipient identity and support destination validation remain release blockers.

## Current State

`velion-gateway-rs` is a real Rust onboarding/gateway BFF. It is not a stub.

2026-07-02 note: this research file lives under Application Plane docs, but the current entry points listed below are under `apps/Frontend Plane/velionv3/apps/gateway`. Treat the gateway as a Frontend Plane ownership item unless the Application Plane service is re-established separately.

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

- The historical `velionv2` BFF implements equivalent onboarding proxy logic in `apps/Frontend Plane/velionv2/src/app/api/onboarding/_lib/onboarding-proxy.ts`.
- Velion v3 browser code calls same-origin `/api` paths; Vite/nginx proxying then targets the Rust gateway rather than naming `velion-gateway-rs` directly in browser code.
- Application Plane docs still mention this gateway, while the current source path is Frontend Plane. That is an ownership/documentation drift item.

That makes this service a boundary-ownership issue. It may still be used operationally through same-origin proxying, but Application Plane should not be treated as the owner without a fresh compose/deploy decision.

## Stub, Mock, Placeholder, and Partial Audit

- No explicit placeholder routes were found in the main router.
- The main risk is overlap and architectural duplication, not missing implementation.

## Notes

Decide whether this gateway is a Frontend Plane gateway, an Application Plane onboarding BFF, or a transitional compatibility layer. Do that before extending new onboarding behavior.
