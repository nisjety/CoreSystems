/**
 * Organizations, users, authority tombstones, reconciliation nonces and Control session projection.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const identityTables = {
  // Organizations - Multi-tenant support
  organizations: defineTable({
    // External sync fields
    externalOrgId: v.string(),  // ID from auth-core/org-core
    
    name: v.string(),
    slug: v.string(),
    settings: v.optional(v.object({
      maxTokens: v.optional(v.number()),
      defaultModel: v.optional(v.string()),
      // Deprecated by ADR-0005: no Convex function reads this field. Model
      // entitlement is org-core's policy, enforced by inference-core. The field
      // stays so stored documents keep validating; removing it needs a data
      // migration first.
      allowedModels: v.optional(v.array(v.string())),
    })),
    
    // Sync tracking
    syncStatus: v.union(v.literal("syncing"), v.literal("synced"), v.literal("deleted")),
    lastSyncedAt: v.number(),
    sourceRevision: v.optional(v.number()),
    sourceEventId: v.optional(v.string()),
    sourceFingerprint: v.optional(v.string()),

    // ADR-0003: org-admin-authored instructions, composed into every chat
    // turn's system message alongside the platform and Space layers. Plain
    // optional content, not an authority decision (see the ADR's "Presence,
    // not authority" section) -- no versioning in this slice.
    instructions: v.optional(v.string()),
    updatedByExternalAuthId: v.optional(v.string()),

    createdAt: v.number(),
    updatedAt: v.number(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_slug", ["slug"])
    .index("by_external_id", ["externalOrgId"]),

  organizationTombstones: defineTable({
    externalOrgId: v.string(),
    sourceRevision: v.number(),
    sourceEventId: v.string(),
    sourceFingerprint: v.string(),
    removedAt: v.number(),
  }).index("by_external_org", ["externalOrgId"]),

  // Users - Connected to auth system
  users: defineTable({
    // External sync fields
    externalAuthId: v.string(),  // ID from auth-core
    
    email: v.string(),
    name: v.optional(v.string()),
    orgId: v.id("organizations"),
    role: v.union(v.literal("admin"), v.literal("member"), v.literal("viewer")),
    
    // Sync tracking
    syncStatus: v.union(v.literal("syncing"), v.literal("synced"), v.literal("deleted")),
    lastSyncedAt: v.number(),
    sourceUpdatedAt: v.optional(v.number()),
    sourceRevision: v.optional(v.number()),
    sourceEventId: v.optional(v.string()),
    sourceFingerprint: v.optional(v.string()),

    createdAt: v.number(),
    lastSeenAt: v.number(),
    deletedAt: v.optional(v.number()),

    // Persona: per-user always-on custom instructions, composed into every
    // chat turn's system message as the layer beneath the platform/org/Space
    // authored-instruction hierarchy (ADR-0003) and beneath an explicitly
    // summoned Space agent's own persona. Self-owned -- no admin/membership
    // gate, mirrors `organizations.instructions` above but keyed by the
    // user's own row instead of the org's. `personaEnabled` undefined/absent
    // means enabled (same "absence == default true" convention the org layer
    // does not need but persona's per-thread override does).
    personaInstructions: v.optional(v.string()),
    personaEnabled: v.optional(v.boolean()),
    personaUpdatedAt: v.optional(v.number()),
  })
    .index("by_external_auth_id", ["externalAuthId"])
    .index("by_org", ["orgId"])
    .index("by_email", ["email"])
    .index("by_external_and_org", ["externalAuthId", "orgId"]),

  // Authority tombstones prevent a delayed member-added event from
  // resurrecting access after Control Plane has removed the membership.
  membershipTombstones: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    sourceUpdatedAt: v.number(),
    sourceRevision: v.optional(v.number()),
    sourceEventId: v.optional(v.string()),
    sourceFingerprint: v.optional(v.string()),
    removedAt: v.number(),
  })
    .index("by_external_org_and_user", ["externalOrgId", "externalAuthId"])
    .index("by_external_org", ["externalOrgId"]),

  reconciliationNonces: defineTable({
    nonce: v.string(),
    externalOrgId: v.string(),
    requestTimestamp: v.number(),
    createdAt: v.number(),
    expiresAt: v.number(),
  })
    .index("by_nonce", ["nonce"])
    .index("by_expires_at", ["expiresAt"]),

  // G35: controlSessions — projection of the Control Session aggregate
  // produced by CP session-core (ADR 0002). One row per
  // (externalUserId, externalOrgId) pair. session-core's
  // `ControlSessionService.Refresh` (and any future writer) calls the
  // `upsertControlSession` HTTP action whenever the aggregate is refreshed.
  // Verevon subscribes via `useQuery(api.controlSessions.byUser, ...)` for
  // reactive plan / entitlement / billing UI without polling.
  //
  // The full snapshot is stored as a single JSONB-style `snapshot` field so
  // the schema doesn't have to track every upstream shape change (the
  // contract is the JSON, not the Convex types). `index by_external_user`
  // is the verevon subscription target; `by_external_user_and_org` covers
  // org-switch flows where the active org changes within a user.
  controlSessions: defineTable({
    externalUserId: v.string(),
    externalOrgId: v.optional(v.string()),
    snapshot: v.any(),
    fetchedAt: v.number(),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_external_user", ["externalUserId"])
    .index("by_external_user_and_org", ["externalUserId", "externalOrgId"]),
};
