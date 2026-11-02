/**
 * Planner document metadata and BlockSuite state.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const plannerTables = {
  // Planner documents - lightweight metadata for BlockSuite-backed notes/canvases.
  plannerDocuments: defineTable({
    workspaceId: v.string(),
    documentId: v.string(),
    title: v.string(),
    ownerExternalAuthId: v.optional(v.string()),
    parentDocumentId: v.optional(v.string()),
    isFavorite: v.optional(v.boolean()),
    lastViewedAt: v.optional(v.number()),
    space: v.optional(v.union(v.literal("private"), v.literal("shared"), v.literal("collection"))),
    createdAt: v.number(),
    updatedAt: v.number(),
    archivedAt: v.optional(v.number()),
  })
    .index("by_workspace", ["workspaceId"])
    .index("by_workspace_and_document", ["workspaceId", "documentId"])
    .index("by_workspace_and_updated", ["workspaceId", "updatedAt"])
    .index("by_workspace_and_parent", ["workspaceId", "parentDocumentId"])
    .index("by_workspace_and_last_viewed", ["workspaceId", "lastViewedAt"]),

  plannerDocumentStates: defineTable({
    workspaceId: v.string(),
    documentId: v.string(),
    stateBase64: v.string(),
    updatedAt: v.number(),
  })
    .index("by_workspace_and_document", ["workspaceId", "documentId"]),
};
