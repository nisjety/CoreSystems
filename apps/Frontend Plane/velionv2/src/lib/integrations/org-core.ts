import "server-only";
import type { ControlPlaneOrganization } from "@/lib/control-plane/context-types";
import type { RequestActor } from "@/lib/integrations/request-actor";

export class OrgCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

type RawOrgCoreOrganization = {
  id?: string;
  name?: string;
  slug?: string;
  plan?: string;
  status?: string;
  primary_domain?: string;
  primaryDomain?: string;
  metadata?: Record<string, unknown>;
};

const HEX_COLOR = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

function getOrgCoreUrl() {
  return (process.env.ORG_SERVICE_URL ?? process.env.ORG_CORE_URL ?? "http://localhost:18080").replace(/\/+$/, "");
}

function getInternalApiKey() {
  return process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET;
}

function buildHeaders(actor: RequestActor) {
  const internalApiKey = getInternalApiKey();

  if (!internalApiKey) {
    throw new OrgCoreError(
      503,
      "org_core_key_not_configured",
      "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET is required for org-core.",
    );
  }

  const headers = new Headers({
    "X-Internal-Api-Key": internalApiKey,
    "X-User-Id": actor.userId,
    "Content-Type": "application/json",
  });

  if (actor.email) headers.set("X-User-Email", actor.email);
  if (actor.name) headers.set("X-User-Name", actor.name);
  if (actor.cookieHeader) headers.set("Cookie", actor.cookieHeader);

  return headers;
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function string(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

function safeUrl(value: unknown): string | null {
  const raw = string(value);
  if (!raw) return null;

  try {
    const url = new URL(raw);
    return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function safeColor(value: unknown): string | null {
  const raw = string(value);
  return raw && HEX_COLOR.test(raw) ? raw.toLowerCase() : null;
}

function nestedMetadata(metadata: Record<string, unknown>) {
  const onboarding = record(metadata.onboarding);
  return [
    metadata,
    record(metadata.branding),
    record(metadata.onboarding_branding),
    record(metadata.onboardingBranding),
    record(onboarding?.branding),
    record(metadata.website),
    record(record(metadata.website)?.branding),
  ].filter((item): item is Record<string, unknown> => Boolean(item));
}

function firstMetadataUrl(metadata: Record<string, unknown>) {
  for (const source of nestedMetadata(metadata)) {
    const found =
      safeUrl(source.logoUrl) ??
      safeUrl(source.logo_url) ??
      safeUrl(source.logoCandidate) ??
      safeUrl(source.logo_candidate) ??
      safeUrl(source.appleTouchIcon) ??
      safeUrl(source.apple_touch_icon) ??
      safeUrl(source.favicon) ??
      safeUrl(source.icon);

    if (found) return found;
  }

  return null;
}

function firstMetadataColor(metadata: Record<string, unknown>) {
  for (const source of nestedMetadata(metadata)) {
    const found =
      safeColor(source.primaryColor) ??
      safeColor(source.primary_color) ??
      safeColor(source.themeColor) ??
      safeColor(source.theme_color) ??
      safeColor(source.colorScheme) ??
      safeColor(source.color_scheme);

    if (found) return found;
  }

  return null;
}

function normalizeOrganization(raw: RawOrgCoreOrganization): ControlPlaneOrganization | null {
  if (!raw.id) return null;

  const metadata = record(raw.metadata) ?? {};
  const name = string(raw.name) ?? string(raw.slug) ?? "Workspace";

  return {
    id: raw.id,
    name,
    slug: string(raw.slug),
    plan: string(raw.plan),
    status: string(raw.status),
    primaryDomain: string(raw.primaryDomain) ?? string(raw.primary_domain),
    logoUrl: firstMetadataUrl(metadata),
    accentColor: firstMetadataColor(metadata),
  };
}

export async function fetchOrganization(
  actor: RequestActor,
  orgId: string,
): Promise<ControlPlaneOrganization | null> {
  const response = await fetch(`${getOrgCoreUrl()}/orgs/${encodeURIComponent(orgId)}`, {
    headers: buildHeaders(actor),
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  }).catch((error: unknown) => {
    throw new OrgCoreError(
      502,
      "org_core_unreachable",
      error instanceof Error ? error.message : "org-core request failed",
    );
  });

  if (response.status === 404) return null;
  if (!response.ok) {
    throw new OrgCoreError(response.status, "org_core_error", `org-core returned ${response.status}`);
  }

  const body = (await response.json()) as unknown;
  const envelope = record(body);
  const raw =
    record(envelope?.data) ??
    record(envelope?.organization) ??
    record(body);

  return raw ? normalizeOrganization(raw as RawOrgCoreOrganization) : null;
}
