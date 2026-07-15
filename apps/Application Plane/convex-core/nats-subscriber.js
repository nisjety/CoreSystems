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
const NATS_USER = process.env.NATS_USER || "";
const NATS_PASSWORD = process.env.NATS_PASSWORD || "";
const CONVEX_URL = process.env.CONVEX_BACKEND_URL || "http://localhost:3000";
const CONVEX_CONTROL_PROJECTION_KEY =
  process.env.CONVEX_CONTROL_PROJECTION_KEY;

if (!CONVEX_CONTROL_PROJECTION_KEY) {
  throw new Error("CONVEX_CONTROL_PROJECTION_KEY must be configured");
}

const CONTROL_PLANE_SUBJECTS = Object.freeze({
  organizationChanged: "aqencia.controlplane.org.changed",
  memberChanged: "aqencia.controlplane.org.member_changed",
});
const CONTROL_PLANE_DLQ_SUBJECT = "velion.application.dlq.convex.controlplane";
const CONTROL_PLANE_STREAM = "AQENCIA_CONTROLPLANE";
const DEAD_LETTER_AFTER = 5;

function controlPlaneConnectionOptions({ url, user, password }) {
  const normalizedUser = String(user ?? "").trim();
  const normalizedPassword = String(password ?? "").trim();
  if (!normalizedUser || normalizedPassword.length < 32) {
    throw new Error(
      "Control Plane NATS requires a scoped user/password credential",
    );
  }
  return {
    servers: [url],
    user: normalizedUser,
    pass: normalizedPassword,
    name: "convex-subscriber-control",
    inboxPrefix: "_INBOX.APPLICATION_CONVEX_CONTROL",
    maxReconnectAttempts: 10,
    reconnectDelayMs: 2000,
  };
}

function redactControlPlanePayload(payload) {
  const allowed = [
    "schema_version",
    "event_id",
    "action",
    "org_id",
    "user_id",
    "role",
    "revision",
    "organization_revision",
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

function positiveSafeInteger(payload, field) {
  const value = payload?.[field];
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`Control Plane event field ${field} must be a positive safe integer`);
  }
  return value;
}

function canonicalEnvelope(payload) {
  if (payload?.schema_version !== 1) {
    throw new Error("Control Plane event schema_version must be 1");
  }
  if (requiredString(payload, "_source") !== "auth-core") {
    throw new Error("Control Plane event source must be auth-core");
  }
  const action = requiredString(payload, "action");
  if (action !== "upsert" && action !== "remove") {
    throw new Error("Control Plane event action is invalid");
  }
  return {
    action,
    eventId: requiredString(payload, "event_id"),
    revision: positiveSafeInteger(payload, "revision"),
  };
}

function normalizedEmail(payload) {
  const email = requiredString(payload, "user_email").toLowerCase();
  if (email.length > 320 || !/^[^\s@]+@[^\s@]+$/.test(email)) {
    throw new Error("Control Plane event field user_email is invalid");
  }
  return email;
}

function normalizedRole(payload) {
  const role = requiredString(payload, "role").toLowerCase();
  if (!["owner", "admin", "member", "viewer"].includes(role)) {
    throw new Error("Control Plane event role is invalid");
  }
  return role;
}

function normalizeControlPlaneEvent(eventType, payload) {
  const common = canonicalEnvelope(payload);
  switch (eventType) {
    case "organizationChanged": {
      const orgId = requiredString(payload, "org_id");
      const expectedEventId = `organization:${orgId}:${common.revision}:${common.action === "remove" ? "deleted" : "upsert"}`;
      if (common.eventId !== expectedEventId) {
        throw new Error("Control Plane organization event_id does not match its identity");
      }
      if (common.action === "remove") {
        return { ...common, orgId };
      }
      return {
        ...common,
        orgId,
        name: requiredString(payload, "name"),
        slug: requiredString(payload, "slug"),
      };
    }
    case "memberChanged": {
      const orgId = requiredString(payload, "org_id");
      const userId = requiredString(payload, "user_id");
      const organizationRevision = positiveSafeInteger(
        payload,
        "organization_revision",
      );
      const expectedEventId = `organization:${orgId}:member:${userId}:${common.revision}:${common.action}`;
      if (common.eventId !== expectedEventId) {
        throw new Error("Control Plane membership event_id does not match its identity");
      }
      if (common.action === "remove") {
        return {
          ...common,
          orgId,
          userId,
          organizationRevision,
        };
      }
      return {
        ...common,
        orgId,
        userId,
        email: normalizedEmail(payload),
        role: normalizedRole(payload),
        organizationRevision,
      };
    }
    default:
      throw new Error(`Unsupported Control Plane event type: ${eventType}`);
  }
}

// W4-2 (ui-ux-velion-gap.md §13): Model Plane's orchestrator-core publishes
// `mp.v1.run.{id}.event` to `model-plane-nats` (port 4222 inside its compose
// network). Control-shared-nats and model-plane-nats are isolated clusters with
// `routes = []`, so the subscriber needs a SECOND connection here for those
// subjects. When the env var isn't set we silently skip the second
// connection (single-NATS deployments stay unchanged).
const MODEL_PLANE_NATS_URL = process.env.MODEL_PLANE_NATS_URL || "";
const MODEL_PLANE_NATS_USER = process.env.MODEL_PLANE_NATS_USER || "";
const MODEL_PLANE_NATS_PASSWORD = process.env.MODEL_PLANE_NATS_PASSWORD || "";

function modelPlaneConnectionOptions({ url, user, password }) {
  const normalizedUser = String(user ?? "").trim();
  const normalizedPassword = String(password ?? "").trim();
  if (!normalizedUser || normalizedPassword.length < 32) {
    throw new Error(
      "Model Plane NATS requires a scoped user/password credential",
    );
  }
  return {
    servers: [url],
    user: normalizedUser,
    pass: normalizedPassword,
    name: "convex-subscriber-mp",
    maxReconnectAttempts: 10,
    reconnectDelayMs: 2000,
  };
}

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
      this.nc = await nats.connect(controlPlaneConnectionOptions({
        url: NATS_URL,
        user: NATS_USER,
        password: NATS_PASSWORD,
      }));

      // Get JetStream context
      this.js = this.nc.jetstream();

      console.log("[Convex NATS] Connected successfully");

      // W4-2: optional second connection for `mp.v1.*` cross-plane events.
      if (MODEL_PLANE_NATS_URL) {
        console.log(
          "[Convex NATS] Connecting to Model Plane NATS at",
          MODEL_PLANE_NATS_URL,
        );
        try {
          this.ncModelPlane = await nats.connect(
            modelPlaneConnectionOptions({
              url: MODEL_PLANE_NATS_URL,
              user: MODEL_PLANE_NATS_USER,
              password: MODEL_PLANE_NATS_PASSWORD,
            }),
          );
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
      // Auth-canonical organization events on the scoped Control shared bus.
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.organizationChanged,
        "convex-org-changed-v2",
        this.handleOrganizationChanged.bind(this)
      );
      await this.subscribeToDurableTopic(
        CONTROL_PLANE_SUBJECTS.memberChanged,
        "convex-org-member-changed-v2",
        this.handleMemberChanged.bind(this)
      );

      // The former token-only shared broker multiplexed Ingestion and
      // Application subjects into this process. Secure MVP leaves those
      // non-authority mirrors disabled until their owning planes expose scoped
      // principals; Control projection must not regain an admin-capable token.
      console.warn(
        "[Convex NATS] scoped Ingestion mirror is disabled pending an Ingestion-owned principal",
      );
      console.warn(
        "[Convex NATS] scoped Application conversation mirror is disabled pending a dedicated projection principal",
      );

      // U3-3 (ui-ux-velion-gap.md §10): Model Plane agent run lifecycle.
      // orchestrator-core publishes RUN_STARTED / RUN_COMPLETED / RUN_FAILED
      // envelopes on `mp.v1.run.{runId}.event`. We subscribe to the
      // wildcard form so any run lands in Convex without per-run setup.
      //
      // W4-2: in production these events arrive on `model-plane-nats`,
      // NOT `velion-nats` (the two clusters are isolated). Use the
      // optional second connection when a scoped Model principal is present.
      if (this.ncModelPlane) {
        await this.subscribeToTopicOn(
          this.ncModelPlane,
          "mp.v1.run.*.event",
          this.handleAgentRunEvent.bind(this),
          "model-plane-nats",
        );
      } else {
        console.warn(
          "[Convex NATS] scoped Model mirror is disabled; no fallback to the Control principal",
        );
      }

      console.log("[Convex NATS] All subscriptions established");
    } catch (error) {
      console.error("[Convex NATS] Subscription setup error:", error);
      throw error;
    }
  }

  /**
   * Subscribe to a NATS topic on the default Control shared connection.
   */
  async subscribeToTopic(topic, handler) {
    return this.subscribeToTopicOn(this.nc, topic, handler, "velion-nats");
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
    options.bind(CONTROL_PLANE_STREAM, durableName);
    options.queue(durableName);
    options.manualAck();

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
  async handleOrganizationChanged(payload) {
    const event = normalizeControlPlaneEvent("organizationChanged", payload);
    console.log("[Convex NATS] Processing org.changed:", event.orgId);
    await this.callConvexMutation("nats:onOrganizationProjectionChanged", event);
  }

  /**
   * Handle velion.controlplane.org.member.added event
   */
  async handleMemberChanged(payload) {
    const event = normalizeControlPlaneEvent("memberChanged", payload);
    console.log("[Convex NATS] Processing org.member_changed:", event.userId);
    await this.callConvexMutation("nats:onOrganizationMembershipProjectionChanged", event);
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
        Authorization: `Bearer ${CONVEX_CONTROL_PROJECTION_KEY}`,
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
    if (this.ncModelPlane) {
      await this.ncModelPlane.close();
      this.ncModelPlane = null;
    }
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
  controlPlaneConnectionOptions,
  modelPlaneConnectionOptions,
  normalizeControlPlaneEvent,
  processJetStreamMessage,
};
