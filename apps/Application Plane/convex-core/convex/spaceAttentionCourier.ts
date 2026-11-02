/**
 * The out-of-app courier request for Space attention (WS11-S9).
 *
 * Convex decides who is told (rules, budget, flag) and notification-core only
 * carries the message. This file builds the one request and signs it with the
 * HMAC delegation v2 scheme notification-core's verifier requires, as service
 * id `space-attention` (one key per service id). It is pure so the bytes can be
 * pinned by a test against the Go verifier.
 *
 * The request holds ids and closed words only: no title, text or name, so the
 * hide-preview rule is satisfied out of app by construction. It is always
 * `retention_mode: "standard"`; the projection admits persistent events only,
 * and notification-core refuses any other mode for this principal.
 */

export const COURIER_SERVICE_ID = "space-attention";
/** notification-core's verifier audience: a literal, never derived from the URL. */
export const COURIER_AUDIENCE = "notification-core";
/** The only endpoint called, and so the exact URI that is signed. */
export const COURIER_PATH = "/api/v1/notification-requests";

/** Signal kind -> notification-core type. The closed set notification-core allows for this principal. */
export const COURIER_TYPES: Readonly<Record<string, string>> = {
  mention: "space.mention",
  reply: "space.reply",
  invite: "space.invite",
  agent_done: "space.agent_done",
  agent_failed: "space.agent_failed",
  approval_needed: "space.approval_needed",
  agent_awaiting: "space.agent_awaiting",
  reminder: "space.reminder",
};

export type CourierSignal = {
  eventId: string;
  externalOrgId: string;
  recipientAuthId: string;
  spaceRef: string;
  kind: string;
  threadId?: string;
  messageId?: string;
  runId?: string;
  actorKind: string;
};

export type CourierRequest = {
  organization_id: string;
  idempotency_key: string;
  recipient: { kind: "user"; id: string };
  type: string;
  retention_mode: "standard";
  payload: Record<string, string>;
};

/** The room link, same shape as the SPA's `spaceThreadHref`. Ids are encoded, never interpolated raw. */
export function courierDeepLink(spaceRef: string, threadId?: string): string {
  const room = `/spaces/${encodeURIComponent(spaceRef)}`;
  return threadId ? `${room}?thread_id=${encodeURIComponent(threadId)}#chat` : `${room}#chat`;
}

/** Build the request body, or null when the kind is not one notification-core accepts. */
export function buildCourierRequest(signal: CourierSignal): CourierRequest | null {
  const type = COURIER_TYPES[signal.kind];
  if (!type) return null;
  const payload: Record<string, string> = {
    space_ref: signal.spaceRef,
    actor_kind: signal.actorKind,
    deep_link_path: courierDeepLink(signal.spaceRef, signal.threadId),
  };
  if (signal.threadId) payload.thread_id = signal.threadId;
  if (signal.messageId) payload.message_id = signal.messageId;
  if (signal.runId) payload.run_id = signal.runId;
  return {
    organization_id: signal.externalOrgId,
    idempotency_key: `${signal.eventId}:${signal.recipientAuthId}`,
    recipient: { kind: "user", id: signal.recipientAuthId },
    type,
    retention_mode: "standard",
    payload,
  };
}

function base64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** The canonical string notification-core's `delegation.Canonical` builds: "v2" plus ten fields joined by "\n". */
export function courierCanonical(fields: {
  timestamp: string;
  nonce: string;
  method: string;
  uri: string;
  organizationId: string;
  bodySha256: string;
}): string {
  return [
    "v2",
    COURIER_SERVICE_ID,
    COURIER_AUDIENCE,
    fields.timestamp,
    fields.nonce,
    fields.method,
    fields.uri,
    "", // user id: a service principal, no bound user
    fields.organizationId,
    "", // role
    fields.bodySha256,
  ].join("\n");
}

/** Headers for one signed request. `body` must be the exact string that is sent. */
export async function signCourierRequest(
  secret: string,
  organizationId: string,
  body: string,
  now: Date = new Date(),
  nonceBytes: Uint8Array = crypto.getRandomValues(new Uint8Array(18)),
): Promise<Record<string, string>> {
  const encoder = new TextEncoder();
  const digest = base64Url(new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(body))));
  // RFC3339 at second precision, as the Go client sends it.
  const timestamp = now.toISOString().replace(/\.\d{3}Z$/, "Z");
  const nonce = base64Url(nonceBytes);
  const canonical = courierCanonical({
    timestamp,
    nonce,
    method: "POST",
    uri: COURIER_PATH,
    organizationId,
    bodySha256: digest,
  });
  const key = await crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = base64Url(new Uint8Array(await crypto.subtle.sign("HMAC", key, encoder.encode(canonical))));
  return {
    "Content-Type": "application/json",
    "x-service-id": COURIER_SERVICE_ID,
    "x-org-id": organizationId,
    "x-delegation-timestamp": timestamp,
    "x-delegation-nonce": nonce,
    "x-delegation-body-sha256": digest,
    "x-delegation-signature": signature,
  };
}
