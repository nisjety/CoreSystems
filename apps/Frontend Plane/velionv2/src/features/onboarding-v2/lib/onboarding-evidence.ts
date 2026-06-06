import type {
  BrandingSignals,
  ConnectorPick,
  CrawlEvidence,
  CrawlEvidenceSnippet,
  OnboardingState,
  SafeConnectorMetadata,
  WebsitePayload,
} from "./onboarding-machine";

export const DEFAULT_ONBOARDING_ACCENT = "#111111";
const MAX_CRAWL_SNIPPETS = 12;
const MAX_SAMPLE_ENTITIES = 3;
const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const SAFE_ENTITY_COUNT_KEYS = new Set([
  "channels",
  "repos",
  "repositories",
  "pages",
  "databases",
  "sites",
  "folders",
  "files",
  "documents",
  "drives",
  "teams",
]);
const SAFE_SCOPE_KEYS = new Set([
  "channels",
  "public_channels",
  "repos",
  "readme",
  "issues",
  "wiki",
  "pages",
  "databases",
  "sharepoint",
  "onedrive",
  "teams",
  "outlook",
  "google_drive",
  "documents",
]);
const SENSITIVE_SAMPLE_PATTERNS = [
  /@/,
  /\b(subject|salary|invoice|password|secret|token|private|confidential)\b/i,
  /\.(docx?|xlsx?|pptx?|pdf|csv|msg)$/i,
];
const LEGAL_SUFFIXES = new Set(["AS", "ASA", "AB", "A/S", "LLC", "LTD", "INC", "SA"]);
const KNOWN_ACRONYMS = new Set(["AI", "API", "CRM", "ERP", "HR", "IT"]);

export function allOnboardingWebsites(state: Pick<OnboardingState, "website" | "additionalWebsites">): WebsitePayload[] {
  return [state.website, ...(state.additionalWebsites ?? [])].filter((item): item is WebsitePayload => Boolean(item?.url));
}

export function countWebsiteSources(state: Pick<OnboardingState, "website" | "additionalWebsites">): number {
  return allOnboardingWebsites(state).length;
}

export function upsertWebsiteInState(state: OnboardingState, website: WebsitePayload): OnboardingState {
  const key = websiteKey(website.url);
  if (!key) return state;
  if (!state.website) return { ...state, website };
  if (websiteKey(state.website.url) === key) return { ...state, website: { ...state.website, ...website } };

  const additional = state.additionalWebsites ?? [];
  const exists = additional.some((item) => websiteKey(item.url) === key);
  return {
    ...state,
    additionalWebsites: exists
      ? additional.map((item) => (websiteKey(item.url) === key ? { ...item, ...website } : item))
      : [...additional, website],
  };
}

export function removeWebsiteFromState(state: OnboardingState, url: string): OnboardingState {
  const key = websiteKey(url);
  if (!key) return state;
  const additional = state.additionalWebsites ?? [];
  if (websiteKey(state.website?.url) === key) {
    const [nextPrimary, ...rest] = additional;
    return { ...state, website: nextPrimary, additionalWebsites: rest.length > 0 ? rest : undefined };
  }
  const nextAdditional = additional.filter((item) => websiteKey(item.url) !== key);
  return { ...state, additionalWebsites: nextAdditional.length > 0 ? nextAdditional : undefined };
}

export function appendCrawlSnippet(
  current: CrawlEvidence | undefined,
  snippet: CrawlEvidenceSnippet,
): CrawlEvidence {
  const base = current ?? createEmptyCrawlEvidence();
  const key = snippetKey(snippet);
  const snippets = [...base.snippets.filter((item) => snippetKey(item) !== key), compactSnippet(snippet)].slice(
    -MAX_CRAWL_SNIPPETS,
  );
  return {
    ...base,
    status: base.status === "idle" ? "running" : base.status,
    snippets,
    contentTypes: mergeUnique(base.contentTypes, snippet.contentType ? [snippet.contentType] : []),
    lastUpdatedAt: new Date().toISOString(),
  };
}

export function updateCrawlEvidence(
  current: CrawlEvidence | undefined,
  patch: Partial<Omit<CrawlEvidence, "snippets" | "contentTypes" | "warnings">> & {
    contentTypes?: string[];
    warnings?: string[];
  },
): CrawlEvidence {
  const base = current ?? createEmptyCrawlEvidence();
  return {
    ...base,
    ...patch,
    pages: Math.max(base.pages, patch.pages ?? base.pages),
    elements: Math.max(base.elements, patch.elements ?? base.elements),
    contentTypes: mergeUnique(base.contentTypes, patch.contentTypes ?? []),
    warnings: mergeUnique(base.warnings, patch.warnings ?? []),
    lastUpdatedAt: patch.lastUpdatedAt ?? new Date().toISOString(),
  };
}

export function createEmptyCrawlEvidence(): CrawlEvidence {
  return {
    status: "idle",
    pages: 0,
    elements: 0,
    snippets: [],
    contentTypes: [],
    warnings: [],
    lastUpdatedAt: new Date().toISOString(),
  };
}

export function resolveBrandThemeColor(branding: BrandingSignals | undefined): string {
  const candidates = [branding?.themeColor, ...(branding?.palette ?? [])];
  let firstValid: string | null = null;
  for (const candidate of candidates) {
    const color = safeHexColor(candidate);
    if (!color) continue;
    firstValid ??= color;
    if (!isNeutralBrandColor(color)) return color;
  }
  if (!branding) return firstValid ?? DEFAULT_ONBOARDING_ACCENT;
  return DEFAULT_ONBOARDING_ACCENT;
}

export function safeHexColor(value: string | undefined): string | null {
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  return HEX_COLOR.test(trimmed) ? trimmed : null;
}

function isNeutralBrandColor(color: string): boolean {
  const { r, g, b } = hexToRgb(color);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  return max < 38 || min > 228 || max - min < 18;
}

function hexToRgb(color: string): { r: number; g: number; b: number } {
  const raw = color.replace("#", "");
  const expanded = raw.length === 3 ? raw.split("").map((char) => char + char).join("") : raw.slice(0, 6);
  return {
    r: Number.parseInt(expanded.slice(0, 2), 16),
    g: Number.parseInt(expanded.slice(2, 4), 16),
    b: Number.parseInt(expanded.slice(4, 6), 16),
  };
}

export function displayOrganizationName(value: string | undefined): string {
  const cleaned = value?.replace(/\s+/g, " ").trim();
  if (!cleaned) return "";
  return cleaned
    .split(" ")
    .map((part) => {
      const bare = part.replace(/[.,]/g, "");
      const upper = bare.toUpperCase();
      if (LEGAL_SUFFIXES.has(upper)) return upper;
      if (KNOWN_ACRONYMS.has(upper)) return upper;
      if (part.length <= 2 && part === upper) return upper;
      return part.toLocaleLowerCase("nb-NO").replace(/^\p{L}/u, (char) => char.toLocaleUpperCase("nb-NO"));
    })
    .join(" ");
}

export function buildSafeConnectorMetadata(
  connector: Pick<ConnectorPick, "id" | "label">,
  input: {
    status?: SafeConnectorMetadata["status"];
    workspaceName?: string;
    entityCounts?: Record<string, number>;
    sampleEntities?: string[];
    scopes?: string[];
    discoveredAt?: string;
    cleanupStatus?: SafeConnectorMetadata["cleanupStatus"];
  } = {},
): SafeConnectorMetadata {
  const sampleEntities = (input.sampleEntities ?? [])
    .map((item) => item.replace(/\s+/g, " ").trim())
    .filter((item) => item.length >= 2 && !SENSITIVE_SAMPLE_PATTERNS.some((pattern) => pattern.test(item)))
    .slice(0, MAX_SAMPLE_ENTITIES);
  const entityCounts = Object.fromEntries(
    Object.entries(input.entityCounts ?? {}).filter(
      ([key, value]) => SAFE_ENTITY_COUNT_KEYS.has(key.toLowerCase()) && Number.isFinite(value) && value >= 0,
    ),
  );
  const scopes = Array.from(
    new Set(
      (input.scopes ?? [])
        .map((scope) => scope.trim().toLowerCase())
        .filter((scope) => SAFE_SCOPE_KEYS.has(scope)),
    ),
  );

  return {
    status: input.status ?? "ready",
    workspaceName: sanitizeWorkspaceName(input.workspaceName) ?? defaultWorkspaceName(connector),
    entityCounts: Object.keys(entityCounts).length > 0 ? entityCounts : undefined,
    sampleEntities: sampleEntities.length > 0 ? sampleEntities : undefined,
    scopes: scopes.length > 0 ? scopes : undefined,
    discoveredAt: input.discoveredAt ?? new Date().toISOString(),
    cleanupStatus: input.cleanupStatus,
    seedDocumentId: undefined,
    sensitivity: "safe_metadata_only",
  };
}

export function connectorStatusLabel(status: SafeConnectorMetadata["status"] | undefined): "pending" | "real" | "failed" | "removed" {
  switch (status) {
    case "ready":
      return "real";
    case "failed":
      return "failed";
    case "removed":
      return "removed";
    default:
      return "pending";
  }
}

function defaultWorkspaceName(connector: Pick<ConnectorPick, "id" | "label">): string {
  return connector.label || connector.id;
}

function sanitizeWorkspaceName(value: string | undefined): string | undefined {
  const cleaned = value?.replace(/\s+/g, " ").trim();
  if (!cleaned || cleaned.length > 80 || SENSITIVE_SAMPLE_PATTERNS.some((pattern) => pattern.test(cleaned))) {
    return undefined;
  }
  return cleaned;
}

function compactSnippet(snippet: CrawlEvidenceSnippet): CrawlEvidenceSnippet {
  return {
    id: snippet.id,
    kind: snippet.kind,
    title: snippet.title.replace(/\s+/g, " ").trim().slice(0, 100),
    excerpt: snippet.excerpt?.replace(/\s+/g, " ").trim().slice(0, 180),
    url: snippet.url,
    contentType: snippet.contentType,
    source: snippet.source,
    elementCount: snippet.elementCount,
  };
}

function snippetKey(snippet: Pick<CrawlEvidenceSnippet, "kind" | "url">): string {
  return `${snippet.kind}:${websiteKey(snippet.url) || snippet.url}`;
}

function websiteKey(url: string | undefined): string {
  if (!url) return "";
  try {
    const parsed = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(url) ? url : `https://${url}`);
    return `${parsed.hostname.replace(/^www\./i, "").toLowerCase()}${parsed.pathname.replace(/\/+$/, "")}`;
  } catch {
    return url.trim().replace(/^https?:\/\//i, "").replace(/^www\./i, "").replace(/\/+$/, "").toLowerCase();
  }
}

function mergeUnique(current: string[], next: string[]): string[] {
  return Array.from(new Set([...current, ...next].map((item) => item.trim()).filter(Boolean))).slice(0, 12);
}
