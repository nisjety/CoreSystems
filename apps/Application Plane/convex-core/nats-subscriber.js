/**
 * NATS Subscriber Service
 * 
 * This is a standalone service that subscribes to NATS events and calls
 * Convex internal mutations to keep the database synchronized.
 * 
 * This would typically run as a separate service or as a scheduled job
 * in the Convex backend.
 */

const nats = require("nats");

// Configuration
const NATS_URL = process.env.NATS_URL || "nats://localhost:4222";
const NATS_TOKEN = process.env.NATS_TOKEN;
const CONVEX_URL = process.env.CONVEX_BACKEND_URL || "http://localhost:3000";
const CONVEX_API_KEY = process.env.CONVEX_API_KEY;

if (!CONVEX_API_KEY) {
  throw new Error("CONVEX_API_KEY must be configured");
}
if (!NATS_TOKEN) {
  throw new Error("NATS_TOKEN must be configured");
}

const CONTROL_PLANE_SUBJECTS = Object.freeze({
  organizationCreated: "aqencia.controlplane.org.created",
  organizationUpdated: "aqencia.controlplane.org.updated",
  organizationDeleted: "aqencia.controlplane.org.deleted",
  memberAdded: "aqencia.controlplane.org.member_added",
  memberRemoved: "aqencia.controlplane.org.member_removed",
});
const CONTROL_PLANE_DLQ_SUBJECT = "velion.application.dlq.convex.controlplane";
const CONTROL_PLANE_DLQ_STREAM = "CONVEX_CONTROLPLANE_DLQ";
const DEAD_LETTER_AFTER = 5;

function redactControlPlanePayload(payload) {
  const allowed = [
    "org_id",
    "user_id",
    "role",
    "_source",
    "_published_at",
  ];
  return Object.fromEntries(
    allowed
      .filter((key) => payload?.[key] !== undefined)
      .map((key) => [key, payload[key]]),
  );
}

async function processJetStreamMessage(message, handler, publishDeadLetter) {
  let payload = {};
  try {
    payload = JSON.parse(new TextDecoder().decode(message.data));
    await handler(payload);
    message.ack();
    return "applied";
  } catch (error) {
    const redeliveryCount = message.info?.redeliveryCount ?? 1;
    if (redeliveryCount >= DEAD_LETTER_AFTER) {
      try {
        await publishDeadLetter({
          subject: message.subject,
          payload: redactControlPlanePayload(payload),
          failure: error instanceof Error ? error.name : "ProjectionError",
          failedAt: new Date().toISOString(),
          redeliveryCount,
        });
        message.term();
        return "dead_lettered";
      } catch {
        // Never terminate the authoritative event unless the durable DLQ write
        // succeeded. A later redelivery can retry the DLQ write.
        message.nak(5000);
        return "retrying_dlq_unavailable";
      }
    }
    message.nak(1000);
    return "retrying";
  }
}

function requiredString(payload, field) {
  const value = payload?.[field];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`Control Plane event field ${field} is required`);
  }
  return value.trim();
}

function sourceUpdatedAt(payload) {
  const value = Date.parse(requiredString(payload, "_published_at"));
  if (!Number.isFinite(value)) {
    throw new Error("Control Plane event _published_at is invalid");
  }
  return value;
}

function normalizeControlPlaneEvent(eventType, payload) {
  const common = {
    source: requiredString(payload, "_source"),
    sourceUpdatedAt: sourceUpdatedAt(payload),
  };
  switch (eventType) {
    case "organizationCreated":
      return {
        orgId: requiredString(payload, "org_id"),
        name: requiredString(payload, "org_name"),
        slug: requiredString(payload, "slug"),
        createdAt: common.sourceUpdatedAt,
      };
    case "organizationUpdated": {
      const changes = payload?.changes ?? {};
      return {
        orgId: requiredString(payload, "org_id"),
        name: typeof changes.name === "string" ? changes.name : undefined,
        slug: typeof changes.slug === "string" ? changes.slug : undefined,
        settings: changes.settings,
        updatedAt: common.sourceUpdatedAt,
      };
    }
    case "organizationDeleted":
      return { orgId: requiredString(payload, "org_id") };
    case "memberAdded":
      return {
        orgId: requiredString(payload, "org_id"),
        userId: requiredString(payload, "user_id"),
        email: requiredString(payload, "user_email"),
        role: requiredString(payload, "role"),
        addedAt: common.sourceUpdatedAt,
      };
    case "memberRemoved":
      return {
        orgId: requiredString(payload, "org_id"),
        userId: requiredString(payload, "user_id"),
        source: common.source,
        sourceUpdatedAt: common.sourceUpdatedAt,
      };
    default:
      throw new Error(`Unsupported Control Plane event type: ${eventType}`);
  }
}

// W4-2 (ui-ux-velion-gap.md §13): Model Plane's orchestrator-core publishes
// `mp.v1.run.{id}.event` to `model-plane-nats` (port 4222 inside its compose
// network). Velion-nats and model-plane-nats are isolated clusters with
// `routes = []`, so the subscriber needs a SECOND connection here for those
// subjects. When the env var isn't set we silently skip the second
// connection (single-NATS deployments stay unchanged).
const MODEL_PLANE_NATS_URL = process.env.MODEL_PLANE_NATS_URL || "";
const MODEL_PLANE_NATS_TOKEN =
  process.env.MODEL_PLANE_NATS_TOKEN || process.env.NATS_TOKEN || "";

class ConvexNatsSubscriber {
  constructor() {
    this.nc = null;
    this.js = null;
    // W4-2: optional second connection to model-plane-nats for `mp.v1.*` subjects.
    this.ncModelPlane = null;
  }

  /**
   * Connect to NATS and setup subscriptions
   */
  async connect() {
    console.log("[Convex NATS] Connecting to NATS at", NATS_URL);

    try {
      // Connect to NATS
      this.nc = await nats.connect({
        servers: [NATS_URL],
        token: NATS_TOKEN,
        name: "convex-subscriber",
        maxReconnectAttempts: 10,
        reconnectDelayMs: 2000,
      });

      // Get JetStream context
      this.js = this.nc.jetstream();
      await this.ensureDeadLetterStream();

      console.log("[Convex NATS] Connected successfully");

      // W4-2: optional second connection for `mp.v1.*` cross-plane events.
      if (MODEL_PLANE_NATS_URL) {
        console.log(
          "[Convex NATS] Connecting to Model Plane NATS at",
          MODEL_PLANE_NATS_URL,
        );
        try {
          this.ncModelPlane = await nats.connect({
            servers: [MODEL_PLANE_NATS_URL],
            token: MODEL_PLANE_NATS_TOKEN || undefined,
            name: "convex-subscriber-mp",
            maxReconnectAttempts: 10,
            reconnectDelayMs: 2000,
          });
          console.log("[Convex NATS] Model Plane NATS connected");
        } catch (mpErr) {
          // Failure to attach to the second cluster shouldn't block the
          // primary subscriber path — log and continue. The mp.v1.*
          // subscription below will be skipped.
          console.warn(
            "[Convex NATS] Model Plane NATS connection failed:",
            mpErr.message ?? mpErr,
          );
          this.ncModelPlane = null;
        }
      } else {
        console.log(
          "[Convex NATS] MODEL_PLANE_NATS_URL not set — mp.v1.* subjects will not be mirrored",
        );
      }

      // Setup subscriptions
      await this.setupSubscriptions();
    } catch (error) {
      console.error("[Convex NATS] Connection error:", error);
      throw error;
    }
  }

  /**
   * Setup event subscriptions
   */
  async setupSubscriptions() {
    console.log("[Convex NATS] Setting up event subscriptions...");

    try {
      // Control Plane organization events (via velion-nats cross-plane bus)
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.organizationCreated,
        "convex-org-created-v1",
        this.handleOrganizationCreated.bind(this)
      );
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.organizationUpdated,
        "convex-org-updated-v1",
        this.handleOrganizationUpdated.bind(this)
      );
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.organizationDeleted,
        "convex-org-deleted-v1",
        this.handleOrganizationDeleted.bind(this)
      );
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.memberAdded,
        "convex-org-member-added-v1",
        this.handleMemberAdded.bind(this)
      );
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.memberRemoved,
        "convex-org-member-removed-v1",
        this.handleMemberRemoved.bind(this)
      );

      // Ingestion Plane events
      await this.subscribeToTopic(
        "velion.ingestion.import.completed",
        this.handleImportCompleted.bind(this)
      );

      await this.subscribeToTopic(
        "velion.application.conversation.>",
        this.handleConversationEvent.bind(this)
      );

      // Quarry crawl job events
      await this.subscribeToTopic(
        "velion.ingestion.crawl.started",
        this.handleCrawlStarted.bind(this)
      );
      await this.subscribeToTopic(
        "velion.ingestion.crawl.progress",
        this.handleCrawlProgress.bind(this)
      );
      await this.subscribeToTopic(
        "velion.ingestion.crawl.completed",
        this.handleCrawlCompleted.bind(this)
      );
      await this.subscribeToTopic(
        "velion.ingestion.crawl.failed",
        this.handleCrawlFailed.bind(this)
      );
      await this.subscribeToTopic(
        "velion.ingestion.crawl.indexed",
        this.handleCrawlIndexed.bind(this)
      );

      // U3-3 (ui-ux-velion-gap.md §10): Model Plane agent run lifecycle.
      // orchestrator-core publishes RUN_STARTED / RUN_COMPLETED / RUN_FAILED
      // envelopes on `mp.v1.run.{runId}.event`. We subscribe to the
      // wildcard form so any run lands in Convex without per-run setup.
      //
      // W4-2: in production these events arrive on `model-plane-nats`,
      // NOT `velion-nats` (the two clusters are isolated). Use the
      // optional second connection when available; fall back to the
      // primary connection (dev / single-NATS deployments) otherwise.
      const mpNats = this.ncModelPlane ?? this.nc;
      await this.subscribeToTopicOn(
        mpNats,
        "mp.v1.run.*.event",
        this.handleAgentRunEvent.bind(this),
        this.ncModelPlane ? "model-plane-nats" : "velion-nats (fallback)",
      );

      console.log("[Convex NATS] All subscriptions established");
    } catch (error) {
      console.error("[Convex NATS] Subscription setup error:", error);
      throw error;
    }
  }

  /**
   * Subscribe to a NATS topic on the default (velion-nats) connection.
   */
  async subscribeToTopic(topic, handler) {
    return this.subscribeToTopicOn(this.nc, topic, handler, "velion-nats");
  }

  async ensureDeadLetterStream() {
    const manager = await this.nc.jetstreamManager();
    try {
      await manager.streams.info(CONTROL_PLANE_DLQ_STREAM);
      return;
    } catch (error) {
      const code = error?.code ?? error?.api_error?.code;
      const errorCode = error?.api_error?.err_code;
      if (String(code) !== "404" && errorCode !== 10059) {
        throw error;
      }
    }

    await manager.streams.add({
      name: CONTROL_PLANE_DLQ_STREAM,
      subjects: [CONTROL_PLANE_DLQ_SUBJECT],
      retention: nats.RetentionPolicy.Limits,
      storage: nats.StorageType.File,
      discard: nats.DiscardPolicy.Old,
      max_msgs: 10_000,
      max_age: nats.nanos(14 * 24 * 60 * 60 * 1000),
    });
  }

  async publishDeadLetter(payload) {
    const codec = nats.StringCodec();
    await this.js.publish(
      CONTROL_PLANE_DLQ_SUBJECT,
      codec.encode(JSON.stringify(payload)),
    );
  }

  async subscribeToDurableTopic(topic, durableName, handler) {
    const options = nats.consumerOpts();
    options.durable(durableName);
    options.manualAck();
    options.ackExplicit();
    options.ackWait(30_000);
    // The application moves poison events into its own DLQ after five tries.
    // Keep the broker ceiling higher so a transient DLQ outage cannot lose one.
    options.maxDeliver(100);
    options.deliverTo(nats.createInbox());

    const subscription = await this.js.subscribe(topic, options);
    console.log(`[Convex NATS] Durable subscription: ${topic} (${durableName})`);
    (async () => {
      for await (const message of subscription) {
        const result = await processJetStreamMessage(
          message,
          handler,
          this.publishDeadLetter.bind(this),
        );
        if (result !== "applied") {
          console.error(
            `[Convex NATS] Projection ${result}: subject=${topic} redelivery=${message.info?.redeliveryCount ?? 1}`,
          );
        }
      }
    })();
  }

  /**
   * W4-2 (ui-ux-velion-gap.md §13): subscribe to a topic on an
   * arbitrary NATS connection so the subscriber can multiplex across
   * the velion + model-plane clusters. `connection` is the result of
   * a previous `nats.connect()`; `clusterLabel` is purely cosmetic for
   * structured logs ("velion-nats" / "model-plane-nats").
   */
  async subscribeToTopicOn(connection, topic, handler, clusterLabel) {
    if (!connection) {
      console.warn(
        `[Convex NATS] Skipping subscription to ${topic} — connection is null`,
      );
      return;
    }
    const sub = connection.subscribe(topic);

    console.log(`[Convex NATS] Subscribed to: ${topic} (cluster=${clusterLabel})`);

    // Process messages
    (async () => {
      for await (const m of sub) {
        try {
          const data = JSON.parse(new TextDecoder().decode(m.data));
          await handler(data);
        } catch (error) {
          console.error(
            `[Convex NATS] Error handling ${topic} (cluster=${clusterLabel}):`,
            error,
          );
        }
      }
    })();
  }

  /**
   * Handle velion.controlplane.org.created event
   */
  async handleOrganizationCreated(payload) {
    const event = normalizeControlPlaneEvent("organizationCreated", payload);
    console.log("[Convex NATS] Processing org.created:", event.orgId);
    await this.callConvexMutation("nats:onOrganizationCreated", event);
  }

  /**
   * Handle velion.controlplane.org.updated event
   */
  async handleOrganizationUpdated(payload) {
    const event = normalizeControlPlaneEvent("organizationUpdated", payload);
    console.log("[Convex NATS] Processing org.updated:", event.orgId);
    await this.callConvexMutation("nats:onOrganizationUpdated", event);
  }

  /**
   * Handle velion.controlplane.org.deleted event
   */
  async handleOrganizationDeleted(payload) {
    const event = normalizeControlPlaneEvent("organizationDeleted", payload);
    console.log("[Convex NATS] Processing org.deleted:", event.orgId);
    await this.callConvexMutation("nats:onOrganizationDeleted", event);
  }

  /**
   * Handle velion.controlplane.org.member.added event
   */
  async handleMemberAdded(payload) {
    const event = normalizeControlPlaneEvent("memberAdded", payload);
    console.log("[Convex NATS] Processing org.member.added:", event.userId);
    await this.callConvexMutation("nats:onOrganizationMemberAdded", event);
  }

  /**
   * Handle velion.controlplane.org.member.removed event
   */
  async handleMemberRemoved(payload) {
    const event = normalizeControlPlaneEvent("memberRemoved", payload);
    console.log("[Convex NATS] Processing org.member.removed:", event.userId);
    await this.callConvexMutation("nats:onOrganizationMemberRemoved", event);
  }

  /**
   * Handle velion.ingestion.import.completed event
   */
  async handleImportCompleted(payload) {
    console.log(
      "[Convex NATS] Processing import.completed:",
      payload.import_id || payload.job_id
    );

    try {
      await this.callConvexMutation("nats:onImportCompleted", {
        importId: payload.import_id || payload.job_id,
        orgId: payload.org_id,
        sourceType: payload.source_type,
        totalDocuments: payload.total_documents || 0,
        completedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error syncing import completion:", error);
    }
  }

  /**
   * Handle velion.ingestion.crawl.started event
   * Payload: { org_id, url, crawl_id, service, metadata }
   */
  async handleCrawlStarted(payload) {
    console.log("[Convex NATS] Processing crawl.started:", payload.crawl_id);
    try {
      await this.callConvexMutation("nats:onCrawlStarted", {
        crawlId: payload.crawl_id,
        orgId: payload.org_id || "",
        url: payload.url || "",
        startedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error handling crawl.started:", error);
    }
  }

  /**
   * Handle velion.ingestion.crawl.progress event
   * Payload: { org_id, crawl_id, progress, completed, total, message }
   */
  async handleCrawlProgress(payload) {
    console.log("[Convex NATS] Processing crawl.progress:", payload.crawl_id, payload.progress + "%");
    try {
      await this.callConvexMutation("nats:onCrawlProgress", {
        crawlId: payload.crawl_id,
        orgId: payload.org_id || "",
        progress: payload.progress || 0,
        completed: payload.completed || 0,
        total: payload.total || 0,
        message: payload.message || "",
        updatedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error handling crawl.progress:", error);
    }
  }

  /**
   * Handle velion.ingestion.crawl.completed event
   * Payload: { org_id, url, crawl_id, page_count, service, metadata }
   */
  async handleCrawlCompleted(payload) {
    console.log("[Convex NATS] Processing crawl.completed:", payload.crawl_id, "pages:", payload.page_count);
    try {
      await this.callConvexMutation("nats:onCrawlCompleted", {
        crawlId: payload.crawl_id,
        orgId: payload.org_id || "",
        url: payload.url || "",
        pageCount: payload.page_count || 0,
        completedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error handling crawl.completed:", error);
    }
  }

  /**
   * Handle velion.ingestion.crawl.failed event
   * Payload: { org_id, url, crawl_id, error, service, metadata }
   */
  async handleCrawlFailed(payload) {
    console.log("[Convex NATS] Processing crawl.failed:", payload.crawl_id, "error:", payload.error);
    try {
      await this.callConvexMutation("nats:onCrawlFailed", {
        crawlId: payload.crawl_id,
        orgId: payload.org_id || "",
        url: payload.url || "",
        error: payload.error || "unknown error",
        failedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error handling crawl.failed:", error);
    }
  }

  /**
   * Handle velion.ingestion.crawl.indexed event
   * Payload: { org_id, crawl_id, ingested_count, indexed_at }
   */
  async handleCrawlIndexed(payload) {
    console.log("[Convex NATS] Processing crawl.indexed:", payload.crawl_id, "ingested:", payload.ingested_count);
    try {
      await this.callConvexMutation("nats:onCrawlIndexed", {
        crawlId: payload.crawl_id,
        orgId: payload.org_id || "",
        ingestedCount: payload.ingested_count || 0,
        indexedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error handling crawl.indexed:", error);
    }
  }

  /**
   * U3-3 (ui-ux-velion-gap.md §10): Model Plane agent run lifecycle.
   *
   * Subject: `mp.v1.run.{runId}.event` (wildcard subscription)
   * Envelope shape: standard mp.v1 wrapper produced by
   * `pkg/envelope.Wrap()`. The inner payload is published by
   * `orchestrator-core/cmd/activities/activities.go::publishRunEvent`
   * and carries:
   *   { run_id, event_type, org_id, user_id?, agent_id?, error?, ts_ms? }
   *
   * We accept both wrapped and unwrapped shapes — the orchestrator's
   * envelope may evolve and the mirror should keep working.
   */
  async handleAgentRunEvent(payload) {
    // Unwrap mp.v1 envelope when present. The envelope.Wrap() function in
    // pkg/envelope produces a struct with `producer`, `correlation_id`,
    // `event_type`, `payload` fields. The actual orchestrator data is
    // inside `payload` (or `data`).
    const inner =
      (payload && typeof payload === "object" && "payload" in payload && payload.payload) ||
      (payload && typeof payload === "object" && "data" in payload && payload.data) ||
      payload;

    const eventType =
      (payload && payload.event_type) ||
      (inner && inner.event_type) ||
      "";
    const runId =
      (inner && inner.run_id) ||
      (payload && payload.correlation_id) ||
      "";
    if (!runId || !eventType) {
      console.warn("[Convex NATS] agent-run event missing run_id or event_type — skipping");
      return;
    }
    console.log(`[Convex NATS] Processing agent run ${eventType} run=${runId}`);
    try {
      await this.callConvexMutation("nats:onAgentRunEvent", {
        run_id: runId,
        event_type: eventType,
        org_id: inner?.org_id ?? payload?.org_id ?? "",
        user_id: inner?.user_id ?? undefined,
        agent_id: inner?.agent_id ?? undefined,
        error: inner?.error ?? undefined,
        ts_ms: payload?.ts_ms ?? inner?.ts_ms ?? Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error handling agent run event:", error);
    }
  }

  async handleConversationEvent(payload) {
    const conversationId = payload?.conversation_id || payload?.data?.conversation?.id || "";
    console.log("[Convex NATS] Processing conversation event:", payload?.type, conversationId);
    try {
      await this.callConvexMutation("nats:onConversationEvent", payload || {});
    } catch (error) {
      console.error("[Convex NATS] Error handling conversation event:", error);
    }
  }

  async callConvexMutation(name, args) {
    // Self-hosted HTTP Actions are on port 3211
    // The path should match whatever convex http.ts sets up
    const HTTP_ACTIONS_URL = CONVEX_URL.replace("3210", "3211");
    // Fall back to generic /api webhook if specific action isn't available
    const actionUrl = name.startsWith("nats:")
      ? `${HTTP_ACTIONS_URL}/api/webhook/nats/${name.replace("nats:", "")}`
      : `${HTTP_ACTIONS_URL}/api/call/${name}`;

    console.log(`[Convex NATS] Calling internal Webhook: ${actionUrl}`);

    // NOTE: If using the Convex node client, it would be client.mutation(api.nats[name], args)
    // Here we use fetch against Convex HTTP Actions
    const response = await fetch(actionUrl, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${CONVEX_API_KEY}`,
      },
      body: JSON.stringify(args),
    });

    if (!response.ok) {
      throw new Error(
        `Convex mutation failed: ${response.status} ${response.statusText}`
      );
    }

    return response.json();
  }

  /**
   * Close NATS connection
   */
  async close() {
    if (this.nc) {
      await this.nc.close();
      console.log("[Convex NATS] Disconnected");
    }
  }
}

// Main execution
async function main() {
  const subscriber = new ConvexNatsSubscriber();

  try {
    await subscriber.connect();

    // Keep process alive
    console.log("[Convex NATS] Subscriber running, listening for events...");

    // Graceful shutdown
    process.on("SIGINT", async () => {
      console.log("[Convex NATS] Shutting down...");
      await subscriber.close();
      process.exit(0);
    });

    process.on("SIGTERM", async () => {
      console.log("[Convex NATS] Shutting down...");
      await subscriber.close();
      process.exit(0);
    });
  } catch (error) {
    console.error("[Convex NATS] Fatal error:", error);
    process.exit(1);
  }
}

// Run if this is the main module
if (require.main === module) {
  main();
}

module.exports = {
  CONTROL_PLANE_SUBJECTS,
  ConvexNatsSubscriber,
  normalizeControlPlaneEvent,
  processJetStreamMessage,
};
