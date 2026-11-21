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
    .index("by_space_and_idempotency", ["spaceRef", "idempotencyKey"])
    .index("by_space_and_state", ["spaceRef", "state"]),

  // Leased, at-least-once delivery of immutable deletion intent to Control.
  // The receipt is authorization only; owner-plane purge receipts belong to a
  // later deletion-coordinator protocol. `closed` is terminal: the request is no longer
  // pending authorization, so no claim selects the event (the reason is in lastDeliveryError).
  spaceDeletionAuthorizationEvents: defineTable({
    eventId: v.string(),
    requestId: v.string(),
    deliveryState: v.union(
      v.literal("pending"),
      v.literal("claimed"),
      v.literal("acknowledged"),
      v.literal("failed"),
      v.literal("closed"),
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
  // `blocked_legal_hold` is a typed, non-retryable owner refusal: no claim
  // selects it until the re-drive re-arms it once the hold is lifted (flag
  // SPACE_DELETION_HOLD_RELEASE, `spaces.ts`). `failed_permanent` is a parked
  // delivery: a typed, non-retryable owner refusal (400/401/403/412, parked only behind
  // flag SPACE_DELETION_PERMANENT_REFUSAL_PARK), or a claim that threw past its bound
  // (`delivery_retry_bound_reached`). No claim selects it; the trusted ingress re-arms it
  // with `rearmParkedDeletionOwnerDelivery`. The reason code in `lastDeliveryError` is the
  // only detail stored. `closed` is terminal: the deletion completed, or stopped being live,
  // so no claim selects the row and no owner answer moves it. `lastDeliveryError` holds the
  // closing reason code.
  spaceDeletionOwnerDeliveryEvents: defineTable({
    eventId: v.string(),
    requestId: v.string(),
    ownerPlane: v.union(
      v.literal("application"), v.literal("control"), v.literal("data"),
      v.literal("ingestion"), v.literal("model"), v.literal("infra"),
    ),
    deliveryState: v.union(
      v.literal("pending"), v.literal("claimed"), v.literal("acknowledged"), v.literal("failed"),
      v.literal("blocked_legal_hold"), v.literal("failed_permanent"), v.literal("closed"),
    ),
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
