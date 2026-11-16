/** Data Plane deletion adapter: binding revocation, then the room canvas purge (WS19-S8), then the Application's private-panel
 * pointer purge (WS17-S6). Each delivery mints one organization-bound `aud=data-plane` token from the Control deletion
 * principal (auth-core `POST /api/data-plane/internal-token`, W3-FIX-10) and presents it to both Data legs. A mint or leg
 * failure releases the delivery for retry, and no response can update a different owner/request receipt. */
import { v } from "convex/values";
import { internalAction, internalMutation, internalQuery } from "./_generated/server";
import { internal } from "./_generated/api";

const DATA_SPACE_DELETION_URL = process.env.DATA_SPACE_DELETION_URL || "";
// The room canvas purge is the second Data leg (WS19-S8). Its receipt is required before the owner receipt is acknowledged.
const DATA_SPACE_CANVAS_DELETION_URL = process.env.DATA_SPACE_CANVAS_DELETION_URL || "";
// The Data legs take their organization from the bearer, so each delivery mints its own token for the request's organization
// (W3-FIX-10; the static bearer covered one org). The principal id is the subject the Data plane pins; the credential is the
// registry row's consumerEnv. The minted token is never logged or stored.
const AUTH_CORE_URL = process.env.AUTH_CORE_URL || "";
const CONTROL_SPACE_DELETION_SERVICE_API_KEY = process.env.CONTROL_SPACE_DELETION_SERVICE_API_KEY || "";
const CONTROL_SPACE_DELETION_SERVICE_ID = "control-space-deletion";
const DATA_DELETION_TOKEN_SCOPES = ["data:space-binding:delete", "data:space-canvas:delete"];
const DATA_DELETION_TOKEN_SKEW_MS = 10_000;
type DataOutcome = "succeeded" | "partial" | "failed" | "unknown";

export function dataDeletionReceipt(receipt: unknown, requestId: string): { status: DataOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  const body = receipt as { request_id?: unknown; owner_plane?: unknown; owner_outcome?: unknown; remaining_work?: unknown };
  if (body.request_id !== requestId || body.owner_plane !== "data") return null;
  if (body.owner_outcome !== "succeeded" && body.owner_outcome !== "partial" && body.owner_outcome !== "failed" && body.owner_outcome !== "unknown") return null;
  const remaining = typeof body.remaining_work === "string" ? body.remaining_work.slice(0, 180) : "";
  return { status: body.owner_outcome, receiptRef: `data:${requestId}`, detail: remaining ? `Data owner receipt: ${remaining}` : "Data owner receipt accepted" };
}

// The canvas purge answers the same top-level receipt shape. It names the organization its purge ran in (org_id, the verified
// bearer's org, the org the token was minted for) and the Space it purged (space_ref). A missing or different field returns null,
// so the delivery is released for retry and never acknowledged.
export function dataCanvasPurgeReceipt(receipt: unknown, requestId: string, orgId: string, spaceRef: string): { status: DataOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  const named = receipt as { org_id?: unknown; space_ref?: unknown };
  if (named.org_id !== orgId || named.space_ref !== spaceRef) return null;
  const outcome = dataDeletionReceipt(receipt, requestId);
  if (!outcome) return null;
  return { status: outcome.status, receiptRef: outcome.receiptRef, detail: `Data canvas purge receipt: ${outcome.status}` };
}

// The retrieval leg names the organization its purge ran in: its org_id is the verified bearer's org, which is the org the token
// was minted for, so its receipt must name the delivery's organization. Its space_ref names the Space the binding revocation ran
// against, so it must name the delivery's Space too (WS19-S8 P3). dataDeletionReceipt itself is unchanged and checks neither field.
export function dataSpaceDeletionReceipt(receipt: unknown, requestId: string, orgId: string, spaceRef: string): { status: DataOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  const outcome = dataDeletionReceipt(receipt, requestId);
  const named = receipt as { org_id?: unknown; space_ref?: unknown };
  if (!outcome || named.org_id !== orgId || named.space_ref !== spaceRef) return null;
  return outcome;
}

// One owner receipt covers both Data legs, so it keeps the weaker outcome: failed, then unknown, then partial, then
// succeeded. The detail names both legs.
const DATA_OUTCOME_RANK: Record<DataOutcome, number> = { succeeded: 0, partial: 1, unknown: 2, failed: 3 };
export function combineDataOutcomes(
  first: { status: DataOutcome; receiptRef: string; detail: string },
  second: { status: DataOutcome; receiptRef: string; detail: string },
): { status: DataOutcome; receiptRef: string; detail: string } {
  const status = DATA_OUTCOME_RANK[second.status] > DATA_OUTCOME_RANK[first.status] ? second.status : first.status;
  return { status, receiptRef: first.receiptRef, detail: `${first.detail}; ${second.detail}`.slice(0, 256) };
}

function configurationError(): string | null {
  if (!DATA_SPACE_DELETION_URL.trim()) return "DATA_SPACE_DELETION_URL is not configured";
  if (!DATA_SPACE_CANVAS_DELETION_URL.trim()) return "DATA_SPACE_CANVAS_DELETION_URL is not configured";
  if (!AUTH_CORE_URL.trim()) return "AUTH_CORE_URL is not configured";
  if (!CONTROL_SPACE_DELETION_SERVICE_API_KEY.trim()) return "CONTROL_SPACE_DELETION_SERVICE_API_KEY is not configured";
  return null;
}
function nextWorker(eventId: string): string { return `space-deletion-data:${eventId}:${crypto.randomUUID()}`; }

// Typed mint outcome: the caller releases the delivery on any failure and never acknowledges on one.
type DataDeletionTokenMint =
  | { ok: true; token: string }
  | { ok: false; code: "auth_token_rejected" | "auth_token_unavailable" | "auth_token_invalid"; detail: string };

// POST /api/data-plane/internal-token for the Control deletion principal (auth-core plane-token.controller.ts). The row is
// `tenancy: directory`, so auth-core checks that org-core holds `orgId` before it signs (403 when it does not, 503 when the
// directory cannot be read), and it signs only the scopes the row names. The headers and body match the Application Plane's
// Go mint (conversation-core integration client). Nothing here throws, and a token is returned only on success.
async function mintDataDeletionToken(orgId: string): Promise<DataDeletionTokenMint> {
  let response: Response;
  try {
    response = await fetch(`${AUTH_CORE_URL.trim().replace(/\/+$/, "")}/api/data-plane/internal-token`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-service-id": CONTROL_SPACE_DELETION_SERVICE_ID,
        "x-service-api-key": CONTROL_SPACE_DELETION_SERVICE_API_KEY,
      },
      body: JSON.stringify({
        orgId,
        scopes: DATA_DELETION_TOKEN_SCOPES,
        reason: "Control-authorized Space deletion binding revocation and canvas purge",
      }),
    });
  } catch {
    return { ok: false, code: "auth_token_unavailable", detail: "token authority request failed" };
  }
  if (response.status === 401 || response.status === 403) {
    return { ok: false, code: "auth_token_rejected", detail: `token authority refused with HTTP ${response.status}` };
  }
  if (response.status !== 200) {
    return { ok: false, code: "auth_token_unavailable", detail: `token authority returned HTTP ${response.status}` };
  }
  let payload: unknown;
  try { payload = await response.json(); } catch {
    return { ok: false, code: "auth_token_invalid", detail: "token authority returned an invalid body" };
  }
  const minted = (payload && typeof payload === "object" ? payload : {}) as { token?: unknown; expiresAt?: unknown };
  const token = typeof minted.token === "string" ? minted.token.trim() : "";
  const expiresAt = typeof minted.expiresAt === "string" ? Date.parse(minted.expiresAt) : Number.NaN;
  if (!token || !Number.isFinite(expiresAt) || expiresAt <= Date.now() + DATA_DELETION_TOKEN_SKEW_MS) {
    return { ok: false, code: "auth_token_invalid", detail: "token authority returned an unusable token" };
  }
  return { ok: true, token };
}

// The Space record's own organization. Auth Core binds the minted token to the request's organization, so the Space must belong
// to that organization before any token is signed. The receipt check cannot show this, because both sides echo the bearer's org.
// Null when the Space is missing or ambiguous, which releases the delivery.
export const spaceOrgForDeletion = internalQuery({
  args: { spaceRef: v.string() },
  handler: async (ctx, args) => {
    const spaces = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .take(2);
    return spaces.length === 1 ? spaces[0].externalOrgId : null;
  },
});

// WS17-S6: the owner's private-panel pointers for a Space being deleted (Application's own rows, ids only). One batch
// per call, so a large room stays inside one mutation; the adapter repeats until `more` is false. The panel's thread is
// Session Core's and is not touched here: only the pointer that names it goes.
const PANEL_POINTER_PURGE_BATCH = 200;
const PANEL_POINTER_PURGE_MAX_PASSES = 25;

export const purgeSpacePrivatePanelPointers = internalMutation({
  args: { spaceRef: v.string() },
  handler: async (ctx, args) => {
    const rows = await ctx.db
      .query("spacePrivatePanels")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .take(PANEL_POINTER_PURGE_BATCH);
    for (const row of rows) await ctx.db.delete(row._id);
    return { deleted: rows.length, more: rows.length === PANEL_POINTER_PURGE_BATCH };
  },
});

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const claimed = await ctx.runMutation(internal.spaces.claimNextDataDeletionOwnerDelivery, { now: Date.now(), workerId: args.workerId });
    if (!claimed) return { status: "idle" as const };
    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), error: reason });
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceDeletionData.deliverOne, { workerId: nextWorker(claimed.eventId) });
      return { status: "retry_scheduled" as const, retryAt };
    };
    const misconfigured = configurationError();
    if (misconfigured) return release(misconfigured);
    // W3-FIX-10: one org-bound token for both legs. The Space record must belong to the request's organization, checked before any
    // token is signed. A mint failure releases the delivery and never reaches an acknowledgement.
    const orgId = claimed.request.externalOrgId.trim();
    if (!orgId) return release("Data deletion request has no organization");
    const spaceOrg = await ctx.runQuery(internal.spaceDeletionData.spaceOrgForDeletion, { spaceRef: claimed.request.spaceRef });
    if (spaceOrg !== orgId) return release("Space organization does not match the deletion request");
    const minted = await mintDataDeletionToken(orgId);
    if (!minted.ok) return release(`${minted.code}: ${minted.detail}`);
    let response: Response;
    try {
      response = await fetch(DATA_SPACE_DELETION_URL, {
        method: "POST",
        headers: { "Authorization": `Bearer ${minted.token}`, "Content-Type": "application/json" },
        body: JSON.stringify({
          deletion_request_id: claimed.request.requestId,
          space_ref: claimed.request.spaceRef,
          reason: "Control-authorized Space deletion binding revocation",
          idempotency_key: `space-delete:${claimed.request.requestId}`,
        }),
      });
    } catch { return release("Data Space deletion request failed"); }
    if (response.status !== 200) return release(`Data Space deletion returned HTTP ${response.status}`);
    let receipt: unknown;
    try { receipt = await response.json(); } catch { return release("Data Space deletion returned an invalid receipt"); }
    const outcome = dataSpaceDeletionReceipt(receipt, claimed.request.requestId, orgId, claimed.request.spaceRef);
    if (!outcome) return release("Data Space deletion receipt did not match the claimed request, organization or Space");
    // Second leg (WS19-S8): purge the room canvas. Both receipts are needed before the owner receipt is acknowledged. A retry
    // repeats both legs, which is safe because the binding revocation and the canvas purge are idempotent.
    let canvasResponse: Response;
    try {
      canvasResponse = await fetch(DATA_SPACE_CANVAS_DELETION_URL, {
        method: "POST",
        headers: { "Authorization": `Bearer ${minted.token}`, "Content-Type": "application/json" },
        body: JSON.stringify({
          deletion_request_id: claimed.request.requestId,
          space_ref: claimed.request.spaceRef,
          idempotency_key: `space-delete:${claimed.request.requestId}`,
        }),
      });
    } catch { return release("Data canvas purge request failed"); }
    if (canvasResponse.status !== 200) return release(`Data canvas purge returned HTTP ${canvasResponse.status}`);
    let canvasReceipt: unknown;
    try { canvasReceipt = await canvasResponse.json(); } catch { return release("Data canvas purge returned an invalid receipt"); }
    const canvasOutcome = dataCanvasPurgeReceipt(canvasReceipt, claimed.request.requestId, orgId, claimed.request.spaceRef);
    if (!canvasOutcome) return release("Data canvas purge receipt did not match the claimed request, organization or Space");
    // Third leg (WS17-S6): the private-panel pointers for this Space, so no pointer names a deleted room. Idempotent: a
    // failure releases the claim like the Data legs do, and the next attempt purges again. The Data receipts are unchanged.
    let purged = { deleted: 0, more: true };
    try {
      for (let pass = 0; pass < PANEL_POINTER_PURGE_MAX_PASSES && purged.more; pass += 1) {
        purged = await ctx.runMutation(internal.spaceDeletionData.purgeSpacePrivatePanelPointers, { spaceRef: claimed.request.spaceRef });
      }
    } catch { return release("Private panel pointer purge failed"); }
    if (purged.more) return release("Private panel pointer purge is incomplete");
    const combined = combineDataOutcomes(outcome, canvasOutcome);
    await ctx.runMutation(internal.spaces.acknowledgeDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...combined });
    return { eventId: claimed.eventId, status: combined.status };
  },
});
