/**
 * Space deletion intent, authorization events and owner-plane purge receipts.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spaceDeletionTables = {
  // A human deletion request is durable product intent, not a deletion
  // receipt. Control must authorize it against fresh owner/policy/legal-hold
  // facts before Application fences the canonical Space.
  spaceDeletionRequests: defineTable({
    requestId: v.string(),
    idempotencyKey: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    ownerExternalAuthId: v.string(),
    state: v.union(
      v.literal("pending_authorization"),
      v.literal("authorized"),
      v.literal("blocked_legal_hold"),
      v.literal("rejected"),
    ),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_request_id", ["requestId"])
    .index("by_space_and_idempotency", ["spaceRef", "idempotencyKey"]),

  // Leased, at-least-once delivery of immutable deletion intent to Control.
  // The receipt is authorization only; owner-plane purge receipts belong to a
  // later deletion-coordinator protocol.
  spaceDeletionAuthorizationEvents: defineTable({
    eventId: v.string(),
    requestId: v.string(),
    deliveryState: v.union(
      v.literal("pending"),
      v.literal("claimed"),
      v.literal("acknowledged"),
      v.literal("failed"),
    ),
    deliveryAttempts: v.number(),
    leaseOwner: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    nextAttemptAt: v.number(),
    lastDeliveryError: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_request_id", ["requestId"])
    .index("by_delivery_state_and_next_attempt", ["deliveryState", "nextAttemptAt"]),

  // Owner planes publish their own idempotent purge/export result here through
  // a future authenticated coordinator ingress. This is deliberately separate
  // from the authorization event so a timeout cannot be displayed as erased.
  spaceDeletionOwnerReceipts: defineTable({
    requestId: v.string(),
    ownerPlane: v.union(
      v.literal("application"), v.literal("control"), v.literal("data"),
      v.literal("ingestion"), v.literal("model"), v.literal("infra"),
    ),
    status: v.union(
      v.literal("pending"), v.literal("blocked_legal_hold"), v.literal("succeeded"),
      v.literal("partial"), v.literal("failed"), v.literal("unknown"),
    ),
    receiptRef: v.optional(v.string()),
    detail: v.optional(v.string()),
    // Optional only for compatibility with receipts written before deadlines
    // were introduced; every new receipt writes it and the reconciler leaves a
    // legacy row pending for explicit operator migration rather than guessing.
    deadlineAt: v.optional(v.number()),
    updatedAt: v.number(),
  })
    .index("by_request_and_owner", ["requestId", "ownerPlane"])
    .index("by_request", ["requestId"]),

  // A separate leased delivery record for an owner-plane purge adapter. It is
  // not the owner receipt itself: only a parseable owner response can advance
  // the receipt, while a timeout/crash remains retryable and visibly pending.
  spaceDeletionOwnerDeliveryEvents: defineTable({
    eventId: v.string(),
    requestId: v.string(),
    ownerPlane: v.union(
      v.literal("application"), v.literal("control"), v.literal("data"),
      v.literal("ingestion"), v.literal("model"), v.literal("infra"),
    ),
    deliveryState: v.union(v.literal("pending"), v.literal("claimed"), v.literal("acknowledged"), v.literal("failed")),
    deliveryAttempts: v.number(),
    leaseOwner: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    nextAttemptAt: v.number(),
    lastDeliveryError: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_request_and_owner", ["requestId", "ownerPlane"])
    .index("by_owner_and_delivery", ["ownerPlane", "deliveryState", "nextAttemptAt"]),
};
