/**
 * NATS Integration Module
 * 
 * Subscribes to NATS events from auth-service, user-service, and org-core
 * to keep Convex database synchronized with the control-plane services.
 * 
 * Events subscribed to:
 * - auth.user.registered, auth.user.updated, auth.user.deleted
 * - organization.created, organization.updated, organization.deleted
 * - organization.member.added, organization.member.removed
 */

import { internalAction } from "./_generated/server";
import { api } from "./_generated/api";

const CONVEX_INTERNAL_SERVICE_KEY =
  process.env.CONVEX_INTERNAL_SERVICE_KEY ||
  process.env.INTERNAL_API_KEY ||
  "change-me-internal-service-secret";

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
