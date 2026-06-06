import { slugify, type BrregEnhet } from "@/lib/services/brreg-service"

/**
 * Client-side onboarding orchestration. Each call goes through a same-origin
 * Next.js route that forwards to the Control Plane:
 *   - /api/org/*  -> org-core (+ billing-core fan-out) via the gateway
 *   - /api/v1/*   -> user-core
 */

export type OnboardingPlanId =
  | "free"
  | "trial"
  | "hobby"
  | "standard"
  | "pro"
  | "enterprise"

export type CreatedOrganization = {
  id: string
  name: string
  slug: string
  plan: string
  orgNumber?: string
  verificationStatus?: string
}

type OrganizationBranding = {
  appleTouchIcon?: string
  favicon?: string
  logoCandidate?: string
  palette?: string[]
  siteName?: string
  themeColor?: string
}

type RawOrgResponse = {
  id: string
  name: string
  slug?: string
  plan?: string
  org_number?: string
  orgNumber?: string
  verification_status?: string
  verificationStatus?: string
}

const FREE_PLANS: ReadonlySet<OnboardingPlanId> = new Set<OnboardingPlanId>([
  "free",
  "trial",
])

function brandingMetadata(branding?: OrganizationBranding) {
  if (!branding) return undefined

  return {
    onboarding_branding: {
      site_name: branding.siteName,
      theme_color: branding.themeColor,
      favicon: branding.favicon,
      logo_candidate: branding.logoCandidate,
      apple_touch_icon: branding.appleTouchIcon,
      palette: branding.palette?.slice(0, 8),
    },
  }
}

export function isPaidPlan(plan: OnboardingPlanId): boolean {
  return !FREE_PLANS.has(plan)
}

export class OnboardingServiceError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message)
  }
}

async function readServiceError(response: Response): Promise<OnboardingServiceError> {
  const body = (await response.json().catch(() => null)) as
    | { error?: { message?: string } | string }
    | null
  const rawMessage =
    typeof body?.error === "object" ? body?.error?.message : body?.error

  return new OnboardingServiceError(
    response.status,
    rawMessage ?? `Request failed (${response.status})`,
  )
}

async function requestJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, {
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    ...init,
  })

  if (!response.ok) {
    throw await readServiceError(response)
  }

  return response.json() as Promise<T>
}

function toCreatedOrganization(raw: RawOrgResponse): CreatedOrganization {
  return {
    id: raw.id,
    name: raw.name,
    slug: raw.slug ?? "",
    plan: raw.plan ?? "free",
    orgNumber: raw.orgNumber ?? raw.org_number,
    verificationStatus: raw.verificationStatus ?? raw.verification_status,
  }
}

/**
 * Create the org in org-core. Passing `brregData` persists the verified BRREG
 * snapshot. org-core publishes `organization.created` -> billing-core
 * auto-provisions a free account (eventually consistent).
 */
export async function createOrganization(input: {
  name: string
  plan?: OnboardingPlanId
  orgNumber?: string
  brregData?: BrregEnhet | null
  branding?: OrganizationBranding
}): Promise<CreatedOrganization> {
  const raw = await requestJson<RawOrgResponse>("/api/org/orgs", {
    method: "POST",
    body: JSON.stringify({
      name: input.name,
      slug: slugify(input.name),
      plan: input.plan ?? "free",
      org_number: input.orgNumber,
      brreg_data: input.brregData ?? undefined,
      metadata: brandingMetadata(input.branding),
    }),
  })
  return toCreatedOrganization(raw)
}

export async function organizationExists(orgId: string): Promise<boolean> {
  const raw = await requestJson<RawOrgResponse[]>("/api/org/orgs/me", {
    method: "GET",
    cache: "no-store",
  })

  return raw.some((organization) => organization.id === orgId)
}

/** Set a non-checkout plan (free/trial) directly; publishes plan.changed. */
export async function setOrganizationPlan(
  orgId: string,
  plan: OnboardingPlanId,
  onboarding?: Record<string, unknown>,
): Promise<CreatedOrganization> {
  const raw = await requestJson<RawOrgResponse>(`/api/org/orgs/${orgId}/plan`, {
    method: "POST",
    body: JSON.stringify({ plan, reason: "onboarding", onboarding }),
  })
  return toCreatedOrganization(raw)
}

/** Start a Stripe checkout for a paid plan; returns the redirect URL. */
export async function startCheckout(
  orgId: string,
  plan: OnboardingPlanId,
  opts: { successUrl: string; cancelUrl: string },
): Promise<{ id?: string; url?: string }> {
  return requestJson<{ id?: string; url?: string }>(
    `/api/org/orgs/${orgId}/checkout-session`,
    {
      method: "POST",
      body: JSON.stringify({
        plan,
        successUrl: opts.successUrl,
        cancelUrl: opts.cancelUrl,
      }),
    },
  )
}

/** Best-effort profile enrichment — never blocks onboarding progression. */
export async function updateProfile(input: {
  name?: string
  website?: string
  brief?: string
}): Promise<void> {
  await fetch("/api/v1/users/me", {
    method: "PATCH",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      name: input.name,
      metadata: {
        website: input.website,
        onboarding_brief: input.brief,
      },
    }),
  }).catch(() => undefined)
}

/** Best-effort committed website ingest once an org exists. */
export async function startWebsiteIngest(input: {
  orgId: string
  url: string
  brief?: string
}): Promise<boolean> {
  try {
    const response = await fetch("/api/onboarding/website-ingest", {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        orgId: input.orgId,
        url: input.url,
        brief: input.brief,
      }),
    })
    return response.ok
  } catch {
    return false
  }
}

/** Capstone: mark onboarding complete in user-core (with local fallback). */
export async function completeOnboarding(
  payload?: Record<string, unknown>,
): Promise<void> {
  await requestJson("/api/v1/onboarding/status", {
    method: "POST",
    body: JSON.stringify(payload ?? {}),
  })
}
