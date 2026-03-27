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

class ConvexNatsSubscriber {
  constructor() {
    this.nc = null;
    this.js = null;
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
      // Organization events
      await this.subscribeToTopic(
        "organization.created",
        this.handleOrganizationCreated.bind(this)
      );
      await this.subscribeToTopic(
        "organization.updated",
        this.handleOrganizationUpdated.bind(this)
      );
      await this.subscribeToTopic(
        "organization.deleted",
        this.handleOrganizationDeleted.bind(this)
      );
      await this.subscribeToTopic(
        "organization.member.added",
        this.handleMemberAdded.bind(this)
      );
      await this.subscribeToTopic(
        "organization.member.removed",
        this.handleMemberRemoved.bind(this)
      );

      console.log("[Convex NATS] All subscriptions established");
    } catch (error) {
      console.error("[Convex NATS] Subscription setup error:", error);
      throw error;
    }
  }

  /**
   * Subscribe to a NATS topic
   */
  async subscribeToTopic(topic, handler) {
    const safeName = topic.replace(/\./g, "_");
    const sub = await this.js.subscribe(topic, {
      config: {
        durable_name: `convex_${safeName}`,
        deliver_subject: `deliver_${safeName}`,
      }
    });

    console.log(`[Convex NATS] Subscribed to: ${topic}`);

    // Process messages
    (async () => {
      for await (const m of sub) {
        try {
          const data = JSON.parse(new TextDecoder().decode(m.data));
          await handler(data);
          m.ack();
        } catch (error) {
          console.error(`[Convex NATS] Error handling ${topic}:`, error);
          m.nak();
        }
      }
    })();
  }

  /**
   * Handle organization.created event
   */
  async handleOrganizationCreated(payload) {
    console.log("[Convex NATS] Processing organization.created:", payload.id);

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
   * Handle organization.updated event
   */
  async handleOrganizationUpdated(payload) {
    console.log("[Convex NATS] Processing organization.updated:", payload.id);

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
   * Handle organization.deleted event
   */
  async handleOrganizationDeleted(payload) {
    console.log("[Convex NATS] Processing organization.deleted:", payload.id);

    try {
      await this.callConvexMutation("nats:onOrganizationDeleted", {
        orgId: payload.id,
      });
    } catch (error) {
      console.error("[Convex NATS] Error deleting organization:", error);
    }
  }

  /**
   * Handle organization.member.added event
   */
  async handleMemberAdded(payload) {
    console.log(
      "[Convex NATS] Processing organization.member.added:",
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
   * Handle organization.member.removed event
   */
  async handleMemberRemoved(payload) {
    console.log(
      "[Convex NATS] Processing organization.member.removed:",
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
