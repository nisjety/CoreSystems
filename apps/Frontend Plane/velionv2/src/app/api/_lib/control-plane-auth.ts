import { NextRequest, NextResponse } from "next/server"

/**
 * Server-side Control Plane session + header helpers for velionv2 API route
 * handlers (the /api/org gateway and friends). Ported/adapted from velion v1
 * (src/app/api/_lib/control-plane-auth.ts).
 *
 * Env precedence is aligned with velionv2 conventions: *_CORE_URL preferred,
 * *_SERVICE_URL accepted as a fallback for docker-compose parity.
 */

export type ControlPlaneSessionUser = {
  id: string
  email?: string
  name?: string | null
  image?: string | null
  avatar?: string | null
  emailVerified?: boolean
}

export type ControlPlaneSession = {
  user: ControlPlaneSessionUser
  session?: unknown
  authenticated?: boolean
}

export class ControlPlaneAuthError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
    message: string,
  ) {
    super(message)
  }
}

const sessionCache = new WeakMap<NextRequest, Promise<ControlPlaneSession | null>>()
const correlationCache = new WeakMap<NextRequest, string>()

const trimRightSlash = (value: string) => value.replace(/\/+$/, "")

/**
 * Stable per-request correlation id for cross-plane log correlation.
 * Honours an inbound `x-correlation-id` if present, otherwise mints a UUIDv4.
 */
export function getCorrelationId(request: NextRequest): string {
  const cached = correlationCache.get(request)
  if (cached) return cached

  // Sanitize the inbound id to a safe charset to prevent log/CRLF injection
  // into downstream service logs. Fall back to a fresh UUID otherwise.
  const inbound = request.headers.get("x-correlation-id")?.trim()
  const id =
    inbound && /^[\w-]{1,128}$/.test(inbound) ? inbound : crypto.randomUUID()

  correlationCache.set(request, id)
  return id
}

export const getAuthServiceUrl = () =>
  trimRightSlash(
    process.env.CONTROL_PLANE_AUTH_URL ||
      process.env.AUTH_CORE_URL ||
      process.env.AUTH_SERVICE_URL ||
      "http://auth-core:3011",
  )

export const getUserServiceUrl = () =>
  trimRightSlash(
    process.env.USER_CORE_URL ||
      process.env.USER_SERVICE_URL ||
      "http://user-core:3012",
  )

/**
 * The repurposed CP session-core (ADR 0002) hosts the Control Session
 * aggregator at /api/v1/sessions/current. Enabled when
 * CONTROL_SESSION_AUTHORITY_ENABLED=true; otherwise velionv2 falls back to
 * user-core's narrower /api/v1/me/session-context. Deferred by default (D4).
 */
export const getSessionServiceUrl = () =>
  trimRightSlash(
    process.env.SESSION_CORE_URL ||
      process.env.SESSION_SERVICE_URL ||
      "http://session-core-service:3017",
  )

export const isControlSessionAuthorityEnabled = () =>
  (process.env.CONTROL_SESSION_AUTHORITY_ENABLED || "").toLowerCase() === "true"

export const getInternalApiKey = () => {
  const key = process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET
  if (!key?.trim()) {
    throw new ControlPlaneAuthError(
      500,
      "missing_internal_api_key",
      "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET must be set",
    )
  }
  return key.trim()
}

export const getOptionalInternalApiKey = () =>
  (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || "").trim()

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value)
}

function normalizeSession(raw: unknown): ControlPlaneSession | null {
  const payload = isRecord(raw) && isRecord(raw.data) ? raw.data : raw
  if (!isRecord(payload)) return null

  const rawUser = payload.user
  if (!isRecord(rawUser) || typeof rawUser.id !== "string" || !rawUser.id.trim()) {
    return null
  }

  return {
    user: {
      id: rawUser.id,
      email: typeof rawUser.email === "string" ? rawUser.email : undefined,
      name: typeof rawUser.name === "string" ? rawUser.name : null,
      image: typeof rawUser.image === "string" ? rawUser.image : null,
      avatar: typeof rawUser.avatar === "string" ? rawUser.avatar : null,
      emailVerified:
        typeof rawUser.emailVerified === "boolean" ? rawUser.emailVerified : undefined,
    },
    session: payload.session,
    authenticated: true,
  }
}

function buildAuthHeaders(request: NextRequest) {
  const headers = new Headers()
  for (const headerName of ["cookie", "user-agent", "accept-language", "accept"]) {
    const value = request.headers.get(headerName)
    if (value) headers.set(headerName, value)
  }

  const authorization = request.headers.get("authorization")
  if (authorization) headers.set("authorization", authorization)

  const internalKey = getOptionalInternalApiKey()
  if (internalKey) headers.set("x-internal-api-key", internalKey)
  headers.set("x-correlation-id", getCorrelationId(request))
  headers.set("content-type", "application/json")
  return headers
}

async function fetchSession(request: NextRequest): Promise<ControlPlaneSession | null> {
  // Uses Better Auth's native `/api/auth/get-session` (GET) on auth-core.
  const response = await fetch(`${getAuthServiceUrl()}/api/auth/get-session`, {
    method: "GET",
    headers: buildAuthHeaders(request),
    cache: "no-store",
  })

  if (response.status === 401 || response.status === 403 || response.status === 404) {
    return null
  }

  if (!response.ok) {
    throw new ControlPlaneAuthError(
      502,
      "auth_service_unavailable",
      `Auth service returned ${response.status}`,
    )
  }

  const payload = await response.json().catch(() => null)
  return normalizeSession(payload)
}

export async function getCurrentSession(request: NextRequest) {
  const cached = sessionCache.get(request)
  if (cached) return cached

  const promise = fetchSession(request)
  sessionCache.set(request, promise)
  return promise
}

export async function requireSession(request: NextRequest) {
  const session = await getCurrentSession(request)
  if (!session) {
    throw new ControlPlaneAuthError(401, "unauthorized", "Authentication required")
  }
  return session
}

export function buildControlPlaneHeaders(
  request: NextRequest,
  session: ControlPlaneSession,
) {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Internal-Api-Key": getInternalApiKey(),
    "X-User-Id": session.user.id,
    "X-Correlation-Id": getCorrelationId(request),
  }

  const authorization = request.headers.get("authorization")
  if (authorization) headers.Authorization = authorization

  if (session.user.email) headers["X-User-Email"] = session.user.email
  if (session.user.name) headers["X-User-Name"] = session.user.name
  if (session.user.image || session.user.avatar) {
    headers["X-User-Avatar"] = session.user.image || session.user.avatar || ""
  }

  return headers
}

export async function readJsonOrNull(response: Response) {
  const text = await response.text()
  if (!text.trim()) return null
  try {
    return JSON.parse(text) as unknown
  } catch {
    return { raw: text }
  }
}

export function authErrorResponse(error: unknown) {
  if (error instanceof ControlPlaneAuthError) {
    return NextResponse.json(
      { error: { code: error.code, message: error.message } },
      { status: error.status },
    )
  }

  console.error("[control-plane-auth] Unexpected error:", error)
  return NextResponse.json(
    { error: { code: "internal_error", message: "Internal server error" } },
    { status: 500 },
  )
}
