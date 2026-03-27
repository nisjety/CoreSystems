/**
 * Users - Convex Functions
 * 
 * Handles user data management and synchronization with external services.
 * External services (auth-core, user-core, org-core) publish events via NATS which trigger syncs.
 */

import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey } from "./authz";

/**
 * Query: Get user by external auth ID
 * Used internally to check if a user has already been synced
 */
export const getByExternalAuthId = query({
  args: {
    externalAuthId: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const users = await ctx.db
      .query("users")
      .filter((q) => q.eq(q.field("externalAuthId"), args.externalAuthId))
      .collect();
    return users[0] || null;
  },
});

/**
 * Query: Get user by external auth ID within a specific organization
 */
export const getByExternalAndOrg = query({
  args: {
    externalAuthId: v.string(),
    orgId: v.id("organizations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const users = await ctx.db
      .query("users")
      .withIndex("by_external_and_org", (q) =>
        q.eq("externalAuthId", args.externalAuthId).eq("orgId", args.orgId)
      )
      .collect();

    return users[0] || null;
  },
});

/**
 * Query: List all Convex memberships for an external auth user.
 * Used as a resilience fallback when control-plane org context is temporarily unavailable.
 */
export const listByExternalAuthId = query({
  args: {
    externalAuthId: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const users = await ctx.db
      .query("users")
      .filter((q) => q.eq(q.field("externalAuthId"), args.externalAuthId))
      .collect();

    return users.sort((left, right) => {
      const rightActivity = right.lastSeenAt ?? right.lastSyncedAt ?? right.createdAt ?? 0;
      const leftActivity = left.lastSeenAt ?? left.lastSyncedAt ?? left.createdAt ?? 0;
      return rightActivity - leftActivity;
    });
  },
});

/**
 * Query: Get user by email and org
 */
export const getByEmailAndOrg = query({
  args: {
    email: v.string(),
    orgId: v.id("organizations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const users = await ctx.db
      .query("users")
      .filter(
        (q) =>
          q.and(
            q.eq(q.field("email"), args.email),
            q.eq(q.field("orgId"), args.orgId)
          )
      )
      .collect();
    return users[0] || null;
  },
});

/**
 * Query: List users in organization
 */
export const listByOrg = query({
  args: {
    orgId: v.id("organizations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    return await ctx.db
      .query("users")
      .filter((q) => q.eq(q.field("orgId"), args.orgId))
      .collect();
  },
});

/**
 * Query: Get user by ID with organization details
 */
export const getWithOrg = query({
  args: {
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const user = await ctx.db.get(args.userId);
    if (!user) return null;

    const org = await ctx.db.get(user.orgId);

    return { ...user, organization: org };
  },
});

/**
 * Mutation: Create or update user from external sync
 * Called by NATS integration when org-core publishes organization.member.added
 * or auth-core publishes user registration
 */
export const createOrUpdateFromExternal = mutation({
  args: {
    externalAuthId: v.string(),
    email: v.string(),
    convexOrgId: v.id("organizations"),
    serviceKey: v.string(),
    role: v.string(),
    name: v.optional(v.string()),
    externalCreatedAt: v.number(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const { externalAuthId, email, convexOrgId, role, name, externalCreatedAt } = args;

    // Check if user already exists
    const existing = await ctx.db
      .query("users")
      .filter(
        (q) =>
          q.and(
            q.eq(q.field("externalAuthId"), externalAuthId),
            q.eq(q.field("orgId"), convexOrgId)
          )
      )
      .collect();

    if (existing.length > 0) {
      // Update existing user
      const user = existing[0];
      await ctx.db.patch(user._id, {
        email,
        role,
        name: name || user.name,
        syncStatus: "synced",
        lastSyncedAt: Date.now(),
      });
      return user;
    }

    // Create new user
    const userId = await ctx.db.insert("users", {
      externalAuthId,
      email,
      name: name || String(email).split("@")[0],
      orgId: convexOrgId,
      role: role as unknown as "admin" | "member" | "viewer",
      syncStatus: "synced",
      lastSyncedAt: Date.now(),
      createdAt: externalCreatedAt,
      lastSeenAt: externalCreatedAt,
    });

    return await ctx.db.get(userId);
  },
});

/**
 * Mutation: Update user role
 */
export const updateRole = mutation({
  args: {
    userId: v.id("users"),
    serviceKey: v.string(),
    role: v.union(v.literal("admin"), v.literal("member"), v.literal("viewer")),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const user = await ctx.db.get(args.userId);
    if (!user) throw new Error(`User not found: ${args.userId}`);

    await ctx.db.patch(args.userId, { role: args.role });
    return user;
  },
});

/**
 * Mutation: Update user last seen timestamp
 * Called when user interacts with the system
 */
export const updateLastSeen = mutation({
  args: {
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await ctx.db.patch(args.userId, { lastSeenAt: Date.now() });
  },
});

/**
 * Remove a user completely
 * (Called down from Control Plane sync)
 */
export const remove = mutation({
  args: {
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await ctx.db.patch(args.userId, {
      syncStatus: "deleted",
      deletedAt: Date.now(),
    });
  },
});
