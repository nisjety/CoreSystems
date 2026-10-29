/** HTTP actions for health and authenticated internal projection ingestion. */

import { httpAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { httpRouter } from "convex/server";
import { createSessionHandler, postMessageHandler } from "./ingest";
import { upsertControlSessionHandler } from "./controlSessions";
import { authorizeInternalRequest } from "./internalAuth";
import { authorizeReconciliationRequest } from "./reconciliationAuth";

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

const http = httpRouter();

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
// aggregated user/org/billing snapshot here so verevon can subscribe
// reactively via `useQuery(api.controlSessions.byUser, ...)`.
http.route({
  path: "/ingest/control-session",
  method: "POST",
  handler: httpAction(upsertControlSessionHandler),
});

// NATS cross-plane event dispatcher
// Handles POST /api/webhook/nats/{handlerName} from nats-subscriber.js
export const natsWebhook = httpAction(async (ctx, request) => {
  try {
    if (!(await authorizeInternalRequest(request))) {
      return new Response("Unauthorized", { status: 401 });
    }
  } catch (error) {
    console.error("[Convex HTTP] Internal webhook authentication unavailable");
    return new Response("Service unavailable", { status: 503 });
  }

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
      case "onOrganizationProjectionChanged":
        await ctx.runMutation(
          internal.nats.onOrganizationProjectionChanged,
          body as any,
        );
        break;
      case "onOrganizationMembershipProjectionChanged":
        await ctx.runMutation(
          internal.nats.onOrganizationMembershipProjectionChanged,
          body as any,
        );
        break;
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
      JSON.stringify({ error: "Projection handler failed" }),
      { status: 500, headers: { "Content-Type": "application/json" } }
    );
  }

  return new Response(JSON.stringify({ ok: true, handler }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
});

export const reconcileMemberships = httpAction(async (ctx, request) => {
  const rawBody = await request.text();
  if (rawBody.length > 1 << 20) {
    return new Response("Request too large", { status: 413 });
  }

  let authorization;
  try {
    authorization = await authorizeReconciliationRequest(request, rawBody);
  } catch {
    console.error("[Convex HTTP] Reconciliation authentication unavailable");
    return new Response("Service unavailable", { status: 503 });
  }
  if (!authorization.authorized) {
    return new Response("Unauthorized", { status: 401 });
  }

  let body: {
    externalOrgId?: unknown;
    authoritativeMembers?: unknown;
    observedAt?: unknown;
    apply?: unknown;
    confirmOrgId?: unknown;
  };
  try {
    body = JSON.parse(rawBody);
  } catch {
    return new Response("Invalid JSON", { status: 400 });
  }
  if (body.externalOrgId !== authorization.orgId) {
    return new Response("Organization scope mismatch", { status: 403 });
  }
  if (body.apply === true && body.confirmOrgId !== authorization.orgId) {
    return new Response("Apply requires exact organization confirmation", {
      status: 400,
    });
  }

  try {
    await ctx.runMutation(internal.reconciliationNonces.claim, {
      nonce: authorization.nonce,
      externalOrgId: authorization.orgId,
      requestTimestamp: authorization.timestamp,
    });
    const result = await ctx.runMutation(
      internal.membershipReconciliation.reconcile,
      {
        externalOrgId: authorization.orgId,
        authoritativeMembers: body.authoritativeMembers,
        observedAt: body.observedAt,
        apply: body.apply,
      } as any,
    );
    return new Response(JSON.stringify({ ok: true, result }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  } catch (error) {
    console.error("[Convex HTTP] Reconciliation failed:", error);
    return new Response(
      JSON.stringify({ error: "Reconciliation failed" }),
      { status: 500, headers: { "Content-Type": "application/json" } },
    );
  }
});

http.route({
  pathPrefix: "/api/webhook/nats/",
  method: "POST",
  handler: natsWebhook,
});

http.route({
  path: "/api/operator/reconcile-memberships",
  method: "POST",
  handler: reconcileMemberships,
});

export default http;
