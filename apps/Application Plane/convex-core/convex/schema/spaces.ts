/**
 * Space identity and lifecycle, agent bindings, lifecycle outbox, member grants, read markers and room presence.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spaceTables = {
  // Application is the canonical owner of a Space's immutable identity, kind,
  // display metadata, and lifecycle. Control registers this reference and is
  // the separate owner of membership and authorization decisions.
  spaces: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    kind: v.union(
      v.literal("personal"),
      v.literal("room"),
      v.literal("project"),
      v.literal("case"),
    ),
    name: v.string(),
    // WS04-S2: a short label shown under the room name. Never composed into a
    // prompt, event or notification (D-WS04-6); optional so existing rooms
    // need no backfill.
    topic: v.optional(v.string()),
    ownerExternalAuthId: v.optional(v.string()),
    createdByExternalAuthId: v.string(),
    // Marks THE organization room — the one durable `room` every active member
    // of an organization belongs to. Explicit rather than derived from
    // `(externalOrgId, kind)`, because team and external Spaces are also
    // `room`: keying idempotency on kind alone would make the first team room
    // collide with the organization room, and the collision would look like a
    // successful reuse rather than an error.
    //
    // Optional and `true`-only, so an ordinary room simply omits it and no
    // backfill is needed for rooms that already exist.
    isOrganizationRoom: v.optional(v.literal(true)),
    lifecycle: v.union(
      v.literal("pending_registration"),
      v.literal("active"),
      v.literal("suspended"),
      v.literal("deleting"),
      v.literal("deleted"),
      v.literal("failed_registration"),
    ),
    lifecycleRevision: v.number(),
    controlResourceRef: v.optional(v.string()),
    // ADR-0003: Space owner/manager-authored instructions, composed into
    // every turn in this Space alongside the platform and org layers. Same
    // "presence, not authority" status as `organizations.instructions` --
    // authored content, never an authorization primitive.
    instructions: v.optional(v.string()),
    updatedByExternalAuthId: v.optional(v.string()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_space_ref", ["spaceRef"])
    .index("by_external_org", ["externalOrgId"])
    .index("by_personal_owner", ["externalOrgId", "kind", "ownerExternalAuthId"]),

  // An agent definition bound to ONE Space, per
  // `docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` §3.2. The plan's central rule is
  // that a definition is not a membership: `agents` says what an agent *is*,
  // this table says where it is allowed to appear as a room participant. A
  // second Space needs a second row, and a page/system install is a different
  // binding entirely — never this one.
  //
  // # This table is presentation and policy, NOT authority
  //
  // Control owns whether an agent may actually reach a Space: it already models
  // that as a `space_memberships` row with `subject_type='service'`. Duplicating
  // that decision here would create two answers to one question, and the stale
  // one would eventually win. So the gateway intersects: Control decides who is
  // bound, this projection supplies the identity to render. A row here whose
  // Control membership is gone must never appear as an active participant.
  spaceAgentBindings: defineTable({
    bindingRef: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    // The definition this binding points at. Kept as a real reference so a
    // deleted definition cannot leave a binding rendering a ghost name.
    agentId: v.id("agents"),
    // The Control-side subject this binding corresponds to, so the gateway can
    // match a binding to the authoritative roster row without guessing by name.
    subjectId: v.string(),
    // Space-local presentation override. Absent means "use the definition's own
    // name/description", which is the common case.
    displayName: v.optional(v.string()),
    title: v.optional(v.string()),
    // Binding policy (space-defenition.md "Binding model"). These are human
    // decisions about how the agent may be invoked here, enforced at the
    // gateway's invocation path. ABSENT means the pre-policy legacy behavior
    // (mention allowed, confirmation required, legacy tool surface) so old
    // bindings keep working exactly as they did. The doc's remaining policy
    // fields (knowledge_scope, default_thread_policy, audit_visibility) are
    // deliberately NOT stored yet: nothing enforces them, and a stored-but-
    // unenforced policy is a false promise.
    triggerModes: v.optional(v.array(v.union(v.literal("mention"), v.literal("group")))),
    allowedTools: v.optional(v.array(v.string())),
    approvalMode: v.optional(
      v.union(v.literal("auto"), v.literal("require_confirmation"), v.literal("blocked")),
    ),
    status: v.union(
      v.literal("pending"),
      v.literal("active"),
      v.literal("paused"),
      v.literal("revoked"),
      v.literal("failed"),
    ),
    // Where this agent's work can be delivered besides the room itself. These
    // are the surfaces the operator has actually configured — an empty list
    // means no channel is published, and the UI must say exactly that rather
    // than implying the agent is reachable somewhere it is not.
    deliveryTargets: v.optional(
      v.array(
        v.object({
          channel: v.union(
            v.literal("teams"),
            v.literal("messenger"),
            v.literal("embed"),
          ),
          // Operator-facing label for the specific destination (a Teams channel
          // name, a Messenger page). Never a token or a secret.
          label: v.string(),
          status: v.union(
            v.literal("active"),
            v.literal("pending"),
            v.literal("failed"),
          ),
        }),
      ),
    ),
    projectionVersion: v.number(),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_binding_ref", ["bindingRef"])
    .index("by_space_ref", ["spaceRef"])
    .index("by_space_and_subject", ["spaceRef", "subjectId"])
    // ADR-0002 (apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md): the
    // org-wide registry (`spaceAgents:agentInstallationsForOrgForGateway`)
    // needs every binding for an org in one indexed read, the same shape
    // `spaces.by_external_org` already gives `spacesForOrgForGateway`.
    // Additive and backward-compatible: computed from the `externalOrgId`
    // every row already carries, no data migration required.
    .index("by_external_org", ["externalOrgId"]),

  // Transactional outbox for Space lifecycle notifications. Consumers dedupe
  // by the immutable event ID and never infer authorization from this event.
  spaceLifecycleEvents: defineTable({
    eventId: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    lifecycle: v.union(
      v.literal("pending_registration"),
      v.literal("active"),
      v.literal("suspended"),
      v.literal("deleting"),
      v.literal("deleted"),
      v.literal("failed_registration"),
    ),
    revision: v.number(),
    // Delivery is an at-least-once outbox. The Control endpoint deduplicates
    // stable event IDs; a claim lease prevents concurrent workers from
    // delivering the same row in the ordinary case.
    deliveryState: v.optional(v.union(
      v.literal("pending"),
      v.literal("claimed"),
      v.literal("acknowledged"),
      v.literal("failed"),
      v.literal("rejected"),
    )),
    deliveryAttempts: v.optional(v.number()),
    leaseOwner: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    nextAttemptAt: v.optional(v.number()),
    deliveredAt: v.optional(v.number()),
    lastDeliveryError: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_space_and_revision", ["spaceRef", "revision"])
    .index("by_delivery_state_and_next_attempt", ["deliveryState", "nextAttemptAt"]),

  // Who a NAMED room's people are, as decided by its owners and managers.
  //
  // Only for rooms somebody created and populated by hand. The organization
  // room deliberately has no rows here: its roster is derived from org-core by
  // `spaceMembershipSync`, on the rule that being in the organization is what
  // grants a place in its room. Two sources for one room's people would fight,
  // and the derived one would win every sync.
  //
  // This is an INTENT record, not an authority. Control owns membership; this
  // table is what Application declares to it, and a row here means nothing
  // until that declaration is accepted.
  spaceMemberGrants: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    // The person's auth identity, the same subject Control stores in
    // `space_memberships.subject_id` for a user.
    externalAuthId: v.string(),
    // Deliberately narrow: this flow grants participation, not the ability to
    // hand out more of it. Promoting someone to manage a room is a separate
    // decision that does not exist yet, and inventing it here would let anyone
    // who can add a person also create another grantor.
    role: v.literal("editor"),
    grantedByExternalAuthId: v.string(),
    createdAt: v.number(),
  })
    .index("by_space_ref", ["spaceRef"])
    .index("by_space_and_subject", ["spaceRef", "externalAuthId"]),

  // Where a member last caught up with a room (item 4b, "unread per thread").
  // One row per member per Space: opening the room is reading it, and "new"
  // means "since you last had it open" — Slack's channel model. Stores a
  // timestamp against identifiers only, never a thread list and never content,
  // so it records WHEN someone caught up and nothing about WHAT they read.
  // Threads live in Model Plane and membership in Control; what a person has
  // seen is a workspace projection, which is this plane's to own.
  spaceReadMarkers: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    lastReadAt: v.number(),
  })
    .index("by_space_and_subject", ["spaceRef", "externalAuthId"]),

  /**
   * Who is in a room right now (see `convex/spacePresence.ts`). Separate from
   * the dormant `conversationPresence` below, which is keyed per conversation:
   * a room's presence is a fact about the room, not about one of its threads.
   * Stores a status and a timestamp against identifiers — never content.
   */
  spacePresence: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    status: v.union(v.literal("online"), v.literal("typing"), v.literal("offline")),
    updatedAt: v.number(),
  })
    .index("by_space", ["spaceRef"])
    .index("by_space_and_subject", ["spaceRef", "externalAuthId"]),

  // WS04-S1: one row per organization with a debounced room-roster sync queued
  // or running (see `convex/spaceMembershipSync.ts`). Holds an org id and a
  // timestamp only; the row is deleted when the run finishes.
  spaceOrgRoomSyncRequests: defineTable({
    externalOrgId: v.string(),
    dueAt: v.number(),
    state: v.union(v.literal("scheduled"), v.literal("running")),
  })
    .index("by_external_org", ["externalOrgId"]),
};
