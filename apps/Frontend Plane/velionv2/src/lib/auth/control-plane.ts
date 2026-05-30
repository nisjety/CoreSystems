const AUTH_ROUTE_PREFIX = "/api/auth";
const DEFAULT_AUTH_TIMEOUT_MS = 8_000;

type HeaderReader = {
  get(name: string): string | null;
};

type SetCookieReadableHeaders = Headers & {
  getSetCookie?: () => string[];
  raw?: () => Record<string, string[]>;
};

export type ControlPlaneAuthUser = {
  email?: string;
  id: string;
  image?: null | string;
  name?: string;
};

export class ControlPlaneAuthError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

function readOptionalEnv(name: string): string | undefined {
  const value = process.env[name]?.trim();
  return value ? value.replace(/\/+$/, "") : undefined;
}

export function getControlPlaneAuthUrl() {
  return (
    readOptionalEnv("CONTROL_PLANE_AUTH_URL") ??
    readOptionalEnv("AUTH_CORE_URL") ??
    readOptionalEnv("AUTH_SERVICE_URL")
  );
}

export function isControlPlaneAuthConfigured() {
  return Boolean(getControlPlaneAuthUrl());
}

function requestOrigin(headers: HeaderReader) {
  const explicitOrigin = headers.get("origin");
  if (explicitOrigin) {
    return explicitOrigin;
  }

  const host = headers.get("x-forwarded-host") ?? headers.get("host");
  if (!host) {
    return undefined;
  }

  const proto =
    headers.get("x-forwarded-proto") ??
    (host.startsWith("localhost") || host.startsWith("127.0.0.1") ? "http" : "https");

  return `${proto}://${host}`;
}

export function buildControlPlaneAuthUrl(pathname: string, search = "") {
  const baseUrl = getControlPlaneAuthUrl();
  if (!baseUrl) {
    throw new ControlPlaneAuthError(
      503,
      "control_plane_auth_not_configured",
      "CONTROL_PLANE_AUTH_URL or AUTH_CORE_URL must be configured.",
    );
  }

  const upstream = new URL(baseUrl);
  const basePath = upstream.pathname.replace(/\/+$/, "");
  const incomingPath = pathname.startsWith(AUTH_ROUTE_PREFIX)
    ? pathname.slice(AUTH_ROUTE_PREFIX.length)
    : pathname;

  upstream.pathname = basePath.endsWith(AUTH_ROUTE_PREFIX)
    ? `${basePath}${incomingPath || ""}`
    : `${basePath}${pathname.startsWith("/") ? pathname : `/${pathname}`}`;
  upstream.search = search;

  return upstream;
}

function forwardedHeaders(requestHeaders: HeaderReader) {
  const headers = new Headers();
  const passThroughHeaders = [
    "accept",
    "accept-language",
    "cache-control",
    "content-type",
    "cookie",
    "origin",
    "referer",
    "user-agent",
  ];

  for (const name of passThroughHeaders) {
    const value = requestHeaders.get(name);
    if (value) {
      headers.set(name, value);
    }
  }

  const host = requestHeaders.get("x-forwarded-host") ?? requestHeaders.get("host");
  if (host) {
    headers.set("x-forwarded-host", host);
  }

  const proto =
    requestHeaders.get("x-forwarded-proto") ??
    (host?.startsWith("localhost") || host?.startsWith("127.0.0.1") ? "http" : "https");
  if (proto) {
    headers.set("x-forwarded-proto", proto);
  }

  const forwardedFor = requestHeaders.get("x-forwarded-for");
  if (forwardedFor) {
    headers.set("x-forwarded-for", forwardedFor);
  }

  return headers;
}

function extractSetCookie(headers: Headers) {
  const readable = headers as SetCookieReadableHeaders;
  const values = readable.getSetCookie?.();
  if (values?.length) {
    return values;
  }

  const rawValues = readable.raw?.()["set-cookie"];
  if (rawValues?.length) {
    return rawValues;
  }

  const singleValue = headers.get("set-cookie");
  return singleValue ? [singleValue] : [];
}

export function rewriteSetCookieForProxy(cookie: string) {
  if (process.env.AUTH_PROXY_STRIP_COOKIE_DOMAIN === "false") {
    return cookie;
  }

  return cookie.replace(/;\s*domain=[^;]+/i, "");
}

function responseHeadersForProxy(upstreamResponse: Response, requestHeaders: HeaderReader) {
  const headers = new Headers(upstreamResponse.headers);
  const hopByHop = [
    "connection",
    "content-encoding",
    "content-length",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "set-cookie",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
  ];

  for (const header of hopByHop) {
    headers.delete(header);
  }

  const location = upstreamResponse.headers.get("location");
  const appOrigin = requestOrigin(requestHeaders);
  const authOrigin = getControlPlaneAuthUrl();
  if (location && appOrigin && authOrigin && location.startsWith(authOrigin)) {
    headers.set("location", `${appOrigin}${location.slice(authOrigin.length)}`);
  }

  for (const cookie of extractSetCookie(upstreamResponse.headers)) {
    headers.append("set-cookie", rewriteSetCookieForProxy(cookie));
  }

  return headers;
}

export async function proxyControlPlaneAuthRequest(request: Request) {
  const requestUrl = new URL(request.url);
  const upstreamUrl = buildControlPlaneAuthUrl(requestUrl.pathname, requestUrl.search);
  const body =
    request.method === "GET" || request.method === "HEAD"
      ? undefined
      : await request.arrayBuffer();

  const upstreamResponse = await fetch(upstreamUrl, {
    body,
    cache: "no-store",
    headers: forwardedHeaders(request.headers),
    method: request.method,
    redirect: "manual",
    signal: AbortSignal.timeout(
      Number(process.env.AUTH_CORE_TIMEOUT_MS ?? DEFAULT_AUTH_TIMEOUT_MS),
    ),
  }).catch((error: unknown) => {
    throw new ControlPlaneAuthError(
      502,
      "control_plane_auth_unreachable",
      error instanceof Error ? error.message : "auth-core request failed",
    );
  });

  return new Response(upstreamResponse.body, {
    headers: responseHeadersForProxy(upstreamResponse, request.headers),
    status: upstreamResponse.status,
    statusText: upstreamResponse.statusText,
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

export function extractControlPlaneAuthUser(payload: unknown): ControlPlaneAuthUser | null {
  const data = isRecord(payload) && isRecord(payload.data) ? payload.data : payload;
  if (!isRecord(data) || !isRecord(data.user) || typeof data.user.id !== "string") {
    return null;
  }

  return {
    id: data.user.id,
    email: typeof data.user.email === "string" ? data.user.email : undefined,
    image: typeof data.user.image === "string" || data.user.image === null ? data.user.image : undefined,
    name: typeof data.user.name === "string" ? data.user.name : undefined,
  };
}

export async function getControlPlaneCurrentUser(headerList: HeaderReader) {
  const response = await fetch(buildControlPlaneAuthUrl("/api/auth/get-session"), {
    cache: "no-store",
    headers: forwardedHeaders(headerList),
    method: "GET",
    signal: AbortSignal.timeout(
      Number(process.env.AUTH_CORE_TIMEOUT_MS ?? DEFAULT_AUTH_TIMEOUT_MS),
    ),
  }).catch((error: unknown) => {
    throw new ControlPlaneAuthError(
      502,
      "control_plane_auth_unreachable",
      error instanceof Error ? error.message : "auth-core request failed",
    );
  });

  if (response.status === 401 || response.status === 403 || response.status === 404) {
    return null;
  }

  if (!response.ok) {
    throw new ControlPlaneAuthError(
      response.status,
      "control_plane_auth_error",
      `auth-core returned ${response.status}`,
    );
  }

  const payload = await response.json().catch(() => null);
  return extractControlPlaneAuthUser(payload);
}
