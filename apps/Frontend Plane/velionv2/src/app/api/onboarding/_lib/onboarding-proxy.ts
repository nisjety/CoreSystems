import "server-only";

import { createHash, createHmac, randomBytes } from "node:crypto";
import type { NextRequest } from "next/server";

import {
  buildControlPlaneHeaders,
  getAuthServiceUrl,
  getCorrelationId,
  getInternalApiKey,
  getUserServiceUrl,
  type ControlPlaneSession,
} from "@/app/api/_lib/control-plane-auth";

const trimRightSlash = (value: string) => value.replace(/\/+$/, "");

export const getOrgCoreUrl = () =>
  trimRightSlash(process.env.ORG_CORE_URL || process.env.ORG_SERVICE_URL || "http://org-core:8080");

export const getGraphIndexUrl = () =>
  trimRightSlash(
    process.env.GRAPH_INDEX_URL ||
      process.env.DATAPLANE_GRAPH_INDEX_URL ||
      "http://dpv2-graph-index:9203",
  );

export const getQuarryEdgeUrl = () =>
  trimRightSlash(process.env.QUARRY_EDGE_URL || process.env.QUARRY_URL || "http://quarry-edge:8082");

export const getQuarryControlUrl = () =>
  trimRightSlash(
    process.env.QUARRY_API_URL ||
      process.env.QUARRY_CONTROL_URL ||
      process.env.QUARRY_CONTROL_API_URL ||
      "http://quarry-control:8081",
  );

export const getIntegrationCoreUrl = () =>
  trimRightSlash(
    process.env.INTEGRATION_CORE_URL || process.env.INTEGRATION_SERVICE_URL || "http://integration-api:3026",
  );

export const getFinspoCoreUrl = () =>
  trimRightSlash(process.env.FINSPO_CORE_URL || process.env.FINSPO_API_URL || "http://finspo-api:3130");

export const getImportsCoreUrl = () =>
  trimRightSlash(
    process.env.IMPORTS_CORE_URL ||
      process.env.IMPORTS_API_URL ||
      process.env.INGESTION_IMPORTS_URL ||
      "http://imports-api:3025",
  );

export const getDataPlaneDocumentsUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_DOCUMENTS_URL ||
      process.env.DATA_PLANE_DOCUMENTS_BASE_URL ||
      process.env.DATAPLANE_DOCUMENTS_URL ||
      "http://documents-api-go:9001",
  );

export const getKnowledgeRetrievalUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_RETRIEVAL_URL ||
      process.env.DATA_PLANE_RETRIEVAL_BASE_URL ||
      process.env.RETRIEVAL_ENGINE_URL ||
      "http://retrieval-engine:8004",
  );

export const getEmbeddingEngineUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_EMBEDDING_URL ||
      process.env.DATA_PLANE_EMBEDDING_BASE_URL ||
      process.env.EMBEDDING_ENGINE_URL ||
      "http://embedding-engine:9202",
  );

export const getWikiStoreUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_WIKI_URL ||
      process.env.DATA_PLANE_WIKI_BASE_URL ||
      process.env.WIKI_STORE_URL ||
      "http://wiki-store:8011",
  );

export const getQuickwitAdapterUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_QUICKWIT_ADAPTER_URL ||
      process.env.QUICKWIT_ADAPTER_URL ||
      "http://quickwit-adapter:9204",
  );

export const getQuickwitUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_QUICKWIT_URL ||
      process.env.QUICKWIT_URL ||
      "http://quickwit:7280",
  );

export const getQdrantUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_QDRANT_URL ||
      process.env.QDRANT_URL ||
      "http://qdrant:6333",
  );

export const getMinioUrl = () =>
  trimRightSlash(
    process.env.DATA_PLANE_MINIO_URL ||
      process.env.MINIO_URL ||
      "http://minio:9000",
  );

export const getModelPlaneRecommendUrl = () => {
  const explicit = process.env.MODEL_PLANE_RECOMMEND_URL?.trim();
  if (explicit) return trimRightSlash(explicit);
  const base = process.env.MODEL_PLANE_URL?.trim() || process.env.MODEL_PLANE_AI_URL?.trim() || "http://model-gateway:8080";
  return base ? `${trimRightSlash(base)}/v1/recommend/plan` : null;
};

export const getQuarryAudience = () => process.env.QUARRY_AUDIENCE?.trim() || "quarry";

export const getModelPlaneAudience = () =>
  process.env.MODEL_PLANE_AUDIENCE?.trim() || "model-plane";

export const getDataPlaneAudience = () =>
  process.env.DATA_PLANE_AUDIENCE?.trim() || process.env.DATA_PLANE_AUTH_AUDIENCE?.trim() || "data-plane";

const getQuarryControlHmacSecret = () =>
  (
    process.env.QUARRY_INTERNAL_SECRET ||
    process.env.QUARRY_CONTROL_HMAC_SECRET ||
    process.env.QUARRY_HMAC_SECRET ||
    ""
  ).trim();

/**
 * Optional Quarry-control HMAC headers. Dev accepts unsigned requests while
 * QUARRY_INTERNAL_HMAC_REQUIRED=0; production should set the shared secret so
 * these calls keep working after enforcement is enabled.
 */
export function buildQuarryControlHeaders(
  method: string,
  pathWithQuery: string,
  body = "",
): Record<string, string> {
  const secret = getQuarryControlHmacSecret();
  if (!secret) return {};

  const ts = Math.floor(Date.now() / 1000).toString();
  const nonce = randomBytes(16).toString("hex");
  const bodyHash = createHash("sha256").update(body).digest("hex");
  const canonical = `${method.toUpperCase()}\n${pathWithQuery}\n${bodyHash}\n${ts}\n${nonce}`;
  const signature = createHmac("sha256", secret).update(canonical).digest("base64");

  return {
    "X-Quarry-Sig": `sig_v1=${signature}`,
    "X-Quarry-Sig-TS": ts,
    "X-Quarry-Sig-Nonce": nonce,
  };
}

/**
 * Resolve the caller's active organization id from user-core's session-context.
 * Returns null when there is no active org (the caller degrades gracefully).
 */
export async function resolveActiveOrgId(
  request: NextRequest,
  session: ControlPlaneSession,
): Promise<string | null> {
  try {
    const response = await fetch(`${getUserServiceUrl()}/api/v1/me/session-context`, {
      method: "GET",
      headers: buildControlPlaneHeaders(request, session),
      cache: "no-store",
    });
    if (!response.ok) return null;
    const body = (await response.json().catch(() => null)) as
      | { orgId?: string | null; org_id?: string | null; organizationId?: string | null }
      | null;
    return body?.orgId ?? body?.org_id ?? body?.organizationId ?? null;
  } catch {
    return null;
  }
}

/**
 * Mint a short-lived auth-core audience token (e.g. for quarry-edge's Bearer
 * JWT). Forwards the user's cookie so auth-core issues a user-scoped token.
 * Returns null on failure — callers degrade.
 *
 * Results are cached in-process keyed by `(cookie, audience)` until the token's
 * `exp` claim (with a 30s safety margin) — every chat turn was previously
 * paying ~1.6s to re-mint the same token (see [velion-chat timing]). The cache
 * is per server instance and bounded; cookies are hashed before use as a key.
 */
type CachedToken = { token: string; expiresAtMs: number };
const MINT_CACHE = new Map<string, CachedToken>();
const MINT_CACHE_MAX = 256;
const MINT_FALLBACK_TTL_MS = 60_000;
const MINT_SAFETY_MARGIN_MS = 30_000;

function hashKey(input: string): string {
  // FNV-1a 32-bit — cheap, no crypto dep, enough to dedupe a single user's
  // multiple in-flight cookies; we keep audience separate in the key.
  let hash = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

function jwtExpiryMs(token: string): number | null {
  const parts = token.split(".");
  if (parts.length < 2) return null;
  try {
    const payload = parts[1].replace(/-/g, "+").replace(/_/g, "/");
    const padded = payload + "=".repeat((4 - (payload.length % 4)) % 4);
    const decoded = JSON.parse(Buffer.from(padded, "base64").toString("utf8")) as { exp?: unknown };
    return typeof decoded.exp === "number" ? decoded.exp * 1000 : null;
  } catch {
    return null;
  }
}

export async function mintAudienceToken(
  request: NextRequest,
  audience: string,
): Promise<string | null> {
  const cookie = request.headers.get("cookie") ?? "";
  const cacheKey = `${audience}:${hashKey(cookie)}`;

  const cached = MINT_CACHE.get(cacheKey);
  const now = Date.now();
  if (cached && cached.expiresAtMs > now) {
    return cached.token;
  }
  if (cached) {
    MINT_CACHE.delete(cacheKey);
  }

  try {
    const headers = new Headers();
    if (cookie) headers.set("cookie", cookie);
    headers.set("x-internal-api-key", getInternalApiKey());
    headers.set("x-correlation-id", getCorrelationId(request));
    const response = await fetch(`${getAuthServiceUrl()}/api/${encodeURIComponent(audience)}/token`, {
      method: "GET",
      headers,
      cache: "no-store",
    });
    if (!response.ok) return null;
    const body = (await response.json().catch(() => null)) as
      | { token?: string; access_token?: string; jwt?: string }
      | null;
    const token = body?.token ?? body?.access_token ?? body?.jwt ?? null;
    if (!token) return null;

    const expMs = jwtExpiryMs(token);
    const expiresAtMs = expMs && expMs > now
      ? expMs - MINT_SAFETY_MARGIN_MS
      : now + MINT_FALLBACK_TTL_MS;
    if (expiresAtMs > now) {
      if (MINT_CACHE.size >= MINT_CACHE_MAX) {
        // Evict the oldest insertion — Map iterates in insertion order.
        const oldestKey = MINT_CACHE.keys().next().value;
        if (oldestKey !== undefined) {
          MINT_CACHE.delete(oldestKey);
        }
      }
      MINT_CACHE.set(cacheKey, { token, expiresAtMs });
    }
    return token;
  } catch {
    return null;
  }
}

/** Internal headers for a server→service call carrying an explicit org id. */
export function buildServiceHeaders(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId?: string,
): Record<string, string> {
  const headers = buildControlPlaneHeaders(request, session);
  if (orgId) headers["X-Org-ID"] = orgId;
  return headers;
}
