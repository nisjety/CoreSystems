import { v } from "convex/values";
import { internalMutation, mutation, query } from "./_generated/server";
import { internal } from "./_generated/api";
import { assertServiceKey, requireViewerMembership } from "./authz";
import {
  acknowledgeSpaceLifecycleDelivery,
  canClaimSpaceLifecycleDelivery,
  claimSpaceLifecycleDelivery,
  createSpaceLifecycleEvent,
  releaseSpaceLifecycleDelivery,
  transitionSpaceLifecycle,
  type SpaceLifecycleDelivery,
  type SpaceLifecycle,
} from "./spaceLifecycle";
import {
  canonicalRecipientSubjectIds,
  recipientAudienceHash,
  spaceRecipientAudienceRef,
} from "./spaceAudience";
import {
  SPACE_DELETION_OWNERS,
  aggregateDeletionReceipts,
  assertReceiptTransition,
  type SpaceDeletionOwner,
  type SpaceDeletionOwnerStatus,
} from "./spaceDeletionReceipts";

const lifecycleValidator = v.union(
  v.literal("pending_registration"),
  v.literal("active"),
  v.literal("suspended"),
  v.literal("deleting"),
  v.literal("deleted"),
  v.literal("failed_registration"),
);
const SPACE_DELETION_OWNER_DEADLINE_MS = 7 * 24 * 60 * 60 * 1000;

async function getSpaceByRef(ctx: any, spaceRef: string) {
  const spaces = await ctx.db
    .query("spaces")
    .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", spaceRef))
    .collect();
  if (spaces.length > 1) throw new Error("Space reference is ambiguous");
  return spaces[0] ?? null;
}

async function activeRecipientAudience(ctx: any, spaceRef: string) {
  const audiences = await ctx.db
    .query("spaceRecipientAudiences")
    .withIndex("by_space_and_state", (q: any) => q.eq("spaceRef", spaceRef).eq("state", "active"))
    .collect();
  if (audiences.length > 1) throw new Error("active recipient audience is ambiguous");
  return audiences[0] ?? null;
}

function recipientAudienceEventId(spaceRef: string, revision: number): string {
  return `space:${spaceRef}:recipient-audience:${revision}:register`;
}

function deletionOwnerDeliveryEventId(requestId: string, ownerPlane: SpaceDeletionOwner): string {
  return `space-deletion:${requestId}:owner:${ownerPlane}`;
}

async function appendLifecycleEvent(
  ctx: any,
  space: any,
  now: number,
  deliveryState: "pending" | "rejected" = "pending",
) {
  const event = createSpaceLifecycleEvent({
    externalOrgId: space.externalOrgId,
    lifecycle: space.lifecycle,
    revision: space.lifecycleRevision,
    spaceRef: space.spaceRef,
  });
  const existing = await ctx.db
    .query("spaceLifecycleEvents")
    .withIndex("by_event_id", (q: any) => q.eq("eventId", event.eventId))
    .first();
  if (!existing) {
    await ctx.db.insert("spaceLifecycleEvents", {
      ...event,
      createdAt: now,
      deliveryAttempts: 0,
      deliveryState,
      nextAttemptAt: now,
      lastDeliveryError: deliveryState === "rejected" ? "Control rejected immutable Space registration" : undefined,
    });
    // The worker is durable/retryable; a failed or missing Control endpoint
    // leaves the Space pending instead of activating it optimistically.
    if (deliveryState === "pending") {
      await ctx.scheduler.runAfter(0, internal.spaceRegistration.deliverOne, {
        workerId: `space-registration:${event.eventId}`,
      });
    }
  }
  return event;
}

function deliveryFor(event: any): SpaceLifecycleDelivery {
  return {
    attempts: event.deliveryAttempts ?? 0,
    leaseExpiresAt: event.leaseExpiresAt,
    leaseOwner: event.leaseOwner,
    nextAttemptAt: event.nextAttemptAt ?? event.createdAt,
    state: event.deliveryState ?? "pending",
  };
}

function patchForDelivery(delivery: SpaceLifecycleDelivery) {
  return {
    deliveryAttempts: delivery.attempts,
    deliveryState: delivery.state,
    leaseExpiresAt: delivery.leaseExpiresAt,
    leaseOwner: delivery.leaseOwner,
    nextAttemptAt: delivery.nextAttemptAt,
  };
}

function assertDeliveryWorker(workerId: string): void {
  if (!workerId.trim()) throw new Error("Space delivery worker identity is required");
}

function deletionRequestId(spaceRef: string, idempotencyKey: string): string {
  const normalized = idempotencyKey.trim();
  if (!normalized || normalized.length > 160) throw new Error("deletion idempotency key is invalid");
  return `space:${spaceRef}:deletion:${normalized}`;
}

function deletionAuthorizationEventId(requestId: string): string {
  return `${requestId}:authorize`;
}

/**
 * Returns the one personal Space for the signed-in member in an organization.
 * A mutation is serialized by Convex, so the indexed lookup plus insert gives
 * the required one-per-(organization, principal) invariant.
 */
export const ensurePersonalSpace = mutation({
  args: { externalOrgId: v.string(), name: v.optional(v.string()) },
  handler: async (ctx, args) => {
    const viewer = await requireViewerMembership(ctx, args.externalOrgId);
    const existing = await ctx.db
      .query("spaces")
      .withIndex("by_personal_owner", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId)
          .eq("kind", "personal")
          .eq("ownerExternalAuthId", viewer.externalAuthId),
      )
      .collect();
    if (existing.length > 1) throw new Error("personal Space invariant violated");
    if (existing[0]) return existing[0];

    const now = Date.now();
    const recordId = await ctx.db.insert("spaces", {
      spaceRef: "pending",
      externalOrgId: args.externalOrgId,
      kind: "personal",
      name: args.name?.trim() || "Personal Space",
      ownerExternalAuthId: viewer.externalAuthId,
      createdByExternalAuthId: viewer.externalAuthId,
      lifecycle: "pending_registration",
      lifecycleRevision: 1,
      createdAt: now,
      updatedAt: now,
    });
    const spaceRef = String(recordId);
    await ctx.db.patch(recordId, { spaceRef });
    const space = await ctx.db.get(recordId);
    if (!space) throw new Error("Space creation failed");
    const event = await appendLifecycleEvent(ctx, space, now);
    return { ...space, lifecycleEvent: event };
  },
});

/**
 * Resolves the acting member for a service-key call, applying the same two
 * checks the `*ForGateway` reads already apply: the organization must exist and
 * not be deleted, and the caller must be a non-deleted member of it.
 *
 * The service key proves the CALLER is the gateway. It says nothing about which
 * user the gateway is acting for, so that must still be established from the
 * data — a key alone must never be enough to act as an arbitrary user.
 *
 * Extracted rather than inlined a sixth time: this file already repeats these
 * two lookups five times. Those five are left alone deliberately (they are
 * authority code another workstream is actively changing, and consolidating
 * them is a separate change), but a new copy of an authorization check is not
 * something to add.
 */
async function requireGatewayMember(ctx: any, externalAuthId: string, externalOrgId: string) {
  const organizations = await ctx.db
    .query("organizations")
    .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", externalOrgId))
    .collect();
  const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
  if (!organization) throw new Error("Organization not found");
  const members = await ctx.db
    .query("users")
    .withIndex("by_external_and_org", (q: any) =>
      q.eq("externalAuthId", externalAuthId).eq("orgId", organization._id),
    )
    .collect();
  if (!members.some((candidate: any) => candidate.syncStatus !== "deleted")) {
    throw new Error("Unauthorized");
  }
  return organization;
}

/**
 * Service-key counterpart of `ensurePersonalSpace`, so the BFF can provision a
 * caller's own personal Space. Convex identity is unavailable on that path —
 * the gateway holds a Control session, not a Convex one — which is why the
 * viewer is resolved from the arguments after the membership check above.
 *
 * Idempotent by the same rule as the identity version: an owner has at most one
 * personal Space, so a repeat call returns the existing record rather than
 * creating a second. Two would trip the `personal Space invariant violated`
 * guard every read applies.
 *
 * The new Space is `pending_registration`, NOT active. Control registers it
 * afterwards through `applyControlRegistration`; this mutation deliberately
 * cannot shortcut that, because a room that exists is not yet a room Control
 * has authorized.
 */
export const ensurePersonalSpaceForGateway = mutation({
  args: {
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    serviceKey: v.string(),
    name: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);

    const existing = await ctx.db
      .query("spaces")
      .withIndex("by_personal_owner", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId)
          .eq("kind", "personal")
          .eq("ownerExternalAuthId", args.externalAuthId),
      )
      .collect();
    if (existing.length > 1) throw new Error("personal Space invariant violated");
    if (existing[0]) return existing[0];

    const now = Date.now();
    const recordId = await ctx.db.insert("spaces", {
      spaceRef: "pending",
      externalOrgId: args.externalOrgId,
      kind: "personal",
      name: args.name?.trim() || "Personal Space",
      ownerExternalAuthId: args.externalAuthId,
      createdByExternalAuthId: args.externalAuthId,
      lifecycle: "pending_registration",
      lifecycleRevision: 1,
      createdAt: now,
      updatedAt: now,
    });
    const spaceRef = String(recordId);
    await ctx.db.patch(recordId, { spaceRef });
    const space = await ctx.db.get(recordId);
    if (!space) throw new Error("Space creation failed");
    const event = await appendLifecycleEvent(ctx, space, now);
    return { ...space, lifecycleEvent: event };
  },
});

export const getPersonalSpace = query({
  args: { externalOrgId: v.string() },
  handler: async (ctx, args) => {
    const viewer = await requireViewerMembership(ctx, args.externalOrgId);
    const spaces = await ctx.db
      .query("spaces")
      .withIndex("by_personal_owner", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId)
          .eq("kind", "personal")
          .eq("ownerExternalAuthId", viewer.externalAuthId),
      )
      .collect();
    if (spaces.length > 1) throw new Error("personal Space invariant violated");
    return spaces[0] ?? null;
  },
});

/**
 * Records an authenticated personal-Space deletion request. It deliberately
 * does not move the Space to `deleting`: only a fresh Control authorization
 * can do that after checking current owner, deletion entitlement, and legal
 * hold state. Replaying the same idempotency key returns the original intent.
 */
export const requestPersonalSpaceDeletion = mutation({
  args: { externalOrgId: v.string(), idempotencyKey: v.string() },
  handler: async (ctx, args) => {
    const viewer = await requireViewerMembership(ctx, args.externalOrgId);
    const spaces = await ctx.db
      .query("spaces")
      .withIndex("by_personal_owner", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId)
          .eq("kind", "personal")
          .eq("ownerExternalAuthId", viewer.externalAuthId),
      )
      .collect();
    if (spaces.length !== 1) throw new Error("personal Space is unavailable for deletion");
    const space = spaces[0];
    if (space.lifecycle !== "active" && space.lifecycle !== "suspended") {
      throw new Error("personal Space is not deletable in its current lifecycle state");
    }
    const idempotencyKey = args.idempotencyKey.trim();
    const existing = await ctx.db
      .query("spaceDeletionRequests")
      .withIndex("by_space_and_idempotency", (q: any) =>
        q.eq("spaceRef", space.spaceRef).eq("idempotencyKey", idempotencyKey),
      )
      .first();
    if (existing) return existing;

    const now = Date.now();
    const requestId = deletionRequestId(space.spaceRef, idempotencyKey);
    await ctx.db.insert("spaceDeletionRequests", {
      requestId,
      idempotencyKey,
      spaceRef: space.spaceRef,
      externalOrgId: space.externalOrgId,
      ownerExternalAuthId: viewer.externalAuthId,
      state: "pending_authorization",
      createdAt: now,
      updatedAt: now,
    });
    const eventId = deletionAuthorizationEventId(requestId);
    await ctx.db.insert("spaceDeletionAuthorizationEvents", {
      eventId,
      requestId,
      deliveryState: "pending",
      deliveryAttempts: 0,
      nextAttemptAt: now,
      createdAt: now,
    });
    await ctx.scheduler.runAfter(0, internal.spaceDeletionAuthorization.deliverOne, {
      workerId: `space-deletion-authorization:${eventId}:${crypto.randomUUID()}`,
    });
    return {
      requestId,
      idempotencyKey,
      spaceRef: space.spaceRef,
      externalOrgId: space.externalOrgId,
      ownerExternalAuthId: viewer.externalAuthId,
      state: "pending_authorization" as const,
      createdAt: now,
      updatedAt: now,
    };
  },
});

/** BFF-only counterpart to the browser mutation. The service key authenticates
 * the gateway, while this handler independently verifies its supplied
 * `(externalAuthId, externalOrgId)` against the Application projection and
 * the exact personal-Space owner before recording intent. */
export const requestPersonalSpaceDeletionForGateway = mutation({
  args: {
    externalAuthId: v.string(), externalOrgId: v.string(), serviceKey: v.string(),
    spaceRef: v.string(), idempotencyKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organizations = await ctx.db.query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", args.externalOrgId)).collect();
    const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
    if (!organization) throw new Error("Organization not found");
    const members = await ctx.db.query("users")
      .withIndex("by_external_and_org", (q: any) => q.eq("externalAuthId", args.externalAuthId).eq("orgId", organization._id)).collect();
    if (!members.some((candidate: any) => candidate.syncStatus !== "deleted")) throw new Error("Unauthorized");
    const space = await getSpaceByRef(ctx, args.spaceRef);
    if (!space || space.externalOrgId !== args.externalOrgId || space.kind !== "personal" || space.ownerExternalAuthId !== args.externalAuthId) {
      throw new Error("Personal Space not found");
    }
    if (space.lifecycle !== "active" && space.lifecycle !== "suspended") throw new Error("personal Space is not deletable in its current lifecycle state");
    const idempotencyKey = args.idempotencyKey.trim();
    const existing = await ctx.db.query("spaceDeletionRequests")
      .withIndex("by_space_and_idempotency", (q: any) => q.eq("spaceRef", space.spaceRef).eq("idempotencyKey", idempotencyKey)).first();
    if (existing) return existing;
    const now = Date.now();
    const requestId = deletionRequestId(space.spaceRef, idempotencyKey);
    await ctx.db.insert("spaceDeletionRequests", {
      requestId, idempotencyKey, spaceRef: space.spaceRef, externalOrgId: space.externalOrgId,
      ownerExternalAuthId: args.externalAuthId, state: "pending_authorization", createdAt: now, updatedAt: now,
    });
    const eventId = deletionAuthorizationEventId(requestId);
    await ctx.db.insert("spaceDeletionAuthorizationEvents", {
      eventId, requestId, deliveryState: "pending", deliveryAttempts: 0, nextAttemptAt: now, createdAt: now,
    });
    await ctx.scheduler.runAfter(0, internal.spaceDeletionAuthorization.deliverOne, {
      workerId: `space-deletion-authorization:${eventId}:${crypto.randomUUID()}`,
    });
    return { requestId, idempotencyKey, spaceRef: space.spaceRef, externalOrgId: space.externalOrgId, ownerExternalAuthId: args.externalAuthId, state: "pending_authorization" as const, createdAt: now, updatedAt: now };
  },
});

// An honest owner-only receipt surface. This reports authorization and owner
// completion separately; it cannot turn a still-pending delete into a success
// merely because the requester retried or a worker timed out.
export const getPersonalSpaceDeletionReceipt = query({
  args: { externalOrgId: v.string(), requestId: v.string() },
  handler: async (ctx, args) => {
    const viewer = await requireViewerMembership(ctx, args.externalOrgId);
    const request = await ctx.db.query("spaceDeletionRequests")
      .withIndex("by_request_id", (q: any) => q.eq("requestId", args.requestId)).first();
    if (!request || request.externalOrgId !== args.externalOrgId || request.ownerExternalAuthId !== viewer.externalAuthId) {
      throw new Error("Space deletion request not found");
    }
    const receipts = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request", (q: any) => q.eq("requestId", request.requestId)).collect();
    return {
      request,
      receipts,
      purgeStatus: aggregateDeletionReceipts(receipts.map((receipt: any) => ({
        ownerPlane: receipt.ownerPlane as SpaceDeletionOwner,
        status: receipt.status as SpaceDeletionOwnerStatus,
      }))),
    };
  },
});

export const getPersonalSpaceDeletionReceiptForGateway = query({
  args: { externalAuthId: v.string(), externalOrgId: v.string(), serviceKey: v.string(), requestId: v.string() },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const request = await ctx.db.query("spaceDeletionRequests")
      .withIndex("by_request_id", (q: any) => q.eq("requestId", args.requestId)).first();
    if (!request || request.externalOrgId !== args.externalOrgId || request.ownerExternalAuthId !== args.externalAuthId) {
      throw new Error("Space deletion request not found");
    }
    const organizations = await ctx.db.query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", args.externalOrgId)).collect();
    const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
    if (!organization) throw new Error("Organization not found");
    const members = await ctx.db.query("users")
      .withIndex("by_external_and_org", (q: any) => q.eq("externalAuthId", args.externalAuthId).eq("orgId", organization._id)).collect();
    if (!members.some((candidate: any) => candidate.syncStatus !== "deleted")) throw new Error("Unauthorized");
    const receipts = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request", (q: any) => q.eq("requestId", request.requestId)).collect();
    return { request, receipts, purgeStatus: aggregateDeletionReceipts(receipts.map((receipt: any) => ({ ownerPlane: receipt.ownerPlane as SpaceDeletionOwner, status: receipt.status as SpaceDeletionOwnerStatus }))) };
  },
});

/**
 * Narrow BFF projection for the one personal Space associated with a
 * server-derived `(org, principal)` pair. It is not a browser query: callers
 * must hold the Application service key, and this validates the underlying
 * Application membership before returning the lifecycle-owned record. Control
 * remains the authority for access/decisions; this merely exposes lifecycle
 * truth so a gateway can build the Space switcher without direct database use.
 */
export const getPersonalSpaceForGateway = query({
  args: {
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", args.externalOrgId))
      .collect();
    const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
    if (!organization) throw new Error("Organization not found");
    const members = await ctx.db
      .query("users")
      .withIndex("by_external_and_org", (q: any) =>
        q.eq("externalAuthId", args.externalAuthId).eq("orgId", organization._id),
      )
      .collect();
    if (!members.some((candidate: any) => candidate.syncStatus !== "deleted")) {
      throw new Error("Unauthorized");
    }
    const spaces = await ctx.db
      .query("spaces")
      .withIndex("by_personal_owner", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId)
          .eq("kind", "personal")
          .eq("ownerExternalAuthId", args.externalAuthId),
      )
      .collect();
    if (spaces.length > 1) throw new Error("personal Space invariant violated");
    return spaces[0] ?? null;
  },
});

/**
 * A server-only recipient-audience projection for the authenticated BFF.
 * It reveals only the opaque ref/hash/revision, never the participant list.
 * A caller must be an active Application member *and* an active recipient;
 * this does not replace Control's later per-recipient authorization.
 */
export const getRecipientAudienceForGateway = query({
  args: {
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    serviceKey: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const space = await getSpaceByRef(ctx, args.spaceRef);
    if (!space || space.externalOrgId !== args.externalOrgId || space.lifecycle !== "active") {
      throw new Error("Space not found");
    }
    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", args.externalOrgId))
      .collect();
    const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
    if (!organization) throw new Error("Organization not found");
    const members = await ctx.db
      .query("users")
      .withIndex("by_external_and_org", (q: any) =>
        q.eq("externalAuthId", args.externalAuthId).eq("orgId", organization._id),
      )
      .collect();
    if (!members.some((candidate: any) => candidate.syncStatus !== "deleted")) {
      throw new Error("Unauthorized");
    }
    const audience = await activeRecipientAudience(ctx, args.spaceRef);
    if (!audience || audience.controlState !== "acknowledged" || !audience.recipientExternalAuthIds.includes(args.externalAuthId)) {
      throw new Error("Unauthorized");
    }
    return {
      audienceHash: audience.audienceHash,
      audienceRef: audience.audienceRef,
      revision: audience.revision,
      spaceRef: audience.spaceRef,
    };
  },
});

/**
 * Trusted Application workflow ingress for recipient-set changes. Browser
 * callers cannot invoke an internal mutation. CAS protects a room/case from
 * silently last-writer-wins participant changes; Control must still consume
 * and authorize the resulting snapshot before it can authorize shared work.
 */
export const replaceRecipientAudience = internalMutation({
  args: {
    expectedRevision: v.optional(v.number()),
    recipientExternalAuthIds: v.array(v.string()),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    const space = await getSpaceByRef(ctx, args.spaceRef);
    if (!space || space.lifecycle !== "active") throw new Error("active Space required");
    const previous = await activeRecipientAudience(ctx, args.spaceRef);
    const expectedRevision = args.expectedRevision;
    if (expectedRevision !== undefined && previous?.revision !== expectedRevision) {
      throw new Error("recipient audience revision conflict");
    }
    if (expectedRevision === undefined && previous) {
      throw new Error("recipient audience replacement requires expected revision");
    }
    const recipients = canonicalRecipientSubjectIds(args.recipientExternalAuthIds);
    const organization = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", space.externalOrgId))
      .first();
    if (!organization || organization.syncStatus === "deleted") throw new Error("Organization not found");
    for (const recipientExternalAuthId of recipients) {
      const candidates = await ctx.db
        .query("users")
        .withIndex("by_external_and_org", (q: any) =>
          q.eq("externalAuthId", recipientExternalAuthId).eq("orgId", organization._id),
        )
        .collect();
      if (!candidates.some((candidate: any) => candidate.syncStatus !== "deleted")) {
        throw new Error("recipient is not an active Application organization member");
      }
    }
    const revision = (previous?.revision ?? 0) + 1;
    const audienceRef = spaceRecipientAudienceRef(space.spaceRef, revision);
    const audienceHash = await recipientAudienceHash(recipients);
    const now = Date.now();
    if (previous) await ctx.db.patch(previous._id, { state: "superseded", updatedAt: now });
    await ctx.db.insert("spaceRecipientAudiences", {
      audienceHash,
      audienceRef,
      controlState: "pending",
      createdAt: now,
      externalOrgId: space.externalOrgId,
      recipientExternalAuthIds: recipients,
      revision,
      spaceRef: space.spaceRef,
      state: "active",
      updatedAt: now,
    });
    const eventId = recipientAudienceEventId(space.spaceRef, revision);
    await ctx.db.insert("spaceRecipientAudienceEvents", {
      audienceRef,
      createdAt: now,
      deliveryAttempts: 0,
      deliveryState: "pending",
      eventId,
      externalOrgId: space.externalOrgId,
      nextAttemptAt: now,
      revision,
      spaceRef: space.spaceRef,
    });
    await ctx.scheduler.runAfter(0, internal.spaceAudienceRegistration.deliverOne, {
      workerId: `space-audience-registration:${eventId}:${crypto.randomUUID()}`,
    });
    return { audienceHash, audienceRef, revision, spaceRef: space.spaceRef };
  },
});

export const claimNextRecipientAudienceRegistration = internalMutation({
  args: { workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const events = await ctx.db.query("spaceRecipientAudienceEvents").collect();
    const candidate = events
      .filter((event: any) => canClaimSpaceLifecycleDelivery(deliveryFor(event), args.now))
      .sort((left: any, right: any) =>
        (left.nextAttemptAt ?? left.createdAt) - (right.nextAttemptAt ?? right.createdAt) || left.createdAt - right.createdAt,
      )[0];
    if (!candidate) return null;
    const audience = await ctx.db
      .query("spaceRecipientAudiences")
      .withIndex("by_audience_ref", (q: any) => q.eq("audienceRef", candidate.audienceRef))
      .first();
    if (!audience) throw new Error("recipient audience delivery has no audience snapshot");
    const delivery = claimSpaceLifecycleDelivery(deliveryFor(candidate), args.workerId, args.now, 30_000);
    await ctx.db.patch(candidate._id, patchForDelivery(delivery));
    return { audience, eventId: candidate.eventId, workerId: args.workerId };
  },
});

export const releaseRecipientAudienceRegistrationClaim = internalMutation({
  args: { error: v.string(), eventId: v.string(), now: v.number(), workerId: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceRecipientAudienceEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event) throw new Error("recipient audience event not found");
    const delivery = releaseSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: args.error.slice(0, 256) });
    return delivery.nextAttemptAt;
  },
});

export const acknowledgeRecipientAudienceRegistrationClaim = internalMutation({
  args: { eventId: v.string(), now: v.number(), workerId: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceRecipientAudienceEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event) throw new Error("recipient audience event not found");
    const delivery = acknowledgeSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), deliveredAt: args.now, lastDeliveryError: undefined });
    const audience = await ctx.db.query("spaceRecipientAudiences")
      .withIndex("by_audience_ref", (q: any) => q.eq("audienceRef", event.audienceRef)).first();
    if (!audience) throw new Error("recipient audience snapshot not found");
    await ctx.db.patch(audience._id, { controlState: "acknowledged", controlRegisteredAt: args.now, updatedAt: args.now });
    return delivery;
  },
});

export const rejectRecipientAudienceRegistrationClaim = internalMutation({
  args: { eventId: v.string(), now: v.number(), workerId: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceRecipientAudienceEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event) throw new Error("recipient audience event not found");
    const delivery = deliveryFor(event);
    if (delivery.state !== "claimed" || delivery.leaseOwner !== args.workerId) throw new Error("only the active delivery worker may reject the claim");
    await ctx.db.patch(event._id, { deliveryAttempts: delivery.attempts, deliveryState: "rejected", deliveredAt: args.now, lastDeliveryError: "Control rejected recipient audience", leaseExpiresAt: undefined, leaseOwner: undefined, nextAttemptAt: args.now });
    const audience = await ctx.db.query("spaceRecipientAudiences")
      .withIndex("by_audience_ref", (q: any) => q.eq("audienceRef", event.audienceRef)).first();
    if (!audience) throw new Error("recipient audience snapshot not found");
    await ctx.db.patch(audience._id, { controlState: "rejected", updatedAt: args.now });
    return { status: "rejected" as const };
  },
});

/** Control registration result ingress. It is internal-only until Control's
 * signed registration client is landed; no browser caller can activate a Space. */
export const applyControlRegistration = internalMutation({
  args: {
    spaceRef: v.string(),
    accepted: v.boolean(),
    controlResourceRef: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    const current = await getSpaceByRef(ctx, args.spaceRef);
    if (!current) throw new Error("Space not found");
    if (current.lifecycle === "active" && args.accepted) return current;
    if (current.lifecycle === "failed_registration" && !args.accepted) return current;
    const lifecycle = args.accepted ? "active" : "failed_registration";
    const next = transitionSpaceLifecycle(
      { lifecycle: current.lifecycle as SpaceLifecycle, revision: current.lifecycleRevision },
      lifecycle,
    );
    const now = Date.now();
    await ctx.db.patch(current._id, {
      ...next,
      controlResourceRef: args.accepted ? args.controlResourceRef : undefined,
      updatedAt: now,
    });
    const updated = await ctx.db.get(current._id);
    if (!updated) throw new Error("Space registration update failed");
    const event = await appendLifecycleEvent(ctx, updated, now);
    return { ...updated, lifecycleEvent: event };
  },
});

export const transitionLifecycle = internalMutation({
  args: { spaceRef: v.string(), lifecycle: lifecycleValidator },
  handler: async (ctx, args) => {
    const current = await getSpaceByRef(ctx, args.spaceRef);
    if (!current) throw new Error("Space not found");
    const next = transitionSpaceLifecycle(
      { lifecycle: current.lifecycle as SpaceLifecycle, revision: current.lifecycleRevision },
      args.lifecycle,
    );
    const now = Date.now();
    await ctx.db.patch(current._id, { ...next, updatedAt: now });
    const updated = await ctx.db.get(current._id);
    if (!updated) throw new Error("Space lifecycle update failed");
    const event = await appendLifecycleEvent(ctx, updated, now);
    return { ...updated, lifecycleEvent: event };
  },
});

export const claimNextDeletionAuthorization = internalMutation({
  args: { workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const events = await ctx.db.query("spaceDeletionAuthorizationEvents").collect();
    const event = events
      .filter((candidate: any) => canClaimSpaceLifecycleDelivery(deliveryFor(candidate), args.now))
      .sort((left: any, right: any) => left.nextAttemptAt - right.nextAttemptAt || left.createdAt - right.createdAt)[0];
    if (!event) return null;
    const request = await ctx.db
      .query("spaceDeletionRequests")
      .withIndex("by_request_id", (q: any) => q.eq("requestId", event.requestId))
      .first();
    if (!request || request.state !== "pending_authorization") {
      throw new Error("deletion authorization event has no pending request");
    }
    const delivery = claimSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now, 30_000);
    await ctx.db.patch(event._id, patchForDelivery(delivery));
    return { eventId: event.eventId, request, workerId: args.workerId };
  },
});

export const releaseDeletionAuthorizationClaim = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number(), error: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionAuthorizationEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event) throw new Error("deletion authorization event not found");
    const delivery = releaseSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: args.error.slice(0, 256) });
    return delivery.nextAttemptAt;
  },
});

export const acknowledgeDeletionAuthorizationClaim = internalMutation({
  args: {
    eventId: v.string(), workerId: v.string(), now: v.number(),
    status: v.union(v.literal("authorized"), v.literal("blocked_legal_hold"), v.literal("rejected")),
  },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionAuthorizationEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event) throw new Error("deletion authorization event not found");
    const delivery = acknowledgeSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    const request = await ctx.db.query("spaceDeletionRequests")
      .withIndex("by_request_id", (q: any) => q.eq("requestId", event.requestId)).first();
    if (!request || request.state !== "pending_authorization") throw new Error("deletion request is not pending authorization");
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: undefined });
    const state = args.status === "authorized" ? "authorized" : args.status;
    await ctx.db.patch(request._id, { state, updatedAt: args.now });
    if (args.status !== "authorized") return { state };

    for (const ownerPlane of SPACE_DELETION_OWNERS) {
      await ctx.db.insert("spaceDeletionOwnerReceipts", {
        requestId: request.requestId,
        ownerPlane,
        status: "pending",
        deadlineAt: args.now + SPACE_DELETION_OWNER_DEADLINE_MS,
        updatedAt: args.now,
      });
    }
    // Only adapters that exist get delivery work. The Model event is still
    // separate from its receipt so a crash after remote acceptance cannot be
    // misreported as erased and can safely retry the same request reference.
    const modelEventId = deletionOwnerDeliveryEventId(request.requestId, "model");
    await ctx.db.insert("spaceDeletionOwnerDeliveryEvents", {
      eventId: modelEventId,
      requestId: request.requestId,
      ownerPlane: "model",
      deliveryState: "pending",
      deliveryAttempts: 0,
      nextAttemptAt: args.now,
      createdAt: args.now,
    });
    const dataEventId = deletionOwnerDeliveryEventId(request.requestId, "data");
    await ctx.db.insert("spaceDeletionOwnerDeliveryEvents", {
      eventId: dataEventId,
      requestId: request.requestId,
      ownerPlane: "data",
      deliveryState: "pending",
      deliveryAttempts: 0,
      nextAttemptAt: args.now,
      createdAt: args.now,
    });
    const ingestionEventId = deletionOwnerDeliveryEventId(request.requestId, "ingestion");
    await ctx.db.insert("spaceDeletionOwnerDeliveryEvents", {
      eventId: ingestionEventId,
      requestId: request.requestId,
      ownerPlane: "ingestion",
      deliveryState: "pending",
      deliveryAttempts: 0,
      nextAttemptAt: args.now,
      createdAt: args.now,
    });

    const space = await getSpaceByRef(ctx, request.spaceRef);
    if (!space || space.lifecycle === "deleted" || space.lifecycle === "deleting") throw new Error("deletion request Space is unavailable");
    const next = transitionSpaceLifecycle(
      { lifecycle: space.lifecycle as SpaceLifecycle, revision: space.lifecycleRevision },
      "deleting",
    );
    await ctx.db.patch(space._id, { ...next, updatedAt: args.now });
    const updated = await ctx.db.get(space._id);
    if (!updated) throw new Error("deletion fence update failed");
    await appendLifecycleEvent(ctx, updated, args.now);
    // Application owns participant-set projections. Purge them atomically with
    // the fence so no current or future recipient snapshot can be served for a
    // deleting Space. Keep the Space/tombstone and deletion-request evidence;
    // those are the minimum durable reconciliation record, not user content.
    const audiences = (await ctx.db.query("spaceRecipientAudiences").collect())
      .filter((candidate: any) => candidate.spaceRef === request.spaceRef);
    const audienceEvents = await ctx.db.query("spaceRecipientAudienceEvents").collect();
    for (const audience of audiences) await ctx.db.delete(audience._id);
    for (const audienceEvent of audienceEvents.filter((candidate: any) => candidate.spaceRef === request.spaceRef)) {
      await ctx.db.delete(audienceEvent._id);
    }
    const applicationReceipt = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request_and_owner", (q: any) => q.eq("requestId", request.requestId).eq("ownerPlane", "application"))
      .first();
    if (!applicationReceipt) throw new Error("Application deletion owner receipt is missing");
    assertReceiptTransition(applicationReceipt.status as SpaceDeletionOwnerStatus, "succeeded");
    await ctx.db.patch(applicationReceipt._id, {
      status: "succeeded",
      receiptRef: `application:${request.spaceRef}:lifecycle:${updated.lifecycleRevision}`,
      detail: "Application participant-set projections purged; canonical tombstone retained",
      updatedAt: args.now,
    });
    await ctx.scheduler.runAfter(0, internal.spaceDeletionModel.deliverOne, {
      workerId: `space-deletion-model:${modelEventId}:${crypto.randomUUID()}`,
    });
    await ctx.scheduler.runAfter(0, internal.spaceDeletionData.deliverOne, {
      workerId: `space-deletion-data:${dataEventId}:${crypto.randomUUID()}`,
    });
    await ctx.scheduler.runAfter(0, internal.spaceDeletionIngestion.deliverOne, {
      workerId: `space-deletion-ingestion:${ingestionEventId}:${crypto.randomUUID()}`,
    });
    await ctx.scheduler.runAfter(SPACE_DELETION_OWNER_DEADLINE_MS, internal.spaceDeletionReceiptReconciler.reconcileOne, {
      requestId: request.requestId,
    });
    return { state };
  },
});

export const claimNextModelDeletionOwnerDelivery = internalMutation({
  args: { workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    if (!Number.isSafeInteger(args.now) || args.now <= 0) throw new Error("deletion delivery claim time is invalid");
    const events = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_owner_and_delivery", (q: any) => q.eq("ownerPlane", "model")).collect();
    const candidate = events.filter((event: any) => canClaimSpaceLifecycleDelivery(deliveryFor(event), args.now))
      .sort((left: any, right: any) => (left.nextAttemptAt ?? left.createdAt) - (right.nextAttemptAt ?? right.createdAt))[0];
    if (!candidate) return null;
    const request = await ctx.db.query("spaceDeletionRequests")
      .withIndex("by_request_id", (q: any) => q.eq("requestId", candidate.requestId)).first();
    if (!request || request.state !== "authorized") throw new Error("Model deletion delivery lacks an authorized request");
    const space = await getSpaceByRef(ctx, request.spaceRef);
    if (!space || space.lifecycle !== "deleting") throw new Error("Model deletion delivery Space is not fenced");
    const delivery = claimSpaceLifecycleDelivery(deliveryFor(candidate), args.workerId, args.now, 30_000);
    await ctx.db.patch(candidate._id, patchForDelivery(delivery));
    return { eventId: candidate.eventId, request, workerId: args.workerId };
  },
});

export const releaseModelDeletionOwnerDelivery = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number(), error: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event || event.ownerPlane !== "model") throw new Error("Model deletion delivery event not found");
    const delivery = releaseSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: args.error.slice(0, 256) });
    return delivery.nextAttemptAt;
  },
});

async function finalizeDeletionWhenEveryOwnerSucceeded(ctx: any, requestId: string, now: number): Promise<boolean> {
  const receipts = await ctx.db.query("spaceDeletionOwnerReceipts")
    .withIndex("by_request", (q: any) => q.eq("requestId", requestId)).collect();
  const aggregate = aggregateDeletionReceipts(receipts.map((receipt: any) => ({
    ownerPlane: receipt.ownerPlane as SpaceDeletionOwner,
    status: receipt.status as SpaceDeletionOwnerStatus,
  })));
  if (aggregate !== "succeeded") return false;
  const request = await ctx.db.query("spaceDeletionRequests")
    .withIndex("by_request_id", (q: any) => q.eq("requestId", requestId)).first();
  if (!request) throw new Error("completed deletion has no durable request");
  const space = await getSpaceByRef(ctx, request.spaceRef);
  if (!space) throw new Error("completed deletion has no canonical Space");
  if (space.lifecycle === "deleted") return true;
  if (space.lifecycle !== "deleting") throw new Error("only a fenced Space can finalize deletion");
  const next = transitionSpaceLifecycle(
    { lifecycle: space.lifecycle as SpaceLifecycle, revision: space.lifecycleRevision },
    "deleted",
  );
  await ctx.db.patch(space._id, { ...next, updatedAt: now });
  const deleted = await ctx.db.get(space._id);
  if (!deleted) throw new Error("Space deletion finalization failed");
  await appendLifecycleEvent(ctx, deleted, now);
  return true;
}

export const acknowledgeModelDeletionOwnerDelivery = internalMutation({
  args: {
    eventId: v.string(), workerId: v.string(), now: v.number(),
    status: v.union(v.literal("succeeded"), v.literal("partial"), v.literal("failed"), v.literal("unknown")),
    receiptRef: v.string(), detail: v.string(),
  },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event || event.ownerPlane !== "model") throw new Error("Model deletion delivery event not found");
    const delivery = acknowledgeSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    const receipt = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request_and_owner", (q: any) => q.eq("requestId", event.requestId).eq("ownerPlane", "model"))
      .first();
    if (!receipt) throw new Error("Model deletion owner receipt is not expected");
    assertReceiptTransition(receipt.status as SpaceDeletionOwnerStatus, args.status as SpaceDeletionOwnerStatus);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: undefined });
    await ctx.db.patch(receipt._id, {
      status: args.status,
      receiptRef: args.receiptRef.slice(0, 256),
      detail: args.detail.slice(0, 256),
      updatedAt: args.now,
    });
    const finalized = await finalizeDeletionWhenEveryOwnerSucceeded(ctx, event.requestId, args.now);
    return { status: args.status, finalized };
  },
});

export const claimNextDataDeletionOwnerDelivery = internalMutation({
  args: { workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    if (!Number.isSafeInteger(args.now) || args.now <= 0) throw new Error("deletion delivery claim time is invalid");
    const events = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_owner_and_delivery", (q: any) => q.eq("ownerPlane", "data")).collect();
    const candidate = events.filter((event: any) => canClaimSpaceLifecycleDelivery(deliveryFor(event), args.now))
      .sort((left: any, right: any) => (left.nextAttemptAt ?? left.createdAt) - (right.nextAttemptAt ?? right.createdAt))[0];
    if (!candidate) return null;
    const request = await ctx.db.query("spaceDeletionRequests")
      .withIndex("by_request_id", (q: any) => q.eq("requestId", candidate.requestId)).first();
    if (!request || request.state !== "authorized") throw new Error("Data deletion delivery lacks an authorized request");
    const space = await getSpaceByRef(ctx, request.spaceRef);
    if (!space || space.lifecycle !== "deleting") throw new Error("Data deletion delivery Space is not fenced");
    const delivery = claimSpaceLifecycleDelivery(deliveryFor(candidate), args.workerId, args.now, 30_000);
    await ctx.db.patch(candidate._id, patchForDelivery(delivery));
    return { eventId: candidate.eventId, request, workerId: args.workerId };
  },
});

export const releaseDataDeletionOwnerDelivery = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number(), error: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event || event.ownerPlane !== "data") throw new Error("Data deletion delivery event not found");
    const delivery = releaseSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: args.error.slice(0, 256) });
    return delivery.nextAttemptAt;
  },
});

export const acknowledgeDataDeletionOwnerDelivery = internalMutation({
  args: {
    eventId: v.string(), workerId: v.string(), now: v.number(),
    status: v.union(v.literal("succeeded"), v.literal("partial"), v.literal("failed"), v.literal("unknown")),
    receiptRef: v.string(), detail: v.string(),
  },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event || event.ownerPlane !== "data") throw new Error("Data deletion delivery event not found");
    const delivery = acknowledgeSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    const receipt = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request_and_owner", (q: any) => q.eq("requestId", event.requestId).eq("ownerPlane", "data"))
      .first();
    if (!receipt) throw new Error("Data deletion owner receipt is not expected");
    assertReceiptTransition(receipt.status as SpaceDeletionOwnerStatus, args.status as SpaceDeletionOwnerStatus);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: undefined });
    await ctx.db.patch(receipt._id, {
      status: args.status,
      receiptRef: args.receiptRef.slice(0, 256),
      detail: args.detail.slice(0, 256),
      updatedAt: args.now,
    });
    const finalized = await finalizeDeletionWhenEveryOwnerSucceeded(ctx, event.requestId, args.now);
    return { status: args.status, finalized };
  },
});

export const claimNextIngestionDeletionOwnerDelivery = internalMutation({
  args: { workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const events = await ctx.db.query("spaceDeletionOwnerDeliveryEvents")
      .withIndex("by_owner_and_delivery", (q: any) => q.eq("ownerPlane", "ingestion")).collect();
    const candidate = events.filter((event: any) => canClaimSpaceLifecycleDelivery(deliveryFor(event), args.now))
      .sort((left: any, right: any) => (left.nextAttemptAt ?? left.createdAt) - (right.nextAttemptAt ?? right.createdAt))[0];
    if (!candidate) return null;
    const request = await ctx.db.query("spaceDeletionRequests").withIndex("by_request_id", (q: any) => q.eq("requestId", candidate.requestId)).first();
    if (!request || request.state !== "authorized") throw new Error("Ingestion deletion delivery lacks an authorized request");
    const space = await getSpaceByRef(ctx, request.spaceRef);
    if (!space || space.lifecycle !== "deleting") throw new Error("Ingestion deletion delivery Space is not fenced");
    const delivery = claimSpaceLifecycleDelivery(deliveryFor(candidate), args.workerId, args.now, 30_000);
    await ctx.db.patch(candidate._id, patchForDelivery(delivery));
    return { eventId: candidate.eventId, request };
  },
});

export const releaseIngestionDeletionOwnerDelivery = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number(), error: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionOwnerDeliveryEvents").withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event || event.ownerPlane !== "ingestion") throw new Error("Ingestion deletion delivery event not found");
    const delivery = releaseSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: args.error.slice(0, 256) });
    return delivery.nextAttemptAt;
  },
});

export const acknowledgeIngestionDeletionOwnerDelivery = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number(), status: v.union(v.literal("succeeded"), v.literal("partial"), v.literal("failed"), v.literal("unknown")), receiptRef: v.string(), detail: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db.query("spaceDeletionOwnerDeliveryEvents").withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId)).first();
    if (!event || event.ownerPlane !== "ingestion") throw new Error("Ingestion deletion delivery event not found");
    const delivery = acknowledgeSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    const receipt = await ctx.db.query("spaceDeletionOwnerReceipts").withIndex("by_request_and_owner", (q: any) => q.eq("requestId", event.requestId).eq("ownerPlane", "ingestion")).first();
    if (!receipt) throw new Error("Ingestion deletion owner receipt is not expected");
    assertReceiptTransition(receipt.status as SpaceDeletionOwnerStatus, args.status as SpaceDeletionOwnerStatus);
    await ctx.db.patch(event._id, { ...patchForDelivery(delivery), lastDeliveryError: undefined });
    await ctx.db.patch(receipt._id, { status: args.status, receiptRef: args.receiptRef.slice(0, 256), detail: args.detail.slice(0, 256), updatedAt: args.now });
    const finalized = await finalizeDeletionWhenEveryOwnerSucceeded(ctx, event.requestId, args.now);
    return { status: args.status, finalized };
  },
});

// Trusted coordinator ingress only. An owner receipt may improve an uncertain
// result after reconciliation, but cannot silently turn a completed successful
// receipt into another state.
export const recordDeletionOwnerReceipt = internalMutation({
  args: {
    requestId: v.string(),
    ownerPlane: v.union(v.literal("application"), v.literal("control"), v.literal("data"), v.literal("ingestion"), v.literal("model"), v.literal("infra")),
    status: v.union(v.literal("pending"), v.literal("blocked_legal_hold"), v.literal("succeeded"), v.literal("partial"), v.literal("failed"), v.literal("unknown")),
    receiptRef: v.optional(v.string()),
    detail: v.optional(v.string()),
    now: v.number(),
  },
  handler: async (ctx, args) => {
    const receipt = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request_and_owner", (q: any) => q.eq("requestId", args.requestId).eq("ownerPlane", args.ownerPlane))
      .first();
    if (!receipt) throw new Error("Space deletion owner receipt is not expected");
    assertReceiptTransition(receipt.status as SpaceDeletionOwnerStatus, args.status as SpaceDeletionOwnerStatus);
    await ctx.db.patch(receipt._id, {
      status: args.status,
      receiptRef: args.receiptRef?.trim() || undefined,
      detail: args.detail?.slice(0, 256) || undefined,
      updatedAt: args.now,
    });
    const finalized = await finalizeDeletionWhenEveryOwnerSucceeded(ctx, args.requestId, args.now);
    return { ownerPlane: args.ownerPlane as SpaceDeletionOwner, status: args.status as SpaceDeletionOwnerStatus, finalized };
  },
});

// A timeout is an uncertainty signal, never a deletion result. Each request
// gets an explicit deadline so an absent Infra/provider reply cannot leave the
// human receipt perpetually pending or, worse, be inferred as complete.
export const markExpiredDeletionOwnerReceiptsUnknown = internalMutation({
  args: { requestId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    const receipts = await ctx.db.query("spaceDeletionOwnerReceipts")
      .withIndex("by_request", (q: any) => q.eq("requestId", args.requestId)).collect();
    let changed = 0;
    for (const receipt of receipts) {
      if (receipt.status !== "pending" || !receipt.deadlineAt || receipt.deadlineAt > args.now) continue;
      assertReceiptTransition(receipt.status as SpaceDeletionOwnerStatus, "unknown");
      await ctx.db.patch(receipt._id, {
        status: "unknown",
        detail: "Owner deletion receipt deadline elapsed; operator reconciliation required",
        updatedAt: args.now,
      });
      changed += 1;
    }
    return { changed };
  },
});

/**
 * Atomically claims one due lifecycle outbox row. Delivery itself happens in
 * an Action, but the claim and its lease are persisted before any network I/O.
 */
export const claimNextRegistration = internalMutation({
  args: { workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    if (!Number.isSafeInteger(args.now) || args.now <= 0) {
      throw new Error("Space delivery claim time is invalid");
    }
    const events = await ctx.db.query("spaceLifecycleEvents").collect();
    const candidate = events
      .filter((event: any) => canClaimSpaceLifecycleDelivery(deliveryFor(event), args.now))
      .sort((left: any, right: any) =>
        (left.nextAttemptAt ?? left.createdAt) - (right.nextAttemptAt ?? right.createdAt) ||
        left.createdAt - right.createdAt,
      )[0];
    if (!candidate) return null;

    const space = await getSpaceByRef(ctx, candidate.spaceRef);
    if (!space) throw new Error("Space lifecycle event has no canonical Space");
    const delivery = claimSpaceLifecycleDelivery(deliveryFor(candidate), args.workerId, args.now, 30_000);
    await ctx.db.patch(candidate._id, patchForDelivery(delivery));
    return {
      event: {
        eventId: candidate.eventId,
        externalOrgId: candidate.externalOrgId,
        lifecycle: candidate.lifecycle,
        revision: candidate.revision,
        spaceRef: candidate.spaceRef,
      },
      space: {
        kind: space.kind,
        ownerPrincipalId: space.ownerExternalAuthId ?? space.createdByExternalAuthId,
      },
    };
  },
});

export const releaseRegistrationClaim = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number(), error: v.string() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db
      .query("spaceLifecycleEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId))
      .first();
    if (!event) throw new Error("Space lifecycle event not found");
    const delivery = releaseSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, {
      ...patchForDelivery(delivery),
      lastDeliveryError: args.error.slice(0, 256),
    });
    return delivery.nextAttemptAt;
  },
});

export const acknowledgeRegistrationClaim = internalMutation({
  args: { controlResourceRef: v.string(), eventId: v.string(), workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db
      .query("spaceLifecycleEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId))
      .first();
    if (!event) throw new Error("Space lifecycle event not found");
    const delivery = acknowledgeSpaceLifecycleDelivery(deliveryFor(event), args.workerId, args.now);
    await ctx.db.patch(event._id, {
      ...patchForDelivery(delivery),
      deliveredAt: args.now,
      lastDeliveryError: undefined,
    });

    const space = await getSpaceByRef(ctx, event.spaceRef);
    if (!space) throw new Error("Space lifecycle event has no canonical Space");
    // Control's accepted `deleting` lifecycle revision is also a receipt for
    // Control's own transactional projection cleanup. It is not inferred from
    // an authorization decision: only the specific lifecycle delivery that
    // received Control's success response may advance this owner receipt.
    if (event.lifecycle === "deleting") {
      const requests = await ctx.db.query("spaceDeletionRequests").collect();
      const request = requests.find((candidate: any) =>
        candidate.spaceRef === event.spaceRef
        && candidate.externalOrgId === event.externalOrgId
        && candidate.state === "authorized",
      );
      if (!request) throw new Error("Control deleting lifecycle has no authorized deletion request");
      const receipt = await ctx.db.query("spaceDeletionOwnerReceipts")
        .withIndex("by_request_and_owner", (q: any) => q.eq("requestId", request.requestId).eq("ownerPlane", "control"))
        .first();
      if (!receipt) throw new Error("Control deletion owner receipt is missing");
      assertReceiptTransition(receipt.status as SpaceDeletionOwnerStatus, "succeeded");
      await ctx.db.patch(receipt._id, {
        status: "succeeded",
        receiptRef: `control:${event.spaceRef}:lifecycle:${event.revision}`,
        detail: "Control Space projections purged with accepted deleting lifecycle revision",
        updatedAt: args.now,
      });
    }
    // Only Control's accepted receipt for the original registration can make a
    // pending Space active. Later lifecycle events are acknowledgements of the
    // revision observed by Control, not an authorization decision.
    if (event.lifecycle === "pending_registration" && space.lifecycle === "pending_registration") {
      const next = transitionSpaceLifecycle(
        { lifecycle: space.lifecycle as SpaceLifecycle, revision: space.lifecycleRevision },
        "active",
      );
      await ctx.db.patch(space._id, {
        ...next,
        controlResourceRef: args.controlResourceRef,
        updatedAt: args.now,
      });
      const updated = await ctx.db.get(space._id);
      if (!updated) throw new Error("Space activation update failed");
      await appendLifecycleEvent(ctx, updated, args.now);
    }
    return delivery;
  },
});

/** A `409` is an immutable Space identity conflict, not a transport failure.
 * Terminalize it so the retry worker cannot loop indefinitely, and preserve a
 * failed Application lifecycle state for operator reconciliation. */
export const rejectRegistrationClaim = internalMutation({
  args: { eventId: v.string(), workerId: v.string(), now: v.number() },
  handler: async (ctx, args) => {
    assertDeliveryWorker(args.workerId);
    const event = await ctx.db
      .query("spaceLifecycleEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", args.eventId))
      .first();
    if (!event) throw new Error("Space lifecycle event not found");
    const delivery = deliveryFor(event);
    if (delivery.state !== "claimed" || delivery.leaseOwner !== args.workerId) {
      throw new Error("only the active delivery worker may reject the claim");
    }
    await ctx.db.patch(event._id, {
      deliveryAttempts: delivery.attempts,
      deliveryState: "rejected",
      deliveredAt: args.now,
      lastDeliveryError: "Control rejected immutable Space registration",
      leaseExpiresAt: undefined,
      leaseOwner: undefined,
      nextAttemptAt: args.now,
    });

    const space = await getSpaceByRef(ctx, event.spaceRef);
    if (!space) throw new Error("Space lifecycle event has no canonical Space");
    if (event.lifecycle === "pending_registration" && space.lifecycle === "pending_registration") {
      const next = transitionSpaceLifecycle(
        { lifecycle: space.lifecycle as SpaceLifecycle, revision: space.lifecycleRevision },
        "failed_registration",
      );
      await ctx.db.patch(space._id, { ...next, updatedAt: args.now });
      const updated = await ctx.db.get(space._id);
      if (!updated) throw new Error("Space registration rejection update failed");
      // The conflicting Control reference cannot consume a later lifecycle
      // revision. Keep the local evidence but mark it terminal as well.
      await appendLifecycleEvent(ctx, updated, args.now, "rejected");
    }
    return { status: "rejected" as const };
  },
});
