// Per-org Projects — backs the "Add to project" picker in
// ChatSettingsModal (velion ui-ux-velion-gap.md §10 / U2-14 follow-up).
//
// Velion calls these via /api/projects (REST proxy). Pure CRUD, scoped by
// `externalOrgId`. No internal-key gate at the Convex layer — Convex's
// own admin-key middleware (set when the velion proxy forwards the call)
// is the access boundary.

import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { requireEditorMembership, requireViewerMembership } from "./authz";

// ─────────────────────────────────────────────────────────────────────────
// Queries
// ─────────────────────────────────────────────────────────────────────────

export const listByOrg = query({
  args: {
    externalOrgId: v.string(),
    includeArchived: v.optional(v.boolean()),
  },
  handler: async (ctx, { externalOrgId, includeArchived }) => {
    await requireViewerMembership(ctx, externalOrgId);
    const archived = includeArchived === true;
    if (archived) {
      return await ctx.db
        .query("projects")
        .withIndex("by_external_org", (q) => q.eq("externalOrgId", externalOrgId))
        .order("desc")
        .take(100);
    }
    return await ctx.db
      .query("projects")
      .withIndex("by_org_and_archived", (q) =>
        q.eq("externalOrgId", externalOrgId).eq("archived", false),
      )
      .order("desc")
      .take(100);
  },
});

export const getById = query({
  args: { projectId: v.id("projects") },
  handler: async (ctx, { projectId }) => {
    const project = await ctx.db.get(projectId);
    if (!project) return null;
    await requireViewerMembership(ctx, project.externalOrgId);
    return project;
  },
});

// ─────────────────────────────────────────────────────────────────────────
// Mutations
// ─────────────────────────────────────────────────────────────────────────

export const create = mutation({
  args: {
    externalOrgId: v.string(),
    title: v.string(),
    description: v.optional(v.string()),
    createdBy: v.string(),
    color: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    const viewer = await requireEditorMembership(ctx, args.externalOrgId);
    if (args.createdBy !== viewer.externalAuthId) throw new Error("Unauthorized");
    const title = args.title.trim();
    if (title.length === 0) {
      throw new Error("title must not be empty");
    }
    const now = Date.now();
    const id = await ctx.db.insert("projects", {
      externalOrgId: args.externalOrgId,
      title,
      description: args.description?.trim() || undefined,
      createdBy: viewer.externalAuthId,
      color: args.color,
      archived: false,
      createdAt: now,
      updatedAt: now,
    });
    return { id };
  },
});

export const update = mutation({
  args: {
    projectId: v.id("projects"),
    title: v.optional(v.string()),
    description: v.optional(v.string()),
    color: v.optional(v.string()),
    archived: v.optional(v.boolean()),
  },
  handler: async (ctx, { projectId, ...patch }) => {
    const existing = await ctx.db.get(projectId);
    if (!existing) {
      throw new Error("project not found");
    }
    await requireEditorMembership(ctx, existing.externalOrgId);
    const update: Record<string, unknown> = { updatedAt: Date.now() };
    if (patch.title !== undefined) {
      const t = patch.title.trim();
      if (t.length === 0) throw new Error("title must not be empty");
      update.title = t;
    }
    if (patch.description !== undefined) {
      update.description = patch.description.trim() || undefined;
    }
    if (patch.color !== undefined) update.color = patch.color;
    if (patch.archived !== undefined) update.archived = patch.archived;
    await ctx.db.patch(projectId, update);
    return { id: projectId };
  },
});

export const remove = mutation({
  args: { projectId: v.id("projects") },
  handler: async (ctx, { projectId }) => {
    const existing = await ctx.db.get(projectId);
    if (!existing) return { ok: true };
    await requireEditorMembership(ctx, existing.externalOrgId);
    await ctx.db.delete(projectId);
    return { ok: true };
  },
});
