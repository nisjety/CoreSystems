# Zero Data Retention (ZDR) in the Model Plane

Status: **the enforcement machinery is built and correct; ZDR is NOT usable end
to end because no provider deployment is attested.** This document explains what
ZDR means here, what exists, the single thing that is missing, and the exact
steps to make it production-ready.

Last verified: 2026-08-01.

---

## 1. What ZDR means in Velion

A **Zero Data Retention** turn is one where the user's prompt and the model's
answer leave **nothing durable behind** — not in Velion's own stores, and not in
the model provider's. It is the mode a user picks for a sensitive question they
do not want remembered, and it is also an org-wide posture an admin can set for a
whole tenant.

Two distinct sources set it, and they combine as a **monotonic floor** — ZDR can
only be turned *on* by a caller, never off:

- **Per-turn**: the "Midlertidig samtale / Temporary chat" toggle in the composer
  sets `zdr: true` on the request.
- **Org-wide**: the Zero Data Retention switch in workspace settings sets
  `claims.zdr` on the session, so every turn for that org is ZDR regardless of the
  toggle.

`effective_zdr = issuer_zdr || request_zdr` (`inference-core/src/auth.rs`), and the
BFF ORs the header flag with the body flag, deliberately preserving a malformed
value so the typed boundary rejects rather than silently coercing it to `false`.

---

## 2. What ZDR must guarantee, boundary by boundary

ZDR is only real if **every** content-persisting boundary honours it. These are
the boundaries and their current state.

| Boundary | What ZDR requires | State |
|---|---|---|
| **session-core threads/messages** | No thread row, no message row for a ZDR turn | ✅ Done. `prepare_managed_run_with_bearer` returns an empty thread id and appends no user message when `zdr` is set. A ZDR turn is structurally unreachable from `messages`. |
| **Dreaming / agent_memory** | No memory extracted from a ZDR turn | ✅ Done — by construction. Dreaming reads `messages JOIN threads`, and a ZDR turn writes neither, so it can never be extracted. `dream_extractor` has a test pinning this against the gateway source. |
| **Response cache (CAG)** | No prompt/response cached | ✅ Done. `SemanticCache::lookup`/`store` short-circuit on `zdr` (`cache_io_allowed(zdr) == !zdr`). |
| **Implicit feedback** | No feedback envelope emitted (the feedback store is durable) | ✅ Done. `implicit_feedback::envelopes_for` returns empty for a ZDR turn. |
| **Anthropic prompt caching** | No provider-side prompt cache write | ✅ Done — gated on the same flag. |
| **NATS / audit envelopes** | ZDR-derived content not published to durable subjects | ✅ Done — `zdr: true` envelopes are suppressed by natsx. |
| **The model provider itself** | The provider must contractually not retain the request | ❌ **NOT satisfied — this is the whole gap. See §3.** |

The first six are enforced and were verified this session. The seventh is the one
that does not hold, and because inference-core fails **closed**, it takes the
whole feature down: a ZDR turn today reaches `inference-core`, is refused, and the
user sees an error.

---

## 3. The one missing piece: provider attestation

inference-core will only send a ZDR request to a provider that is **attested** to
have a real zero-retention contract. It does this deliberately, in
`provider/fallback.rs`:

```rust
if req.zdr && !provider.capabilities_dyn().supports_zdr {
    // skip this provider
}
```

If no provider survives the skip, the request exhausts and returns
`ProviderError::ZdrUnavailable`. Today **no provider is attested**:

- **azure-openai** reads `supports_zdr` from `AZURE_OPENAI_ZDR_CONFIRMED`
  (`config.rs`), which defaults to `false` and is `${AZURE_OPENAI_ZDR_CONFIRMED:-false}`
  in compose.
- **azure-anthropic** hardcodes `supports_zdr: false` in `provider/anthropic.rs`
  with no knob at all.

So every ZDR turn — per-turn toggle OR org-wide switch — currently fails.

**This is correct behaviour, not a bug.** `AZURE_OPENAI_ZDR_CONFIRMED` is an
*operator attestation* that a signed commercial no-retention contract covers the
deployment. Setting it to `true` without that contract would be fabricating a
compliance claim. **Only a human with the contract can flip it.**

### What the UI does about it (shipped 2026-08-01)

Because the gate is real, the product no longer offers a mode that always fails:

- The composer reads `/api/v1/models` (which advertises a `zdr` capability
  per model, derived from `supports_zdr`), and **disables** the temporary-chat
  toggle with an explanation when no model is attested.
- inference-core's `ZdrUnavailable` reason is surfaced to the user as a
  **non-retryable** error, not the old opaque "The ephemeral inference request
  failed".

⚠️ **The org-wide switch is NOT yet gated the same way.** It is shipped and
reachable in workspace settings, and it routes through the same unattested gate —
so an admin who enables Zero Data Retention today breaks chat for their entire
org. Either attest a deployment (§4) or gate that switch behind the same
catalogue check as the composer toggle. **This is the highest-priority follow-up.**

---

## 4. Making ZDR production-ready — the guide

### Step 0 — Obtain the contract (human, not code)
Zero Data Retention is a contractual property of the model deployment. For Azure
OpenAI this is the "no data retention / no human review" configuration
(abuse-monitoring disabled), which Microsoft grants per-subscription on approval.
For Anthropic it is a ZDR agreement on the account. **Nothing below is safe until
this exists in writing.** Record the evidence where §5's release gate can find it.

### Step 1 — Attest the OpenAI path
Set `AZURE_OPENAI_ZDR_CONFIRMED=true` for inference-core **only for the deployment
the contract covers**. `scripts/release-artifact.sh` already gates this behind
`ZDR_RETENTION_PATH=zdr-provider-route-attested` plus signed evidence and a
region/residency match — use that path; do not set the raw env by hand in prod.
Verify: `/api/v1/models` now lists a `zdr` feature on the covered model, and a
per-turn ZDR chat succeeds.

### Step 2 — Add the Anthropic attestation knob
`azure-anthropic` currently hardcodes `supports_zdr: false`. Add a symmetric
`AZURE_ANTHROPIC_ZDR_CONFIRMED` (default `false`) read the same way OpenAI's is,
and wire it into `AnthropicProvider::capabilities`. Ship inference-core rebuilt.
This changes nothing until an operator sets it — it just unblocks the path for
whoever holds an Anthropic ZDR agreement. Without it, ZDR can never route to
Claude even under contract.

### Step 3 — Confirm retention posture flows to the callers
Data Plane services (retrieval/embedding/graph) that call inference-core carry
`MODEL_PLANE_INFERENCE_RETENTION_POSTURE` (`persistent` or `zdr`), which MUST equal
their `retentionByAudience` entry in the Auth Core registry. inference-core treats
the issuer's posture as a **floor**. Keep the env and the registry entry in sync;
a mismatch is a silent downgrade or a hard 503.

### Step 4 — Gate the org-wide switch
Apply the same catalogue check the composer toggle uses to the workspace-settings
Zero Data Retention switch, so an org cannot enable a mode its provider fleet
cannot serve. Until §1–§3 land for at least one deployment, the switch should be
disabled with the same explanation.

### Step 5 — Verify the whole boundary set under a real ZDR turn
With an attested deployment, run one ZDR turn and confirm, live:
- it succeeds and streams an answer;
- `threads`/`messages` gain no row (`SELECT count(*)` before/after);
- the response cache stores nothing (`DBSIZE` unchanged for the `mp:gw:cache:*`
  keyspace);
- no `feedback_ratings` row and no durable audit envelope for that run;
- `/readyz` and the model catalogue report the attested capability honestly.

---

## 5. Where the release process already helps

`scripts/release-artifact.sh:836-868` refuses to build a ZDR-confirmed artifact
unless `ZDR_RETENTION_PATH=zdr-provider-route-attested`, signed evidence is
present, the region/residency matches, and Azure is ordered first in the provider
chain. The attestation workflow therefore **exists** — it has simply never been
exercised because no contract has been recorded. Step 0 is what makes the rest of
this document executable.

---

## 6. Summary

- ZDR **enforcement inside Velion** is complete and verified across seven durable
  boundaries.
- ZDR is **not usable** because no provider deployment is attested, and the gate
  correctly fails closed rather than leaking.
- Closing the gap is a **contract + attestation** task (§4 steps 0–2), not a code
  redesign, plus one real code follow-up (gate the org-wide switch, step 4) and
  one small enabler (the Anthropic knob, step 2).
- **Do not mark ZDR "complete" until at least one deployment is attested and §5
  passes live.** Until then it is "enforcement-ready, provider-unattested".
