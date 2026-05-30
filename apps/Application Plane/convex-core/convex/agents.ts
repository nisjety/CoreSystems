/**
 * Agent Mutations & Queries
 *
 * CRUD for per-org AI agent configurations.
 * All calls require a valid service key (server-side only).
 */

import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey } from "./authz";

const agentFields = {
  name: v.string(),
  description: v.optional(v.string()),
  useCase: v.union(
    v.literal("customer_support"),
    v.literal("sales"),
    v.literal("marketing"),
    v.literal("hr"),
    v.literal("faq"),
    v.literal("onboarding"),
    v.literal("other"),
  ),
  status: v.union(
    v.literal("active"),
    v.literal("inactive"),
    v.literal("draft"),
  ),
  model: v.string(),
  temperature: v.optional(v.number()),
  systemPrompt: v.optional(v.string()),
  tone: v.optional(v.string()),
  greeting: v.optional(v.string()),
  tools: v.optional(v.array(v.string())),
  knowledgeSources: v.optional(
    v.array(
      v.object({
        type: v.string(),
        name: v.string(),
        id: v.optional(v.string()),
      }),
    ),
  ),
  // Harness profile (docs/HARNESS_PHASE1.md §1). See resolveAgentProfile.
  profile: v.optional(v.union(v.literal("chat"), v.literal("deployed_agent"))),
  createdBy: v.optional(v.id("users")),
} as const;

/**
 * Create a new agent for an org.
 * Returns the new agent's Convex ID.
 */
export const create = mutation({
  args: {
    serviceKey: v.string(),
    orgId: v.id("organizations"),
    ...agentFields,
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);

    const { serviceKey, ...fields } = args;
    const now = Date.now();

    return await ctx.db.insert("agents", {
      ...fields,
      createdAt: now,
      updatedAt: now,
    });
  },
});

/**
 * Update an existing agent.
 * Validates org ownership before updating.
 */
export const update = mutation({
  args: {
    serviceKey: v.string(),
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
    name: v.optional(v.string()),
    description: v.optional(v.string()),
    useCase: v.optional(
      v.union(
        v.literal("customer_support"),
        v.literal("sales"),
        v.literal("marketing"),
        v.literal("hr"),
        v.literal("faq"),
        v.literal("onboarding"),
        v.literal("other"),
      ),
    ),
    status: v.optional(
      v.union(
        v.literal("active"),
        v.literal("inactive"),
        v.literal("draft"),
      ),
    ),
    model: v.optional(v.string()),
    temperature: v.optional(v.number()),
    systemPrompt: v.optional(v.string()),
    tone: v.optional(v.string()),
    greeting: v.optional(v.string()),
    tools: v.optional(v.array(v.string())),
    knowledgeSources: v.optional(
      v.array(
        v.object({
          type: v.string(),
          name: v.string(),
          id: v.optional(v.string()),
        }),
      ),
    ),
    profile: v.optional(v.union(v.literal("chat"), v.literal("deployed_agent"))),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);

    const agent = await ctx.db.get(args.agentId);
    if (!agent) {
      throw new Error("Agent not found");
    }
    if (agent.orgId !== args.orgId) {
      throw new Error("Agent access denied");
    }

    const { serviceKey, agentId, orgId, ...patch } = args;

    // Remove undefined values so we don't overwrite existing fields with undefined
    const cleanPatch = Object.fromEntries(
      Object.entries(patch).filter(([, v]) => v !== undefined),
    );

    await ctx.db.patch(agentId, {
      ...cleanPatch,
      updatedAt: Date.now(),
    });

    return agentId;
  },
});

/**
 * Get a single agent by ID.
 * Returns null if not found or org mismatch.
 */
export const getById = query({
  args: {
    serviceKey: v.string(),
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);

    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== args.orgId) {
      return null;
    }

    return { id: agent._id, ...agent };
  },
});

/**
 * List all agents for an org, ordered by most recently updated.
 */
export const listByOrg = query({
  args: {
    serviceKey: v.string(),
    orgId: v.id("organizations"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);

    const agents = await ctx.db
      .query("agents")
      .withIndex("by_org", (q) => q.eq("orgId", args.orgId))
      .order("desc")
      .collect();

    return agents.map((agent) => ({ id: agent._id, ...agent }));
  },
});

/**
 * Wave 9 (ui-ux-velion-gap.md §19): enable the public embed widget.
 * Generates a fresh `publicSecret` and flips `publicEnabled=true`.
 * Calling again rotates the secret.
 */
export const enablePublicEmbed = mutation({
  args: {
    serviceKey: v.string(),
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
    theme: v.optional(
      v.object({
        accentColor: v.optional(v.string()),
        buttonLabel: v.optional(v.string()),
        welcomeMessage: v.optional(v.string()),
      }),
    ),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== args.orgId) {
      throw new Error("Agent not found in this organisation");
    }
    const secret = Array.from({ length: 32 }, () =>
      "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".charAt(
        Math.floor(Math.random() * 62),
      ),
    ).join("");
    await ctx.db.patch(args.agentId, {
      publicEnabled: true,
      publicSecret: secret,
      embedTheme: args.theme,
      updatedAt: Date.now(),
    });
    return { publicSecret: secret };
  },
});

export const disablePublicEmbed = mutation({
  args: {
    serviceKey: v.string(),
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== args.orgId) {
      throw new Error("Agent not found in this organisation");
    }
    await ctx.db.patch(args.agentId, {
      publicEnabled: false,
      publicSecret: undefined,
      updatedAt: Date.now(),
    });
    return { ok: true };
  },
});

/**
 * Public-surface lookup for the embed widget. Returns the minimal
 * config to render a chat bubble (name, greeting, theme) and confirms
 * the supplied secret. Never returns the system prompt, tools, or
 * sources — those are server-side concerns.
 */
export const getEmbedConfig = query({
  args: {
    serviceKey: v.string(),
    agentId: v.id("agents"),
    publicSecret: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const agent = await ctx.db.get(args.agentId);
    if (
      !agent ||
      agent.publicEnabled !== true ||
      agent.publicSecret !== args.publicSecret
    ) {
      return null;
    }
    return {
      id: agent._id,
      name: agent.name,
      greeting: agent.greeting ?? "",
      theme: agent.embedTheme ?? {},
      orgId: agent.orgId,
    };
  },
});
/**
 * List active agents only for an org.
 */
export const listActiveByOrg = query({
  args: {
    serviceKey: v.string(),
    orgId: v.id("organizations"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);

    const agents = await ctx.db
      .query("agents")
      .withIndex("by_org_and_status", (q) =>
        q.eq("orgId", args.orgId).eq("status", "active"),
      )
      .order("desc")
      .collect();

    return agents.map((agent) => ({ id: agent._id, ...agent }));
  },
});

/**
 * Wave 11 §6 — set the per-agent knowledge-binding scope.
 *
 *   `scope='all'`        retrieve from org's entire knowledge base
 *   `scope='selected'`   intersection of documentIds + sourceIds + Q&A toggle
 *
 * Model-gateway's `kb_search` tool forwards these to
 * `retrieval-engine-rs::hybrid` as filters on every retrieval.
 */
export const updateKnowledgeBindings = mutation({
  args: {
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
    scope: v.union(v.literal("all"), v.literal("selected")),
    documentIds: v.optional(v.array(v.string())),
    sourceIds: v.optional(v.array(v.string())),
    includeQnA: v.optional(v.boolean()),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db.get(args.agentId);
    if (!existing) throw new Error("Agent not found");
    if (existing.orgId !== args.orgId) {
      throw new Error("Agent belongs to a different organization");
    }
    await ctx.db.patch(args.agentId, {
      knowledgeBindings: {
        scope: args.scope,
        documentIds:
          args.scope === "selected" ? args.documentIds ?? [] : undefined,
        sourceIds:
          args.scope === "selected" ? args.sourceIds ?? [] : undefined,
        includeQnA: args.includeQnA ?? true,
      },
      updatedAt: Date.now(),
    });
    return await ctx.db.get(args.agentId);
  },
});

/**
 * Wave 11 §6.5 — per-agent RAG knob persistence (ElevenLabs
 * "Configure RAG"). Stored on the agents row; gateway forwards on
 * every retrieval call. Defaults applied retrieval-side so empty
 * fields stay empty (operator-set vs derived).
 */
export const updateRetrievalConfig = mutation({
  args: {
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
    weights: v.optional(
      v.object({
        dense: v.number(),
        bm25: v.number(),
        graph: v.number(),
        wiki: v.number(),
      })
    ),
    chunkSize: v.optional(v.number()),
    topK: v.optional(v.number()),
    rerank: v.optional(v.boolean()),
    graphHops: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db.get(args.agentId);
    if (!existing) throw new Error("Agent not found");
    if (existing.orgId !== args.orgId) {
      throw new Error("Agent belongs to a different organization");
    }
    const previous = existing.retrievalConfig ?? {};
    await ctx.db.patch(args.agentId, {
      retrievalConfig: {
        ...previous,
        ...(args.weights !== undefined ? { weights: args.weights } : {}),
        ...(args.chunkSize !== undefined ? { chunkSize: args.chunkSize } : {}),
        ...(args.topK !== undefined ? { topK: args.topK } : {}),
        ...(args.rerank !== undefined ? { rerank: args.rerank } : {}),
        ...(args.graphHops !== undefined ? { graphHops: args.graphHops } : {}),
      },
      updatedAt: Date.now(),
    });
    return await ctx.db.get(args.agentId);
  },
});
