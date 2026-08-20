/**
 * Organizations - Convex Functions
 * 
 * Handles organization data management and synchronization with external services.
 * External services (auth-core, org-core) publish events via NATS which trigger syncs.
 */

import { internalMutation, mutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey, requireGatewayMember } from "./authz";

const MAX_INSTRUCTIONS_LENGTH = 4000;

/**
 * Query: Get organization by external ID
 * Used internally to check if an org has already been synced
 */
export const getByExternalId = query({
  args: {
    externalOrgId: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const orgs = await ctx.db
      .query("organizations")
      .filter((q) => q.eq(q.field("externalOrgId"), args.externalOrgId))
      .collect();
    return orgs[0] || null;
  },
});

/**
 * Query: Get organization by Convex document ID
 */
export const getById = query({
  args: {
    orgId: v.id("organizations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    return await ctx.db.get(args.orgId);
  },
});

/**
 * Query: Get organization by slug
 */
export const getBySlug = query({
  args: {
    slug: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const orgs = await ctx.db
      .query("organizations")
      .filter((q) => q.eq(q.field("slug"), args.slug))
      .collect();
    return orgs[0] || null;
  },
});

/**
 * Query: List organizations for current user
 */
export const listForUser = query({
  args: {
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const users = await ctx.db
      .query("users")
      .filter((q) => q.eq(q.field("_id"), args.userId))
      .collect();

    if (users.length === 0) return [];

    const user = users[0];
    return await ctx.db
      .query("organizations")
      .filter((q) => q.eq(q.field("_id"), user.orgId))
      .collect();
  },
});

/**
 * Mutation: Create organization from external sync
 * Called by NATS integration when org-core publishes organization.created
 */
export const createFromExternal = internalMutation({
  args: {
    externalOrgId: v.string(),
    name: v.string(),
    slug: v.string(),
    externalCreatedAt: v.number(),
  },
  handler: async (ctx, args) => {
    const { externalOrgId, name, slug, externalCreatedAt } = args;

    const orgId = await ctx.db.insert("organizations", {
      externalOrgId,
      name,
      slug,
      syncStatus: "synced",
      lastSyncedAt: Date.now(),
      settings: {},
      createdAt: externalCreatedAt,
      updatedAt: Date.now(),
    });

    return orgId;
  },
});

/**
 * Mutation: Update organization from external sync
 */
export const updateFromExternal = internalMutation({
  args: {
    convexOrgId: v.id("organizations"),
    name: v.optional(v.string()),
    slug: v.optional(v.string()),
    settings: v.optional(v.any()),
  },
  handler: async (ctx, args) => {
    const { convexOrgId, name, slug, settings } = args;

    const org = await ctx.db.get(convexOrgId);
    if (!org) throw new Error(`Organization not found: ${convexOrgId}`);

    const updates: any = {
      syncStatus: "synced",
      lastSyncedAt: Date.now(),
      updatedAt: Date.now(),
    };

    if (name !== undefined) updates.name = name;
    if (slug !== undefined) updates.slug = slug;
    if (settings !== undefined) updates.settings = settings;

    await ctx.db.patch(convexOrgId, updates);

    return org;
  },
});

/**
 * Remove an organization completely
 * (Called down from Control Plane sync)
 */
export const remove = internalMutation({
  args: {
    convexOrgId: v.id("organizations"),
  },
  handler: async (ctx, args) => {
    await ctx.db.patch(args.convexOrgId, {
      syncStatus: "deleted",
      deletedAt: Date.now(),
    });
  },
});

/**
 * Query: Get single organization with users
 */
export const getWithUsers = query({
  args: {
    orgId: v.id("organizations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const org = await ctx.db.get(args.orgId);
    if (!org) return null;

    const users = await ctx.db
      .query("users")
      .filter((q) => q.eq(q.field("orgId"), args.orgId))
      .collect();

    return { ...org, users };
  },
});

/**
 * ADR-0003 -- the org layer of the authored-instruction hierarchy. Read by
 * model-gateway (via the BFF) on every chat turn to compose alongside the
 * platform and Space layers. `null` (no org, or nothing authored) is the
 * common case and produces no system-message segment downstream.
 */
export const instructionsForGateway = query({
  args: {
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
    if (!organization) return null;
    return { instructions: organization.instructions ?? null };
  },
});

/**
 * ADR-0003 -- org-admin authoring write. The gateway verifies the caller is
 * an org owner/admin (`has_authorized_org_role`) before calling this; this
 * mutation only re-verifies org membership (`requireGatewayMember`), the same
 * trust split every other `*ForGateway` mutation in this file uses -- the
 * admin decision itself is not re-derived here.
 */
export const setInstructionsForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    instructions: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);

    const instructions = args.instructions?.trim() || undefined;
    if ((instructions?.length ?? 0) > MAX_INSTRUCTIONS_LENGTH) {
      throw new Error(`Organization instructions must be ${MAX_INSTRUCTIONS_LENGTH} characters or fewer`);
    }

    await ctx.db.patch(organization._id, {
      instructions,
      updatedAt: Date.now(),
      updatedByExternalAuthId: args.externalAuthId,
    });

    return { instructions: instructions ?? null };
  },
});
