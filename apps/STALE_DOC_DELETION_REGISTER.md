# Stale Doc Deletion Register

This register tracks outdated or superseded documentation candidates discovered during the plane deep-dive audits.

## Scope

- Control Plane
- Data Plane v2
- Ingestion Plane
- Model Plane
- Application Plane
- Frontend Plane
- Channel Plane

## Decision rules

- `delete`: safe to remove now because a newer source of truth exists and the old file materially misstates the current system.
- `update`: still useful, but needs correction.
- `archive`: historical or planning value remains, but it should not present as current runtime truth.
- `keep`: current enough or intentionally future-facing.

## Register

| Path | Plane | Reason stale | Evidence | Current replacement/source of truth | Recommended action | Risk | Deletion ready |
|---|---|---|---|---|---|---|---|
| `apps/Control Plane/CONTROL_PLANE_ARCHITECTURE.md` | Control | materially under-describes current services and gRPC/event reality | deep dive found `audit-core`, partial gRPC surfaces, moved session/orchestration responsibilities, first-party network topology changes | `apps/Control Plane/CONTROL_PLANE_DEEP_DIVE.md` | delete | low | yes |
| `apps/Control Plane/auth-core/docs/auth-plan.md` | Control/auth-core | historical implementation plan, not current runtime truth | auth-core research found dual-route runtime plus live placeholder paths that differ from the original implementation narrative | `apps/Control Plane/docs/core-research/auth-core.md` | delete | low | yes |
| `apps/Control Plane/auth-core/docs/api.md` | Control/auth-core | outdated API examples and route behavior | auth-core research found drift in live Better Auth and enhanced auth behavior | `apps/Control Plane/docs/core-research/auth-core.md` | delete | low | yes |
| `apps/Control Plane/auth-core/docs/SPRINT_4_TEST_REPORT.md` | Control/auth-core | point-in-time completion report, not current source of truth | auth-core research shows later runtime growth, placeholders, and inactive residue | `apps/Control Plane/docs/core-research/auth-core.md` | delete | low | yes |
| `apps/Control Plane/auth-core/src/auth/orpc-router.ts.backup` | Control/auth-core | inactive backup source | extension plus research audit confirm it is not runtime code | `apps/Control Plane/docs/core-research/auth-core.md` | delete | low | yes |
| `apps/Control Plane/auth-core/src/orpc/consolidated-auth.controller.ts.unused` | Control/auth-core | inactive unused source | extension plus research audit confirm it is not runtime code | `apps/Control Plane/docs/core-research/auth-core.md` | delete | low | yes |
| `apps/Control Plane/auth-core/src/orpc/unified-auth.controller.ts.unused` | Control/auth-core | inactive unused source | extension plus research audit confirm it is not runtime code | `apps/Control Plane/docs/core-research/auth-core.md` | delete | low | yes |
| `apps/Control Plane/user-core/IMPLEMENTATION.md` | Control/user-core | stale completion report and ownership model | user-core research found active HTTP/gRPC/event/runtime shape that no longer matches the document | `apps/Control Plane/docs/core-research/user-core.md` | delete | low | yes |
| `apps/Control Plane/user-core/README.md` | Control/user-core | outdated project structure and feature/status claims | user-core research found current path/runtime shape, missing shared ACL publish, and ownership split different from the README | `apps/Control Plane/docs/core-research/user-core.md` | delete | low | yes |
| `apps/Control Plane/session-core/90_PERCENT_COMPLETE.md` | Control/session-core | obsolete status report for removed session-core responsibilities | session-core research found plans/todos/lineage moved out to Rust Model Plane session-core | `apps/Control Plane/docs/core-research/session-core.md` | delete | low | yes |
| `apps/Control Plane/session-core/API_REFERENCE.md` | Control/session-core | documents removed plans/todos/lineage-heavy API surface | session-core research found current service narrowed to legacy bridge plus Control Session aggregator | `apps/Control Plane/docs/core-research/session-core.md` | delete | low | yes |
| `apps/Control Plane/session-core/GAP_ANALYSIS.md` | Control/session-core | gap analysis for an older ownership model | session-core research found the listed surfaces no longer belong to this core | `apps/Control Plane/docs/core-research/session-core.md` | delete | low | yes |
| `apps/Control Plane/session-core/IMPLEMENTATION_SUMMARY.md` | Control/session-core | obsolete progress report for removed surfaces | session-core research found the current core no longer matches this summary | `apps/Control Plane/docs/core-research/session-core.md` | delete | low | yes |
| `apps/Control Plane/auth-core/.DS_Store` | Control/auth-core | non-source metadata file | filesystem artifact only | `apps/Control Plane/docs/core-research/auth-core.md` | delete | none | yes |
| `apps/Control Plane/auth-core/src/.DS_Store` | Control/auth-core | non-source metadata file | filesystem artifact only | `apps/Control Plane/docs/core-research/auth-core.md` | delete | none | yes |
| `apps/Control Plane/org-core/.DS_Store` | Control/org-core | non-source metadata file | filesystem artifact only | `apps/Control Plane/docs/core-research/org-core.md` | delete | none | yes |
| `apps/Application Plane/APPLICATION_PLANE_ARCHITECTURE.md` | Application | describes a smaller plane and stale service ownership assumptions | deep dive found `conversation-core-go`, `conversation-ingest-rs`, `information-core`, missing `affine-core` directory, broader notification surface, shared Postgres | `apps/Application Plane/APPLICATION_PLANE_DEEP_DIVE.md` | delete | low | yes |
| `apps/Ingestion Plane/INGESTION_PLANE_ARCHITECTURE.md` | Ingestion | still describes old Quarry/imports shape and outdated language/runtime split | deep dive found active `Quarry-v2`, `integration-corev2`, `finspo-core`, `support-worker`, and legacy residue not reflected here | `apps/Ingestion Plane/INGESTION_PLANE_DEEP_DIVE.md` | delete | low | yes |
| `apps/Model Plane/docs/ARCHITECTURE.md` | Model | undercalls implemented agent/orchestration state and overstates stub status in several areas | deep dive found durable approvals and subagent lineage, live browser-agent loop, current Go services beyond old future framing | `apps/Model Plane/MODEL_PLANE_DEEP_DIVE.md` | update | medium | no |
| `apps/Model Plane/docs/gap-model.md` | Model | overstates closure and understates remaining hybrid/in-memory surfaces | deep dive found partial bridge, cost, browser grant, memory, and approval-store behaviors | `apps/Model Plane/MODEL_PLANE_DEEP_DIVE.md` | update | medium | no |
| `apps/Model Plane/docs/STUBS.md` | Model | likely stale against current runtime because several services described as stubs are now partially or fully live | core research now shows live session-core orchestration durability, live browser-broker and sandbox-manager handlers, and partial rather than absent bridge/cost surfaces | `apps/Model Plane/docs/core-research/README.md` and `apps/Model Plane/MODEL_PLANE_DEEP_DIVE.md` | review | medium | no |
| `apps/Model Plane/docs/ARCHITECTURE.md` | Model | now secondary to service-level truth after core research pass | current state is better represented by plane deep dive plus service-level research docs | `apps/Model Plane/docs/core-research/README.md` and `apps/Model Plane/MODEL_PLANE_DEEP_DIVE.md` | review | low | no |
| `apps/Data Plane v2/docs/gap-data.md` | Data | completion-oriented target/gap doc still claims fully closed areas while some runtime caveats remain | deep dive found retrieval eval placeholder, authctx enforce-mode gap, tests overstating coverage, direct Azure fallback patterns | `apps/Data Plane v2/DATA_PLANE_DEEP_DIVE.md` | archive | medium | no |
| `apps/Data Plane v2/docs/WIRE_SURFACE_PLAN.md` | Data | planning doc, not runtime truth | deep dive documents actual service/runtime shape | `apps/Data Plane v2/DATA_PLANE_DEEP_DIVE.md` | archive | low | no |
| `apps/Data Plane v2/docs/gap-data.md` | Data | still implies `retrieval-eval-py` scaffold as active quality surface in places | current runtime service inventory shows `data-quality-go` is the active eval surface and `services/retrieval-eval-py` is empty | `apps/Data Plane v2/docs/core-research/data-quality-go.md` and `apps/Data Plane v2/docs/core-research/retrieval-eval-py.md` | update | low | no |
| `apps/Data Plane v2/docs/WIRE_RECONCILIATION.md` | Data | may become secondary after service-level core docs exist | current service truth is now split into `docs/core-research/*.md` plus `DATA_PLANE_DEEP_DIVE.md` | `apps/Data Plane v2/docs/core-research/README.md` and `apps/Data Plane v2/DATA_PLANE_DEEP_DIVE.md` | review | low | no |
| `apps/Ingestion Plane/IMPORTS_COMPLETION_SUMMARY.md` | Ingestion | completion framing may not match current active imports state | deep dive found active but still partial imports/integration surfaces | `apps/Ingestion Plane/INGESTION_PLANE_DEEP_DIVE.md` | update | medium | no |
| `apps/Ingestion Plane/README.md` | Ingestion | broad plane README likely mixes current runtime and historical framing | plane deep dive and new core-research docs now provide service-level current-state references | `apps/Ingestion Plane/INGESTION_PLANE_DEEP_DIVE.md` and `apps/Ingestion Plane/docs/core-research/README.md` | review | low | no |
| `apps/Ingestion Plane/Quarry-v2/docs/REST_RESOURCES.md` | Ingestion | route-parity/planning doc includes intended and stubbed surfaces, not just live-backed runtime behavior | `quarry-control` still carries explicit stubbed schedule/source behavior while current source-of-truth now exists in core research | `apps/Ingestion Plane/docs/core-research/quarry-v2.md` | archive | low | no |
| `apps/Ingestion Plane/Quarry-v2/docs/gap-quarry.md` | Ingestion | progress and gap ledger, not current runtime truth | current runtime is better represented by the plane deep dive plus core-research doc | `apps/Ingestion Plane/INGESTION_PLANE_DEEP_DIVE.md` and `apps/Ingestion Plane/docs/core-research/quarry-v2.md` | archive | low | no |
| `apps/Application Plane/convex-core/README.md` | Application | narrower than current Convex usage and may miss missing-job-module caveat | deep dive documents current projection/runtime role | `apps/Application Plane/APPLICATION_PLANE_DEEP_DIVE.md` | update | low | no |
| `apps/Application Plane/convex-core/CONVEX_INTEGRATION.md` | Application | integration guidance predates current cross-plane projection reality | deep dive plus live `convex/` code | `apps/Application Plane/APPLICATION_PLANE_DEEP_DIVE.md` | update | low | no |
| `apps/Application Plane/convex-core/DEPLOYMENT.md` | Application | still points parts of the stack at `ai-core:8000` and older Convex integration assumptions | current compose and core research point Convex at `model-gateway:8080` and document the missing `api.jobs.*` surface | `apps/Application Plane/docs/core-research/convex-core.md` | update | low | no |
| `apps/Application Plane/notification-core/README.md` | Application | still describes inbox feeds, preferences, and read-state projections as non-goals even though they now exist in live code | current runtime exposes feed, unread and unseen counts, preferences, channel config, and subscriber upsert | `apps/Application Plane/docs/core-research/notification-core.md` | update | medium | no |
| `apps/Application Plane/docker-compose.ui.yml` | Application | likely redundant overlay because the main compose already exposes `convex-dashboard`; only a small `affine-runtime` port override remains | core research found missing `affine-core` build context and duplicated dashboard exposure | `apps/Application Plane/docs/core-research/README.md` | review | low | no |
| `apps/Application Plane/convex-core/IMPLEMENTATION_COMPLETE.md` | Application | completion-style document may overstate the live and correct state of integrations | core research found broken or placeholder webhook and NATS subscriber surfaces alongside active projection logic | `apps/Application Plane/docs/core-research/convex-core.md` | review | low | no |
| `apps/Frontend Plane/velionv2/AUTH_ONBOARDING_PORT_PLAN.md` | Frontend | migration plan, not current runtime truth | live routes and tests now exist; remaining gaps are more specific than the plan doc | `apps/Frontend Plane/velionv2/FRONTEND_PLANE_DEEP_DIVE.md` | archive | low | no |
| `apps/Frontend Plane/velionv2/docs/Onboarding-plan.md` | Frontend | planning doc with mocked/unavailable expectations | live onboarding routes and current deep dive provide actual runtime state | `apps/Frontend Plane/velionv2/FRONTEND_PLANE_DEEP_DIVE.md` | archive | low | no |
| `apps/Frontend Plane/velionv2/CONTROL_PLANE_PARITY_AUDIT.md` | Frontend | point-in-time parity snapshot, still useful but not broad plane truth | current frontend deep dive and current route code are more authoritative | `apps/Frontend Plane/velionv2/FRONTEND_PLANE_DEEP_DIVE.md` | keep | medium | no |
| `apps/Frontend Plane/velionv3/README.md` | Velion v3 | mostly directionally correct, but it advertises `shared/api` and `shared/rpc` transport areas that are empty today | current v3 core research separates live onboarding gateway integration from mock-backed workspace and planned transport surfaces | `apps/Frontend Plane/velionv3/docs/core-research/README.md` | update | low | no |
| `apps/Frontend Plane/velionv2/velionv3-trust-after-port.md` | Velion v3 | generated Playwright/accessibility snapshot, not maintained architecture or runtime documentation | v3 source and current core research now describe the actual app state | `apps/Frontend Plane/velionv3/docs/core-research/README.md` | archive | low | no |
| `apps/Channel Plane/docs/vision.md` | Channel | not stale; intentionally future-facing and matches current deferred status | no runtime exists; doc explicitly says do not build yet | `apps/Channel Plane/CHANNEL_PLANE_DEEP_DIVE.md` | keep | low | no |
| `apps/Channel Plane/.DS_Store` | Channel | non-source metadata file | filesystem artifact only | none needed | delete | none | yes |

## Completed immediate deletions

These files were removed during this audit pass on `2026-06-07` because they were both superseded and low-risk:

1. `apps/Control Plane/CONTROL_PLANE_ARCHITECTURE.md`
2. `apps/Application Plane/APPLICATION_PLANE_ARCHITECTURE.md`
3. `apps/Ingestion Plane/INGESTION_PLANE_ARCHITECTURE.md`
4. `apps/Channel Plane/.DS_Store`
5. `apps/Control Plane/auth-core/docs/auth-plan.md`
6. `apps/Control Plane/auth-core/docs/api.md`
7. `apps/Control Plane/auth-core/docs/SPRINT_4_TEST_REPORT.md`
8. `apps/Control Plane/auth-core/src/auth/orpc-router.ts.backup`
9. `apps/Control Plane/auth-core/src/orpc/consolidated-auth.controller.ts.unused`
10. `apps/Control Plane/auth-core/src/orpc/unified-auth.controller.ts.unused`
11. `apps/Control Plane/user-core/IMPLEMENTATION.md`
12. `apps/Control Plane/user-core/README.md`
13. `apps/Control Plane/session-core/90_PERCENT_COMPLETE.md`
14. `apps/Control Plane/session-core/API_REFERENCE.md`
15. `apps/Control Plane/session-core/GAP_ANALYSIS.md`
16. `apps/Control Plane/session-core/IMPLEMENTATION_SUMMARY.md`
17. `apps/Control Plane/auth-core/.DS_Store`
18. `apps/Control Plane/auth-core/src/.DS_Store`
19. `apps/Control Plane/org-core/.DS_Store`

## Not deletion-ready yet

The remaining candidates should stay for now because they still have planning, historical, or service-local value, even if they are no longer good runtime truth documents.
