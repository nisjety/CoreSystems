/**
 * Session Ingestion - Internal HTTP actions for session-core
 *
 * session-core (Go) calls these endpoints after creating or updating a session
 * so the frontend can subscribe to conversation state reactively via Convex,
 * eliminating the Postgres-poll SSE loop.
 *
 * Auth: X-Service-Key header, validated against CONVEX_INTERNAL_SERVICE_KEY.
 */

import { httpAction } from "./_generated/server";
import { api } from "./_generated/api";

function getServiceKey(): string {
  return (
    process.env.CONVEX_INTERNAL_SERVICE_KEY ||
    process.env.INTERNAL_API_KEY ||
    "change-me-internal-service-secret"
  );
}

function assertIngestKey(request: Request): void {
  const provided = request.headers.get("X-Service-Key");
  if (!provided || provided !== getServiceKey()) {
    throw new Error("Unauthorized");
  }
}

function jsonOk(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function jsonErr(message: string, status: number): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/**
 * POST /ingest/session
 *
 * Creates a Convex conversation linked to a session-core session.
 * Resolves org and user by external IDs so session-core doesn't
 * need to track Convex document IDs.
 *
 * Body:
 *   session_id:        string  — session-core session UUID
 *   external_org_id:   string  — tenant ID from auth-core/org-core
 *   external_user_id:  string  — user ID from auth-core
 *   title?:            string  — defaults to "Session <ISO timestamp>"
 *   plan_mode?:        boolean — tags the conversation when planning is active
 */
export async function createSessionHandler(ctx: any, request: Request) {
  try {
    assertIngestKey(request);
  } catch {
    return jsonErr("Unauthorized", 401);
  }

  let payload: {
    session_id: string;
    external_org_id: string;
    external_user_id: string;
    title?: string;
    plan_mode?: boolean;
  };
  try {
    payload = await request.json();
  } catch {
    return jsonErr("Invalid JSON", 400);
  }

  const { session_id, external_org_id, external_user_id } = payload;
  if (!session_id || !external_org_id || !external_user_id) {
    return jsonErr(
      "Missing required fields: session_id, external_org_id, external_user_id",
      400,
    );
  }

  const key = getServiceKey();

  try {
    const org = await ctx.runQuery(api.organizations.getByExternalId, {
      externalOrgId: external_org_id,
      serviceKey: key,
    });
    if (!org) {
      return jsonErr("Organization not found", 404);
    }

    const user = await ctx.runQuery(api.users.getByExternalAuthId, {
      externalAuthId: external_user_id,
      serviceKey: key,
    });
    if (!user) {
      return jsonErr("User not found", 404);
    }

    const title =
      payload.title ||
      `Session ${new Date().toISOString().slice(0, 16).replace("T", " ")}`;

    const conversationId = await ctx.runMutation(api.conversations.create, {
      orgId: org._id,
      userId: user._id,
      title,
      sessionId: session_id,
      serviceKey: key,
      metadata: payload.plan_mode ? { tags: ["plan_mode"] } : undefined,
    });

    return jsonOk({ conversation_id: conversationId });
  } catch (err) {
    console.error("[ingest/session] error:", err);
    return jsonErr(String(err), 500);
  }
}

/**
 * POST /ingest/session/message
 *
 * Appends a message to the Convex conversation linked to a session_id.
 * Triggers reactive updates to all subscribed frontend clients automatically.
 *
 * Body:
 *   session_id: string             — session-core session UUID
 *   role:       user|assistant|system
 *   content:    string
 */
export async function postMessageHandler(ctx: any, request: Request) {
  try {
    assertIngestKey(request);
  } catch {
    return jsonErr("Unauthorized", 401);
  }

  let payload: { session_id: string; role: string; content: string };
  try {
    payload = await request.json();
  } catch {
    return jsonErr("Invalid JSON", 400);
  }

  const { session_id, role, content } = payload;
  if (!session_id || !role || content === undefined) {
    return jsonErr("Missing required fields: session_id, role, content", 400);
  }
  if (!["user", "assistant", "system"].includes(role)) {
    return jsonErr("Invalid role; must be user, assistant, or system", 400);
  }

  const key = getServiceKey();

  try {
    const conversation = await ctx.runQuery(api.conversations.getBySessionId, {
      sessionId: session_id,
      serviceKey: key,
    });
    if (!conversation) {
      return jsonErr("Conversation not found for session_id", 404);
    }

    const messageId = await ctx.runMutation(api.messages.create, {
      conversationId: conversation._id,
      role: role as "user" | "assistant" | "system",
      content,
      serviceKey: key,
    });

    return jsonOk({ message_id: messageId });
  } catch (err) {
    console.error("[ingest/session/message] error:", err);
    return jsonErr(String(err), 500);
  }
}

export const createSession = httpAction(createSessionHandler);
export const postMessage = httpAction(postMessageHandler);
