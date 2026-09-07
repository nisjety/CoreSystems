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
    sourceRevision: v.optional(v.number()),
    sourceEventId: v.optional(v.string()),
    sourceFingerprint: v.optional(v.string()),

    // ADR-0003: org-admin-authored instructions, composed into every chat
    // turn's system message alongside the platform and Space layers. Plain
    // optional content, not an authority decision (see the ADR's "Presence,
    // not authority" section) -- no versioning in this slice.
    instructions: v.optional(v.string()),
    updatedByExternalAuthId: v.optional(v.string()),

    createdAt: v.number(),
    updatedAt: v.number(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_slug", ["slug"])
    .index("by_external_id", ["externalOrgId"]),

  organizationTombstones: defineTable({
    externalOrgId: v.string(),
    sourceRevision: v.number(),
    sourceEventId: v.string(),
    sourceFingerprint: v.string(),
    removedAt: v.number(),
  }).index("by_external_org", ["externalOrgId"]),

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
    sourceRevision: v.optional(v.number()),
    sourceEventId: v.optional(v.string()),
    sourceFingerprint: v.optional(v.string()),
    
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
    sourceRevision: v.optional(v.number()),
    sourceEventId: v.optional(v.string()),
    sourceFingerprint: v.optional(v.string()),
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

  // G35: controlSessions — projection of the Control Session aggregate
  // produced by CP session-core (ADR 0002). One row per
  // (externalUserId, externalOrgId) pair. session-core's
  // `ControlSessionService.Refresh` (and any future writer) calls the
  // `upsertControlSession` HTTP action whenever the aggregate is refreshed.
  // Verevon subscribes via `useQuery(api.controlSessions.byUser, ...)` for
  // reactive plan / entitlement / billing UI without polling.
  //
  // The full snapshot is stored as a single JSONB-style `snapshot` field so
  // the schema doesn't have to track every upstream shape change (the
  // contract is the JSON, not the Convex types). `index by_external_user`
  // is the verevon subscription target; `by_external_user_and_org` covers
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

  // Application is the canonical owner of a Space's immutable identity, kind,
  // display metadata, and lifecycle. Control registers this reference and is
  // the separate owner of membership and authorization decisions.
  spaces: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    kind: v.union(
      v.literal("personal"),
      v.literal("room"),
      v.literal("project"),
      v.literal("case"),
    ),
    name: v.string(),
    ownerExternalAuthId: v.optional(v.string()),
    createdByExternalAuthId: v.string(),
    // Marks THE organization room — the one durable `room` every active member
    // of an organization belongs to. Explicit rather than derived from
    // `(externalOrgId, kind)`, because team and external Spaces are also
    // `room`: keying idempotency on kind alone would make the first team room
    // collide with the organization room, and the collision would look like a
    // successful reuse rather than an error.
    //
    // Optional and `true`-only, so an ordinary room simply omits it and no
    // backfill is needed for rooms that already exist.
    isOrganizationRoom: v.optional(v.literal(true)),
    lifecycle: v.union(
      v.literal("pending_registration"),
      v.literal("active"),
      v.literal("suspended"),
      v.literal("deleting"),
      v.literal("deleted"),
      v.literal("failed_registration"),
    ),
    lifecycleRevision: v.number(),
    controlResourceRef: v.optional(v.string()),
    // ADR-0003: Space owner/manager-authored instructions, composed into
    // every turn in this Space alongside the platform and org layers. Same
    // "presence, not authority" status as `organizations.instructions` --
    // authored content, never an authorization primitive.
    instructions: v.optional(v.string()),
    updatedByExternalAuthId: v.optional(v.string()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_space_ref", ["spaceRef"])
    .index("by_external_org", ["externalOrgId"])
    .index("by_personal_owner", ["externalOrgId", "kind", "ownerExternalAuthId"]),

  // An agent definition bound to ONE Space, per
  // `docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` §3.2. The plan's central rule is
  // that a definition is not a membership: `agents` says what an agent *is*,
  // this table says where it is allowed to appear as a room participant. A
  // second Space needs a second row, and a page/system install is a different
  // binding entirely — never this one.
  //
  // # This table is presentation and policy, NOT authority
  //
  // Control owns whether an agent may actually reach a Space: it already models
  // that as a `space_memberships` row with `subject_type='service'`. Duplicating
  // that decision here would create two answers to one question, and the stale
  // one would eventually win. So the gateway intersects: Control decides who is
  // bound, this projection supplies the identity to render. A row here whose
  // Control membership is gone must never appear as an active participant.
  spaceAgentBindings: defineTable({
    bindingRef: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    // The definition this binding points at. Kept as a real reference so a
    // deleted definition cannot leave a binding rendering a ghost name.
    agentId: v.id("agents"),
    // The Control-side subject this binding corresponds to, so the gateway can
    // match a binding to the authoritative roster row without guessing by name.
    subjectId: v.string(),
    // Space-local presentation override. Absent means "use the definition's own
    // name/description", which is the common case.
    displayName: v.optional(v.string()),
    title: v.optional(v.string()),
    // Binding policy (space-defenition.md "Binding model"). These are human
    // decisions about how the agent may be invoked here, enforced at the
    // gateway's invocation path. ABSENT means the pre-policy legacy behavior
    // (mention allowed, confirmation required, legacy tool surface) so old
    // bindings keep working exactly as they did. The doc's remaining policy
    // fields (knowledge_scope, default_thread_policy, audit_visibility) are
    // deliberately NOT stored yet: nothing enforces them, and a stored-but-
    // unenforced policy is a false promise.
    triggerModes: v.optional(v.array(v.union(v.literal("mention"), v.literal("group")))),
    allowedTools: v.optional(v.array(v.string())),
    approvalMode: v.optional(
      v.union(v.literal("auto"), v.literal("require_confirmation"), v.literal("blocked")),
    ),
    status: v.union(
      v.literal("pending"),
      v.literal("active"),
      v.literal("paused"),
      v.literal("revoked"),
      v.literal("failed"),
    ),
    // Where this agent's work can be delivered besides the room itself. These
    // are the surfaces the operator has actually configured — an empty list
    // means no channel is published, and the UI must say exactly that rather
    // than implying the agent is reachable somewhere it is not.
    deliveryTargets: v.optional(
      v.array(
        v.object({
          channel: v.union(
            v.literal("teams"),
            v.literal("messenger"),
            v.literal("embed"),
          ),
          // Operator-facing label for the specific destination (a Teams channel
          // name, a Messenger page). Never a token or a secret.
          label: v.string(),
          status: v.union(
            v.literal("active"),
            v.literal("pending"),
            v.literal("failed"),
          ),
        }),
      ),
    ),
    projectionVersion: v.number(),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_binding_ref", ["bindingRef"])
    .index("by_space_ref", ["spaceRef"])
    .index("by_space_and_subject", ["spaceRef", "subjectId"])
    // ADR-0002 (apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md): the
    // org-wide registry (`spaceAgents:agentInstallationsForOrgForGateway`)
    // needs every binding for an org in one indexed read, the same shape
    // `spaces.by_external_org` already gives `spacesForOrgForGateway`.
    // Additive and backward-compatible: computed from the `externalOrgId`
    // every row already carries, no data migration required.
    .index("by_external_org", ["externalOrgId"]),

  // Transactional outbox for Space lifecycle notifications. Consumers dedupe
  // by the immutable event ID and never infer authorization from this event.
  spaceLifecycleEvents: defineTable({
    eventId: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    lifecycle: v.union(
      v.literal("pending_registration"),
      v.literal("active"),
      v.literal("suspended"),
      v.literal("deleting"),
      v.literal("deleted"),
      v.literal("failed_registration"),
    ),
    revision: v.number(),
    // Delivery is an at-least-once outbox. The Control endpoint deduplicates
    // stable event IDs; a claim lease prevents concurrent workers from
    // delivering the same row in the ordinary case.
    deliveryState: v.optional(v.union(
      v.literal("pending"),
      v.literal("claimed"),
      v.literal("acknowledged"),
      v.literal("failed"),
      v.literal("rejected"),
    )),
    deliveryAttempts: v.optional(v.number()),
    leaseOwner: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    nextAttemptAt: v.optional(v.number()),
    deliveredAt: v.optional(v.number()),
    lastDeliveryError: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_space_and_revision", ["spaceRef", "revision"])
    .index("by_delivery_state_and_next_attempt", ["deliveryState", "nextAttemptAt"]),

  // A human deletion request is durable product intent, not a deletion
  // receipt. Control must authorize it against fresh owner/policy/legal-hold
  // facts before Application fences the canonical Space.
  spaceDeletionRequests: defineTable({
    requestId: v.string(),
    idempotencyKey: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    ownerExternalAuthId: v.string(),
    state: v.union(
      v.literal("pending_authorization"),
      v.literal("authorized"),
      v.literal("blocked_legal_hold"),
      v.literal("rejected"),
    ),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_request_id", ["requestId"])
    .index("by_space_and_idempotency", ["spaceRef", "idempotencyKey"]),

  // Leased, at-least-once delivery of immutable deletion intent to Control.
  // The receipt is authorization only; owner-plane purge receipts belong to a
  // later deletion-coordinator protocol.
  spaceDeletionAuthorizationEvents: defineTable({
    eventId: v.string(),
    requestId: v.string(),
    deliveryState: v.union(
      v.literal("pending"),
      v.literal("claimed"),
      v.literal("acknowledged"),
      v.literal("failed"),
    ),
    deliveryAttempts: v.number(),
    leaseOwner: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    nextAttemptAt: v.number(),
    lastDeliveryError: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_request_id", ["requestId"])
    .index("by_delivery_state_and_next_attempt", ["deliveryState", "nextAttemptAt"]),

  // Owner planes publish their own idempotent purge/export result here through
  // a future authenticated coordinator ingress. This is deliberately separate
  // from the authorization event so a timeout cannot be displayed as erased.
  spaceDeletionOwnerReceipts: defineTable({
    requestId: v.string(),
    ownerPlane: v.union(
      v.literal("application"), v.literal("control"), v.literal("data"),
      v.literal("ingestion"), v.literal("model"), v.literal("infra"),
    ),
    status: v.union(
      v.literal("pending"), v.literal("blocked_legal_hold"), v.literal("succeeded"),
      v.literal("partial"), v.literal("failed"), v.literal("unknown"),
    ),
    receiptRef: v.optional(v.string()),
    detail: v.optional(v.string()),
    // Optional only for compatibility with receipts written before deadlines
    // were introduced; every new receipt writes it and the reconciler leaves a
    // legacy row pending for explicit operator migration rather than guessing.
    deadlineAt: v.optional(v.number()),
    updatedAt: v.number(),
  })
    .index("by_request_and_owner", ["requestId", "ownerPlane"])
    .index("by_request", ["requestId"]),

  // A separate leased delivery record for an owner-plane purge adapter. It is
  // not the owner receipt itself: only a parseable owner response can advance
  // the receipt, while a timeout/crash remains retryable and visibly pending.
  spaceDeletionOwnerDeliveryEvents: defineTable({
    eventId: v.string(),
    requestId: v.string(),
    ownerPlane: v.union(
      v.literal("application"), v.literal("control"), v.literal("data"),
      v.literal("ingestion"), v.literal("model"), v.literal("infra"),
    ),
    deliveryState: v.union(v.literal("pending"), v.literal("claimed"), v.literal("acknowledged"), v.literal("failed")),
    deliveryAttempts: v.number(),
    leaseOwner: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    nextAttemptAt: v.number(),
    lastDeliveryError: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_request_and_owner", ["requestId", "ownerPlane"])
    .index("by_owner_and_delivery", ["ownerPlane", "deliveryState", "nextAttemptAt"]),

  // Application owns the current conversation/case recipient set as a
  // versioned product fact. It is not an authorization decision: Control
  // independently verifies every member before any owner plane receives a
  // shared-effect decision. Superseded snapshots remain for visibility-safe
  // replay/fork decisions; their principal IDs never go to the browser.
  // Who a NAMED room's people are, as decided by its owners and managers.
  //
  // Only for rooms somebody created and populated by hand. The organization
  // room deliberately has no rows here: its roster is derived from org-core by
  // `spaceMembershipSync`, on the rule that being in the organization is what
  // grants a place in its room. Two sources for one room's people would fight,
  // and the derived one would win every sync.
  //
  // This is an INTENT record, not an authority. Control owns membership; this
  // table is what Application declares to it, and a row here means nothing
  // until that declaration is accepted.
  spaceMemberGrants: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    // The person's auth identity, the same subject Control stores in
    // `space_memberships.subject_id` for a user.
    externalAuthId: v.string(),
    // Deliberately narrow: this flow grants participation, not the ability to
    // hand out more of it. Promoting someone to manage a room is a separate
    // decision that does not exist yet, and inventing it here would let anyone
    // who can add a person also create another grantor.
    role: v.literal("editor"),
    grantedByExternalAuthId: v.string(),
    createdAt: v.number(),
  })
    .index("by_space_ref", ["spaceRef"])
    .index("by_space_and_subject", ["spaceRef", "externalAuthId"]),

  spaceRecipientAudiences: defineTable({
    audienceRef: v.string(),
    audienceHash: v.string(),
    controlState: v.union(v.literal("pending"), v.literal("acknowledged"), v.literal("rejected")),
    controlRegisteredAt: v.optional(v.number()),
    externalOrgId: v.string(),
    recipientExternalAuthIds: v.array(v.string()),
    revision: v.number(),
    spaceRef: v.string(),
    state: v.union(v.literal("active"), v.literal("superseded")),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_audience_ref", ["audienceRef"])
    .index("by_space_and_revision", ["spaceRef", "revision"])
    .index("by_space_and_state", ["spaceRef", "state"]),

  // Transactional outbox for Application -> Control audience registration.
  // The event carries no participant IDs; the worker rereads the immutable
  // audience revision only after leasing this row.
  spaceRecipientAudienceEvents: defineTable({
    audienceRef: v.string(),
    deliveryAttempts: v.optional(v.number()),
    deliveryState: v.optional(v.union(
      v.literal("pending"), v.literal("claimed"), v.literal("acknowledged"),
      v.literal("failed"), v.literal("rejected"),
    )),
    eventId: v.string(),
    externalOrgId: v.string(),
    lastDeliveryError: v.optional(v.string()),
    leaseExpiresAt: v.optional(v.number()),
    leaseOwner: v.optional(v.string()),
    nextAttemptAt: v.optional(v.number()),
    revision: v.number(),
    spaceRef: v.string(),
    createdAt: v.number(),
    deliveredAt: v.optional(v.number()),
  })
    .index("by_event_id", ["eventId"])
    .index("by_delivery_state_and_next_attempt", ["deliveryState", "nextAttemptAt"]),

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

  // verevonv2 search-v2 history — persisted AI answer-search threads + follow-up
  // turns. External-ID scoped (Better Auth org/user ids) like `projects` /
  // `controlSessions` so reads need no join through organizations/users and a
  // browser can subscribe to its own rows reactively. Writes come from the
  // verevonv2 BFF via the service-key mutations below (search authority stays
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
