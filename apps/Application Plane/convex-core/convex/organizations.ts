/**
 * Organizations - Convex Functions
 * 
 * Handles organization data management and synchronization with external services.
 * External services (auth-core, org-core) publish events via NATS which trigger syncs.
 */

import { internalMutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey } from "./authz";

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
