/**
 * Convex Schema Definition
 * 
 * This defines the shape of data in the Convex database.
 * TypeScript types are automatically generated from this schema.
 */

import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  // Organizations - Multi-tenant support
  organizations: defineTable({
    // External sync fields
    externalOrgId: v.string(),  // ID from auth-core/org-core
    
    name: v.string(),
    slug: v.string(),
    settings: v.optional(v.object({
      maxTokens: v.optional(v.number()),
      defaultModel: v.optional(v.string()),
      allowedModels: v.optional(v.array(v.string())),
    })),
    
    // Sync tracking
    syncStatus: v.union(v.literal("syncing"), v.literal("synced"), v.literal("deleted")),
    lastSyncedAt: v.number(),
    
    createdAt: v.number(),
    updatedAt: v.number(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_slug", ["slug"])
    .index("by_external_id", ["externalOrgId"]),

  // Users - Connected to auth system
  users: defineTable({
    // External sync fields
    externalAuthId: v.string(),  // ID from auth-core
    
    email: v.string(),
    name: v.optional(v.string()),
    orgId: v.id("organizations"),
    role: v.union(v.literal("admin"), v.literal("member"), v.literal("viewer")),
    
    // Sync tracking
    syncStatus: v.union(v.literal("syncing"), v.literal("synced"), v.literal("deleted")),
    lastSyncedAt: v.number(),
    sourceUpdatedAt: v.optional(v.number()),
    
    createdAt: v.number(),
    lastSeenAt: v.number(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_external_auth_id", ["externalAuthId"])
    .index("by_org", ["orgId"])
    .index("by_email", ["email"])
    .index("by_external_and_org", ["externalAuthId", "orgId"]),

  // Authority tombstones prevent a delayed member-added event from
  // resurrecting access after Control Plane has removed the membership.
  membershipTombstones: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    sourceUpdatedAt: v.number(),
    removedAt: v.number(),
  })
    .index("by_external_org_and_user", ["externalOrgId", "externalAuthId"])
    .index("by_external_org", ["externalOrgId"]),

  reconciliationNonces: defineTable({
    nonce: v.string(),
    externalOrgId: v.string(),
    requestTimestamp: v.number(),
    createdAt: v.number(),
    expiresAt: v.number(),
  })
    .index("by_nonce", ["nonce"])
    .index("by_expires_at", ["expiresAt"]),

  // Conversations - Chat sessions
  conversations: defineTable({
    orgId: v.id("organizations"),
    title: v.string(),
    userId: v.id("users"),
    sessionId: v.optional(v.string()),  // Link to Org Core session
    metadata: v.optional(v.object({
      model: v.optional(v.string()),
      temperature: v.optional(v.number()),
      maxTokens: v.optional(v.number()),
      tags: v.optional(v.array(v.string())),
    })),
    status: v.union(
      v.literal("active"),
      v.literal("archived"),
      v.literal("deleted")
    ),
    createdAt: v.number(),
    updatedAt: v.number(),
    lastMessageAt: v.optional(v.number()),
  })
    .index("by_org", ["orgId"])
    .index("by_user", ["userId"])
    .index("by_org_and_status", ["orgId", "status"])
    .index("by_session", ["sessionId"]),

  // Messages - Chat messages with streaming support
  messages: defineTable({
    conversationId: v.id("conversations"),
    role: v.union(v.literal("user"), v.literal("assistant"), v.literal("system")),
    content: v.string(),
    clientId: v.optional(v.string()),
    userId: v.optional(v.id("users")),
    
    // Streaming support
    isStreaming: v.optional(v.boolean()),
    streamedChunks: v.optional(v.array(v.string())),
    
    // Metadata
    metadata: v.optional(v.object({
      source: v.optional(v.string()),
      citations: v.optional(v.array(v.string())),
      model: v.optional(v.string()),
      tokens: v.optional(v.number()),
      latencyMs: v.optional(v.number()),
      ragContext: v.optional(v.array(v.string())),
      error: v.optional(v.string()),
    })),
    
    // Attachments
    attachments: v.optional(v.array(v.object({
      type: v.union(v.literal("file"), v.literal("image"), v.literal("document")),
      storageId: v.string(),
      filename: v.string(),
      size: v.number(),
      mimeType: v.string(),
    }))),
    
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_conversation_and_created", ["conversationId", "createdAt"])
    .index("by_conversation_and_client", ["conversationId", "clientId"]),

  // Jobs - Async operations (RAG indexing, exports, etc.)
  jobs: defineTable({
    orgId: v.id("organizations"),
    userId: v.id("users"),
    type: v.union(
      v.literal("rag_index"),
      v.literal("crawl"),
      v.literal("export"),
      v.literal("batch_delete")
    ),
    status: v.union(
      v.literal("pending"),
      v.literal("running"),
      v.literal("completed"),
      v.literal("failed"),
      v.literal("cancelled")
    ),
    
    // Job details
    payload: v.object({
      // Flexible payload based on job type
      documents: v.optional(v.array(v.string())),
      collection: v.optional(v.string()),
      url: v.optional(v.string()),
      options: v.optional(v.any()),
    }),
    
    // Progress tracking
    progress: v.optional(v.number()),  // 0-100
    progressMessage: v.optional(v.string()),
    
    // Results
    result: v.optional(v.object({
      success: v.optional(v.number()),
      failed: v.optional(v.number()),
      errors: v.optional(v.array(v.string())),
      data: v.optional(v.any()),
    })),
    
    // External job ID (from Org Core)
    externalJobId: v.optional(v.string()),
    
    // Timing
    startedAt: v.optional(v.number()),
    completedAt: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_user", ["userId"])
    .index("by_status", ["status"])
    .index("by_org_and_status", ["orgId", "status"])
    .index("by_external_id", ["externalJobId"]),

  // Presence - Who's online, typing, etc.
  presence: defineTable({
    userId: v.id("users"),
    conversationId: v.optional(v.id("conversations")),
    status: v.union(
      v.literal("online"),
      v.literal("typing"),
      v.literal("away"),
      v.literal("offline")
    ),
    lastSeenAt: v.number(),
    metadata: v.optional(v.object({
      device: v.optional(v.string()),
      userAgent: v.optional(v.string()),
    })),
  })
    .index("by_user", ["userId"])
    .index("by_conversation", ["conversationId"])
    .index("by_status", ["status"]),

  // Webhooks - External integrations
  webhooks: defineTable({
    orgId: v.id("organizations"),
    name: v.string(),
    url: v.string(),
    events: v.array(v.string()),
    secret: v.string(),
    active: v.boolean(),
    
    // Stats
    lastTriggeredAt: v.optional(v.number()),
    successCount: v.number(),
    failureCount: v.number(),
    
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_active", ["active"]),

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

  // Audit Log - Track all actions
  auditLog: defineTable({
    orgId: v.id("organizations"),
    userId: v.optional(v.id("users")),
    action: v.string(),
    resource: v.string(),
    resourceId: v.optional(v.string()),
    changes: v.optional(v.any()),
    metadata: v.optional(v.object({
      ipAddress: v.optional(v.string()),
      userAgent: v.optional(v.string()),
    })),
    createdAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_user", ["userId"])
    .index("by_resource", ["resource"])
    .index("by_created", ["createdAt"]),

  // Agents - Per-org AI agent configurations
  agents: defineTable({
    orgId: v.id("organizations"),
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
    knowledgeSources: v.optional(v.array(v.object({
      type: v.string(),
      name: v.string(),
      id: v.optional(v.string()),
    }))),
    // Wave 9 (ui-ux-velion-gap.md §19): public embed widget. When
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
    // publicEnabled else `chat` (see resolveAgentProfile in velion).
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

  // U3-3 (ui-ux-velion-gap.md §10): agent run lifecycle mirror.
  //
  // The Model Plane's orchestrator-core publishes RUN_STARTED / RUN_COMPLETED
  // / RUN_FAILED envelopes on `mp.v1.run.{runId}.event` (see
  // `apps/Model Plane/go/services/orchestrator-core/cmd/activities/activities.go`).
  // The `nats-subscriber.js` service subscribes to the wildcard
  // `mp.v1.run.*.event` and calls `upsertAgentRun` here so velion's
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

  // G35: controlSessions — projection of the Control Session aggregate
  // produced by CP session-core (ADR 0002). One row per
  // (externalUserId, externalOrgId) pair. session-core's
  // `ControlSessionService.Refresh` (and any future writer) calls the
  // `upsertControlSession` HTTP action whenever the aggregate is refreshed.
  // Velion subscribes via `useQuery(api.controlSessions.byUser, ...)` for
  // reactive plan / entitlement / billing UI without polling.
  //
  // The full snapshot is stored as a single JSONB-style `snapshot` field so
  // the schema doesn't have to track every upstream shape change (the
  // contract is the JSON, not the Convex types). `index by_external_user`
  // is the velion subscription target; `by_external_user_and_org` covers
  // org-switch flows where the active org changes within a user.
  controlSessions: defineTable({
    externalUserId: v.string(),
    externalOrgId: v.optional(v.string()),
    snapshot: v.any(),
    fetchedAt: v.number(),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_external_user", ["externalUserId"])
    .index("by_external_user_and_org", ["externalUserId", "externalOrgId"]),

  // IngestJobs - Cross-plane ingestion job tracking (quarry crawl/scrape events)
  // Separate from user-scoped jobs; entries driven by Ingestion Plane NATS events.
  ingestJobs: defineTable({
    externalJobId: v.string(),
    externalOrgId: v.optional(v.string()),
    type: v.union(
      v.literal("crawl"),
      v.literal("scrape"),
      v.literal("import")
    ),
    status: v.union(
      v.literal("pending"),
      v.literal("running"),
      v.literal("indexing"),
      v.literal("completed"),
      v.literal("failed")
    ),
    url: v.optional(v.string()),
    progress: v.number(),
    progressMessage: v.optional(v.string()),
    pageCount: v.optional(v.number()),
    documentCount: v.optional(v.number()),
    sourceType: v.optional(v.string()),
    error: v.optional(v.string()),
    startedAt: v.optional(v.number()),
    completedAt: v.optional(v.number()),
    failedAt: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_external_id", ["externalJobId"])
    .index("by_org", ["externalOrgId"])
    .index("by_status", ["status"])
    .index("by_org_and_status", ["externalOrgId", "status"]),

  // Chat composer "Add to project" picker — per-org named buckets users can
  // file conversations under. Backs the ChatSettingsModal Projects view
  // (U2-14 follow-up — was SAMPLE_PROJECTS array of two hardcoded strings).
  //
  // `externalOrgId` mirrors the Better Auth / org-core organization id so
  // we can scope without joining through `organizations` (saves a hop).
  // Conversations link back via `conversations.metadata.projectId`.
  projects: defineTable({
    externalOrgId: v.string(),
    title: v.string(),
    description: v.optional(v.string()),
    createdBy: v.string(),       // user id (Better Auth subject)
    color: v.optional(v.string()), // optional UI hint, e.g. "amber-500"
    archived: v.boolean(),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_external_org", ["externalOrgId"])
    .index("by_org_and_archived", ["externalOrgId", "archived"]),

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

  // First-party Application Plane conversation-core projection.
  // These tables are separate from the existing AI chat `conversations` /
  // `messages` tables because conversation-core is the durable support inbox
  // truth while Convex only powers live UI projection.
  conversationProjectionEvents: defineTable({
    eventId: v.string(),
    type: v.string(),
    externalOrgId: v.string(),
    conversationId: v.optional(v.string()),
    messageId: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_conversation", ["conversationId"])
    .index("by_org", ["externalOrgId"]),

  conversationInboxItems: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    inboxId: v.string(),
    title: v.string(),
    status: v.string(),
    priority: v.string(),
    channel: v.string(),
    provider: v.optional(v.string()),
    providerThreadId: v.optional(v.string()),
    assigneeUserId: v.optional(v.string()),
    assigneeName: v.optional(v.string()),
    contactName: v.optional(v.string()),
    contactEmail: v.optional(v.string()),
    tags: v.array(v.string()),
    lastMessagePreview: v.optional(v.string()),
    lastMessageAt: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["externalOrgId"])
    .index("by_conversation", ["conversationId"])
    .index("by_org_and_status", ["externalOrgId", "status"])
    .index("by_org_and_updated", ["externalOrgId", "updatedAt"]),

  conversationProjectedMessages: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    messageId: v.string(),
    direction: v.string(),
    senderType: v.string(),
    senderName: v.optional(v.string()),
    senderEmail: v.optional(v.string()),
    bodyText: v.string(),
    bodyHtml: v.optional(v.string()),
    internal: v.boolean(),
    provider: v.optional(v.string()),
    occurredAt: v.number(),
    createdAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_message", ["messageId"])
    .index("by_conversation_and_occurred", ["conversationId", "occurredAt"]),

  conversationPresence: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    externalUserId: v.string(),
    status: v.union(v.literal("online"), v.literal("typing"), v.literal("away"), v.literal("offline")),
    updatedAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_user", ["externalUserId"]),

  conversationAiActions: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    aiActionId: v.string(),
    kind: v.string(),
    status: v.string(),
    payload: v.optional(v.any()),
    updatedAt: v.number(),
    createdAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_ai_action", ["aiActionId"]),

  // velionv2 search-v2 history — persisted AI answer-search threads + follow-up
  // turns. External-ID scoped (Better Auth org/user ids) like `projects` /
  // `controlSessions` so reads need no join through organizations/users and a
  // browser can subscribe to its own rows reactively. Writes come from the
  // velionv2 BFF via the service-key mutations below (search authority stays
  // server-side); reads are public arg-scoped queries for `useQuery`.
  searchThreads: defineTable({
    externalOrgId: v.string(),
    externalUserId: v.string(),
    // The originating query (turn 0) and its synthesized grounded answer.
    query: v.string(),
    answer: v.string(),
    citations: v.array(
      v.object({
        url: v.string(),
        title: v.optional(v.union(v.string(), v.null())),
      }),
    ),
    status: v.union(v.literal("active"), v.literal("deleted")),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["externalOrgId"])
    .index("by_user", ["externalUserId"])
    .index("by_org_and_user", ["externalOrgId", "externalUserId"])
    .index("by_user_and_updated", ["externalUserId", "updatedAt"]),

  searchTurns: defineTable({
    threadId: v.id("searchThreads"),
    // Denormalized external ids so turn-level reads can scope without a thread
    // lookup, mirroring how `messages` carries enough to authorize in one hop.
    externalOrgId: v.string(),
    externalUserId: v.string(),
    role: v.union(v.literal("user"), v.literal("assistant")),
    text: v.string(),
    citations: v.optional(
      v.array(
        v.object({
          url: v.string(),
          title: v.optional(v.union(v.string(), v.null())),
        }),
      ),
    ),
    createdAt: v.number(),
  })
    .index("by_thread", ["threadId"])
    .index("by_thread_and_created", ["threadId", "createdAt"]),
});
