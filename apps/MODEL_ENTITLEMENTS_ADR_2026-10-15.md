# ADR-0005: Model entitlements: org-core stores, inference-core enforces, Model Plane fails closed

**Date**: 2026-10-15  
**Status**: accepted for implementation as the plan's default (DEC-13 (a)); WS10 S2-S7 merge only after the owner ratifies it in the plan's decision register (Appendix B, K1-8)  
**Deciders**: owner (DEC-13); Control Plane (ownership split); Model Plane (enforcement); Frontend Plane (admin section and BFF)  
**Scope**: chat model admission. Control Plane owns the organisation and role policy. Model Plane's inference-core enforces it and model-gateway asks it. Frontend Plane owns the admin section and the BFF write guard.  
**Supersedes / extends**: reverses the model-allowlist direction of `apps/Model Plane/docs/GATEWAY_DEDUPLICATION_PLAN.md` CR-06 (lines 225-241 and 388). Adds matrix rule 19 to `apps/master-ownership-matrix.md`, beside rule 9.

## Context

`ALLOWED_MODELS` is an operator ceiling read by `load_allowed_models` in `apps/Model Plane/rust/services/model-gateway/src/normalize.rs`. It is enforced only on the normalize path, which the unary route and the stream route call. The gRPC invoke path does not reach it, and execution-core is a separate service that never calls it. The dev override sets `ALLOWED_MODELS` to an empty string, which enforces nothing.

The organisation-level `OrgPolicy.allowed_models` field is refused by name at `set_policy` and returns zero-valued from `get_policy`. The gateway's in-memory policy store does not survive a restart (CR-06 in `GATEWAY_DEDUPLICATION_PLAN.md`). The model list the gateway serves is requested without any filter, so it offers models the caller may not use. The automatic routing chain (intent resolution, the tool ladder and the provider default) can pick a model the organisation has denied. inference-core sees the real organisation and user on every call, but no token carries a role.

Control already owns organisations, memberships, roles (projected from auth-core) and the service-principal registry. The retention posture is already read by auth-core from org-core through a scoped internal route, and this decision reuses that pattern. What is missing is one authority that says which models a member may use, and one place where that answer is enforced before a provider is called.

## Decision

1. **Control owns the policy.** org-core stores a chat model policy per organisation and per role (`org_model_policies`, with append-only revisions and history), validates it, resolves it and explains it. It is the only writer. A role override replaces the organisation default, `deny` wins over `allow`, and a non-member gets nothing. Entries are exact model ids or `provider:<id>`, with no patterns. The policy covers chat only. It may also set `max_effort`, a ceiling on the reasoning level owned by the effort ladder (WS11), and `min_privacy_tier`.

2. **inference-core enforces, and it is the only puller.** It is the Model Plane's one path to a model provider, so every route to a model passes its check. It fetches the answer for an organisation and user with its own service principal, which holds only the entitlement read scope. It caches the answer in memory for 30 s (TTL), with at most one fetch in flight per organisation and user. A call with no user (a service principal) is exempt and counted under `service_exempt`.

3. **model-gateway asks and decides nothing.** It asks inference-core over the new `GetModelAccess` RPC for the filtered catalogue, for the admission verdict before a run exists, and for the effort clamp. A refused request creates no thread, message or run.

4. **Fail closed, with bounded staleness.** When a refresh fails, the last answer is served as stale-if-error for 900 s if that answer restricted the member, and for 120 s if it did not. Past that bound, or with no answer ever seen, the request is refused with 503 `model_entitlement_unavailable`. Under `enforce`, an unresolved entitlement refuses. It never falls back to an unrestricted route, ZDR included.

5. **Three modes.** `MODEL_ENTITLEMENT_MODE` is `off`, `shadow` or `enforce`. Unset means `off`, and `off` makes no call to org-core, so behaviour is byte-identical to today. `shadow` counts the decisions it would have made (`would_refuse`) and never refuses or filters. An unrecognised value enforces. Rollback is one restart with `off`.

6. **Refusals are typed and content-free.** A refused model is `model_not_entitled` on the HTTP and SSE surfaces and `PERMISSION_DENIED` on gRPC. The body names the model and the reason, and never content. An automatic route with no entitled model returns `no_entitled_route`.

7. **ZDR.** The entitlement metadata holds model ids and actor ids only, never content. inference-core keeps it in memory, and no other store holds a copy. The BFF refuses, with 409 `model_policy_leaves_no_zdr_model`, a write that would leave a ZDR organisation with no attested model. If a policy does leave a ZDR token with no attested model, requests from that token are refused as `model_not_entitled`. Hard-deleting an organisation removes its policy rows.

8. **Quota stays fail-open. This is the asymmetry.** Quota reads keep failing open on a Control outage (`apps/Model Plane/rust/services/model-gateway/src/org_quota.rs`, logged), because cost-core still records the spend, so a cost ceiling has a second line. A deny list has none: a denied model that gets through is simply used. Entitlement therefore fails closed, and quota behaviour does not change.

9. **Delivery is a pull, not a claim.** The answer is never a token claim, and a caller never attaches it. A claim stays stale for its whole life, and an attached answer is a caller-supplied value the enforcer would have to trust.

10. **The exemption is a stated gap.** Service principals and scheduled runs on a service token sit outside the policy until an acts-for-user contract exists (WS20). The exemption is counted, not hidden.

## Alternatives considered

### Keep the allowlist in capability-core `routing_policies`

Rejected. The table has no runtime reader, and a registry row cannot see membership or roles. Roles resolve in org-core, so the registry would need its own copy of who holds which role, and that copy would drift.

### Mirror the policy into Convex and enforce from the Application Plane

Rejected. Convex may only narrow access (matrix rule 10). A mirror that is missing or stale must never widen access, and a client-side check is not a control.

### Enforce in model-gateway only

Rejected. The gRPC invoke path never reaches the gateway's normalize gate, and execution-core is a separate service. inference-core is the one path to a provider that every route reaches.

### Deliver the answer as a claim, or attach it to each request

Rejected for the reasons in decision 9.

### Fail open on an outage, as quota does

Rejected. See decision 8: a deny list has no second line.

## Consequences

### Positive

- One owner for the fact (org-core) and one enforcement point (inference-core) for every provider call. The catalogue and the admission check read the same answer.
- Refusals happen before a run exists, are typed, and carry no content.
- The change is dark by default and turns off with one restart.

### Negative

- Under `enforce`, chat depends on org-core for an answer that is not cached. An outage longer than the stale bound refuses chat.
- A policy edit takes effect within the 30 s TTL, not at once.
- A cold (organisation, user) pair costs one extra internal call, single-flighted.
- Service principals and scheduled runs stay outside the policy until WS20's contract lands.
- Role projection lags auth-core's membership, so a role change takes effect when the projection catches up.

### Risks and release gates

- A revoked model can pass through a stale cache. The TTL is 30 s, a restrictive answer is held longer on purpose, and the WS10 S7 live proof shows an edit landing within the TTL.
- A shadow period comes before `enforce`, so a wrong policy shows up as `would_refuse` before it refuses anyone. The S7 rollout records the mode that each running service reports.
- An unrecognised `MODEL_ENTITLEMENT_MODE` value enforces rather than being ignored. A mistyped rollback value would therefore turn enforcement on. The owner confirms this choice before WS10 S3 merges. WS10 S3's parser test must pin whichever choice is made.
- Proto tags, migration numbers and this ADR's number are assigned by the integrator (DEC-39). The only proto change is additive.
- Live ZDR hunks in `grpc.rs`, `auth.rs`, `sse.rs` and `http_routes.rs` land first (WS01 S0 precedes S3 and S4).
- This ADR changes no code. Its only switch, `MODEL_ENTITLEMENT_MODE`, is introduced by WS10 S3 with the default `off`.
