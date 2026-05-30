import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

const plannerDocumentSpace = v.union(
  v.literal("private"),
  v.literal("shared"),
  v.literal("collection"),
);

function normalizeTitle(title: string) {
  const trimmed = title.trim();
  return trimmed.length > 0 ? trimmed : "Untitled note";
}

function normalizeParentDocumentId(parentDocumentId: string | null | undefined, documentId: string) {
  const trimmed = parentDocumentId?.trim();

  if (!trimmed) {
    return undefined;
  }

  if (trimmed === documentId) {
    throw new Error("Planner document cannot be its own parent");
  }

  return trimmed;
}

function normalizeSpace(space: "private" | "shared" | "collection" | undefined) {
  return space ?? "private";
}

function buildMetadataBackfillPatch(document: {
  isFavorite?: boolean;
  lastViewedAt?: number;
  space?: "private" | "shared" | "collection";
  updatedAt: number;
  createdAt: number;
}) {
  const patch: Record<string, unknown> = {};

  if (document.isFavorite === undefined) {
    patch.isFavorite = false;
  }

  if (document.lastViewedAt === undefined) {
    patch.lastViewedAt = document.updatedAt ?? document.createdAt;
  }

  if (document.space === undefined) {
    patch.space = "private";
  }

  return patch;
}

async function getExistingDocument(
  ctx: any,
  workspaceId: string,
  documentId: string,
) {
  const matches = await ctx.db
    .query("plannerDocuments")
    .withIndex("by_workspace_and_document", (q: any) =>
      q.eq("workspaceId", workspaceId).eq("documentId", documentId),
    )
    .collect();

  return matches[0] || null;
}

async function listWorkspaceDocuments(ctx: any, workspaceId: string) {
  return ctx.db
    .query("plannerDocuments")
    .withIndex("by_workspace", (q: any) => q.eq("workspaceId", workspaceId))
    .collect();
}

async function getDescendantDocuments(ctx: any, workspaceId: string, documentId: string) {
  const documents = await listWorkspaceDocuments(ctx, workspaceId);
  const childrenByParent = new Map<string, any[]>();

  for (const document of documents) {
    if (!document.parentDocumentId) {
      continue;
    }

    const siblings = childrenByParent.get(document.parentDocumentId) ?? [];
    siblings.push(document);
    childrenByParent.set(document.parentDocumentId, siblings);
  }

  const descendants: any[] = [];
  const stack = [...(childrenByParent.get(documentId) ?? [])];
  const seen = new Set<string>();

  while (stack.length > 0) {
    const current = stack.pop();
    if (!current || seen.has(current.documentId)) {
      continue;
    }

    seen.add(current.documentId);
    descendants.push(current);

    for (const child of childrenByParent.get(current.documentId) ?? []) {
      stack.push(child);
    }
  }

  return descendants;
}

export const listByWorkspace = query({
  args: {
    workspaceId: v.string(),
    includeArchived: v.optional(v.boolean()),
  },
  handler: async (ctx, args) => {
    const includeArchived = args.includeArchived ?? false;

    const documents = await ctx.db
      .query("plannerDocuments")
      .withIndex("by_workspace", (q) => q.eq("workspaceId", args.workspaceId))
      .collect();

    return documents
      .filter((document) => includeArchived || !document.archivedAt)
      .sort((left, right) => right.updatedAt - left.updatedAt);
  },
});

export const getDocument = query({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
  },
  handler: async (ctx, args) => {
    return await getExistingDocument(ctx, args.workspaceId, args.documentId);
  },
});

export const create = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
    title: v.string(),
    ownerExternalAuthId: v.optional(v.string()),
    parentDocumentId: v.optional(v.union(v.string(), v.null())),
    space: v.optional(plannerDocumentSpace),
  },
  handler: async (ctx, args) => {
    const now = Date.now();
    const existing = await getExistingDocument(ctx, args.workspaceId, args.documentId);
    const parentDocumentId = normalizeParentDocumentId(args.parentDocumentId, args.documentId);
    const space = normalizeSpace(args.space);

    if (existing) {
      await ctx.db.patch(existing._id, {
        title: normalizeTitle(args.title),
        parentDocumentId,
        space,
        updatedAt: now,
        lastViewedAt: now,
        archivedAt: undefined,
        ownerExternalAuthId: args.ownerExternalAuthId ?? existing.ownerExternalAuthId,
      });
      return await ctx.db.get(existing._id);
    }

    const documentId = await ctx.db.insert("plannerDocuments", {
      workspaceId: args.workspaceId,
      documentId: args.documentId,
      title: normalizeTitle(args.title),
      ownerExternalAuthId: args.ownerExternalAuthId,
      parentDocumentId,
      isFavorite: false,
      lastViewedAt: now,
      space,
      createdAt: now,
      updatedAt: now,
    });

    return await ctx.db.get(documentId);
  },
});

export const backfillMetadata = mutation({
  args: {
    workspaceId: v.optional(v.string()),
    dryRun: v.optional(v.boolean()),
  },
  handler: async (ctx, args) => {
    const dryRun = args.dryRun ?? false;
    const documents = args.workspaceId
      ? await listWorkspaceDocuments(ctx, args.workspaceId)
      : await ctx.db.query("plannerDocuments").collect();

    let updated = 0;
    const patchedDocuments: Array<{
      documentId: string;
      workspaceId: string;
      patch: Record<string, unknown>;
    }> = [];

    for (const document of documents) {
      const patch = buildMetadataBackfillPatch(document);
      if (Object.keys(patch).length === 0) {
        continue;
      }

      updated += 1;
      patchedDocuments.push({
        documentId: document.documentId,
        workspaceId: document.workspaceId,
        patch,
      });

      if (!dryRun) {
        await ctx.db.patch(document._id, patch);
      }
    }

    return {
      dryRun,
      scanned: documents.length,
      updated,
      patchedDocuments,
    };
  },
});

export const update = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
    title: v.optional(v.string()),
    parentDocumentId: v.optional(v.union(v.string(), v.null())),
    isFavorite: v.optional(v.boolean()),
    lastViewedAt: v.optional(v.number()),
    space: v.optional(plannerDocumentSpace),
  },
  handler: async (ctx, args) => {
    const existing = await getExistingDocument(ctx, args.workspaceId, args.documentId);
    if (!existing || existing.archivedAt) {
      throw new Error("Planner document not found");
    }

    const patch: Record<string, unknown> = {};
    let shouldUpdateTimestamp = false;

    if (args.title !== undefined) {
      patch.title = normalizeTitle(args.title);
      shouldUpdateTimestamp = true;
    }

    if (args.parentDocumentId !== undefined) {
      patch.parentDocumentId = normalizeParentDocumentId(args.parentDocumentId, args.documentId);
      shouldUpdateTimestamp = true;
    }

    if (args.isFavorite !== undefined) {
      patch.isFavorite = args.isFavorite;
    }

    if (args.lastViewedAt !== undefined) {
      patch.lastViewedAt = args.lastViewedAt;
    }

    if (args.space !== undefined) {
      patch.space = normalizeSpace(args.space);
      shouldUpdateTimestamp = true;
    }

    if (Object.keys(patch).length === 0) {
      return existing;
    }

    if (shouldUpdateTimestamp) {
      patch.updatedAt = Date.now();
    }

    await ctx.db.patch(existing._id, patch);
    return await ctx.db.get(existing._id);
  },
});

export const rename = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
    title: v.string(),
  },
  handler: async (ctx, args) => {
    const existing = await getExistingDocument(ctx, args.workspaceId, args.documentId);
    if (!existing || existing.archivedAt) {
      throw new Error("Planner document not found");
    }

    await ctx.db.patch(existing._id, {
      title: normalizeTitle(args.title),
      updatedAt: Date.now(),
    });

    return await ctx.db.get(existing._id);
  },
});

export const archive = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
  },
  handler: async (ctx, args) => {
    const existing = await getExistingDocument(ctx, args.workspaceId, args.documentId);
    if (!existing || existing.archivedAt) {
      return null;
    }

    const now = Date.now();
    const descendants = await getDescendantDocuments(ctx, args.workspaceId, args.documentId);

    await ctx.db.patch(existing._id, {
      archivedAt: now,
      updatedAt: now,
    });

    for (const descendant of descendants) {
      if (descendant.archivedAt) {
        continue;
      }

      await ctx.db.patch(descendant._id, {
        archivedAt: now,
        updatedAt: now,
      });
    }

    return await ctx.db.get(existing._id);
  },
});

export const restore = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
  },
  handler: async (ctx, args) => {
    const existing = await getExistingDocument(ctx, args.workspaceId, args.documentId);
    if (!existing) {
      return null;
    }

    const now = Date.now();
    const descendants = await getDescendantDocuments(ctx, args.workspaceId, args.documentId);

    await ctx.db.patch(existing._id, {
      archivedAt: undefined,
      updatedAt: now,
      lastViewedAt: now,
    });

    for (const descendant of descendants) {
      await ctx.db.patch(descendant._id, {
        archivedAt: undefined,
        updatedAt: now,
      });
    }

    return await ctx.db.get(existing._id);
  },
});

export const touch = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
  },
  handler: async (ctx, args) => {
    const existing = await getExistingDocument(ctx, args.workspaceId, args.documentId);
    if (!existing || existing.archivedAt) {
      return null;
    }

    await ctx.db.patch(existing._id, {
      lastViewedAt: Date.now(),
    });

    return await ctx.db.get(existing._id);
  },
});

export const getState = query({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
  },
  handler: async (ctx, args) => {
    const matches = await ctx.db
      .query("plannerDocumentStates")
      .withIndex("by_workspace_and_document", (q) =>
        q.eq("workspaceId", args.workspaceId).eq("documentId", args.documentId),
      )
      .collect();

    return matches[0] ?? null;
  },
});

export const saveState = mutation({
  args: {
    workspaceId: v.string(),
    documentId: v.string(),
    stateBase64: v.string(),
  },
  handler: async (ctx, args) => {
    const now = Date.now();

    const existingState = await ctx.db
      .query("plannerDocumentStates")
      .withIndex("by_workspace_and_document", (q) =>
        q.eq("workspaceId", args.workspaceId).eq("documentId", args.documentId),
      )
      .collect();

    if (existingState[0]) {
      await ctx.db.patch(existingState[0]._id, {
        stateBase64: args.stateBase64,
        updatedAt: now,
      });
    } else {
      await ctx.db.insert("plannerDocumentStates", {
        workspaceId: args.workspaceId,
        documentId: args.documentId,
        stateBase64: args.stateBase64,
        updatedAt: now,
      });
    }

    const plannerDocument = await getExistingDocument(
      ctx,
      args.workspaceId,
      args.documentId,
    );

    if (plannerDocument && !plannerDocument.archivedAt) {
      await ctx.db.patch(plannerDocument._id, {
        updatedAt: now,
      });
    }

    return { updatedAt: now };
  },
});
