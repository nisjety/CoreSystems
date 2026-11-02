import { createHash, timingSafeEqual } from "node:crypto";
import { getVitalsConfig, type VitalsConfig } from "./config";
import { readSample, VitalsRequestError } from "./request";
import { readVitals, recordVital } from "./storage";

type Dependencies = Readonly<{
  config?: () => VitalsConfig | null;
  record?: typeof recordVital;
  read?: typeof readVitals;
  now?: () => Date;
}>;
const failure = (
  status: number,
  code: string,
  headers: Record<string, string> = {},
) =>
  Response.json(
    {
      success: false,
      error: {
        code,
        message:
          "Performance reporting is unavailable or the request is invalid.",
      },
    },
    { status, headers: { "Cache-Control": "no-store", ...headers } },
  );
const method = () => failure(405, "method_not_allowed");
const unavailable = () => failure(503, "web_vitals_unavailable");

export async function handleVitalsPost(
  request: Request,
  deps: Dependencies = {},
): Promise<Response> {
  if (request.method !== "POST") return method();
  if (new URL(request.url).search)
    return failure(400, "invalid_web_vitals_request");
  if (
    request.headers.get("dnt") === "1" ||
    request.headers.get("sec-gpc") === "1"
  )
    return new Response(null, {
      status: 204,
      headers: { "Cache-Control": "no-store" },
    });
  const config = (deps.config ?? getVitalsConfig)();
  if (!config) return unavailable();
  if (
    request.headers.get("origin") !== config.reportingOrigin ||
    request.headers.get("sec-fetch-site") !== "same-origin"
  )
    return failure(403, "same_origin_required");
  try {
    const sample = await readSample(request);
    const result = await (deps.record ?? recordVital)(
      config,
      sample,
      (deps.now ?? (() => new Date()))(),
    );
    if (result === "conflict")
      return failure(409, "web_vitals_sample_conflict");
    if (result === "rate_limited" || result === "capacity")
      return failure(429, "web_vitals_capacity", { "Retry-After": "60" });
    return new Response(null, {
      status: 204,
      headers: { "Cache-Control": "no-store" },
    });
  } catch (error) {
    if (error instanceof VitalsRequestError)
      return failure(error.status, "invalid_web_vitals_request");
    return unavailable();
  }
}

export async function handleVitalsExport(
  request: Request,
  deps: Dependencies = {},
): Promise<Response> {
  if (request.method !== "GET") return method();
  if (new URL(request.url).search || request.body)
    return failure(400, "invalid_web_vitals_request");
  const config = (deps.config ?? getVitalsConfig)();
  if (!config) return unavailable();
  const header = request.headers.get("authorization") ?? "";
  const candidate = /^Bearer ([A-Za-z0-9_-]{32,256})$/.exec(header)?.[1] ?? "";
  const hash = (value: string) => createHash("sha256").update(value).digest();
  if (!timingSafeEqual(hash(candidate), hash(config.exportToken)))
    return failure(401, "unauthorized");
  try {
    const now = (deps.now ?? (() => new Date()))();
    const samples = await (deps.read ?? readVitals)(config, now);
    const body = JSON.stringify({
      success: true,
      data: {
        websiteId: config.websiteId,
        site: config.site,
        websiteRevision: config.websiteRevision,
        environment: config.environment,
        reportingOrigin: config.reportingOrigin,
        status: "connected",
        samples,
        observedAt: now.toISOString(),
      },
    });
    if (Buffer.byteLength(body) > 2 * 1024 * 1024) return unavailable();
    return new Response(body, {
      headers: {
        "Content-Type": "application/json",
        "Cache-Control": "no-store",
      },
    });
  } catch {
    return unavailable();
  }
}
