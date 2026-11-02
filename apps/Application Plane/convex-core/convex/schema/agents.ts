/**
 * Agent definitions, run lifecycle mirror and operator-curated Q&A.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const agentTables = {
  // Agents - Per-org AI agent configurations
  agents: defineTable({
    orgId: v.id("organizations"),
    name: v.string(),
    description: v.optional(v.string()),
    // Room-created agents pick an identity color (Grok-style). Presentation
    // only — never part of any authority decision.
    avatarColor: v.optional(v.string()),
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
    knowledgeSources: v.optional(v.array(v.object({
      type: v.string(),
      name: v.string(),
      id: v.optional(v.string()),
    }))),
    // Wave 9 (ui-ux-verevon-gap.md §19): public embed widget. When
    // `publicEnabled` is true, the agent can be invoked from the
    // public-facing embed surface (`/api/embed/{agentId}/...`).
    // `publicSecret` is rotated per enable — any site embedding the
    // widget must include it in the script src. Without the secret,
    // the embed API rejects with 403 even if the agent id is known.
    // This stops drive-by usage of someone else's agent from a
    // scraped agent id.
    publicEnabled: v.optional(v.boolean()),
    publicSecret: v.optional(v.string()),
    // Branding for the embed bubble. All optional — sensible defaults
    // come from the agent's name + greeting.
    embedTheme: v.optional(v.object({
      accentColor: v.optional(v.string()),     // hex, e.g. "#1A1A1A"
      buttonLabel: v.optional(v.string()),     // floating button text
      welcomeMessage: v.optional(v.string()),  // overrides `greeting`
    })),
    // Wave 11 §6 — per-agent knowledge scoping. When `scope='all'` the
    // agent can retrieve from the org's entire knowledge base. When
    // `scope='selected'` retrieval is filtered to the listed document
    // and source ids (intersection: union of both lists). Migrating
    // agents default to `scope='all'`.
    knowledgeBindings: v.optional(v.object({
      scope: v.union(v.literal("all"), v.literal("selected")),
      documentIds: v.optional(v.array(v.string())),
      sourceIds: v.optional(v.array(v.string())),
      // When false, the agent ignores Q&A entries even when scope='all'.
      includeQnA: v.optional(v.boolean()),
    })),
    // Wave 11 §6.5 — per-agent RAG knobs forwarded to retrieval-engine-rs
    // on every `/v1/retrieve/hybrid` call. ElevenLabs "Configure RAG"
    // pattern. Defaults applied server-side when fields are absent.
    retrievalConfig: v.optional(v.object({
      weights: v.optional(v.object({
        dense: v.number(),
        bm25: v.number(),
        graph: v.number(),
        wiki: v.number(),
      })),
      chunkSize: v.optional(v.number()),
      topK: v.optional(v.number()),
      rerank: v.optional(v.boolean()),
      graphHops: v.optional(v.number()),
    })),
    // Harness profile (docs/HARNESS_PHASE1.md §1). `chat` keeps the surface
    // clean (ChatGPT/Claude-style, harness invisible); `deployed_agent`
    // enables operator surfaces (inbox, HITL handoff, run-event feed). Optional
    // for migration — code resolves a default of `deployed_agent` when
    // publicEnabled else `chat` (see resolveAgentProfile in verevon).
    profile: v.optional(v.union(
      v.literal("chat"),
      v.literal("deployed_agent"),
    )),
    createdBy: v.optional(v.id("users")),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_org_and_status", ["orgId", "status"]),

  // U3-3 (ui-ux-verevon-gap.md §10): agent run lifecycle mirror.
  //
  // The Model Plane's orchestrator-core publishes RUN_STARTED / RUN_COMPLETED
  // / RUN_FAILED envelopes on `mp.v1.run.{runId}.event` (see
  // `apps/Model Plane/go/services/orchestrator-core/cmd/activities/activities.go`).
  // The `nats-subscriber.js` service subscribes to the wildcard
  // `mp.v1.run.*.event` and calls `upsertAgentRun` here so verevon's
  // `agents/runs` UI can render lifecycle reactively (no polling).
  //
  // `runId` is the natural key (caller-supplied ULID). `agentId` is
  // optional because not every run originates from an agent definition —
  // a one-shot chat invocation still produces a run with no agent. The
  // `payload` JSONB column is left intentionally schema-less so the
  // mirror keeps working when the orchestrator adds new fields.
  agentRuns: defineTable({
    runId: v.string(),
    externalOrgId: v.string(),
    externalUserId: v.optional(v.string()),
    agentId: v.optional(v.string()),
    status: v.union(
      v.literal("started"),
      v.literal("completed"),
      v.literal("failed"),
      v.literal("cancelled"),
    ),
    error: v.optional(v.string()),
    payload: v.optional(v.any()),
    /**
     * Wave 11 §5 — Fin G/A/P feedback. Operator rating set from the
     * playground reply chips (keyboard shortcuts G/A/P). Feeds back
     * into the re-ranker via the nightly orchestrator job (D5).
     */
    rating: v.optional(
      v.union(v.literal("good"), v.literal("acceptable"), v.literal("poor"))
    ),
    /** Free-form note attached when rating is set; never shown to end-users. */
    ratingNote: v.optional(v.string()),
    /** When the rating was set (separate from row updates so we can audit). */
    ratedAt: v.optional(v.number()),
    startedAt: v.number(),
    completedAt: v.optional(v.number()),
    updatedAt: v.number(),
  })
    .index("by_run", ["runId"])
    .index("by_org_and_started", ["externalOrgId", "startedAt"])
    .index("by_org_and_status", ["externalOrgId", "status"])
    .index("by_org_and_rating", ["externalOrgId", "rating"]),

  // Wave 11 §2.1 — operator-curated Q&A pairs as a first-class entity.
  //
  // Why a dedicated table (not `documents.type='qa'`): Chatbase, Lindy,
  // ElevenLabs all separate Q&A so it has its own evaluation surface
  // (status, rating, citation count). Intercom Fin's "Custom Answers"
  // is the most polished reference; we mirror its shape.
  //
  // Retrieval picks these up via the `wiki-store-go` service in
  // Wave 11.B / D4 (`apps/Data Plane/docs/d4-d5-graph-wiki-hybrid-spec.md`).
  // Until D4 ships, the playground citation panel surfaces Q&A hits
  // directly so operators see what's matched.
  knowledgeQnA: defineTable({
    orgId: v.id("organizations"),
    createdBy: v.id("users"),
    question: v.string(),
    answer: v.string(),
    status: v.union(
      v.literal("draft"),
      v.literal("published"),
      v.literal("deprecated")
    ),
    /** Aggregate operator rating from playground turns where this Q&A was cited. */
    rating: v.optional(
      v.union(v.literal("good"), v.literal("acceptable"), v.literal("poor"))
    ),
    /** Cumulative # of agent answers that have cited this entry. */
    citationCount: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_org_and_status", ["orgId", "status"]),
};
