import { NextRequest, NextResponse } from "next/server"
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  ControlPlaneAuthError,
  requireSession,
} from "../../_lib/control-plane-auth"
import { emitAuditEvent } from "@/lib/integrations/audit-core"
import type { RequestActor } from "@/lib/integrations/request-actor"

export const runtime = "nodejs"
export const dynamic = "force-dynamic"

const ORG_SERVICE_URL = (
  process.env.ORG_SERVICE_URL ||
  process.env.ORG_CORE_URL ||
  "http://org-core:8080"
).replace(/\/+$/, "")
const BILLING_SERVICE_URL = (
  process.env.BILLING_SERVICE_URL ||
  process.env.BILLING_CORE_URL ||
  "http://billing-core-service:3014"
).replace(/\/+$/, "")

const UPSTREAM_TIMEOUT_MS = 10_000

type BillingAccountResponse = {
  org_id: string
  plan: string
  subscription_state: string
  credits?: number
  products?: Record<string, boolean>
  feature_flags?: Record<string, boolean>
  entitlements?: Record<string, boolean>
  quota_limits?: Record<string, number>
  provider_customer_id?: Record<string, string>
  metadata?: Record<string, unknown>
  updated_at: string
  created_at: string
}

type OrgCoreOrganizationResponse = {
  id: string
  name: string
  slug?: string
  plan?: string
  status?: string
  primary_domain?: string
  region?: string
  default_locale?: string
  metadata?: Record<string, unknown>
  created_at?: string
  updated_at?: string
  org_number?: string
  verification_status?: string
  brreg_data?: Record<string, unknown>
}

type OrgCoreMemberResponse = {
  id: string
  org_id?: string
  user_id?: string
  role?: string
  status?: string
  invited_by?: string
  joined_at?: string
  updated_at?: string
}

type BillingQuotaStatus = {
  org_id: string
  metric: string
  limit: number
  used: number
  remaining?: number
  is_exceeded?: boolean
  utilization?: number
}

type BillingQuotaStatusResponse = {
  quota?: BillingQuotaStatus
}

const ORG_PLAN_VALUES = [
  "free",
  "trial",
  "hobby",
  "standard",
  "pro",
  "enterprise",
] as const

type OrgPlan = (typeof ORG_PLAN_VALUES)[number]

type PlanPostBody = {
  plan?: string
  reason?: string
  onboarding?: Record<string, unknown>
}

const isOrgPlan = (value: string): value is OrgPlan =>
  (ORG_PLAN_VALUES as readonly string[]).includes(value)

// Trust the canonical env URL. If misconfigured, fail loudly so the operator notices.
const orgBaseUrl = () => ORG_SERVICE_URL
const billingBaseUrl = () => BILLING_SERVICE_URL

// SECURITY: validate every catch-all segment before it is concatenated into an
// upstream URL. Blocks path traversal (`..`) and any non-safe character that
// could alter the request target. Base authority is fixed from env, never user
// input. Rejected requests fail with 400 via authErrorResponse.
const SAFE_PATH_SEGMENT = /^[A-Za-z0-9._-]+$/

function assertSafePathSegments(path: string[]): void {
  for (const segment of path) {
    if (
      !segment ||
      segment === "." ||
      segment === ".." ||
      !SAFE_PATH_SEGMENT.test(segment)
    ) {
      throw new ControlPlaneAuthError(400, "invalid_path", "Invalid request path.")
    }
  }
}

// SECURITY: never forward an upstream error body to the client (it may contain
// stack traces / internal config). Log server-side, return a fixed envelope.
function upstreamErrorResponse(status: number, upstreamBody: unknown) {
  if (upstreamBody) {
    console.error("[org-gateway] upstream error", status, upstreamBody)
  }
  return NextResponse.json(
    {
      error: {
        code: "upstream_error",
        message: `Upstream service error (${status}).`,
      },
    },
    { status: status || 502 },
  )
}

const appendQuery = (path: string, request: NextRequest) => {
  const query = request.nextUrl.searchParams.toString()
  return query ? `${path}?${query}` : path
}

const buildOrgPath = (request: NextRequest, pathSegments: string[] = []) =>
  appendQuery(`/${pathSegments.join("/")}`, request)

const isBillingPath = (pathSegments: string[]) =>
  pathSegments.length === 3 &&
  pathSegments[0] === "orgs" &&
  pathSegments[2] === "billing"

const isQuotaCollectionPath = (pathSegments: string[]) =>
  pathSegments.length === 3 &&
  pathSegments[0] === "orgs" &&
  pathSegments[2] === "quotas"

const isQuotaItemPath = (pathSegments: string[]) =>
  pathSegments.length === 4 &&
  pathSegments[0] === "orgs" &&
  pathSegments[2] === "quotas"

const isPlanPath = (pathSegments: string[]) =>
  pathSegments.length === 3 &&
  pathSegments[0] === "orgs" &&
  pathSegments[2] === "plan"

const isCheckoutSessionPath = (pathSegments: string[]) =>
  pathSegments.length === 3 &&
  pathSegments[0] === "orgs" &&
  pathSegments[2] === "checkout-session"

async function readJson(response: Response) {
  const text = await response.text()
  if (!text.trim()) {
    return null
  }

  try {
    return JSON.parse(text) as unknown
  } catch {
    throw new Error(`Invalid JSON from upstream: ${text}`)
  }
}

async function fetchUpstream(baseUrl: string, path: string, init: RequestInit) {
  return fetch(`${baseUrl}${path}`, {
    ...init,
    cache: "no-store",
    signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS),
  })
}

const buildHeaders = async (request: NextRequest) => {
  const session = await requireSession(request)
  return buildControlPlaneHeaders(request, session)
}

// Build a RequestActor from the cached session for fire-and-forget audit writes.
const auditActor = async (request: NextRequest): Promise<RequestActor> => {
  const session = await requireSession(request) // cached per request (WeakMap)
  return {
    userId: session.user.id,
    email: session.user.email ?? undefined,
    name: session.user.name ?? undefined,
    cookieHeader: request.headers.get("cookie") ?? undefined,
  }
}

// Emit an org-scoped audit event from a (cloned) upstream response body.
async function emitOrgEventFromResponse(
  request: NextRequest,
  response: Response,
  event: string,
) {
  try {
    const data = (await response.json()) as { id?: string; name?: string } | null
    if (data?.id) {
      await emitAuditEvent(await auditActor(request), {
        orgId: data.id,
        event,
        subject: data.id,
        details: data.name ? { name: data.name } : undefined,
      })
    }
  } catch {
    // best-effort
  }
}

function normalizeBillingAccount(account: BillingAccountResponse) {
  const providerIds = account.provider_customer_id ?? {}
  const metadata = account.metadata ?? {}

  return {
    orgId: account.org_id,
    plan: account.plan || "free",
    subscriptionStatus: account.subscription_state || "active",
    stripeCustomerId:
      providerIds.stripe_customer_id || providerIds.stripe || undefined,
    stripeSubscriptionId:
      providerIds.stripe_subscription_id || providerIds.stripe_subscription || undefined,
    paymentMethodId:
      providerIds.payment_method_id || providerIds.payment_method || undefined,
    billingEmail:
      typeof metadata.billing_email === "string" ? metadata.billing_email : undefined,
    currentPeriodStart:
      typeof metadata.current_period_start === "string"
        ? metadata.current_period_start
        : undefined,
    currentPeriodEnd:
      typeof metadata.current_period_end === "string"
        ? metadata.current_period_end
        : undefined,
    entitlements: account.entitlements ?? {},
    quotaLimits: account.quota_limits ?? {},
    credits: typeof account.credits === "number" ? account.credits : 0,
    createdAt: account.created_at,
    updatedAt: account.updated_at,
  }
}

function normalizeOrganization(org: OrgCoreOrganizationResponse) {
  return {
    id: org.id,
    name: org.name,
    slug: org.slug ?? "",
    plan: org.plan ?? "free",
    status: org.status ?? "active",
    primaryDomain: org.primary_domain ?? undefined,
    region: org.region ?? "eu",
    defaultLocale: org.default_locale ?? "nb-NO",
    ownerUserId: "",
    metadata: org.metadata ?? {},
    createdAt: org.created_at ?? "",
    updatedAt: org.updated_at ?? org.created_at ?? "",
    orgNumber: org.org_number ?? undefined,
    verificationStatus:
      org.verification_status === "verified" ? "verified" : "unverified",
    brregData: org.brreg_data ?? undefined,
  }
}

function normalizeOrganizationMember(member: OrgCoreMemberResponse) {
  return {
    id: member.id,
    organizationId: member.org_id ?? "",
    userId: member.user_id ?? "",
    role: member.role ?? "member",
    invitedBy: member.invited_by ?? undefined,
    joinedAt: member.joined_at ?? "",
    status:
      member.status === "invited" || member.status === "suspended"
        ? member.status
        : "active",
  }
}

function normalizeQuotaStatus(quota: BillingQuotaStatus, updatedAt: string) {
  return {
    orgId: quota.org_id,
    quotaKey: quota.metric,
    quotaValue: quota.used,
    quotaLimit: quota.limit,
    updatedAt,
  }
}

async function fetchBillingAccount(headers: Record<string, string>, orgId: string) {
  const response = await fetchUpstream(
    billingBaseUrl(),
    `/api/v1/billing/orgs/${orgId}/account`,
    {
      method: "GET",
      headers,
    },
  )

  const data = (await readJson(response)) as BillingAccountResponse | null
  return { response, data }
}

function normalizeOrgProxyPayload(pathSegments: string[], data: unknown) {
  if (!data) {
    return data
  }

  const isOrgCollection = pathSegments.length === 1 && pathSegments[0] === "orgs"

  const isOrgSelfCollection =
    pathSegments.length === 2 &&
    pathSegments[0] === "orgs" &&
    pathSegments[1] === "me"

  const isOrgItem =
    pathSegments.length === 2 &&
    pathSegments[0] === "orgs" &&
    pathSegments[1] !== "me"

  const isMemberCollection =
    pathSegments.length === 3 &&
    pathSegments[0] === "orgs" &&
    pathSegments[2] === "members"

  if ((isOrgCollection || isOrgSelfCollection) && Array.isArray(data)) {
    return data.map((item) =>
      normalizeOrganization(item as OrgCoreOrganizationResponse),
    )
  }

  if (isOrgItem && typeof data === "object" && !Array.isArray(data)) {
    return normalizeOrganization(data as OrgCoreOrganizationResponse)
  }

  if (isMemberCollection && typeof data === "object" && data !== null) {
    const payload = data as { members?: OrgCoreMemberResponse[]; count?: number }
    return {
      members: Array.isArray(payload.members)
        ? payload.members.map(normalizeOrganizationMember)
        : [],
      count: typeof payload.count === "number" ? payload.count : 0,
    }
  }

  return data
}

async function fetchQuotaStatus(
  headers: Record<string, string>,
  orgId: string,
  metric: string,
) {
  const response = await fetchUpstream(
    billingBaseUrl(),
    `/api/v1/billing/orgs/${orgId}/quotas/${metric}`,
    {
      method: "GET",
      headers,
    },
  )

  const data = (await readJson(response)) as BillingQuotaStatusResponse | null
  return { response, data }
}

async function handleBillingGet(headers: Record<string, string>, orgId: string) {
  const { response, data } = await fetchBillingAccount(headers, orgId)

  if (!response.ok || !data) {
    return upstreamErrorResponse(response.status, data)
  }

  return NextResponse.json(normalizeBillingAccount(data), { status: response.status })
}

async function handleQuotaCollectionGet(
  headers: Record<string, string>,
  orgId: string,
) {
  const { response, data } = await fetchBillingAccount(headers, orgId)

  if (!response.ok || !data) {
    return upstreamErrorResponse(response.status, data)
  }

  const quotaKeys = Object.keys(data.quota_limits ?? {})
  if (quotaKeys.length === 0) {
    return NextResponse.json([], { status: 200 })
  }

  const quotas = await Promise.all(
    quotaKeys.map(async (metric) => {
      const accountLimit = Number(data.quota_limits?.[metric] ?? 0)
      const result = await fetchQuotaStatus(headers, orgId, metric)
      if (result.response.ok && result.data?.quota) {
        return normalizeQuotaStatus(
          {
            ...result.data.quota,
            limit: Number.isFinite(accountLimit) ? accountLimit : result.data.quota.limit,
          },
          data.updated_at,
        )
      }

      return normalizeQuotaStatus(
        {
          org_id: orgId,
          metric,
          limit: accountLimit,
          used: 0,
        },
        data.updated_at,
      )
    }),
  )

  return NextResponse.json(quotas, { status: 200 })
}

async function handleBillingPut(
  request: NextRequest,
  headers: Record<string, string>,
  orgId: string,
) {
  const body = (await request.json()) as Record<string, unknown>
  const existing = await fetchBillingAccount(headers, orgId)

  if (!existing.response.ok || !existing.data) {
    return upstreamErrorResponse(existing.response.status, existing.data)
  }

  const metadata = { ...(existing.data.metadata ?? {}) }
  const providerIds = { ...(existing.data.provider_customer_id ?? {}) }

  if ("billingEmail" in body) {
    if (typeof body.billingEmail === "string" && body.billingEmail.trim()) {
      metadata.billing_email = body.billingEmail
    } else {
      delete metadata.billing_email
    }
  }

  if ("currentPeriodStart" in body) {
    if (typeof body.currentPeriodStart === "string" && body.currentPeriodStart.trim()) {
      metadata.current_period_start = body.currentPeriodStart
    } else {
      delete metadata.current_period_start
    }
  }

  if ("currentPeriodEnd" in body) {
    if (typeof body.currentPeriodEnd === "string" && body.currentPeriodEnd.trim()) {
      metadata.current_period_end = body.currentPeriodEnd
    } else {
      delete metadata.current_period_end
    }
  }

  if ("stripeCustomerId" in body) {
    if (typeof body.stripeCustomerId === "string" && body.stripeCustomerId.trim()) {
      providerIds.stripe = body.stripeCustomerId
    } else {
      delete providerIds.stripe
    }
  }

  if ("stripeSubscriptionId" in body) {
    if (
      typeof body.stripeSubscriptionId === "string" &&
      body.stripeSubscriptionId.trim()
    ) {
      providerIds.stripe_subscription = body.stripeSubscriptionId
    } else {
      delete providerIds.stripe_subscription
    }
  }

  if ("paymentMethodId" in body) {
    if (typeof body.paymentMethodId === "string" && body.paymentMethodId.trim()) {
      providerIds.payment_method = body.paymentMethodId
    } else {
      delete providerIds.payment_method
    }
  }

  const payload: BillingAccountResponse = {
    ...existing.data,
    org_id: orgId,
    subscription_state:
      typeof body.subscriptionStatus === "string" && body.subscriptionStatus.trim()
        ? body.subscriptionStatus
        : existing.data.subscription_state,
    provider_customer_id: providerIds,
    metadata,
  }

  const response = await fetchUpstream(
    billingBaseUrl(),
    `/api/v1/billing/orgs/${orgId}/account`,
    {
      method: "PUT",
      headers,
      body: JSON.stringify(payload),
    },
  )

  const data = (await readJson(response)) as BillingAccountResponse | null

  if (!response.ok || !data) {
    return upstreamErrorResponse(response.status, data)
  }

  return NextResponse.json(normalizeBillingAccount(data), { status: response.status })
}

async function handleQuotaItemPut(
  request: NextRequest,
  headers: Record<string, string>,
  orgId: string,
  quotaKey: string,
) {
  const body = (await request.json()) as { value?: number }
  const nextValue = Number(body.value)

  if (!Number.isFinite(nextValue) || nextValue < 0) {
    return NextResponse.json(
      { error: "Quota value must be a non-negative number" },
      { status: 400 },
    )
  }

  const existing = await fetchBillingAccount(headers, orgId)
  if (!existing.response.ok || !existing.data) {
    return upstreamErrorResponse(existing.response.status, existing.data)
  }

  const payload: BillingAccountResponse = {
    ...existing.data,
    org_id: orgId,
    quota_limits: {
      ...(existing.data.quota_limits ?? {}),
      [quotaKey]: nextValue,
    },
  }

  const updateResponse = await fetchUpstream(
    billingBaseUrl(),
    `/api/v1/billing/orgs/${orgId}/account`,
    {
      method: "PUT",
      headers,
      body: JSON.stringify(payload),
    },
  )

  if (!updateResponse.ok) {
    const updateData = await readJson(updateResponse)
    return upstreamErrorResponse(updateResponse.status, updateData)
  }

  const quotaResponse = await fetchQuotaStatus(headers, orgId, quotaKey)
  if (quotaResponse.response.ok && quotaResponse.data?.quota) {
    return NextResponse.json(
      normalizeQuotaStatus(quotaResponse.data.quota, payload.updated_at),
      { status: 200 },
    )
  }

  return NextResponse.json(
    normalizeQuotaStatus(
      {
        org_id: orgId,
        metric: quotaKey,
        limit: nextValue,
        used: 0,
      },
      payload.updated_at,
    ),
    { status: 200 },
  )
}

async function handlePlanPost(
  request: NextRequest,
  headers: Record<string, string>,
  orgId: string,
) {
  const body = (await request.json()) as PlanPostBody
  const nextPlan =
    typeof body.plan === "string" ? body.plan.trim().toLowerCase() : ""

  if (!isOrgPlan(nextPlan)) {
    return NextResponse.json(
      { error: `Plan must be one of ${ORG_PLAN_VALUES.join(", ")}` },
      { status: 400 },
    )
  }

  const orgResponse = await fetchUpstream(orgBaseUrl(), `/orgs/${orgId}/plan`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      plan: nextPlan,
      reason: typeof body.reason === "string" ? body.reason : undefined,
    }),
  })

  const orgData = await readJson(orgResponse)
  if (!orgResponse.ok || !orgData) {
    return upstreamErrorResponse(orgResponse.status, orgData)
  }

  const existingBilling = await fetchBillingAccount(headers, orgId)
  if (existingBilling.response.ok && existingBilling.data) {
    const existingOnboardingPlan = existingBilling.data.metadata?.onboarding_plan
    const previousOnboardingMetadata =
      existingOnboardingPlan &&
      typeof existingOnboardingPlan === "object" &&
      !Array.isArray(existingOnboardingPlan)
        ? (existingOnboardingPlan as Record<string, unknown>)
        : {}
    const onboardingMetadata =
      body.onboarding && typeof body.onboarding === "object"
        ? {
            ...previousOnboardingMetadata,
            ...body.onboarding,
            selected_plan_id: nextPlan,
          }
        : undefined

    const billingPayload: BillingAccountResponse = {
      ...existingBilling.data,
      org_id: orgId,
      plan: nextPlan,
      metadata: onboardingMetadata
        ? {
            ...(existingBilling.data.metadata ?? {}),
            onboarding_plan: onboardingMetadata,
          }
        : existingBilling.data.metadata,
    }

    const billingResponse = await fetchUpstream(
      billingBaseUrl(),
      `/api/v1/billing/orgs/${orgId}/account`,
      {
        method: "PUT",
        headers,
        body: JSON.stringify(billingPayload),
      },
    )

    if (!billingResponse.ok) {
      const billingData = await readJson(billingResponse)
      return upstreamErrorResponse(billingResponse.status, billingData)
    }
  }

  void emitAuditEvent(await auditActor(request), {
    orgId,
    event: "org.plan.changed",
    subject: orgId,
    details: { plan: nextPlan },
  })

  return NextResponse.json(
    normalizeOrganization(orgData as OrgCoreOrganizationResponse),
    { status: orgResponse.status },
  )
}

async function handleCheckoutSessionPost(
  request: NextRequest,
  headers: Record<string, string>,
  orgId: string,
) {
  const body = (await request.json()) as {
    plan?: string
    successUrl?: string
    cancelUrl?: string
  }

  const response = await fetchUpstream(
    billingBaseUrl(),
    `/api/v1/billing/orgs/${orgId}/checkout-session`,
    {
      method: "POST",
      headers,
      body: JSON.stringify({
        plan: body.plan,
        success_url: body.successUrl,
        cancel_url: body.cancelUrl,
      }),
    },
  )

  const data = await readJson(response)
  if (!response.ok) {
    return upstreamErrorResponse(response.status, data)
  }
  return NextResponse.json(data, { status: response.status })
}

async function proxyJsonResponse(response: Response, pathSegments: string[]) {
  if (response.status === 204) {
    return new NextResponse(null, { status: 204 })
  }

  const data = await readJson(response)

  if (!response.ok) {
    return upstreamErrorResponse(response.status, data)
  }

  return NextResponse.json(normalizeOrgProxyPayload(pathSegments, data), {
    status: response.status,
  })
}

export async function GET(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    assertSafePathSegments(path)
    const headers = await buildHeaders(request)

    if (isBillingPath(path)) {
      return await handleBillingGet(headers, path[1]!)
    }

    if (isQuotaCollectionPath(path)) {
      return await handleQuotaCollectionGet(headers, path[1]!)
    }

    const response = await fetchUpstream(orgBaseUrl(), buildOrgPath(request, path), {
      method: "GET",
      headers,
    })

    return await proxyJsonResponse(response, path)
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function POST(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    assertSafePathSegments(path)
    const headers = await buildHeaders(request)

    if (isPlanPath(path)) {
      return await handlePlanPost(request, headers, path[1]!)
    }

    if (isCheckoutSessionPath(path)) {
      return await handleCheckoutSessionPost(request, headers, path[1]!)
    }

    const body = await request.json()

    const response = await fetchUpstream(orgBaseUrl(), buildOrgPath(request, path), {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    })

    if (response.ok && path.length === 1 && path[0] === "orgs") {
      void emitOrgEventFromResponse(request, response.clone(), "org.created")
    }

    return await proxyJsonResponse(response, path)
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function PUT(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    assertSafePathSegments(path)
    const headers = await buildHeaders(request)

    if (isBillingPath(path)) {
      return await handleBillingPut(request, headers, path[1]!)
    }

    if (isQuotaItemPath(path)) {
      return await handleQuotaItemPut(request, headers, path[1]!, path[3]!)
    }

    const body = await request.json()
    const response = await fetchUpstream(orgBaseUrl(), buildOrgPath(request, path), {
      method: "PUT",
      headers,
      body: JSON.stringify(body),
    })

    return await proxyJsonResponse(response, path)
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function PATCH(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    assertSafePathSegments(path)
    const headers = await buildHeaders(request)
    const body = await request.json()

    const response = await fetchUpstream(orgBaseUrl(), buildOrgPath(request, path), {
      method: "PATCH",
      headers,
      body: JSON.stringify(body),
    })

    return await proxyJsonResponse(response, path)
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function DELETE(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  try {
    const { path } = await context.params
    assertSafePathSegments(path)
    const headers = await buildHeaders(request)

    const response = await fetchUpstream(orgBaseUrl(), buildOrgPath(request, path), {
      method: "DELETE",
      headers,
    })

    return await proxyJsonResponse(response, path)
  } catch (error) {
    return authErrorResponse(error)
  }
}
