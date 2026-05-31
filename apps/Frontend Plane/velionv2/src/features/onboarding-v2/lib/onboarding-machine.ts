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
  "organization",
  "website",
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

export interface WebsitePayload {
  url: string;
  agentBrief: string;
  crawlJobId?: string;
  branding?: BrandingSignals;
}

export interface ConnectorPick {
  id: string;
  label: string;
  authedAt?: string;
}

export interface PlanRecommendation {
  /** ID of one of the 5 cards. */
  planId: OnboardingPlanId;
  /** Short human-readable reason; the Model Plane (or local engine) fills this in. */
  reason: string;
  /** Quick sales-engineer summary rendered on the paywall. */
  summary?: string;
  /** When the recommendation was last computed (ISO). */
  generatedAt: string;
}

export interface OnboardingState {
  step: OnboardingStep;
  organization?: OrganizationPayload;
  website?: WebsitePayload;
  connectors: ConnectorPick[];
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

    // Steps after `organization` need the control-plane org id; if it is
    // missing from an older record, resume at the org step.
    if (requiresCreatedOrg && !organization?.id) {
      return {
        ...createInitialState(),
        ...parsed,
        organization,
        step: "organization",
        connectors: Array.isArray(parsed.connectors) ? parsed.connectors : [],
      };
    }

    return {
      ...createInitialState(),
      ...parsed,
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
  setOrganization: (org: OrganizationPayload) => void;
  setWebsite: (website: WebsitePayload) => void;
  addConnector: (connector: ConnectorPick) => void;
  removeConnector: (id: string) => void;
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

  const setOrganization = useCallback(
    (org: OrganizationPayload) => persist((prev) => ({ ...prev, organization: org })),
    [persist],
  );

  const setWebsite = useCallback(
    (website: WebsitePayload) => persist((prev) => ({ ...prev, website })),
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
    setOrganization,
    setWebsite,
    addConnector,
    removeConnector,
    setRecommendation,
    markIntroPlayed,
    reset,
  };
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
