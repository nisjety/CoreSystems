'use client'

// G21 + G43 — Enterprise trust banner (Convex-reactive).
//
// G21 surfaces what the zero-input enterprise sign-in flow auto-decided on
// the user's behalf (organization, role, tenant / domain) so the user can
// verify it on first dashboard load. Required by the zero-input roadmap for
// SOC2 transparency.
//
// G43 (velion-gap.md §8.30): the banner now subscribes reactively to
// `api.controlSessions.byUser` on Convex. CP session-core publishes a fresh
// snapshot whenever the Control Session aggregate changes (plan upgrade,
// org switch, entitlement change) via the `upsertControlSession` HTTP
// action (G35 §8.21). With this wiring the banner re-renders without a
// refresh when the plan flips or a new org/role is resolved.
//
// Fallback: the REST one-shot to `/api/user/me/session-context` runs first
// to get the user id needed for the Convex subscription. When Convex has
// no snapshot yet (race: signed in before session-core's first upsert),
// the REST payload renders the banner; once the snapshot lands, useQuery
// replaces the initial data.
//
// Dismissal: stored in `sessionStorage` (per-tab, clears on browser close).
// Cookie was considered but session-only behaviour is the better default
// for the "verify what was auto-decided" use case.

import { useEffect, useState } from 'react'
import { useQuery } from 'convex/react'

import { api } from '@/lib/convex-api-stub'
import { emit as emitTelemetry } from '@/lib/telemetry/client'

interface OrganizationView {
  id?: string
  name?: string
  plan?: string
  tenantId?: string
  role?: string
}

interface ControlSessionResponse {
  // Wave-3 rich shape (CONTROL_SESSION_AUTHORITY_ENABLED=true)
  organization?: OrganizationView
  user?: { id?: string; email?: string; name?: string }
  // Legacy narrow shape (when CONTROL_SESSION_AUTHORITY_ENABLED=false)
  orgId?: string
  role?: string
}

/**
 * Convex `controlSessions` row. The `snapshot` field is the opaque Control
 * Session JSON written by CP session-core (we don't enforce its shape in
 * Convex — see schema.ts:308). At runtime it follows `ControlSessionResponse`.
 */
interface ControlSessionsRow {
  externalUserId?: string
  externalOrgId?: string
  snapshot?: ControlSessionResponse
  fetchedAt?: number
}

const DISMISS_KEY = 'velion.enterprise-trust-banner.dismissed'

function readOrganizationView(payload: ControlSessionResponse | null | undefined): OrganizationView | null {
  if (!payload) return null
  if (payload.organization && (payload.organization.id || payload.organization.name)) {
    return payload.organization
  }
  if (payload.orgId) {
    return { id: payload.orgId, role: payload.role }
  }
  return null
}

function domainFromEmail(email: string | null | undefined): string | null {
  if (!email) return null
  const at = email.indexOf('@')
  if (at < 0 || at === email.length - 1) return null
  return email.slice(at + 1)
}

function roleLabel(role: string | undefined): string {
  if (!role) return 'member'
  // org-core uses lowercase enums (owner / admin / member). Title-case for UI.
  return role.charAt(0).toUpperCase() + role.slice(1).toLowerCase()
}

export function EnterpriseTrustBanner(): React.JSX.Element | null {
  const [restPayload, setRestPayload] = useState<ControlSessionResponse | null>(null)
  const [userId, setUserId] = useState<string | null>(null)
  const [dismissed, setDismissed] = useState(false)

  useEffect(() => {
    if (typeof window === 'undefined') return
    if (window.sessionStorage.getItem(DISMISS_KEY) === '1') {
      setDismissed(true)
      return
    }

    let cancelled = false
    const controller = new AbortController()

    void (async () => {
      try {
        const response = await fetch('/api/user/me/session-context', {
          method: 'GET',
          credentials: 'include',
          signal: controller.signal,
        })
        if (!response.ok || cancelled) return
        const payload = (await response.json()) as ControlSessionResponse
        if (cancelled) return
        setRestPayload(payload)
        setUserId(payload.user?.id ?? null)
        // G25: signal "first value visible" — the dashboard has resolved the
        // signed-in user's org context. One emit per mount keeps the metric
        // honest (router-pushed sub-views remount the layout in dev).
        const org = readOrganizationView(payload)
        emitTelemetry('dashboard.first_paint', {
          props: {
            has_org: Boolean(org?.id ?? org?.name),
            has_role: Boolean(org?.role),
          },
        })
      } catch {
        // Network/abort — silently skip. Banner just won't render this paint.
      }
    })()

    return () => {
      cancelled = true
      controller.abort()
    }
  }, [])

  // G43: subscribe to the Convex projection once we know the user id.
  // `useQuery` is reactive — when CP session-core upserts a new snapshot
  // (plan upgrade, org switch, entitlement change), this re-renders with
  // no refresh. Passing 'skip' before the REST one-shot lands the user id
  // is the Convex idiom for a conditional subscription.
  const convexSnapshot = useQuery(
    api.controlSessions.byUser,
    userId ? { externalUserId: userId } : 'skip',
  ) as ControlSessionsRow | null | undefined

  const dismiss = (): void => {
    if (typeof window !== 'undefined') {
      window.sessionStorage.setItem(DISMISS_KEY, '1')
    }
    setDismissed(true)
  }

  // Pick the freshest source: Convex snapshot when present, REST otherwise.
  // The snapshot wins because CP session-core invalidates + re-aggregates
  // on upstream events; the REST payload is the synchronous fallback.
  const payload: ControlSessionResponse | null = convexSnapshot?.snapshot ?? restPayload
  if (dismissed || !payload) return null

  const organization = readOrganizationView(payload)
  if (!organization) return null

  const role = roleLabel(organization.role)
  const domain = organization.tenantId ?? domainFromEmail(payload.user?.email)
  const orgLabel = organization.name ?? organization.id ?? 'your organization'

  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed bottom-4 right-4 z-50 flex max-w-[min(720px,calc(100dvw-2rem))] items-center justify-between gap-3 rounded-2xl border border-slate-200 bg-white/95 px-4 py-3 text-xs text-slate-700 shadow-[0_18px_48px_rgba(15,23,42,0.16)] backdrop-blur dark:border-slate-800 dark:bg-slate-900/95 dark:text-slate-300"
      data-testid="enterprise-trust-banner"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="inline-flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className="inline-block size-1.5 rounded-full bg-emerald-500"
          />
          <span className="font-medium">Signed in as {role}</span>
        </span>
        <span aria-hidden="true" className="text-slate-300 dark:text-slate-700">
          ·
        </span>
        <span>
          <span className="text-slate-500 dark:text-slate-400">Organization:</span>{' '}
          <span className="font-medium">{orgLabel}</span>
        </span>
        {domain ? (
          <>
            <span aria-hidden="true" className="text-slate-300 dark:text-slate-700">
              ·
            </span>
            <span>
              <span className="text-slate-500 dark:text-slate-400">Domain:</span>{' '}
              <span className="font-medium">{domain}</span>
            </span>
          </>
        ) : null}
        {organization.plan ? (
          <>
            <span aria-hidden="true" className="text-slate-300 dark:text-slate-700">
              ·
            </span>
            <span>
              <span className="text-slate-500 dark:text-slate-400">Plan:</span>{' '}
              <span className="font-medium">{roleLabel(organization.plan)}</span>
            </span>
          </>
        ) : null}
      </div>
      <button
        type="button"
        onClick={dismiss}
        className="shrink-0 rounded px-2 py-0.5 text-slate-500 transition hover:bg-slate-200/60 hover:text-slate-900 focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-400 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-100"
        aria-label="Dismiss banner"
      >
        Dismiss
      </button>
    </div>
  )
}
