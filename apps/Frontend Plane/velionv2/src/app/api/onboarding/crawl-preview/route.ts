import { createHash } from "node:crypto";
import { type NextRequest } from "next/server";

import { ControlPlaneAuthError, requireSession } from "@/app/api/_lib/control-plane-auth";
import {
  buildQuarryControlHeaders,
  getQuarryControlUrl,
  getQuarryAudience,
  getQuarryEdgeUrl,
  mintAudienceToken,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { normalizePublicHttpUrl } from "@/app/api/onboarding/_lib/public-url";
import { consumeSse, parseEventJson } from "@/lib/net/sse";
import { extractCustomPropertyColors, prioritizeStylesheetUrls } from "./branding-css";

export const dynamic = "force-dynamic";
export const maxDuration = 30;

const DNS_TIMEOUT_MS = 2_500;
const SCRAPE_TIMEOUT_MS = 8_000;
const CRAWL_EVENT_TIMEOUT_MS = 18_000;
const POLL_INTERVAL_MS = 750;
const BRANDING_DISCOVERY_TIMEOUT_MS = 3_500;
const MAX_BRANDING_HTML_BYTES = 200_000;
const MAX_BRANDING_CSS_BYTES = 180_000;
const MAX_BRANDING_STYLESHEETS = 4;
const MAX_BRANDING_REDIRECTS = 4;

/**
 * POST /api/onboarding/crawl-preview (SSE)
 *
 * Composes Quarry v2: streams the seed page from quarry-edge for instant
 * snippets + branding, creates a real quarry-control crawl job, then polls
 * `/v1/jobs/{id}/events` and forwards page_fetched / branding_extracted as
 * browser SSE. Branding is normalized + SSRF-filtered here before it reaches
 * the client. Degrades open: always terminates with a `done` event.
 */
export async function POST(request: NextRequest) {
  let session;
  try {
    session = await requireSession(request);
  } catch (error) {
    return warningDoneResponse(authWarningCode(error));
  }

  try {
    const body = (await request.json().catch(() => null)) as
      | { url?: string; brief?: string; maxPages?: number }
      | null;
    const inputUrl = body?.url?.trim();
    if (!inputUrl) {
      return warningDoneResponse("invalid_url");
    }

    const validatedUrl = await normalizePublicHttpUrl(inputUrl, DNS_TIMEOUT_MS);
    if (!validatedUrl.ok) {
      return warningDoneResponse(validatedUrl.code);
    }
    const maxPages = typeof body?.maxPages === "number" ? Math.min(20, Math.max(1, body.maxPages)) : 8;
    const orgId = await resolveActiveOrgId(request, session);

    const quarry = getQuarryEdgeUrl();
    const quarryControl = getQuarryControlUrl();

    const stream = new ReadableStream({
      async start(controller) {
      const encoder = new TextEncoder();
      let closed = false;
      const send = (event: string, data: unknown) => {
        if (closed) return;
        try {
          controller.enqueue(encoder.encode(sse(event, data)));
        } catch {
          closed = true;
        }
      };

      let snippetCount = 0;
      let livePageCount = 0;
      let elementCount = 0;
      let terminalStatus: "completed" | "failed" | "cancelled" | null = null;
      const seenSnippetUrls = new Set<string>();

      const emitSnippet = (snippet: ReturnType<typeof toSnippet>) => {
        if (!snippet) return;
        const key = `${snippet.kind}:${snippet.url}`;
        if (seenSnippetUrls.has(key)) return;
        seenSnippetUrls.add(key);
        snippetCount += 1;
        send("snippet", snippet);
      };

      const emitProgress = (input: {
        status: "starting" | "running" | "completed" | "failed" | "cancelled";
        jobId?: string;
        latestUrl?: string;
        latestTitle?: string;
        pending?: number;
      }) => {
        send("progress", {
          target: maxPages,
          pages: livePageCount,
          elements: elementCount,
          ...input,
        });
      };

      try {
        const token = await mintAudienceToken(request, getQuarryAudience());
        const authHeaders: Record<string, string> = token ? { Authorization: `Bearer ${token}` } : {};
        const stylesheetBrandingPromise = discoverStylesheetBranding(validatedUrl.url)
          .then((branding) => {
            if (branding) send("branding", branding);
          })
          .catch(() => undefined);
        const crawlJobPromise = createCrawlJob({
          controlUrl: quarryControl,
          orgId: orgId ?? undefined,
          url: validatedUrl.url,
          maxPages,
          signal: request.signal,
        })
          .then(async (job) => {
            send("started", { jobId: job.id, url: validatedUrl.url, target: maxPages });
            emitProgress({ status: "starting", jobId: job.id });

            const result = await pollCrawlEvents({
              controlUrl: quarryControl,
              jobId: job.id,
              signal: request.signal,
              onEvent: (event) => {
                const type = event.type;
                const payload = record(event.payload) ?? {};

                if (type === "branding_extracted") {
                  const branding = normalizeBranding(payload);
                  if (branding) send("branding", branding);
                  return;
                }

                if (type === "page_fetched") {
                  const linkCount = numberValue(payload.links);
                  livePageCount += 1;
                  elementCount += linkCount;
                  emitSnippet(
                    toSnippet(payload, snippetCount, {
                      eventId: event.event_id,
                      source: "live",
                      elementCount: linkCount,
                    }),
                  );
                  emitProgress({
                    status: "running",
                    jobId: job.id,
                    latestUrl: str(payload.url),
                    latestTitle: str(payload.title),
                  });
                  return;
                }

                if (type === "run_started") {
                  emitProgress({ status: "running", jobId: job.id });
                  return;
                }

                if (type === "run_completed") {
                  terminalStatus = "completed";
                  emitProgress({ status: "completed", jobId: job.id });
                  return;
                }

                if (type === "run_cancelled") {
                  terminalStatus = "cancelled";
                  emitProgress({ status: "cancelled", jobId: job.id });
                  return;
                }

                if (type === "run_failed") {
                  terminalStatus = "failed";
                  send("warning", { code: "crawl_failed", message: str(payload.error) });
                  emitProgress({ status: "failed", jobId: job.id });
                }
              },
            });

            if (!result.sawPage && snippetCount === 0) {
              send("warning", { code: "no_events" });
            }
          })
          .catch((error) => {
            if ((error as { name?: string }).name === "AbortError") return;
            send("warning", { code: "control_unreachable" });
          });

        // 1) Seed-page scrape stream — instant snippets + branding.
        const seedScrapePromise = consumeSse(`${quarry}/v1/scrape/stream`, {
            method: "POST",
            headers: authHeaders,
            body: JSON.stringify({ url: validatedUrl.url }),
            signal: AbortSignal.timeout(SCRAPE_TIMEOUT_MS),
            onEvent: (event) => {
              const payload = parseEventJson<Record<string, unknown>>(event);
              if (!payload) return;
              const quarryEvent = normalizeQuarryEvent(payload, event.event);
              if (quarryEvent.type === "branding_extracted") {
                const branding = normalizeBranding(record(quarryEvent.payload) ?? payload);
                if (branding) send("branding", branding);
                return;
              }
              if (quarryEvent.type === "page_fetched") {
                const payloadRecord = record(quarryEvent.payload) ?? payload;
                emitSnippet(
                  toSnippet(payloadRecord, snippetCount, {
                    eventId: quarryEvent.event_id,
                    source: "seed",
                    elementCount: numberValue(payloadRecord.links),
                  }),
                );
              }
            },
        }).catch((error) => {
          if ((error as { name?: string }).name === "AbortError") return;
          // The quarry-control crawl job is the authoritative live path; seed
          // scrape is an optional fast-preview path and should not alarm the UI.
        });

        await Promise.allSettled([stylesheetBrandingPromise, seedScrapePromise, crawlJobPromise]);
        send("done", {
          count: Math.max(snippetCount, livePageCount),
          pages: livePageCount,
          elements: elementCount,
          status: terminalStatus ?? "completed",
        });
      } catch {
        send("warning", { code: "control_unreachable" });
        send("done", { count: snippetCount });
      } finally {
        closed = true;
        controller.close();
      }
      },
    });

    return new Response(stream, { headers: sseHeaders() });
  } catch (error) {
    console.error("[onboarding:crawl-preview] preflight failed", error);
    return warningDoneResponse("control_unreachable");
  }
}

function warningDoneResponse(code: string): Response {
  return new Response(
    `${sse("warning", { code })}${sse("done", {
      count: 0,
      pages: 0,
      elements: 0,
      status: "failed",
    })}`,
    {
    headers: sseHeaders(),
    },
  );
}

function authWarningCode(error: unknown): string {
  if (error instanceof ControlPlaneAuthError && error.status === 401) {
    return "unauthorized";
  }
  return "auth_unavailable";
}

function sse(event: string, data: unknown): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
}

function sseHeaders(): HeadersInit {
  return {
    "Content-Type": "text/event-stream; charset=utf-8",
    "Cache-Control": "no-store, no-transform",
    Connection: "keep-alive",
  };
}

/* -------------------------------------------------------------- control job */

interface ControlJob {
  id: string;
}

interface QuarryEvent {
  event_id?: string;
  type: string;
  seq: number;
  payload?: unknown;
}

async function createCrawlJob(input: {
  controlUrl: string;
  orgId?: string;
  url: string;
  maxPages: number;
  signal: AbortSignal;
}): Promise<ControlJob> {
  const path = "/v1/jobs/";
  const requestBody = JSON.stringify({
    kind: "crawl",
    params: {
      url: input.url,
      max_pages: input.maxPages,
      max_depth: 1,
      auto_commit: Boolean(input.orgId),
      org_id: input.orgId,
    },
  });
  const response = await fetch(`${input.controlUrl}${path}`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      "Idempotency-Key": crawlIdempotencyKey(input.url, input.maxPages),
      ...buildQuarryControlHeaders("POST", path, requestBody),
    },
    cache: "no-store",
    signal: input.signal,
    body: requestBody,
  });
  if (!response.ok) throw new Error(`quarry-control job create ${response.status}`);
  const responseBody = await response.json().catch(() => null);
  const job = unwrapData(responseBody);
  const jobRecord = record(job);
  if (!jobRecord || typeof jobRecord.id !== "string") {
    throw new Error("quarry-control job create returned no id");
  }
  return { id: jobRecord.id };
}

async function pollCrawlEvents(input: {
  controlUrl: string;
  jobId: string;
  signal: AbortSignal;
  onEvent: (event: QuarryEvent) => void;
}): Promise<{ sawEvent: boolean; sawPage: boolean }> {
  const startedAt = Date.now();
  let afterSeq = 0;
  let sawEvent = false;
  let sawPage = false;
  let terminal = false;

  while (!terminal && Date.now() - startedAt < CRAWL_EVENT_TIMEOUT_MS) {
    if (input.signal.aborted) throw new DOMException("Aborted", "AbortError");
    const events = await fetchJobEvents(input.controlUrl, input.jobId, afterSeq, input.signal);
    for (const event of events) {
      sawEvent = true;
      afterSeq = Math.max(afterSeq, event.seq);
      input.onEvent(event);
      if (event.type === "page_fetched") sawPage = true;
      terminal =
        event.type === "run_completed" ||
        event.type === "run_failed" ||
        event.type === "run_cancelled";
    }
    if (!terminal) await sleep(POLL_INTERVAL_MS, input.signal);
  }

  return { sawEvent, sawPage };
}

async function fetchJobEvents(
  controlUrl: string,
  jobId: string,
  afterSeq: number,
  signal: AbortSignal,
): Promise<QuarryEvent[]> {
  const path = `/v1/jobs/${encodeURIComponent(jobId)}/events?after_seq=${afterSeq}&limit=100`;
  const response = await fetch(`${controlUrl}${path}`, {
    method: "GET",
    headers: { Accept: "application/json", ...buildQuarryControlHeaders("GET", path) },
    cache: "no-store",
    signal,
  });
  if (!response.ok) throw new Error(`quarry-control job events ${response.status}`);
  const body = await response.json().catch(() => null);
  const data = unwrapData(body);
  const items = Array.isArray(data) ? data : [];
  return items.map((item) => normalizeQuarryEvent(item, "event")).filter((event) => event.seq > afterSeq);
}

function crawlIdempotencyKey(url: string, maxPages: number): string {
  const day = new Date().toISOString().slice(0, 10);
  const digest = createHash("sha256").update(`${url}:${maxPages}:${day}`).digest("hex").slice(0, 40);
  return `verevon-crawl-${digest}`;
}

/* ----------------------------------------------------------- snippet mapping */

function normalizeQuarryEvent(input: unknown, fallbackType: string): QuarryEvent {
  const root = record(input) ?? {};
  const payload = "payload" in root ? root.payload : root;
  return {
    event_id: str(root.event_id) ?? str(root.id),
    type: str(root.type) ?? str(root.event_type) ?? fallbackType,
    seq: numberValue(root.seq),
    payload,
  };
}

function toSnippet(payload: Record<string, unknown>, index: number, options?: {
  eventId?: string;
  source?: "seed" | "live";
  elementCount?: number;
}): {
  id: string;
  kind: "text" | "image" | "file" | "link";
  title: string;
  excerpt?: string;
  thumbUrl?: string;
  url: string;
  contentType?: string;
  source?: "seed" | "live";
  elementCount?: number;
} | null {
  const url = str(payload.url) ?? str(payload.href) ?? str(payload.source);
  if (!url) return null;
  const contentType = str(payload.content_type) ?? str(payload.contentType);
  const kind = classifyKind(contentType, str(payload.kind));
  const title = str(payload.title) ?? str(payload.name) ?? safeHost(url);
  const linkCount = options?.elementCount ?? numberValue(payload.links);
  const generatedExcerpt =
    linkCount > 0 ? `${linkCount} discovered element${linkCount === 1 ? "" : "s"}` : undefined;
  const excerpt = truncate(str(payload.text) ?? str(payload.excerpt) ?? str(payload.summary) ?? generatedExcerpt);
  return {
    id: str(payload.id) ?? options?.eventId ?? `${index}:${url}`,
    kind,
    title,
    excerpt,
    thumbUrl: kind === "image" ? safeUrl(str(payload.thumb_url) ?? str(payload.image) ?? url) : undefined,
    url,
    contentType,
    source: options?.source,
    elementCount: linkCount,
  };
}

function classifyKind(contentType: string | undefined, explicit: string | undefined): "text" | "image" | "file" | "link" {
  const explicitKind = explicit?.toLowerCase();
  if (explicitKind === "image" || explicitKind === "file" || explicitKind === "link" || explicitKind === "text") {
    return explicitKind;
  }
  const ct = (contentType ?? "").toLowerCase();
  if (ct.startsWith("image/")) return "image";
  if (ct === "application/pdf" || ct.includes("word") || ct.includes("excel") || ct.includes("spreadsheet") || ct === "text/csv") {
    return "file";
  }
  return "text";
}

/* ----------------------------------------------------------- branding (SSRF-safe) */

interface BrandingSignals {
  url?: string;
  siteName?: string;
  favicon?: string;
  themeColor?: string;
  ogImage?: string;
  appleTouchIcon?: string;
  palette?: string[];
  fontFamily?: string;
  logoCandidate?: string;
  bodyBackground?: string;
}

const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

/**
 * Normalize quarry's snake_case branding payload into camelCase BrandingSignals.
 * Every URL is run through `safeBrandingURL` (blocks data:/relative/private
 * hosts) and palette entries are restricted to hex — these values are inlined
 * into `<img src>`/`backgroundColor` on the client.
 */
function normalizeBranding(input: Record<string, unknown>): BrandingSignals | null {
  const root = input.branding && typeof input.branding === "object" ? (input.branding as Record<string, unknown>) : input;
  const staticSignals =
    root.static_signals && typeof root.static_signals === "object"
      ? (root.static_signals as Record<string, unknown>)
      : root;

  const paletteIn = Array.isArray(root.palette) ? (root.palette as unknown[]) : [];
  const palette = paletteIn
    .filter((x): x is string => typeof x === "string")
    .filter((s) => HEX_COLOR.test(s.trim()))
    .map((s) => s.trim().toLowerCase());

  const themeColorRaw = str(staticSignals.theme_color);
  const themeColor = themeColorRaw && HEX_COLOR.test(themeColorRaw) ? themeColorRaw.toLowerCase() : undefined;

  const out: BrandingSignals = {
    url: safeBrandingURL(str(input.url)),
    siteName: str(staticSignals.site_name),
    favicon: safeBrandingURL(str(staticSignals.favicon)),
    themeColor,
    ogImage: safeBrandingURL(str(staticSignals.og_image)),
    appleTouchIcon: safeBrandingURL(str(staticSignals.apple_touch_icon)),
    palette: palette.length > 0 ? palette : undefined,
    fontFamily: str(root.font_family),
    logoCandidate: safeBrandingURL(str(root.logo_candidate)),
    bodyBackground: str(root.body_background),
  };

  const hasAny = Object.values(out).some(
    (v) => v !== undefined && (typeof v !== "object" || (Array.isArray(v) && v.length > 0)),
  );
  return hasAny ? out : null;
}

async function discoverStylesheetBranding(seedUrl: string): Promise<BrandingSignals | null> {
  const htmlResponse = await fetchPublicText(seedUrl, {
    accept: "text/html,application/xhtml+xml",
    maxBytes: MAX_BRANDING_HTML_BYTES,
    timeoutMs: BRANDING_DISCOVERY_TIMEOUT_MS,
  });
  if (!htmlResponse.contentType.includes("text/html")) return null;

  const html = htmlResponse.text;
  const stylesheetUrls = stylesheetUrlsFromHtml(html, htmlResponse.url).slice(0, MAX_BRANDING_STYLESHEETS);
  if (stylesheetUrls.length === 0) return null;

  const stylesheets = await Promise.allSettled(
    stylesheetUrls.map((url) =>
      fetchPublicText(url, {
        accept: "text/css,*/*;q=0.1",
        maxBytes: MAX_BRANDING_CSS_BYTES,
        timeoutMs: BRANDING_DISCOVERY_TIMEOUT_MS,
      }),
    ),
  );
  const css = stylesheets
    .flatMap((result) => result.status === "fulfilled" && result.value.contentType.includes("css") ? [result.value.text] : [])
    .join("\n");
  if (!css.trim()) return null;

  const htmlThemeColor = cssColorToHex(metaContent(html, ["theme-color"]));
  const primaryColors = extractUtilityClassColors(css, ["text-primary", "bg-primary", "border-primary"]);
  const accentColors = extractUtilityClassColors(css, [
    "text-glacous",
    "bg-glacous",
    "border-glacous",
    "bg-glacous-light",
    "border-glacous-light",
  ]);
  const customPropertyColors = extractCustomPropertyColors(css, cssColorToHex);
  const fallbackColors = primaryColors.length > 0 || customPropertyColors.length > 0
    ? []
    : extractFrequentCssColors(css);
  const palette = uniqueStrings([
    ...primaryColors,
    ...accentColors,
    ...customPropertyColors,
    ...fallbackColors,
  ]).slice(0, 8);
  const themeColor = firstUsableThemeColor([htmlThemeColor, ...primaryColors, ...customPropertyColors, ...palette]);

  if (!themeColor && palette.length === 0) return null;
  return {
    url: htmlResponse.url,
    siteName: metaContent(html, ["og:site_name", "application-name"]) ?? undefined,
    themeColor,
    palette: palette.length > 0 ? palette : undefined,
  };
}

async function fetchPublicText(
  inputUrl: string,
  options: { accept: string; maxBytes: number; timeoutMs: number },
): Promise<{ contentType: string; text: string; url: string }> {
  let currentUrl = inputUrl;
  for (let redirects = 0; redirects <= MAX_BRANDING_REDIRECTS; redirects += 1) {
    const validated = await normalizePublicHttpUrl(currentUrl, DNS_TIMEOUT_MS);
    if (!validated.ok) throw new Error(validated.code);

    const response = await fetch(validated.url, {
      cache: "no-store",
      headers: {
        Accept: options.accept,
        "User-Agent": "Verevon-Onboarding/1.0 (+https://verevon.com)",
      },
      redirect: "manual",
      signal: AbortSignal.timeout(options.timeoutMs),
    });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get("location");
      if (!location) throw new Error("redirect_without_location");
      currentUrl = new URL(location, validated.url).toString();
      continue;
    }
    if (!response.ok) throw new Error(`fetch_failed_${response.status}`);
    return {
      contentType: response.headers.get("content-type")?.toLowerCase() ?? "",
      text: await readTextWithLimit(response, options.maxBytes),
      url: response.url || validated.url,
    };
  }
  throw new Error("too_many_redirects");
}

async function readTextWithLimit(response: Response, maxBytes: number): Promise<string> {
  if (!response.body) return (await response.text()).slice(0, maxBytes);
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!value) continue;
      const remaining = maxBytes - total;
      if (remaining <= 0) break;
      chunks.push(value.byteLength > remaining ? value.slice(0, remaining) : value);
      total += Math.min(value.byteLength, remaining);
      if (total >= maxBytes) break;
    }
  } finally {
    reader.cancel().catch(() => undefined);
  }
  const buffer = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    buffer.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder("utf-8", { fatal: false }).decode(buffer);
}

function stylesheetUrlsFromHtml(html: string, pageUrl: string): string[] {
  const page = new URL(pageUrl);
  const urls: string[] = [];
  for (const match of html.matchAll(/<link\b[^>]*>/gi)) {
    const attrs = parseHtmlAttributes(match[0]);
    const rel = attrs.rel?.toLowerCase() ?? "";
    const as = attrs.as?.toLowerCase() ?? "";
    if (!attrs.href || (!rel.includes("stylesheet") && !(rel.includes("preload") && as === "style"))) {
      continue;
    }
    try {
      const resolved = new URL(attrs.href, page).toString();
      const parsed = new URL(resolved);
      if (parsed.origin === page.origin) urls.push(parsed.toString());
    } catch {
      // Ignore malformed asset URLs; Quarry still provides the crawl.
    }
  }
  return prioritizeStylesheetUrls(uniqueStrings(urls), page.toString());
}

function metaContent(html: string, names: string[]): string | null {
  const expected = new Set(names.map((name) => name.toLowerCase()));
  for (const match of html.matchAll(/<meta\b[^>]*>/gi)) {
    const attrs = parseHtmlAttributes(match[0]);
    const key = (attrs.name ?? attrs.property ?? "").toLowerCase();
    if (key && expected.has(key) && attrs.content) return attrs.content.trim();
  }
  return null;
}

function parseHtmlAttributes(tag: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  for (const match of tag.matchAll(/([:\w-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g)) {
    attrs[match[1].toLowerCase()] = match[2] ?? match[3] ?? match[4] ?? "";
  }
  return attrs;
}

function extractUtilityClassColors(css: string, classNames: string[]): string[] {
  const colors: string[] = [];
  for (const className of classNames) {
    const pattern = new RegExp(`\\.${escapeRegExp(className)}\\{([^}]*)\\}`, "g");
    for (const match of css.matchAll(pattern)) {
      colors.push(...colorsFromDeclaration(match[1]));
    }
  }
  return uniqueStrings(colors);
}

function extractFrequentCssColors(css: string): string[] {
  const counts = new Map<string, number>();
  for (const color of colorsFromDeclaration(css)) {
    if (isHardNeutral(color)) continue;
    counts.set(color, (counts.get(color) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([color]) => color)
    .slice(0, 5);
}

function colorsFromDeclaration(input: string): string[] {
  const colors: string[] = [];
  for (const match of input.matchAll(/#[0-9a-f]{3,8}\b|rgba?\([^)]*\)/gi)) {
    const color = cssColorToHex(match[0]);
    if (color) colors.push(color);
  }
  return colors;
}

function cssColorToHex(value: string | null | undefined): string | null {
  if (!value) return null;
  const hex = value.match(/#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})\b/i)?.[0];
  if (hex) {
    const raw = hex.toLowerCase().replace("#", "");
    if (raw.length === 3) return `#${raw.split("").map((char) => char + char).join("")}`;
    return `#${raw.slice(0, 6)}`;
  }
  const rgb = value.match(/rgba?\(\s*([0-9.]+)[\s,]+([0-9.]+)[\s,]+([0-9.]+)/i);
  if (!rgb) return null;
  const channels = rgb.slice(1, 4).map((channel) => Math.max(0, Math.min(255, Math.round(Number(channel)))));
  if (channels.some((channel) => !Number.isFinite(channel))) return null;
  return `#${channels.map((channel) => channel.toString(16).padStart(2, "0")).join("")}`;
}

function firstUsableThemeColor(colors: Array<string | null | undefined>): string | undefined {
  return colors.find((color): color is string => Boolean(color && !isHardNeutral(color)));
}

function isHardNeutral(color: string): boolean {
  const raw = color.replace("#", "").slice(0, 6);
  const r = Number.parseInt(raw.slice(0, 2), 16);
  const g = Number.parseInt(raw.slice(2, 4), 16);
  const b = Number.parseInt(raw.slice(4, 6), 16);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  return max < 18 || min > 245 || max - min < 8;
}

function uniqueStrings(values: Array<string | undefined | null>): string[] {
  return Array.from(new Set(values.filter((value): value is string => Boolean(value))));
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function safeBrandingURL(input: string | undefined): string | undefined {
  if (!input) return undefined;
  let parsed: URL;
  try {
    parsed = new URL(input.trim());
  } catch {
    return undefined;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return undefined;
  const host = parsed.hostname;
  if (
    !host ||
    host === "localhost" ||
    /^(127|10)\./.test(host) ||
    /^192\.168\./.test(host) ||
    /^172\.(1[6-9]|2\d|3[01])\./.test(host) ||
    /^169\.254\./.test(host) ||
    host === "::1" ||
    host.startsWith("fe80:") ||
    host.startsWith("fc") ||
    host.startsWith("fd")
  ) {
    return undefined;
  }
  return parsed.toString();
}

function safeUrl(input: string | undefined): string | undefined {
  return safeBrandingURL(input);
}

/* ----------------------------------------------------------------- helpers */

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function unwrapData(value: unknown): unknown {
  const root = record(value);
  return root && "data" in root ? root.data : value;
}

function str(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function numberValue(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, Math.round(value)) : 0;
}

function truncate(value: string | undefined): string | undefined {
  if (!value) return undefined;
  const clean = value.replace(/\s+/g, " ").trim();
  return clean.length > 180 ? `${clean.slice(0, 177)}...` : clean;
}

function safeHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(new DOMException("Aborted", "AbortError"));
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        reject(new DOMException("Aborted", "AbortError"));
      },
      { once: true },
    );
  });
}
