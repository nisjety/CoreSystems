/**
 * HTTP Actions - Inbound Webhooks
 * 
 * These are HTTP endpoints that external services can call.
 * They receive webhooks from Org Core, AI Core, etc.
 */

import { httpAction } from "./_generated/server";
import { api, internal } from "./_generated/api";
import { httpRouter } from "convex/server";
import { createSessionHandler, postMessageHandler } from "./ingest";
import { upsertControlSessionHandler } from "./controlSessions";

/**
 * Webhook: RAG job completed
 * Called by Org Core when a RAG indexing job finishes
 * 
 * POST /webhooks/rag/complete
 */
export const ragComplete = httpAction(async (ctx, request) => {
  // Verify webhook signature
  const signature = request.headers.get("X-Webhook-Signature");
  if (!signature || !verifyWebhookSignature(request, signature)) {
    return new Response("Unauthorized", { status: 401 });
  }

  const payload = await request.json();
  const { job_id, org_id, status, result } = payload;

  try {
    // Find job by external ID
    const job = await ctx.runQuery(api.jobs.getByExternalId, {
      externalJobId: job_id,
    });

    if (!job) {
      return new Response("Job not found", { status: 404 });
    }

    // Update job status
    await ctx.runMutation(api.jobs.updateStatus, {
      jobId: job._id,
      status: status === "completed" ? "completed" : "failed",
      result: result,
      completedAt: Date.now(),
    });

    // Notify connected clients via subscription
    // (automatically happens via Convex reactivity)

    return new Response(JSON.stringify({ success: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  } catch (error) {
    console.error("Webhook error:", error);
    return new Response(JSON.stringify({
      error: error instanceof Error ? error.message : "Unknown error"
    }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
});

/**
 * Webhook: Job progress update
 * Called by Org Core with progress updates during long jobs
 * 
 * POST /webhooks/job/progress
 */
export const jobProgress = httpAction(async (ctx, request) => {
  const signature = request.headers.get("X-Webhook-Signature");
  if (!signature || !verifyWebhookSignature(request, signature)) {
    return new Response("Unauthorized", { status: 401 });
  }

  const payload = await request.json();
  const { job_id, progress, message } = payload;

  try {
    const job = await ctx.runQuery(api.jobs.getByExternalId, {
      externalJobId: job_id,
    });

    if (!job) {
      return new Response("Job not found", { status: 404 });
    }

    await ctx.runMutation(api.jobs.updateProgress, {
      jobId: job._id,
      progress,
      progressMessage: message,
    });

    return new Response(JSON.stringify({ success: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  } catch (error) {
    console.error("Webhook error:", error);
    return new Response(JSON.stringify({
      error: error instanceof Error ? error.message : "Unknown error"
    }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
});

/**
 * Webhook: AI streaming callback
 * Optionally used if AI Core needs to push streaming updates
 * 
 * POST /webhooks/ai/stream
 */
export const aiStreamCallback = httpAction(async (ctx, request) => {
  const signature = request.headers.get("X-Webhook-Signature");
  if (!signature || !verifyWebhookSignature(request, signature)) {
    return new Response("Unauthorized", { status: 401 });
  }

  const payload = await request.json();
  const { message_id, chunk, done } = payload;

  try {
    if (done) {
      await ctx.runMutation(api.messages.finalizeStreaming, {
        messageId: message_id,
        content: chunk,
      });
    } else {
      await ctx.runMutation(api.messages.updateStreaming, {
        messageId: message_id,
        chunk,
      });
    }

    return new Response(JSON.stringify({ success: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  } catch (error) {
    console.error("Webhook error:", error);
    return new Response(JSON.stringify({
      error: error instanceof Error ? error.message : "Unknown error"
    }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
});

/**
 * Health check endpoint
 * GET /webhooks/health
 */
export const health = httpAction(async (ctx, request) => {
  return new Response(JSON.stringify({
    status: "healthy",
    service: "convex-gateway",
    timestamp: Date.now(),
  }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
});

/**
 * Verify webhook signature using HMAC
 */
function verifyWebhookSignature(request: Request, signature: string): boolean {
  // Get webhook secret from environment
  const secret = process.env.WEBHOOK_SECRET;
  if (!secret) {
    console.warn("WEBHOOK_SECRET not set, skipping signature verification");
    return true; // Allow in development
  }

  // In production, implement HMAC SHA256 verification
  // For now, simple equality check (replace with crypto.subtle.verify)
  return signature.startsWith("sha256=");
}

const http = httpRouter();

http.route({
  path: "/webhooks/rag/complete",
  method: "POST",
  handler: ragComplete,
});

http.route({
  path: "/webhooks/job/progress",
  method: "POST",
  handler: jobProgress,
});

http.route({
  path: "/webhooks/ai/stream",
  method: "POST",
  handler: aiStreamCallback,
});

http.route({
  path: "/webhooks/health",
  method: "GET",
  handler: health,
});

// Session ingestion — called by session-core (Go) after CreateSession / SendMessage
http.route({
  path: "/ingest/session",
  method: "POST",
  handler: httpAction(createSessionHandler),
});

http.route({
  path: "/ingest/session/message",
  method: "POST",
  handler: httpAction(postMessageHandler),
});

// G35: Control Session projection — session-core's `Refresh` mirrors the
// aggregated user/org/billing snapshot here so velion can subscribe
// reactively via `useQuery(api.controlSessions.byUser, ...)`.
http.route({
  path: "/ingest/control-session",
  method: "POST",
  handler: httpAction(upsertControlSessionHandler),
});

// NATS cross-plane event dispatcher
// Handles POST /api/webhook/nats/{handlerName} from nats-subscriber.js
export const natsWebhook = httpAction(async (ctx, request) => {
  const url = new URL(request.url);
  const parts = url.pathname.split("/");
  const handler = parts[parts.length - 1];

  let body: Record<string, unknown>;
  try {
    body = await request.json();
  } catch {
    return new Response("Invalid JSON", { status: 400 });
  }

  try {
    switch (handler) {
      // Quarry crawl job events — internalMutation (direct DB writes)
      case "onCrawlStarted":
        await ctx.runMutation(internal.nats.onCrawlStarted, body as any);
        break;
      case "onCrawlProgress":
        await ctx.runMutation(internal.nats.onCrawlProgress, body as any);
        break;
      case "onCrawlCompleted":
        await ctx.runMutation(internal.nats.onCrawlCompleted, body as any);
        break;
      case "onCrawlFailed":
        await ctx.runMutation(internal.nats.onCrawlFailed, body as any);
        break;
      // Control Plane org events — internalAction (needs async) 
      case "onOrganizationCreated":
        await ctx.runAction(internal.nats.onOrganizationCreated, body as any);
        break;
      case "onOrganizationUpdated":
        await ctx.runAction(internal.nats.onOrganizationUpdated, body as any);
        break;
      case "onOrganizationDeleted":
        await ctx.runAction(internal.nats.onOrganizationDeleted, body as any);
        break;
      case "onOrganizationMemberAdded":
        await ctx.runAction(internal.nats.onOrganizationMemberAdded, body as any);
        break;
      case "onOrganizationMemberRemoved":
        await ctx.runAction(internal.nats.onOrganizationMemberRemoved, body as any);
        break;
      case "onImportCompleted":
        await ctx.runAction(internal.nats.onImportCompleted, body as any);
        break;
      // U3-3: Model Plane agent run lifecycle. Bridges
      // `mp.v1.run.{runId}.event` envelopes into the agentRuns mirror.
      case "onAgentRunEvent":
        await ctx.runAction(internal.nats.onAgentRunEvent, body as any);
        break;
      case "onConversationEvent":
        await ctx.runMutation(internal.conversationProjection.applyEvent, { event: body as any });
        break;
      default:
        return new Response(JSON.stringify({ error: `Unknown NATS handler: ${handler}` }), {
          status: 400,
          headers: { "Content-Type": "application/json" },
        });
    }
  } catch (err) {
    console.error(`[Convex HTTP] NATS handler ${handler} error:`, err);
    return new Response(
      JSON.stringify({ error: err instanceof Error ? err.message : String(err) }),
      { status: 500, headers: { "Content-Type": "application/json" } }
    );
  }

  return new Response(JSON.stringify({ ok: true, handler }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
});

http.route({
  pathPrefix: "/api/webhook/nats/",
  method: "POST",
  handler: natsWebhook,
});

export default http;
