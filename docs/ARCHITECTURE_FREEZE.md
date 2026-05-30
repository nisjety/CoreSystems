# Architecture Freeze — Legacy Model Plane ai-core

**Date**: 2026-04-07  
**Status**: ACTIVE  
**Scope**: `apps/Model Plane/ai-core/` is frozen. All new development goes to `apps/Model Plane v2/`.

## Decision

Model Plane v2 (`apps/Model Plane v2/ai-core` + `apps/Model Plane v2/agent-core`) is the **canonical runtime**.

The legacy `apps/Model Plane/ai-core` is now **read-only** except for:
- Critical security patches
- Emergency production hotfixes with a time-bound expiry

## Ownership Boundaries

| Component | Owner | Scope |
|-----------|-------|-------|
| **AI-Core (MP V2)** | Model Plane v2 | Model access, multimodal normalization, safety, inference APIs, Layer 0 |
| **Agent Core (MP V2)** | Model Plane v2 | Planning, orchestration, reflection, synthesis, tool calling, workflows |
| **Data Plane** | Data Plane | Chunking, indexing, retrieval, reranking, grounding metadata |
| **Legacy ai-core** | FROZEN | Read-only donor — port capabilities to MP V2, then retire |

## Migration Strategy

1. Port missing capabilities from legacy → MP V2 (gRPC services, document routing, Content Understanding)
2. Add compatibility shims in MP V2 for legacy gRPC consumers
3. Migrate callers one by one
4. Remove legacy after burn-in period with zero consumer regressions

## What This Means

- **Do NOT** add new routes, services, or features to `apps/Model Plane/ai-core/`
- **Do NOT** update dependencies in legacy unless security-critical
- **DO** reference legacy code when porting to MP V2
- **DO** file issues if a legacy-only capability blocks MP V2 adoption
