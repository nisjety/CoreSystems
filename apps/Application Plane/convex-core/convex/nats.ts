/**
 * NATS Integration Module
 * 
 * Subscribes to verevon cross-plane NATS events from Control Plane and
 * Ingestion Plane to keep Convex database synchronized.
 * 
 * Events subscribed to:
 * - verevon.controlplane.org.{created,updated,deleted}
 * - verevon.controlplane.org.member.{added,removed}
 * - verevon.ingestion.import.completed
 */

import { v } from "convex/values";
import { internalAction, internalMutation } from "./_generated/server";
import { api, internal } from "./_generated/api";
import { normalizeProjectionRole, shouldApplyMembershipRemoval } from "./membershipProjection";
import {
  applyMembershipProjectionState,
  applyOrganizationProjectionState,
  membershipProjectionFingerprint,
  organizationProjectionFingerprint,
  type ProjectionState,
} from "./authorityProjection";

// Control-Plane-owned shared key; no hardcoded fallback. Empty only if the env
// is misconfigured, in which case the receiving validator rejects the call.
const CONVEX_INTERNAL_SERVICE_KEY =
  process.env.CONVEX_INTERNAL_SERVICE_KEY || "";

/**
 * Start NATS Subscriber
 * Should be called once during service initialization
 * 
 * In production, use Convex scheduled functions or a separate service
 * to maintain the subscription connection
 */
export const startSubscriber = internalAction(async (ctx) => {
  // This is a placeholder for the NATS subscriber logic
  // In a real implementation, this would connect to NATS and subscribe to events

  console.log("[Convex] NATS Subscriber initialized");

  return { status: "started" };
});

function storedProjectionState(value: {
  kind: "active" | "removed";
  sourceRevision?: number;
  sourceEventId?: string;
  sourceFingerprint?: string;
}): ProjectionState | undefined {
  if (
    !Number.isSafeInteger(value.sourceRevision) ||
    !value.sourceRevision ||
    !value.sourceEventId ||
    !value.sourceFingerprint
  ) {
    return undefined;
  }
  return {
    kind: value.kind,
    revision: value.sourceRevision,
    eventId: value.sourceEventId,
    fingerprint: value.sourceFingerprint,
  };
}

function sameProjectionState(
  left: ProjectionState | undefined,
  right: ProjectionState,
): boolean {
  return Boolean(
    left &&
      left.kind === right.kind &&
      left.revision === right.revision &&
      left.eventId === right.eventId &&
      left.fingerprint === right.fingerprint,
  );
}

export const onOrganizationProjectionChanged = internalMutation({
  args: {
    action: v.union(v.literal("upsert"), v.literal("remove")),
    eventId: v.string(),
    orgId: v.string(),
    revision: v.number(),
    name: v.optional(v.string()),
    slug: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    if (!Number.isSafeInteger(args.revision) || args.revision <= 0) {
      throw new Error("organization revision must be a positive safe integer");
    }
    if (!args.orgId.trim() || !args.eventId.trim()) {
      throw new Error("organization projection identity is required");
    }
    if (
      args.action === "upsert" &&
      (!args.name?.trim() || !args.slug?.trim())
    ) {
      throw new Error("organization upsert requires name and slug");
    }

    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q) => q.eq("externalOrgId", args.orgId))
      .collect();
    if (organizations.length > 1) {
      throw new Error("organization projection is ambiguous");
    }
    const organization = organizations[0];
    const tombstones = await ctx.db
      .query("organizationTombstones")
      .withIndex("by_external_org", (q) => q.eq("externalOrgId", args.orgId))
      .collect();
    if (tombstones.length > 1) {
      throw new Error("organization tombstone is ambiguous");
    }
    const tombstone = tombstones[0];
    const current = tombstone
      ? storedProjectionState({ kind: "removed", ...tombstone })
      : organization
        ? storedProjectionState({
            kind: organization.syncStatus === "deleted" ? "removed" : "active",
            ...organization,
          })
        : undefined;
    const incoming = {
      action: args.action,
      revision: args.revision,
      eventId: args.eventId,
      fingerprint: organizationProjectionFingerprint(args),
    } as const;
    const next = applyOrganizationProjectionState(current, incoming);
    if (sameProjectionState(current, next)) {
      return { status: tombstone ? "permanently_removed" : "unchanged" };
    }

    const now = Date.now();
    if (args.action === "upsert") {
      if (tombstone) return { status: "permanently_removed" };
      if (organization) {
        await ctx.db.patch(organization._id, {
          name: args.name!.trim(),
          slug: args.slug!.trim(),
          syncStatus: "synced",
          lastSyncedAt: now,
          updatedAt: now,
          deletedAt: undefined,
          sourceRevision: next.revision,
          sourceEventId: next.eventId,
          sourceFingerprint: next.fingerprint,
        });
        return { status: "updated", organizationId: organization._id };
      }
      const organizationId = await ctx.db.insert("organizations", {
        externalOrgId: args.orgId,
        name: args.name!.trim(),
        slug: args.slug!.trim(),
        settings: {},
        syncStatus: "synced",
        lastSyncedAt: now,
        createdAt: now,
        updatedAt: now,
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
      });
      return { status: "created", organizationId };
    }

    if (organization) {
      await ctx.db.patch(organization._id, {
        syncStatus: "deleted",
        lastSyncedAt: now,
        updatedAt: now,
        deletedAt: now,
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
      });
      const users = await ctx.db
        .query("users")
        .withIndex("by_org", (q) => q.eq("orgId", organization._id))
        .collect();
      for (const user of users) {
        if (user.syncStatus !== "deleted") {
          await ctx.db.patch(user._id, {
            syncStatus: "deleted",
            deletedAt: now,
            lastSyncedAt: now,
          });
        }
      }
    }
    if (tombstone) {
      await ctx.db.patch(tombstone._id, {
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
        removedAt: now,
      });
    } else {
      await ctx.db.insert("organizationTombstones", {
        externalOrgId: args.orgId,
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
        removedAt: now,
      });
    }
    const sessions = await ctx.db
      .query("controlSessions")
      .filter((q) => q.eq(q.field("externalOrgId"), args.orgId))
      .collect();
    for (const session of sessions) await ctx.db.delete(session._id);
    return { status: "removed", organizationId: organization?._id };
  },
});

export const onOrganizationMembershipProjectionChanged = internalMutation({
  args: {
    action: v.union(v.literal("upsert"), v.literal("remove")),
    eventId: v.string(),
    orgId: v.string(),
    userId: v.string(),
    email: v.optional(v.string()),
    role: v.optional(v.string()),
    revision: v.number(),
    organizationRevision: v.number(),
  },
  handler: async (ctx, args) => {
    if (
      !Number.isSafeInteger(args.revision) ||
      args.revision <= 0 ||
      !Number.isSafeInteger(args.organizationRevision) ||
      args.organizationRevision <= 0
    ) {
      throw new Error("membership revisions must be positive safe integers");
    }
    if (!args.orgId.trim() || !args.userId.trim() || !args.eventId.trim()) {
      throw new Error("membership projection identity is required");
    }
    const normalizedEmail = args.email?.trim().toLowerCase();
    const normalizedRole =
      args.action === "upsert" && args.role
        ? normalizeProjectionRole(args.role)
        : undefined;
    if (
      args.action === "upsert" &&
      (!normalizedEmail || !normalizedRole)
    ) {
      throw new Error("membership upsert requires email and role");
    }

    const organizationTombstones = await ctx.db
      .query("organizationTombstones")
      .withIndex("by_external_org", (q) => q.eq("externalOrgId", args.orgId))
      .collect();
    if (organizationTombstones.length > 0) {
      return { status: "organization_permanently_removed" };
    }
    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q) => q.eq("externalOrgId", args.orgId))
      .collect();
    if (organizations.length !== 1 || organizations[0].syncStatus === "deleted") {
      throw new Error("active organization projection is unavailable");
    }
    const organization = organizations[0];
    if (
      !organization.sourceRevision ||
      organization.sourceRevision < args.organizationRevision
    ) {
      throw new Error("organization projection revision is not ready");
    }

    const memberships = await ctx.db
      .query("users")
      .withIndex("by_external_and_org", (q) =>
        q.eq("externalAuthId", args.userId).eq("orgId", organization._id),
      )
      .collect();
    if (memberships.length > 1) {
      throw new Error("membership projection is ambiguous");
    }
    const membership = memberships[0];
    const tombstones = await ctx.db
      .query("membershipTombstones")
      .withIndex("by_external_org_and_user", (q) =>
        q.eq("externalOrgId", args.orgId).eq("externalAuthId", args.userId),
      )
      .collect();
    if (tombstones.length > 1) {
      throw new Error("membership tombstone is ambiguous");
    }
    const tombstone = tombstones[0];
    const membershipState = membership
      ? storedProjectionState({
          kind: membership.syncStatus === "deleted" ? "removed" : "active",
          ...membership,
        })
      : undefined;
    const tombstoneState = tombstone
      ? storedProjectionState({ kind: "removed", ...tombstone })
      : undefined;
    if (
      membershipState &&
      tombstoneState &&
      membershipState.revision === tombstoneState.revision &&
      !sameProjectionState(membershipState, tombstoneState)
    ) {
      throw new Error("stored same-revision membership conflict");
    }
    const current =
      tombstoneState &&
      (!membershipState || tombstoneState.revision > membershipState.revision)
        ? tombstoneState
        : membershipState;
    const incoming = {
      action: args.action,
      revision: args.revision,
      eventId: args.eventId,
      fingerprint: membershipProjectionFingerprint({
        action: args.action,
        email: normalizedEmail,
        role: normalizedRole,
        organizationRevision: args.organizationRevision,
      }),
    } as const;
    const next = applyMembershipProjectionState(current, incoming);
    if (sameProjectionState(current, next)) {
      return { status: "unchanged" };
    }

    const now = Date.now();
    if (args.action === "upsert") {
      if (membership) {
        await ctx.db.patch(membership._id, {
          email: normalizedEmail!,
          role: normalizedRole!,
          syncStatus: "synced",
          lastSyncedAt: now,
          lastSeenAt: membership.lastSeenAt || now,
          deletedAt: undefined,
          sourceRevision: next.revision,
          sourceEventId: next.eventId,
          sourceFingerprint: next.fingerprint,
        });
      } else {
        await ctx.db.insert("users", {
          externalAuthId: args.userId,
          email: normalizedEmail!,
          name: normalizedEmail!.split("@")[0],
          orgId: organization._id,
          role: normalizedRole!,
          syncStatus: "synced",
          lastSyncedAt: now,
          createdAt: now,
          lastSeenAt: now,
          sourceRevision: next.revision,
          sourceEventId: next.eventId,
          sourceFingerprint: next.fingerprint,
        });
      }
      if (tombstone) await ctx.db.delete(tombstone._id);
      return { status: membership ? "updated" : "created" };
    }

    if (membership) {
      await ctx.db.patch(membership._id, {
        syncStatus: "deleted",
        deletedAt: now,
        lastSyncedAt: now,
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
      });
    }
    if (tombstone) {
      await ctx.db.patch(tombstone._id, {
        sourceUpdatedAt: now,
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
        removedAt: now,
      });
    } else {
      await ctx.db.insert("membershipTombstones", {
        externalOrgId: args.orgId,
        externalAuthId: args.userId,
        sourceUpdatedAt: now,
        sourceRevision: next.revision,
        sourceEventId: next.eventId,
        sourceFingerprint: next.fingerprint,
        removedAt: now,
      });
    }
    const sessions = await ctx.db
      .query("controlSessions")
      .withIndex("by_external_user_and_org", (q) =>
        q.eq("externalUserId", args.userId).eq("externalOrgId", args.orgId),
      )
      .collect();
    for (const session of sessions) await ctx.db.delete(session._id);
    return { status: membership ? "removed" : "tombstoned" };
  },
});

/**
 * INTERNAL: Handle user registration event
 * Called via NATS when auth-service publishes auth.user.registered
 * 
 * Payload: { userId: string, email: string, name: string, createdAt: number }
 */
export const onUserRegistered = internalAction(async (ctx, args: any) => {
  const { userId, email, name, createdAt } = args;

  console.log(`[Convex] Processing user.registered: ${userId}`);

  // Note: We don't create the user here yet as we need to know which org they belong to
  // The user will be created when they're invited to an org or create one

  return { status: "processed", userId };
});

/**
 * INTERNAL: Handle organization created event
 * Called via NATS when auth-service publishes organization.created
 * 
 * Payload: { orgId: string, name: string, slug: string, createdAt: number }
 */
export const onOrganizationCreated = internalAction(
  async (ctx, args: { orgId: string; name: string; slug: string; createdAt: number }) => {
    const { orgId, name, slug, createdAt } = args;

    console.log(`[Convex] Processing organization.created: ${slug}`);

    try {
      // Check if organization already exists
      const existing = await ctx.runQuery(api.organizations.getByExternalId, {
        externalOrgId: orgId,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      });

      if (existing) {
        console.log(`[Convex] Organization already synced: ${orgId}`);
        return { status: "already_synced", convexOrgId: existing._id };
      }

      // Create organization in Convex
      const convexOrgId = await ctx.runMutation(internal.organizations.createFromExternal, {
        externalOrgId: orgId,
        name,
        slug,
        externalCreatedAt: createdAt,
      });

      console.log(`[Convex] Organization synced: ${slug} -> ${convexOrgId}`);

      return { status: "synced", convexOrgId };
    } catch (error) {
      console.error(`[Convex] Error syncing organization ${orgId}:`, error);
      throw error;
    }
  }
);

/**
 * INTERNAL: Handle organization member added event
 * Called via NATS when org-core publishes organization.member.added
 * 
 * Payload: { orgId: string, userId: string, email: string, role: string, addedAt: number }
 */
export const onOrganizationMemberAdded = internalAction(
  async (ctx, args: { orgId: string; userId: string; email: string; role: string; addedAt: number }) => {
    const { orgId, userId, email, role, addedAt } = args;

    console.log(`[Convex] Processing organization.member.added: ${userId} -> ${orgId}`);

    try {
      // Get the Convex org ID from the external org ID
      const org = await ctx.runQuery(api.organizations.getByExternalId, {
        externalOrgId: orgId,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      });

      if (!org) {
        throw new Error(`Organization projection not found: ${orgId}`);
      }

      // Create user in Convex if they don't exist
      const user = await ctx.runMutation(internal.users.createOrUpdateFromExternal, {
        externalAuthId: userId,
        email,
        convexOrgId: org._id,
        role: normalizeProjectionRole(role),
        externalCreatedAt: addedAt,
        sourceUpdatedAt: addedAt,
      });

      if (!user) {
        return { status: "stale_ignored", orgId: org._id };
      }

      console.log(`[Convex] User synced: ${userId} (${user._id}) in org ${org._id}`);

      return { status: "synced", userId: user._id, orgId: org._id };
    } catch (error) {
      console.error(`[Convex] Error syncing member ${userId}:`, error);
      throw error;
    }
  }
);

export const removeOrganizationMemberProjection = internalMutation({
  args: {
    orgId: v.string(),
    userId: v.string(),
    sourceUpdatedAt: v.number(),
  },
  handler: async (ctx, args) => {
    if (!args.orgId.trim() || !args.userId.trim()) {
      throw new Error("orgId and userId are required");
    }

    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q) => q.eq("externalOrgId", args.orgId))
      .collect();
    const organization = organizations.find((item) => item.syncStatus !== "deleted");

    let membership = null;
    if (organization) {
      const memberships = await ctx.db
        .query("users")
        .withIndex("by_external_and_org", (q) =>
          q.eq("externalAuthId", args.userId).eq("orgId", organization._id)
        )
        .collect();
      membership = memberships[0] ?? null;
      if (
        memberships.length > 0 &&
        memberships.every((candidate) =>
          !shouldApplyMembershipRemoval(
            candidate.sourceUpdatedAt,
            args.sourceUpdatedAt
          )
        )
      ) {
        return { status: "stale_ignored" };
      }
      for (const candidate of memberships) {
        if (
          candidate.syncStatus !== "deleted" &&
          shouldApplyMembershipRemoval(
            candidate.sourceUpdatedAt,
            args.sourceUpdatedAt
          )
        ) {
          await ctx.db.patch(candidate._id, {
            syncStatus: "deleted",
            deletedAt: Date.now(),
            lastSyncedAt: Date.now(),
            sourceUpdatedAt: args.sourceUpdatedAt,
          });
        }
      }
    }

    const tombstones = await ctx.db
      .query("membershipTombstones")
      .withIndex("by_external_org_and_user", (q) =>
        q.eq("externalOrgId", args.orgId).eq("externalAuthId", args.userId)
      )
      .collect();
    const latestTombstone = tombstones.sort(
      (left, right) => right.sourceUpdatedAt - left.sourceUpdatedAt
    )[0];
    if (latestTombstone && latestTombstone.sourceUpdatedAt >= args.sourceUpdatedAt) {
      return { status: "already_removed" };
    }

    const removedAt = Date.now();
    if (latestTombstone) {
      await ctx.db.patch(latestTombstone._id, {
        sourceUpdatedAt: args.sourceUpdatedAt,
        removedAt,
      });
    } else {
      await ctx.db.insert("membershipTombstones", {
        externalOrgId: args.orgId,
        externalAuthId: args.userId,
        sourceUpdatedAt: args.sourceUpdatedAt,
        removedAt,
      });
    }

    for (const duplicate of tombstones.filter(
      (item) => item._id !== latestTombstone?._id
    )) {
      await ctx.db.delete(duplicate._id);
    }

    const controlSessions = await ctx.db
      .query("controlSessions")
      .withIndex("by_external_user_and_org", (q) =>
        q.eq("externalUserId", args.userId).eq("externalOrgId", args.orgId)
      )
      .collect();
    for (const session of controlSessions) {
      await ctx.db.delete(session._id);
    }

    if (organization) {
      await ctx.db.insert("auditLog", {
        orgId: organization._id,
        userId: membership?._id,
        action: "membership.projection.removed",
        resource: "user_membership",
        resourceId: args.userId,
        changes: { sourceUpdatedAt: args.sourceUpdatedAt },
        createdAt: removedAt,
      });
    }

    return { status: membership ? "removed" : "tombstoned" };
  },
});

export const onOrganizationMemberRemoved = internalAction(
  async (
    ctx,
    args: { orgId: string; userId: string; sourceUpdatedAt: number }
  ) => {
    return await ctx.runMutation(internal.nats.removeOrganizationMemberProjection, {
      orgId: args.orgId,
      userId: args.userId,
      sourceUpdatedAt: args.sourceUpdatedAt,
    });
  }
);

/**
 * INTERNAL: Handle organization updated event
 * Called via NATS when org-core publishes organization.updated
 * 
 * Payload: { orgId: string, name?: string, slug?: string, settings?: any, updatedAt: number }
 */
export const onOrganizationUpdated = internalAction(
  async (
    ctx,
    args: {
      orgId: string;
      name?: string;
      slug?: string;
      settings?: any;
      updatedAt: number;
    }
  ) => {
    const { orgId, name, slug, settings, updatedAt } = args;

    console.log(`[Convex] Processing organization.updated: ${orgId}`);

    try {
      const org = await ctx.runQuery(api.organizations.getByExternalId, {
        externalOrgId: orgId,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      });

      if (!org) {
        console.warn(`[Convex] Organization not found for update: ${orgId}`);
        return { status: "org_not_found" };
      }

      // Update organization
      await ctx.runMutation(internal.organizations.updateFromExternal, {
        convexOrgId: org._id,
        name,
        slug,
        settings,
      });

      console.log(`[Convex] Organization updated: ${orgId}`);

      return { status: "updated", convexOrgId: org._id };
    } catch (error) {
      console.error(`[Convex] Error updating organization ${orgId}:`, error);
      throw error;
    }
  }
);

/**
 * INTERNAL: Handle organization deleted event
 * Called via NATS when org-core publishes organization.deleted
 * 
 * Payload: { orgId: string }
 */
export const onOrganizationDeleted = internalAction(async (ctx, args: { orgId: string }) => {
  const { orgId } = args;

  console.log(`[Convex] Processing organization.deleted: ${orgId}`);

  try {
    const org = await ctx.runQuery(api.organizations.getByExternalId, {
      externalOrgId: orgId,
      serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
    });

    if (!org) {
      console.warn(`[Convex] Organization not found for deletion: ${orgId}`);
      return { status: "not_found" };
    }

    // Soft delete organization
    await ctx.runMutation(internal.organizations.remove, {
      convexOrgId: org._id,
    });

    console.log(`[Convex] Organization deleted: ${orgId}`);

    return { status: "deleted", convexOrgId: org._id };
  } catch (error) {
    console.error(`[Convex] Error deleting organization ${orgId}:`, error);
    throw error;
  }
});

/**
 * INTERNAL: Handle import completed event
 * Called via NATS when Ingestion Plane publishes verevon.ingestion.import.completed
 *
 * Payload: { importId: string, orgId: string, sourceType: string, totalDocuments: number, completedAt: number }
 */
export const onImportCompleted = internalAction(
  async (
    ctx,
    args: {
      importId: string;
      orgId: string;
      sourceType: string;
      totalDocuments: number;
      completedAt: number;
    }
  ) => {
    const { importId, orgId, sourceType, totalDocuments, completedAt } = args;

    console.log(`[Convex] Processing import.completed: ${importId} for org ${orgId}`);

    try {
      const org = await ctx.runQuery(api.organizations.getByExternalId, {
        externalOrgId: orgId,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      });

      if (!org) {
        console.warn(`[Convex] Organization not found for import: ${orgId}`);
        return { status: "org_not_found", orgId };
      }

      await ctx.runMutation(internal.imports.recordCompleted, {
        externalOrgId: orgId,
        externalImportId: importId,
        sourceType,
        totalDocuments,
        completedAt,
      });

      console.log(`[Convex] Import recorded: ${importId} (${totalDocuments} docs, ${sourceType})`);

      return { status: "recorded", importId, convexOrgId: org._id };
    } catch (error) {
      console.error(`[Convex] Error recording import ${importId}:`, error);
      throw error;
    }
  }
);

// ---------------------------------------------------------------------------
// Quarry Crawl Job Handlers
// Called via HTTP actions from nats-subscriber.js when quarry publishes
// verevon.ingestion.crawl.{started,progress,completed,failed} events.
// These use internalMutation for direct DB writes with full Convex reactivity.
// ---------------------------------------------------------------------------

const now = () => Date.now();

/**
 * Handle verevon.ingestion.crawl.started
 * Upserts an ingestJob record with status=running.
 */
export const onCrawlStarted = internalMutation(
  async (
    ctx,
    args: { crawlId: string; orgId: string; url: string; startedAt: number }
  ) => {
    const { crawlId, orgId, url, startedAt } = args;
    const existing = await ctx.db
      .query("ingestJobs")
      .withIndex("by_external_id", (q) => q.eq("externalJobId", crawlId))
      .first();
    if (existing) {
      await ctx.db.patch(existing._id, {
        status: "running",
        progress: 0,
        updatedAt: startedAt,
      });
    } else {
      await ctx.db.insert("ingestJobs", {
        externalJobId: crawlId,
        externalOrgId: orgId || undefined,
        type: "crawl",
        status: "running",
        url,
        progress: 0,
        startedAt,
        createdAt: startedAt,
        updatedAt: startedAt,
      });
    }
  }
);

/**
 * Handle verevon.ingestion.crawl.progress
 */
export const onCrawlProgress = internalMutation(
  async (
    ctx,
    args: {
      crawlId: string;
      orgId: string;
      progress: number;
      completed: number;
      total: number;
      message: string;
      updatedAt: number;
    }
  ) => {
    const { crawlId, progress, completed, total, message, updatedAt } = args;
    const existing = await ctx.db
      .query("ingestJobs")
      .withIndex("by_external_id", (q) => q.eq("externalJobId", crawlId))
      .first();
    if (existing) {
      await ctx.db.patch(existing._id, {
        progress,
        progressMessage: message || `${completed}/${total} pages`,
        updatedAt,
      });
    }
  }
);

/**
 * Handle verevon.ingestion.crawl.completed
 * Crawl scraping is done — pages collected. Transitions to "indexing" while
 * Quarry pushes documents to the data plane. "completed" is set by onCrawlIndexed.
 */
export const onCrawlCompleted = internalMutation(
  async (
    ctx,
    args: {
      crawlId: string;
      orgId: string;
      url: string;
      pageCount: number;
      completedAt: number;
    }
  ) => {
    const { crawlId, orgId, url, pageCount, completedAt } = args;
    const existing = await ctx.db
      .query("ingestJobs")
      .withIndex("by_external_id", (q) => q.eq("externalJobId", crawlId))
      .first();
    if (existing) {
      await ctx.db.patch(existing._id, {
        status: "indexing",
        progress: 100,
        progressMessage: "Indexing pages...",
        pageCount,
        updatedAt: completedAt,
      });
    } else {
      await ctx.db.insert("ingestJobs", {
        externalJobId: crawlId,
        externalOrgId: orgId || undefined,
        type: "crawl",
        status: "indexing",
        url,
        progress: 100,
        progressMessage: "Indexing pages...",
        pageCount,
        createdAt: completedAt,
        updatedAt: completedAt,
      });
    }
  }
);

/**
 * Handle verevon.ingestion.crawl.indexed
 * Data-plane ingestion is complete — all pages submitted for embedding.
 * Transitions job to "completed".
 */
export const onCrawlIndexed = internalMutation(
  async (
    ctx,
    args: {
      crawlId: string;
      orgId: string;
      ingestedCount: number;
      indexedAt: number;
    }
  ) => {
    const { crawlId, ingestedCount, indexedAt } = args;
    const existing = await ctx.db
      .query("ingestJobs")
      .withIndex("by_external_id", (q) => q.eq("externalJobId", crawlId))
      .first();
    if (existing) {
      await ctx.db.patch(existing._id, {
        status: "completed",
        progressMessage: `${ingestedCount} pages indexed`,
        completedAt: indexedAt,
        updatedAt: indexedAt,
      });
    }
  }
);

/**
 * Handle verevon.ingestion.crawl.failed
 */
export const onCrawlFailed = internalMutation(
  async (
    ctx,
    args: {
      crawlId: string;
      orgId: string;
      url: string;
      error: string;
      failedAt: number;
    }
  ) => {
    const { crawlId, orgId, url, error, failedAt } = args;
    const existing = await ctx.db
      .query("ingestJobs")
      .withIndex("by_external_id", (q) => q.eq("externalJobId", crawlId))
      .first();
    if (existing) {
      await ctx.db.patch(existing._id, {
        status: "failed",
        error,
        failedAt,
        updatedAt: failedAt,
      });
    } else {
      await ctx.db.insert("ingestJobs", {
        externalJobId: crawlId,
        externalOrgId: orgId || undefined,
        type: "crawl",
        status: "failed",
        url,
        progress: 0,
        error,
        failedAt,
        createdAt: failedAt,
        updatedAt: failedAt,
      });
    }
  }
);

/**
 * U3-3 (ui-ux-verevon-gap.md §10) — Agent run lifecycle handler.
 *
 * Receives the inner payload from a `mp.v1.run.{runId}.event` envelope
 * after `nats-subscriber.js` has unwrapped it. Maps the orchestrator's
 * event type to one of the four statuses the `agentRuns` table accepts
 * and upserts the row.
 *
 * Expected payload (matches activities.go::publishRunEvent):
 *   {
 *     run_id:        string,                       // primary key
 *     event_type:    "RUN_STARTED" | "RUN_COMPLETED" | "RUN_FAILED" | "RUN_CANCELLED",
 *     org_id:        string,                       // mapped to externalOrgId
 *     user_id?:      string,
 *     agent_id?:     string,
 *     error?:        string,
 *     ts_ms?:        number,                       // event time
 *     ...extra fields preserved in payload
 *   }
 */
export const onAgentRunEvent = internalAction({
  args: {
    run_id: v.string(),
    event_type: v.string(),
    org_id: v.optional(v.string()),
    user_id: v.optional(v.string()),
    agent_id: v.optional(v.string()),
    error: v.optional(v.string()),
    ts_ms: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const status = mapAgentRunStatus(args.event_type);
    if (!status) {
      console.log(`[Convex NATS] Ignoring unknown agent run event: ${args.event_type}`);
      return { status: "ignored", event_type: args.event_type };
    }
    if (!args.org_id) {
      console.warn(
        `[Convex NATS] agent run event missing org_id; skipping run_id=${args.run_id}`,
      );
      return { status: "skipped", reason: "no_org_id" };
    }

    await ctx.runMutation(internal.agentRuns.upsertAgentRun, {
      runId: args.run_id,
      externalOrgId: args.org_id,
      externalUserId: args.user_id,
      agentId: args.agent_id,
      status,
      error: args.error,
      payload: undefined,
      eventTimestampMs: args.ts_ms,
    });

    return { status: "processed", run_id: args.run_id, new_status: status };
  },
});

function mapAgentRunStatus(
  eventType: string,
):
  | "started"
  | "completed"
  | "failed"
  | "cancelled"
  | null {
  switch (eventType) {
    case "RUN_STARTED":
      return "started";
    case "RUN_COMPLETED":
      return "completed";
    case "RUN_FAILED":
      return "failed";
    case "RUN_CANCELLED":
      return "cancelled";
    default:
      return null;
  }
}
