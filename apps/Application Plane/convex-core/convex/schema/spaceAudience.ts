/**
 * Versioned recipient audiences and the audience-registration outbox.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spaceAudienceTables = {
  // Application owns the current conversation/case recipient set as a
  // versioned product fact. It is not an authorization decision: Control
  // independently verifies every member before any owner plane receives a
  // shared-effect decision. Superseded snapshots remain for visibility-safe
  // replay/fork decisions; their principal IDs never go to the browser.
  spaceRecipientAudiences: defineTable({
    audienceRef: v.string(),
    audienceHash: v.string(),
    controlState: v.union(v.literal("pending"), v.literal("acknowledged"), v.literal("rejected")),
    controlRegisteredAt: v.optional(v.number()),
    externalOrgId: v.string(),
    recipientExternalAuthIds: v.array(v.string()),
    revision: v.number(),
    spaceRef: v.string(),
    state: v.union(v.literal("active"), v.literal("superseded")),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_audience_ref", ["audienceRef"])
    .index("by_space_and_revision", ["spaceRef", "revision"])
    .index("by_space_and_state", ["spaceRef", "state"]),

  // Transactional outbox for Application -> Control audience registration.
  // The event carries no participant IDs; the worker rereads the immutable
  // audience revision only after leasing this row.
  spaceRecipientAudienceEvents: defineTable({
    audienceRef: v.string(),
    deliveryAttempts: v.optional(v.number()),
    deliveryState: v.optional(v.union(
      v.literal("pending"), v.literal("claimed"), v.literal("acknowledged"),
      v.literal("failed"), v.literal("rejected"),
    )),
    eventId: v.string(),
    externalOrgId: v.string(),
    lastDeliveryError: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    leaseOwner: v.optional(v.string()),
    nextAttemptAt: v.optional(v.number()),
    revision: v.number(),
    spaceRef: v.string(),
    createdAt: v.number(),
    deliveredAt: v.optional(v.number()),
  })
    .index("by_event_id", ["eventId"])
    .index("by_delivery_state_and_next_attempt", ["deliveryState", "nextAttemptAt"]),
};
