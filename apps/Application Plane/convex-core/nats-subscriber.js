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
const NATS_TOKEN = process.env.NATS_TOKEN || "nats";
const CONVEX_URL = process.env.CONVEX_BACKEND_URL || "http://localhost:3000";
const CONVEX_API_KEY = process.env.CONVEX_API_KEY || "dev-key";

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
      await this.subscribeToTopic(
        "velion.controlplane.org.created",
        this.handleOrganizationCreated.bind(this)
      );
      await this.subscribeToTopic(
        "velion.controlplane.org.updated",
        this.handleOrganizationUpdated.bind(this)
      );
      await this.subscribeToTopic(
        "velion.controlplane.org.deleted",
        this.handleOrganizationDeleted.bind(this)
      );
      await this.subscribeToTopic(
        "velion.controlplane.org.member.added",
        this.handleMemberAdded.bind(this)
      );
      await this.subscribeToTopic(
        "velion.controlplane.org.member.removed",
        this.handleMemberRemoved.bind(this)
      );

      // Ingestion Plane events
      await this.subscribeToTopic(
        "velion.ingestion.import.completed",
        this.handleImportCompleted.bind(this)
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
    console.log("[Convex NATS] Processing org.created:", payload.id);

    try {
      await this.callConvexMutation("nats:onOrganizationCreated", {
        orgId: payload.id,
        name: payload.name,
        slug: payload.slug,
        createdAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error syncing organization:", error);
    }
  }

  /**
   * Handle velion.controlplane.org.updated event
   */
  async handleOrganizationUpdated(payload) {
    console.log("[Convex NATS] Processing org.updated:", payload.id);

    try {
      await this.callConvexMutation("nats:onOrganizationUpdated", {
        orgId: payload.id,
        name: payload.name,
        slug: payload.slug,
        settings: payload.settings,
        updatedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error updating organization:", error);
    }
  }

  /**
   * Handle velion.controlplane.org.deleted event
   */
  async handleOrganizationDeleted(payload) {
    console.log("[Convex NATS] Processing org.deleted:", payload.id);

    try {
      await this.callConvexMutation("nats:onOrganizationDeleted", {
        orgId: payload.id,
      });
    } catch (error) {
      console.error("[Convex NATS] Error deleting organization:", error);
    }
  }

  /**
   * Handle velion.controlplane.org.member.added event
   */
  async handleMemberAdded(payload) {
    console.log(
      "[Convex NATS] Processing org.member.added:",
      payload.email
    );

    try {
      await this.callConvexMutation("nats:onOrganizationMemberAdded", {
        orgId: payload.orgId,
        userId: payload.userId,
        email: payload.email,
        role: payload.role,
        addedAt: Date.now(),
      });
    } catch (error) {
      console.error("[Convex NATS] Error adding member:", error);
    }
  }

  /**
   * Handle velion.controlplane.org.member.removed event
   */
  async handleMemberRemoved(payload) {
    console.log(
      "[Convex NATS] Processing org.member.removed:",
      payload.email
    );

    try {
      await this.callConvexMutation("nats:onOrganizationMemberRemoved", {
        orgId: payload.orgId,
        userId: payload.userId,
      });
    } catch (error) {
      console.error("[Convex NATS] Error removing member:", error);
    }
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

module.exports = { ConvexNatsSubscriber };
