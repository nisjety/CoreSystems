/**
 * Agent Mutations & Queries
 *
 * CRUD for per-org AI agent configurations.
 * All calls require a valid service key (server-side only).
 */

import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey, requireGatewayMember } from "./authz";
import { agentDefinitionBodyFields } from "./schema/agentDefinitionVersions";

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
    assertLegacyWriteAllowed(agent, args);

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
 * Wave 9 (ui-ux-verevon-gap.md §19): enable the public embed widget.
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

/**
 * EP-6 S7 - versioned agent definitions (flag AGENT_DEFINITION_VERSIONS).
 *
 * The draft lives on the agent row and is the only thing the builder edits. A
 * publish copies it into an immutable `agentDefinitionVersions` row and moves the
 * pointer. Nothing here is reachable while the BFF flag is off, so an agent without
 * a pointer keeps the legacy `systemPrompt` and `tools` behaviour exactly.
 *
 * Authority: every gateway function checks the service key and gateway membership.
 * The role gate (admin or creator, or the Space manager for Space agents) belongs
 * to the BFF and lands with the builder routes (EP-6 S8).
 */

/** The cap `createSpaceAgentForGateway` already applies to Space agent instructions. */
const MAX_SYSTEM_PROMPT_LENGTH = 4000;
// Space agents and org agents share one `agents` row, so they share the name bounds.
const MIN_NAME_LENGTH = 2;
const MAX_NAME_LENGTH = 60;
const MAX_MODEL_LENGTH = 200;
// A bound on a short allowlist, not a product limit.
const MAX_TOOL_CAPABILITY_IDS = 64;
// The shape the WS20 contract already uses for capability ids (`contractCapabilityID`).
const CAPABILITY_ID_PATTERN = /^[a-z][a-z0-9_.-]{0,127}$/;
const AUTONOMY_CAPS = ["read_only", "ask_first", "auto"] as const;
const MAX_VERSIONS_LISTED = 50;

export type AutonomyCap = (typeof AUTONOMY_CAPS)[number];

/** The authored definition a version is made of. */
export type DefinitionBody = {
  name: string;
  systemPrompt: string;
  model: string;
  toolCapabilityIds: string[];
  autonomyCap: AutonomyCap;
  knowledgeBindings?: {
    scope: "all" | "selected";
    documentIds?: string[];
    sourceIds?: string[];
    includeQnA?: boolean;
  };
};

/** A version row as the gateway reads it. */
export type AgentVersionRow = DefinitionBody & {
  orgId: unknown;
  version: number;
  digest: string;
  publishedBy: string;
  publishedAt: number;
};

/**
 * Validates a draft or publish body and returns its canonical form: name and model
 * trimmed, tool ids de-duplicated and sorted, so the same content always yields the
 * same digest. Throws a stable code on the first violation.
 */
export function canonicalDefinitionBody(input: DefinitionBody): DefinitionBody {
  const name = input.name.trim();
  if (name.length < MIN_NAME_LENGTH || name.length > MAX_NAME_LENGTH) {
    throw new Error("agent_name_invalid");
  }
  const model = input.model.trim();
  if (model.length === 0 || model.length > MAX_MODEL_LENGTH) {
    throw new Error("agent_model_invalid");
  }
  if (input.systemPrompt.length > MAX_SYSTEM_PROMPT_LENGTH) {
    throw new Error("agent_system_prompt_too_long");
  }
  if (!AUTONOMY_CAPS.includes(input.autonomyCap)) {
    throw new Error("agent_autonomy_cap_invalid");
  }
  const toolCapabilityIds = [...new Set(input.toolCapabilityIds)].sort();
  if (toolCapabilityIds.length > MAX_TOOL_CAPABILITY_IDS) {
    throw new Error("agent_too_many_tool_capabilities");
  }
  if (toolCapabilityIds.some((id) => !CAPABILITY_ID_PATTERN.test(id))) {
    throw new Error("agent_tool_capability_invalid");
  }
  return {
    name,
    systemPrompt: input.systemPrompt,
    model,
    toolCapabilityIds,
    autonomyCap: input.autonomyCap,
    ...(input.knowledgeBindings ? { knowledgeBindings: input.knowledgeBindings } : {}),
  };
}

/**
 * JSON with object keys sorted at every depth and undefined values dropped. Key
 * order and absent optional fields therefore never change the bytes a digest is
 * taken over.
 */
export function canonicalJson(value: unknown): string {
  return JSON.stringify(sortKeysDeep(value));
}

function sortKeysDeep(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeysDeep);
  if (value !== null && typeof value === "object") {
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) {
      const child = (value as Record<string, unknown>)[key];
      if (child !== undefined) sorted[key] = sortKeysDeep(child);
    }
    return sorted;
  }
  return value;
}

/** `sha256:` plus the hex SHA-256 of the canonical JSON of a version body. */
export async function versionDigest(body: DefinitionBody): Promise<string> {
  const bytes = new TextEncoder().encode(canonicalJson(body));
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return `sha256:${Array.from(hash, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

/** One past the newest version, so a number is never reused, even after a rollback. */
export function nextVersionNumber(latest: { version: number } | null): number {
  return (latest?.version ?? 0) + 1;
}

/**
 * What a gateway caller may read of one version. A version of another organisation
 * reads as absent, the same answer as an unknown id.
 */
export function projectAgentVersion(row: AgentVersionRow, organizationId: unknown) {
  if (row.orgId !== organizationId) return null;
  return {
    version: row.version,
    digest: row.digest,
    name: row.name,
    systemPrompt: row.systemPrompt,
    model: row.model,
    toolCapabilityIds: row.toolCapabilityIds,
    autonomyCap: row.autonomyCap,
    knowledgeBindings: row.knowledgeBindings,
    publishedBy: row.publishedBy,
    publishedAt: row.publishedAt,
  };
}

/**
 * Once an agent serves a published version, its `systemPrompt` and `tools` change only
 * through draft and publish. A direct write would leave the row disagreeing with the
 * version the gateway serves. The other legacy fields keep their existing write path.
 */
export function assertLegacyWriteAllowed(
  agent: { publishedVersion?: number },
  patch: { systemPrompt?: unknown; tools?: unknown },
) {
  if (agent.publishedVersion === undefined) return;
  if (patch.systemPrompt !== undefined || patch.tools !== undefined) {
    throw new Error("agent_versioned_field_refused");
  }
}

/**
 * The persona the gateway injects for a mentioned agent. An unversioned agent gets the
 * legacy `{name, systemPrompt}` as before. A versioned agent gets its published version,
 * and a pointer whose version cannot be read gives no persona at all: the legacy prompt
 * is never served in its place.
 */
export function definitionPersona(
  agent: { name: string; systemPrompt?: string; publishedVersion?: number; orgId: unknown },
  published: AgentVersionRow | null,
) {
  if (agent.publishedVersion === undefined) {
    return { name: agent.name, systemPrompt: agent.systemPrompt ?? null };
  }
  if (
    !published ||
    published.orgId !== agent.orgId ||
    published.version !== agent.publishedVersion
  ) {
    return null;
  }
  return {
    name: published.name,
    systemPrompt: published.systemPrompt,
    version: published.version,
    digest: published.digest,
    toolCapabilityIds: published.toolCapabilityIds,
    autonomyCap: published.autonomyCap,
  };
}

/**
 * Save the agent's editable draft. A draft is not a version: it has no number and
 * changes nothing a running turn reads. `status: "draft"` on the agent is a different
 * lifecycle state and is untouched.
 */
export const saveDraftForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
    draft: v.object(agentDefinitionBodyFields),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== organization._id) {
      throw new Error("Agent not found");
    }

    await ctx.db.patch(args.agentId, {
      draft: canonicalDefinitionBody(args.draft),
      updatedAt: Date.now(),
    });
    return { ok: true };
  },
});

/**
 * Publish the draft as the agent's next version: one immutable row is written and the
 * pointer moves. Earlier rows are never touched, so a turn pinned to version N keeps N.
 * Publishing an unchanged draft still takes a new number, because the act is what the
 * history records.
 */
export const publishForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== organization._id) {
      throw new Error("Agent not found");
    }
    if (!agent.draft) {
      throw new Error("agent_draft_missing");
    }

    const body = canonicalDefinitionBody(agent.draft);
    const digest = await versionDigest(body);
    const latest = await ctx.db
      .query("agentDefinitionVersions")
      .withIndex("by_agent_version", (q) => q.eq("agentId", args.agentId))
      .order("desc")
      .first();
    const version = nextVersionNumber(latest);
    const now = Date.now();

    await ctx.db.insert("agentDefinitionVersions", {
      ...body,
      agentId: args.agentId,
      orgId: agent.orgId,
      version,
      digest,
      publishedBy: args.externalAuthId,
      publishedAt: now,
    });
    await ctx.db.patch(args.agentId, { publishedVersion: version, updatedAt: now });
    return { version, digest };
  },
});

/**
 * The version history of one agent, newest first. An agent of another organisation
 * reads as an empty history, the same as an unknown id.
 */
export const listVersionsForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== organization._id) return [];

    const rows = await ctx.db
      .query("agentDefinitionVersions")
      .withIndex("by_agent_version", (q) => q.eq("agentId", args.agentId))
      .order("desc")
      .take(MAX_VERSIONS_LISTED);
    return rows.flatMap((row) => {
      const view = projectAgentVersion(row, organization._id);
      return view ? [view] : [];
    });
  },
});

/** One version of an agent, or null when it does not exist or belongs to another org. */
export const getVersionForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
    version: v.number(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const row = await ctx.db
      .query("agentDefinitionVersions")
      .withIndex("by_agent_version", (q) =>
        q.eq("agentId", args.agentId).eq("version", args.version),
      )
      .unique();
    return row ? projectAgentVersion(row, organization._id) : null;
  },
});

/**
 * What the agent builder (EP-6 S8) reads before the gateway authorises an edit: the draft, the
 * pointer, the legacy fields a first draft is seeded from, and the two facts its gate needs (the
 * creator's Control subject, and the Space a room-born agent was created in). An agent of another
 * organisation reads as null, the same answer as an unknown id.
 */
export const builderDefinitionForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== organization._id) return null;
    const creator = agent.createdBy ? await ctx.db.get(agent.createdBy) : null;
    // createSpaceAgentForGateway writes the agent and its first binding in one transaction with
    // the same timestamp and no createdBy; that binding's Space is where the agent was born.
    const origin = agent.createdBy
      ? null
      : (
          await ctx.db
            .query("spaceAgentBindings")
            .withIndex("by_external_org", (q: any) => q.eq("externalOrgId", args.externalOrgId))
            .collect()
        ).find((binding: any) => binding.agentId === args.agentId && binding.createdAt === agent.createdAt);
    return {
      agentRef: agent._id,
      name: agent.name,
      systemPrompt: agent.systemPrompt ?? null,
      model: agent.model ?? null,
      publishedVersion: agent.publishedVersion ?? null,
      draft: agent.draft ?? null,
      creatorExternalAuthId: creator?.externalAuthId ?? null,
      originSpaceRef: origin?.spaceRef ?? null,
    };
  },
});
