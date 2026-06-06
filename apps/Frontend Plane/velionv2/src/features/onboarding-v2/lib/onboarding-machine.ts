"use client";

/**
 * Onboarding · state machine + shapes (ported from velion v1
 * `components/auth/onboarding/state/{types,useOnboardingMachine}.ts`).
 *
 * The wizard runs as a state machine. The current step + every collected
 * field is persisted to `localStorage` so a refresh / tab close / accidental
 * URL change resumes at the same place.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export const ONBOARDING_STEPS = [
  "post-signin",
  "website",
  "organization",
  "connect",
  "social-proof",
  "paywall",
  "assembly",
] as const;

export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

export type OnboardingPlanId =
  | "trial"
  | "hobby"
  | "standard"
  | "pro"
  | "enterprise";

export type OrganizationSize =
  | "solo"
  | "small"
  | "medium"
  | "large"
  | "enterprise";

export interface OrganizationPayload {
  /** Control-plane org id returned by org-core once the org is created. */
  id?: string;
  name: string;
  slug?: string;
  /** Selected Velion plan card from the paywall step. */
  plan?: OnboardingPlanId;
  size?: OrganizationSize;
  brregOrgNumber?: string;
  /** Exact employee count from Brreg when available. */
  employeeCount?: number;
}

/**
 * Brand signals extracted from the seed page by the crawl preview and
 * forwarded as `branding` events. Every field is best-effort; downstream
 * renderers must degrade gracefully when missing.
 */
export interface BrandingSignals {
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

export type CrawlEvidenceStatus =
  | "idle"
  | "starting"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type CrawlEvidenceSnippetKind = "text" | "image" | "file" | "link";

export interface CrawlEvidenceSnippet {
  id: string;
  kind: CrawlEvidenceSnippetKind;
  title: string;
  excerpt?: string;
  url: string;
  contentType?: string;
  source?: "seed" | "live";
  elementCount?: number;
}

export interface CrawlEvidence {
  status: CrawlEvidenceStatus;
  pages: number;
  elements: number;
  latestUrl?: string;
  latestTitle?: string;
  snippets: CrawlEvidenceSnippet[];
  contentTypes: string[];
  warnings: string[];
  lastUpdatedAt: string;
  seedStatus?: "pending" | "ready" | "failed" | "removed";
}

export interface WebsitePayload {
  url: string;
  agentBrief: string;
  crawlJobId?: string;
  branding?: BrandingSignals;
  crawlEvidence?: CrawlEvidence;
}

export interface SafeConnectorMetadata {
  status: "pending" | "ready" | "failed" | "removed";
  workspaceName?: string;
  entityCounts?: Record<string, number>;
  sampleEntities?: string[];
  scopes?: string[];
  discoveredAt?: string;
  sensitivity: "safe_metadata_only";
  cleanupStatus?: "idle" | "requested" | "completed" | "failed";
  seedDocumentId?: string;
}

export interface ConnectorPick {
  id: string;
  label: string;
  authedAt?: string;
  metadata?: SafeConnectorMetadata;
}

export interface OnboardingBrandTheme {
  mode: "velion" | "brand";
  primaryColor: string;
  selectedAt: string;
  saveStatus?: "idle" | "saving" | "saved" | "failed";
}

export interface PlanRecommendation {
  /** ID of one of the 5 cards. */
  planId: OnboardingPlanId;
  /** Short human-readable reason; the Model Plane (or local engine) fills this in. */
  reason: string;
  /** Quick sales-engineer summary rendered on the paywall. */
  summary?: string;
  /** Concrete signals Velion used when making the recommendation. */
  proofPoints?: string[];
  /** Short scope bullets derived from the website, integrations and graph. */
  scopeSignals?: string[];
  /** Likely first improvements Velion can make for this organization. */
  opportunities?: string[];
  /** Rough, non-guaranteed launch outcomes shown as a proof of concept. */
  expectedOutcomes?: Array<{ label: string; value: string; detail?: string }>;
  proofOfConcept?: {
    companyIdentity?: string[];
    learnedSignals?: string[];
    likelyIntents?: string[];
    nextActions?: string[];
    operationalImpact?: Array<{ label: string; value: string; detail?: string }>;
    recommendationFit?: string[];
  };
  /** When the recommendation was last computed (ISO). */
  generatedAt: string;
  /** Where the copy came from. Used so the UI can avoid presenting fallback text as model output. */
  source?: "model" | "local";
  modelVersion?: string;
  confidence?: number;
}

export interface OnboardingState {
  step: OnboardingStep;
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  additionalWebsites?: WebsitePayload[];
  connectors: ConnectorPick[];
  brandTheme?: OnboardingBrandTheme;
  recommendation?: PlanRecommendation;
  /** Set true once `post-signin` intro has played through once. */
  introPlayed: boolean;
  /** Started timestamp (ms since epoch). Used for analytics. */
  startedAt: number;
}

function createInitialState(): OnboardingState {
  return {
    step: "post-signin",
    connectors: [],
    introPlayed: false,
    startedAt: Date.now(),
  };
}

/** localStorage key. Bumped when the wire shape changes so stale state is dropped. */
export const STORAGE_KEY = "velion.onboarding.v1";

function readStored(): OnboardingState | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<OnboardingState>;
    if (!parsed.step || !ONBOARDING_STEPS.includes(parsed.step)) {
      return null;
    }
    const stepIndex = ONBOARDING_STEPS.indexOf(parsed.step);
    const requiresCreatedOrg = stepIndex > ONBOARDING_STEPS.indexOf("organization");
    const organization =
      parsed.organization && typeof parsed.organization === "object"
        ? parsed.organization
        : undefined;
    const website =
      parsed.website && typeof parsed.website === "object" ? parsed.website : undefined;

    if (parsed.step === "organization" && !organization?.id && !website?.url) {
      return {
        ...createInitialState(),
        ...parsed,
        website,
        organization,
        step: "website",
        connectors: Array.isArray(parsed.connectors) ? parsed.connectors : [],
      };
    }

    // Steps after `organization` need the control-plane org id; if it is
    // missing from an older record, resume at the org step.
    if (requiresCreatedOrg && !organization?.id) {
      return {
        ...createInitialState(),
        ...parsed,
        website,
        organization,
        step: "organization",
        connectors: Array.isArray(parsed.connectors) ? parsed.connectors : [],
      };
    }

    return {
      ...createInitialState(),
      ...parsed,
      website,
      organization,
      connectors: Array.isArray(parsed.connectors) ? parsed.connectors : [],
    };
  } catch {
    return null;
  }
}

function writeStored(state: OnboardingState): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Safari private mode + Lockdown Mode reject setItem — resume-on-refresh
    // is lost but the wizard still works.
    window.console?.warn(
      "[onboarding] localStorage write failed; resume-on-refresh disabled for this session",
    );
  }
}

export interface OnboardingMachine {
  state: OnboardingState;
  hydrated: boolean;
  goTo: (step: OnboardingStep) => void;
  next: () => void;
  back: () => void;
  invalidateOrganization: () => void;
  setOrganization: (org: OrganizationPayload) => void;
  setWebsite: (website: WebsitePayload) => void;
  addWebsite: (website: WebsitePayload) => void;
  removeWebsite: (url: string) => void;
  addConnector: (connector: ConnectorPick) => void;
  removeConnector: (id: string) => void;
  updateConnectorMetadata: (id: string, metadata: SafeConnectorMetadata) => void;
  setBrandTheme: (theme: OnboardingBrandTheme) => void;
  setRecommendation: (rec: PlanRecommendation) => void;
  markIntroPlayed: () => void;
  reset: () => void;
}

export function useOnboardingMachine(): OnboardingMachine {
  const [state, setState] = useState<OnboardingState>(createInitialState);
  const [hydrated, setHydrated] = useState(false);
  const initialised = useRef(false);

  useEffect(() => {
    if (initialised.current) return;
    initialised.current = true;

    const hydrateFrame = window.requestAnimationFrame(() => {
      const stored = readStored();
      if (stored) setState(stored);
      setHydrated(true);
    });

    return () => window.cancelAnimationFrame(hydrateFrame);
  }, []);

  const persist = useCallback(
    (updater: (prev: OnboardingState) => OnboardingState) => {
      setState((prev) => {
        const nextState = updater(prev);
        writeStored(nextState);
        return nextState;
      });
    },
    [],
  );

  const goTo = useCallback(
    (step: OnboardingStep) => persist((prev) => ({ ...prev, step })),
    [persist],
  );

  const next = useCallback(() => {
    persist((prev) => {
      const idx = ONBOARDING_STEPS.indexOf(prev.step);
      if (idx < 0 || idx >= ONBOARDING_STEPS.length - 1) return prev;
      return { ...prev, step: ONBOARDING_STEPS[idx + 1] };
    });
  }, [persist]);

  const back = useCallback(() => {
    persist((prev) => {
      const idx = ONBOARDING_STEPS.indexOf(prev.step);
      if (idx <= 0) return prev;
      return { ...prev, step: ONBOARDING_STEPS[idx - 1] };
    });
  }, [persist]);

  const invalidateOrganization = useCallback(() => {
    persist((prev) => ({
      ...prev,
      step: "organization",
      organization: prev.organization
        ? {
            ...prev.organization,
            id: undefined,
            plan: undefined,
          }
        : prev.organization,
      connectors: [],
    }));
  }, [persist]);

  const setOrganization = useCallback(
    (org: OrganizationPayload) => persist((prev) => ({ ...prev, organization: org })),
    [persist],
  );

  const setWebsite = useCallback(
    (website: WebsitePayload) => persist((prev) => ({ ...prev, website })),
    [persist],
  );

  const addWebsite = useCallback(
    (website: WebsitePayload) =>
      persist((prev) => {
        const same = (a?: string, b?: string) => normalizeUrlForCompare(a) === normalizeUrlForCompare(b);
        if (!prev.website) return { ...prev, website };
        if (same(prev.website.url, website.url)) return { ...prev, website: { ...prev.website, ...website } };
        const additional = prev.additionalWebsites ?? [];
        const exists = additional.some((item) => same(item.url, website.url));
        return {
          ...prev,
          additionalWebsites: exists
            ? additional.map((item) => (same(item.url, website.url) ? { ...item, ...website } : item))
            : [...additional, website],
        };
      }),
    [persist],
  );

  const removeWebsite = useCallback(
    (url: string) =>
      persist((prev) => {
        const key = normalizeUrlForCompare(url);
        if (!key) return prev;
        const additional = prev.additionalWebsites ?? [];
        const primaryKey = normalizeUrlForCompare(prev.website?.url);
        if (primaryKey === key) {
          const [nextPrimary, ...rest] = additional;
          return { ...prev, website: nextPrimary, additionalWebsites: rest.length > 0 ? rest : undefined };
        }
        const nextAdditional = additional.filter((item) => normalizeUrlForCompare(item.url) !== key);
        return { ...prev, additionalWebsites: nextAdditional.length > 0 ? nextAdditional : undefined };
      }),
    [persist],
  );

  const addConnector = useCallback(
    (connector: ConnectorPick) => {
      persist((prev) => {
        if (prev.connectors.some((c) => c.id === connector.id)) {
          return {
            ...prev,
            connectors: prev.connectors.map((c) =>
              c.id === connector.id ? { ...c, ...connector } : c,
            ),
          };
        }
        return { ...prev, connectors: [...prev.connectors, connector] };
      });
    },
    [persist],
  );

  const removeConnector = useCallback(
    (id: string) =>
      persist((prev) => ({
        ...prev,
        connectors: prev.connectors.filter((c) => c.id !== id),
      })),
    [persist],
  );

  const updateConnectorMetadata = useCallback(
    (id: string, metadata: SafeConnectorMetadata) =>
      persist((prev) => ({
        ...prev,
        connectors: prev.connectors.map((connector) =>
          connector.id === id ? { ...connector, metadata } : connector,
        ),
      })),
    [persist],
  );

  const setBrandTheme = useCallback(
    (theme: OnboardingBrandTheme) => persist((prev) => ({ ...prev, brandTheme: theme })),
    [persist],
  );

  const setRecommendation = useCallback(
    (rec: PlanRecommendation) => persist((prev) => ({ ...prev, recommendation: rec })),
    [persist],
  );

  const markIntroPlayed = useCallback(() => {
    persist((prev) => (prev.introPlayed ? prev : { ...prev, introPlayed: true }));
  }, [persist]);

  const reset = useCallback(() => {
    setState(createInitialState());
    if (typeof window !== "undefined") {
      try {
        window.localStorage.removeItem(STORAGE_KEY);
      } catch {
        // Swallow — same Safari private-mode case as the writer.
      }
    }
  }, []);

  return {
    state,
    hydrated,
    goTo,
    next,
    back,
    invalidateOrganization,
    setOrganization,
    setWebsite,
    addWebsite,
    removeWebsite,
    addConnector,
    removeConnector,
    updateConnectorMetadata,
    setBrandTheme,
    setRecommendation,
    markIntroPlayed,
    reset,
  };
}

function normalizeUrlForCompare(value: string | undefined): string {
  if (!value) return "";
  try {
    const parsed = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(value) ? value : `https://${value}`);
    return `${parsed.protocol}//${parsed.hostname.replace(/^www\./i, "").toLowerCase()}${parsed.pathname.replace(/\/+$/, "")}`;
  } catch {
    return value.trim().replace(/^https?:\/\//i, "").replace(/^www\./i, "").replace(/\/+$/, "").toLowerCase();
  }
}

/** Derive V1's org "size" enum from a BRREG employee count. */
export function sizeFromEmployees(count: number | undefined): OrganizationSize | undefined {
  if (count == null || count <= 0) return undefined;
  if (count === 1) return "solo";
  if (count <= 10) return "small";
  if (count <= 50) return "medium";
  if (count <= 250) return "large";
  return "enterprise";
}
