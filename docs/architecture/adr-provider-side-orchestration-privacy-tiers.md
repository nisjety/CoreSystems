# ADR: provider-side orchestration is never the only path for a privacy-tier customer

**Status:** accepted — 2026-08-24
**Decision owner:** Model Plane (model-gateway owns tier selection on the chat
surface; inference-core owns enforcement; capability-core owns registry
disclosure).

## Context

`PROVIDER_AND_PRIVACY_STRATEGY.md` §4 introduces programmatic privacy tiers —
`UNSPECIFIED < GLOBAL < EU_RESIDENT < ZDR_CONTRACTUAL < SOVEREIGN` — carried on
requests as `min_privacy_tier` and enforced by inference-core beside the
existing ZDR skip. The tiers exist because customers differ in what they allow
to leave CoreSystem's control: an EU-resident customer constrains data
residency; a ZDR-contractual customer additionally requires zero-data-retention
evidence bound to the provider; a sovereign customer accepts nothing but
Norway-pinned providers under our exclusive control.

Provider-side orchestration features — built-in search, provider-side memory
and conversation state, server-side tool loops — are attractive: less latency,
less code. But they move execution state onto provider-hosted infrastructure
whose retention behavior is exactly what the higher tiers contract away. A
gateway that routed a ZDR-contractual request into a provider-side loop would
satisfy the model call while silently violating the contract that motivated it.

What has landed so far on this branch: the `PrivacyTier` enum and
`min_privacy_tier` on `InferRequest`/`InvokeRequest` in inference.proto;
tier/residency derivation and fail-closed enforcement in all three chain paths
before any network call; tier+residency stamped onto responses and final stream
chunks beside the existing `zdr` stamp; `/v1/models` discloses per-model
`privacy_tier`/`residency` and states honestly when a sovereign selection
narrows the catalog.

## Decision

1. **Never adopt provider-side orchestration as the *only* path for a
   privacy-tier customer.** Built-in search, provider memory, and server-side
   loops may be used as accelerators only when an equivalent CoreSystem-owned
   path exists (the gateway tool loop or the governed execution loop) and is
   the path actually taken for requests above `UNSPECIFIED`. For such requests,
   conversation history is reconstructed locally each turn and no durable
   execution state lives provider-side.

2. **Tier gating stays in inference-core, before any network call**, mirroring
   the ZDR skip pattern: ineligible providers are skipped with typed reason
   `TierUnavailable`; an empty remainder is a typed error naming the tier.
   Never a silent downgrade to a weaker provider, and never a fallback into a
   provider-side loop.

3. **Registry facts stay declarative.** capability-core keeps authority over
   which models carry which tier/residency, loaded at startup — not consulted
   on the hot path (§4.5).

4. **capability-core registry facts land as schema + cache** (implemented in
   this decision's follow-through):
   - Migration `0013_privacy_tier_columns` (after `0012_run_watch_subscriptions`)
     adds `privacy_tier` and `residency` columns to the `models` table with a
     check-constrained label set mirroring the wire enum, and soft-deletes the
     decorative seed row `google/gemini-1.5-pro` (`config_json = '{}'`), which
     had no provider backing and would otherwise force a fabricated tier claim
     now that rows carry disclosure metadata.
   - A startup-loaded `ModelsCache` serves listing/authz projections from an
     in-memory snapshot; there is no synchronous hot-path RPC between
     model-gateway and capability-core (§4.5).

## Consequences

Any future provider-side orchestration feature must either ship together with
an owned-path equivalent usable by constrained tiers, or be gated off entirely
for requests above `UNSPECIFIED`. "It only works provider-side" is a reason to
not offer the feature to those customers, not a reason to route them into it.

With the `models` columns and startup cache landed alongside the tier
disclosure work, capability-core is the declarative source of record: catalogs
can be curated (tier/residency per row) without code changes, and the catalog
cannot contain an enabled row whose tier cannot be honored.
