/**
 * NATS Integration Module
 * 
 * Subscribes to velion cross-plane NATS events from Control Plane and
 * Ingestion Plane to keep Convex database synchronized.
 * 
 * Events subscribed to:
 * - velion.controlplane.org.{created,updated,deleted}
 * - velion.controlplane.org.member.{added,removed}
 * - velion.ingestion.import.completed
 */

import { v } from "convex/values";
import { internalAction, internalMutation } from "./_generated/server";
import { api, internal } from "./_generated/api";

// Control-Plane-owned shared key; no hardcoded fallback. Empty only if the env
// is misconfigured, in which case the receiving validator rejects the call.
const CONVEX_INTERNAL_SERVICE_KEY =
  process.env.CONVEX_INTERNAL_SERVICE_KEY ||
  process.env.INTERNAL_API_KEY ||
  "";

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

/**
 * INTERNAL: Handle user registration event
 * Called via NATS when auth-service publishes auth.user.registered
 * 
 * Payload: { userId: string, email: string, name: string, createdAt: number }
 */
export const onUserRegistered = internalAction(async (ctx, args: any) => {
  const { userId, email, name, createdAt } = args;

  console.log(`[Convex] Processing user.registered: ${email}`);

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
      const convexOrgId = await ctx.runMutation(api.organizations.createFromExternal, {
        externalOrgId: orgId,
        name,
        slug,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
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

    console.log(`[Convex] Processing organization.member.added: ${email} -> ${orgId}`);

    try {
      // Get the Convex org ID from the external org ID
      const org = await ctx.runQuery(api.organizations.getByExternalId, {
        externalOrgId: orgId,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      });

      if (!org) {
        console.warn(`[Convex] Organization not found: ${orgId}`);
        return { status: "org_not_found", orgId };
      }

      // Create user in Convex if they don't exist
      const user = await ctx.runMutation(api.users.createOrUpdateFromExternal, {
        externalAuthId: userId,
        email,
        convexOrgId: org._id,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
        role,
        externalCreatedAt: addedAt,
      });

      console.log(`[Convex] User synced: ${email} (${user._id}) in org ${org._id}`);

      return { status: "synced", userId: user._id, orgId: org._id };
    } catch (error) {
      console.error(`[Convex] Error syncing member ${email}:`, error);
      throw error;
    }
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
      await ctx.runMutation(api.organizations.updateFromExternal, {
        convexOrgId: org._id,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
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
    await ctx.runMutation(api.organizations.remove, {
      convexOrgId: org._id,
      serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
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
 * Called via NATS when Ingestion Plane publishes velion.ingestion.import.completed
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

      await ctx.runMutation(api.imports.recordCompleted, {
        convexOrgId: org._id,
        externalImportId: importId,
        sourceType,
        totalDocuments,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
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
// velion.ingestion.crawl.{started,progress,completed,failed} events.
// These use internalMutation for direct DB writes with full Convex reactivity.
// ---------------------------------------------------------------------------

const now = () => Date.now();

/**
 * Handle velion.ingestion.crawl.started
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
 * Handle velion.ingestion.crawl.progress
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
 * Handle velion.ingestion.crawl.completed
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
 * Handle velion.ingestion.crawl.indexed
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
 * Handle velion.ingestion.crawl.failed
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
 * U3-3 (ui-ux-velion-gap.md §10) — Agent run lifecycle handler.
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
