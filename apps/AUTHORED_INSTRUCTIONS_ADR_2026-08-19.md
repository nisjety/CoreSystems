# ADR-0003: Authored instruction hierarchy (platform + org + Space, composed with agent persona)

**Date**: 2026-08-19
**Status**: proposed
**Deciders**: CoreSystem product direction (Application/Frontend/Model implementation owner)
**Supersedes nothing; extends** `apps/SPACE_AUTHORITY_ADR_2026-08-13.md` (ADR-0001)
and `apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md` (ADR-0002) — this decision
assumes both as given and does not revisit them.

## Context

`apps/Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`'s
capability comparison names this gap directly:

> **Authored instructions** | Org and lower-scope instructions resolved
> hierarchically | Backlog recognizes explicit user/org/project instructions;
> not yet one editable hierarchy | **QM lead. P1.** Platform → org →
> personal/Space, with lower layers unable to weaken policy.

Investigated directly against current source (not the doc's own framing) to
confirm what exists and what QM's reference implementation actually does:

**QM's real mechanism** (`qm/src/resolution/resolution-service.ts:47-68`, the
local reference clone) is narrower than "platform → org → personal/Space"
implies: exactly **two** layers — org (`orgSoul`) and the resolved scope
(`scopeSoul`, which QM's own scope model folds personal/group/channel into
one tier) — concatenated into one system-prompt string. Enforcement that
"lower layers can't weaken policy" is **not code-enforced**: it is a
prompt-framing technique — the lower-scope text is wrapped with literal
instructions to the model ("may add to, but MUST NOT override, the
organization policy above") and a trailing reassertion that the org policy
is authoritative. There is no structural/cryptographic non-override
guarantee anywhere in QM's resolver.

**CoreSystem's current state**, confirmed by direct read:

- `agents.systemPrompt` (Convex, `apps/Application Plane/convex-core/convex/schema.ts:328`)
  is the only instruction-text field that exists anywhere in the codebase.
  It is a per-**agent-definition** field, not a per-org or per-Space one.
- `organizations` (`schema.ts:13-36`) and `spaces` (`schema.ts:527-560`) have
  **zero** instruction/prompt fields today — confirmed by reading both table
  definitions in full and by a repo-wide grep for any org-level or
  organization-wide instruction concept (zero hits).
- Injection only happens for a **mentioned, Space-bound agent**:
  `inject_mentioned_space_agent_persona` (gateway,
  `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:2094-2172`)
  resolves the binding, calls Convex's `agentPersonaForGateway`
  (`spaceAgents.ts:283-307`) for `{name, systemPrompt}`, and stamps
  `agent_name`/`agent_system_prompt` onto the outbound turn.
  Model-gateway's `agent_persona_message`
  (`apps/Model Plane/rust/services/model-gateway/src/sse.rs:3081-3101`) turns
  that into one system message: *"You are currently answering as {name}...
  {name}'s own instructions, which take precedence over Verevon's default
  behavior for this turn: {instructions}"*.
- **For every other turn — no mentioned agent, a personal Space, an
  unbound room — zero instruction system-message is injected at all.**
  There is no default/base system prompt today, org-level or otherwise.
- The codebase's own established convention (`http_routes.rs:5880-5885`,
  cited in the mention-persona work) already treats agent instruction text
  as *"framing text only... not a security boundary."* This matches QM's
  own actual (non-code-enforced) trust model — adopting the same
  prompt-framing technique for org/Space layers is consistent with existing
  practice, not a new trust primitive.
- Existing precedent for **not** over-building: `spaceAgentBindings`'s own
  schema comment (`schema.ts`, next to `triggerModes`/`allowedTools`) states
  plainly that `knowledge_scope`/`default_thread_policy`/`audit_visibility`
  are "deliberately NOT stored yet: nothing enforces them, and a
  stored-but-unenforced policy is a false promise." This ADR follows the
  same discipline: no field ships without its enforcement point in the same
  change.

## Decision

Add **three** new instruction layers — **platform**, **org**, and
**Space** — composed with the existing per-agent `systemPrompt` into one
system message, injected on **every** chat turn (not gated on a mention).
This matches the roadmap bullet's literal "platform → org → personal/Space"
wording; QM's own code only implements the bottom two of these four tiers
(no platform layer, and no separate agent-persona layer — see Context), so
platform and per-agent are CoreSystem-specific extensions of the same
composition idea, not things being ported from QM's source.

### Storage — three different mechanisms, deliberately not one shape

The three new layers are **not** symmetric, because the authority answering
"who gets to change this" is different at each tier, and forcing all three
into the same Convex-table-plus-role-check shape would either invent an
authority tier that doesn't exist (a platform-operator role, distinct from
org-admin) or give every org self-service control over what should be a
CoreSystem-wide constant.

```
# Platform — deployment configuration, not a database row
PLATFORM_SYSTEM_INSTRUCTIONS (env var, model-gateway) : string, optional

# Org — Convex, org-admin authored
organizations.instructions?: string

# Space — Convex, Space owner/manager authored
spaces.instructions?: string
```

**Platform** is a deployment-level environment variable read once at
model-gateway startup, not a database-backed, per-request value. No new
"platform operator" role or authentication tier exists anywhere in this
codebase today (roles stop at org-admin/org-member); inventing one to
gate a database row that in practice has exactly one legitimate editor
(whoever operates the deployment) is authority-model scope this ADR does
not take on. A platform instruction also changes far less often than an
org's or a Space's own — deploy-time configuration is the right cost/
flexibility tradeoff, not a missing feature. If CoreSystem later becomes a
genuine multi-operator platform (each operator running its own instance
with its own instructions), that is already how this works today: each
deployment sets its own env var. The gap that would require a *database*
value is a single CoreSystem process serving multiple independent
*platform* operators from one deployment — a materially different product
shape with no current evidence it is needed, and its own future ADR if it
ever is.

**Org and Space** remain Convex fields exactly as originally scoped: plain
optional strings, no versioning/revision history in this slice (see
Alternatives for why, and what a follow-up would add). `updatedAt`/
`updatedByExternalAuthId` accompany each for basic audit, matching the
existing pattern on `spaceAgentBindings`.

### Composition and enforcement

One new pure function (model-gateway, since that is where the platform
env var is read and where `agent_persona_message` already lives) composes,
in order: platform instructions → org instructions → Space instructions →
agent-specific instructions (existing, only when a mention resolves a
binding — unchanged from today). Each added layer is wrapped in the
QM-style framing:

```
{platform instructions, if any}

--- Organization instructions (may add to, but must not override, the
platform instructions above) ---
{org instructions, if any}

--- Space instructions (may add to, but must not override, the platform or
organization instructions above) ---
{space instructions, if any}

--- {agent_name}'s own instructions, which take precedence for this turn
only over the general behavior above, never over the platform,
organization, or Space instructions ---
{agent-specific systemPrompt, if any}
```

This is **prompt-level framing, not a code-enforced boundary** — consistent
with this codebase's own existing treatment of agent instruction text, and
with QM's real mechanism. A future, separately-decided change could add a
structural check (e.g., a policy linter flagging a Space instruction that
contains override-shaped language) but that is out of scope here.

The composed message replaces `agent_persona_message`'s current
mention-only firing: the new function fires on every turn where the caller
carries a resolved `org_id` (personal chat included) and additionally a
`space_ref` (Space-scoped chat), producing `None` only when every layer is
empty (today's silent-no-message behavior, preserved for a deployment/org/
Space that has authored nothing).

### Authoring permissions

- **Platform instructions**: set via deployment configuration
  (`PLATFORM_SYSTEM_INSTRUCTIONS`), not through any in-product UI or API —
  no org-facing authoring surface for this tier at all.
- **Org instructions**: gated the same way as the existing org-admin-only
  skill-authoring surface (`hasWorkspaceAdminAccess`, mirroring
  `SkillsSection.tsx`'s existing gate) — one org-wide value, one owner.
- **Space instructions**: gated to `editor`/`manager`/`owner` roles on that
  Space, mirroring `ValidateForSharedThread`'s existing role floor
  (ADR-0001) — not `viewer`, since this is a durable write that affects
  every future turn in the Space, not a read.

### Presence, not authority (mirrors ADR-0002)

Neither layer is a Control-issued, revision-fenced authority decision the
way ADR-0001's thread/retrieval decisions are. This is authored *content*,
not an authorization primitive — its enforcement is "the model was told,"
identical in kind to the existing per-agent `systemPrompt`. It must never
be read as granting or restricting a capability; capability enforcement
(retrieval scope, tool access, approval mode) remains entirely with the
existing Control/capability-core mechanisms this ADR does not touch.

## Alternatives considered

### Two layers only (org + Space, no platform tier)

**Pros**: matches QM's actual source mechanism exactly (which has no
platform tier); smaller slice; no need to design where a
deployment-wide value lives.
**Cons**: doesn't match the roadmap bullet's literal "platform → org →
personal/Space" wording, which explicitly calls for a CoreSystem-operator
layer above any customer org's own instructions (e.g., a baseline
compliance disclaimer every org's instructions build on top of, never
override).
**Why not chosen**: confirmed decision below is 3 layers. Recorded here
because it is the more QM-literal alternative, in case a future review
wants the smaller slice instead.

### Platform layer as a database-backed, admin-editable value

**Pros**: changeable without a deployment/redeploy; auditable via the same
`updatedAt`/`updatedBy` pattern as org/Space; symmetric with the other two
layers (one code path handles all three).
**Cons**: requires inventing a genuinely new authority tier — a
"platform operator" role distinct from org-admin — that exists nowhere
else in this codebase; that role would need its own authentication path,
since no current session/JWT claim represents "operates this deployment"
as opposed to "administers this org"; for the realistic near-term
deployment shape (one operator, one instance), the *only* consumer of
this authority would be the same person who already controls the
deployment's environment variables, making the new role pure ceremony
around a distinction with no current practical difference.
**Why not chosen**: env-var configuration achieves the identical practical
outcome (the deployment operator controls the platform instruction) with
zero new authority surface. If CoreSystem later serves genuinely
independent platform operators from one shared deployment — a materially
different product shape than exists today — this becomes the right
design, and deserves its own ADR when that shape is real, not a
speculative role added now.

### Org layer only for v1 (defer Space-level instructions)

**Pros**: smaller slice; "who can author this" is simpler with one owner
(org admin) instead of two (org admin, and every Space's own
editor/manager/owner); avoids a second Convex field + a second permission
gate in the same change.
**Cons**: doesn't actually close the roadmap gap — QM's coherence advantage
specifically comes from the scope-level layer (a room's own standing
context), not the org layer alone; CoreSystem already has richer
Space-level product surface (bindings, roles) than QM's flatter scope
model, so omitting the Space layer wastes an advantage CoreSystem already
has the authority model for.
**Why not**: the Space-level permission gate is not a new design — it
reuses ADR-0001's existing role floor verbatim. The marginal cost of
including it now is small; the marginal product value is the entire point
of the QM comparison's "P1" ranking.

### Versioned/revisioned instructions from day one

**Pros**: matches QM's "learned memory" and Verevon's own "authored/
revisioned memory" ambition elsewhere in the roadmap; auditable change
history; supports a future "who changed this and why" surface.
**Cons**: real added scope (a history table, a diff/rollback UI, a
retention policy for old revisions) with no concrete consumer identified
yet — the roadmap's own gap statement is about the *hierarchy and
composition* being missing, not about versioning specifically.
**Why not**: defer, per this codebase's own "don't build for hypothetical
future requirements" discipline. `updatedAt`/`updatedByExternalAuthId`
alone answers "when and by whom did this last change," which is sufficient
until a real versioning consumer is scoped.

## Consequences

**Positive:**
- Closes a named, real, currently-P1 gap, matching the roadmap's literal
  three-tier wording: one env var, two optional Convex string fields, one
  composition function, one call-site change (replacing a mention-only
  firing with an unconditional one).
- Reuses every permission/role concept that already exists for the two
  in-product tiers (org-admin gate, ADR-0001's Space role floor); the
  platform tier deliberately reuses no in-product authority at all, since
  it needs none.
- Consistent with the codebase's existing "instructions are framing text,
  not a security boundary" convention — no new trust model introduced.
- Avoids inventing a "platform operator" role that would otherwise be pure
  ceremony for the deployment's own operator, the only realistic near-term
  editor of that tier.

**Negative:**
- The composed system message grows by up to three more segments; token-
  budget impact on every turn (previously zero for the no-mention case)
  needs a release-gate check (see below) rather than being assumed free.
- "Must not override" is advisory (prompt framing) at every tier, identical
  in strength to today's per-agent instructions — an adversarial or
  careless author at any of the three layers could still attempt to
  instruct the model to ignore a higher one, exactly as an agent's own
  `systemPrompt` already could today. Not a new risk this ADR introduces,
  but not one it closes either.
- The platform tier is not adjustable without deployment access (no
  in-product UI by design) — a deliberate tradeoff, but worth stating
  plainly: an org-admin cannot see or influence what the platform tier
  says, only that it exists and is authoritative above their own.

## Risks and release gates

- Verify the composed message's token cost against `DEFAULT_CONTEXT_BUDGET_TOKENS`
  and existing context-assembly budgets before enabling unconditional
  (every-turn) firing in production — this is new standing cost the
  mention-gated design never had.
- The org-instructions authoring surface and the Space-instructions
  authoring surface are both **new UI** (neither exists today, per
  `SkillsSection.tsx`'s own confirmed lack of scope UI for the analogous
  skills gap) — implementation must include both, or the fields are
  write-only from a direct API call with no real authoring path, repeating
  the SKILL-1 "schema shipped, no way to use it" pattern this session
  already found and fixed once. The platform tier needs no UI by design
  (see Decision) — do not build one speculatively.
- No migration needed (the env var and both Convex fields are new,
  optional, additive) — existing orgs/Spaces with nothing authored, and
  any deployment that leaves `PLATFORM_SYSTEM_INSTRUCTIONS` unset, produce
  identical behavior to today (silent, no instruction message) aside from
  the (already-existing) per-agent mention case.
