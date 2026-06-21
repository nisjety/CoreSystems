# Phase 3 — B-spike: DP→MP gRPC embedding hop (root cause)

**Status:** RESOLVED (root cause found + live round-trip achieved). 2026-06-21.
**Exit criterion (PR-2 DoD):** ✅ written root cause reproduced once locally; retrieval
returned a **non-empty embedding vector from inference-core over gRPC**. The
written-fallback (ship `org_id`+`zdr` on direct-Azure, re-spike P4) is **NOT** needed —
the hop works; B is **low-risk**, not the big fallback.

## Symptom (as reported)
`EMBEDDING_PROVIDER=model_plane` made the DP retrieval engine call inference-core's
`CreateEmbedding` over gRPC (`:9092`) and fail with a `Status` error and **no
server-side log**, so the hop was never trusted as the deployed path.

## What was tested (live stack, both planes up 12h)
Direct `grpcurl` probe of the running `model-plane-inference-core-1` on `:9092`
(plaintext h2c), bypassing the DP, using `apps/Model Plane/proto/model_plane/v1/inference.proto`:

| `provider_hint` | result |
|---|---|
| `azure_openai` (underscore — **what the DP sends**) | ❌ `Code: Internal — "all providers exhausted after 0 total attempts"` ; **inference-core logged nothing** |
| `azure-openai` (hyphen — canonical) | ✅ `vector_len=3072`, `model_used=text-embedding-3-large`, `provider_used=azure-openai`; logs `"embedding completed"` + `"embedding succeeded" dims:3072` |

## Root cause
A **`provider_hint` naming mismatch**, not a transport/infra fault:

1. The DP client (`retrieval-engine-rs/src/embed/mod.rs`) sends
   `provider_hint = MODEL_PLANE_EMBEDDING_PROVIDER`, whose default is **`azure_openai`** (underscore).
2. inference-core's `FallbackChain::provider_matches`
   (`inference-core/src/provider/fallback.rs:380-385`) only accepts hints
   `azure`, `azure-openai`, `openai` — it does **not** normalise the underscore form.
   So `azure_openai` matches **zero** registered providers → the `create_embedding`
   loop runs `0` iterations → `ProviderError::AllExhausted` with `total_attempts=0`.
3. The gRPC handler (`inference-core/src/grpc.rs:143 create_embedding`) maps the error
   with `map_err(|e| Status::internal(e.to_string()))` and has **no `tracing` on the
   embedding error path** → the client sees a `Status` but the server logs nothing.
   That is the "Status error + no server-side log" symptom, exactly.

## Ruled out (proven by the successful hyphen round-trip)
- ❌ stale image / missing deployment — inference-core embeds via Azure
  `text-embedding-3-large` and returns 3072 dims.
- ❌ creds — the same Azure key embeds successfully.
- ❌ proto / port / h2c mismatch — plaintext h2c gRPC on `:9092` works with the committed proto.
- Also note: the **running** `dpv2-retrieval-engine` is configured `EMBEDDING_PROVIDER=azure_openai`
  (compose default), so the model_plane hop is **not exercised in the current deployment** —
  the failure was only ever hit when someone flipped the provider without the hint fix.

## PR-3 (B-fix) plan — derived from this spike
1. **Normalise `provider_hint`** in `provider_matches` (treat `_`≡`-`, lower-case) so
   `azure_openai` ≡ `azure-openai` — robust against all callers. *(MP side, primary fix.)*
2. Set the DP default `MODEL_PLANE_EMBEDDING_PROVIDER=azure-openai` (canonical) **and**
   make **`EMBEDDING_PROVIDER=model_plane` the real deployed default**, demoting direct-Azure
   to fallback only.
3. **Add a server-side error log** to `create_embedding` (`grpc.rs`) so a future provider
   failure is never silent again.
4. Add the `zdr` field to `CreateEmbeddingRequest` (proto + regen retrieval-engine-rs + MP
   + DPv2 enforcement); thread `org_id`+`zdr` through the hop.
5. Full-pipeline ZDR-reject e2e via the orchestrator **including the direct-Azure fallback
   branch** + a freshness integration test.

> ⚠ Residency honesty: making model_plane the embedding path does **NOT** satisfy EU
> residency — both provider paths still default to Azure. Do not market residency on B's
> completion; an EU model-plane embedding provider is a Phase-4 prerequisite.
