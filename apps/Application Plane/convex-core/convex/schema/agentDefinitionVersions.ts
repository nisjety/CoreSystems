/**
 * Versioned agent definitions (EP-6 S7, flag AGENT_DEFINITION_VERSIONS).
 *
 * `agents` stays the definition store. Its `draft` is the editable copy; a
 * publish copies that draft into one immutable row here and moves the agent's
 * `publishedVersion` pointer. The body fields are shared with `agents.draft` so
 * the draft and the published row cannot drift apart in shape.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Authored
 * configuration only: no transcript, preview output or run state is stored here.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

/**
 * What a version is made of. Shared by the `agents.draft` object and by every
 * version row, so both are validated by the same shape.
 */
export const agentDefinitionBodyFields = {
  name: v.string(),
  systemPrompt: v.string(),
  model: v.string(),
  // Capability ids from the WS05 registry (for example `cap.tool.ticket.create`),
  // never free tool names. Narrows what an invocation may use (EP-6 S9).
  toolCapabilityIds: v.array(v.string()),
  // The WS25-S6 mode vocabulary without `lockdown`: a cap can lower autonomy, it
  // never widens to lockdown.
  autonomyCap: v.union(
    v.literal("read_only"),
    v.literal("ask_first"),
    v.literal("auto"),
  ),
  // Same shape as `agents.knowledgeBindings`, copied at draft save.
  knowledgeBindings: v.optional(
    v.object({
      scope: v.union(v.literal("all"), v.literal("selected")),
      documentIds: v.optional(v.array(v.string())),
      sourceIds: v.optional(v.array(v.string())),
      includeQnA: v.optional(v.boolean()),
    }),
  ),
};

export const agentDefinitionVersionTables = {
  // One row per publish. Rows are never edited or deleted: a turn that pinned
  // version N keeps N, and a rollback publishes an older body under a new number.
  agentDefinitionVersions: defineTable({
    agentId: v.id("agents"),
    orgId: v.id("organizations"),
    // Numbered per agent from 1, never reused.
    version: v.number(),
    ...agentDefinitionBodyFields,
    // `sha256:` over the canonical JSON of the body (see `versionDigest` in ../agents.ts).
    digest: v.string(),
    // The Control subject (externalAuthId) that published. An id, never content.
    publishedBy: v.string(),
    publishedAt: v.number(),
  })
    .index("by_agent_version", ["agentId", "version"])
    .index("by_org", ["orgId"]),
};
