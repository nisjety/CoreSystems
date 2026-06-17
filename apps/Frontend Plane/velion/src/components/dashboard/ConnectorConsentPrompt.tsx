'use client'

// G45 — Connector consent after first value (Slice F).
//
// Replaces the legacy `/onboarding/connect` wizard step. Instead of putting
// data-source consent BEFORE the user has experienced product value, this
// component surfaces a contextual prompt AFTER the dashboard has been live
// for `FIRST_VALUE_DELAY_MS` AND the user has no Microsoft connection yet.
//
// "First value" definition (intentionally simple):
//   - User has reached the dashboard (any subroute under `(dashboard)`) and
//   - has been on the dashboard for at least `FIRST_VALUE_DELAY_MS`.
//
// This is a behavioural proxy for "they saw the product render and didn't
// bounce". A richer definition (first chat answer / first search result) is
// possible later — we wire it via the same `useEntitlementToast`-shaped
// hook pattern.
//
// Dismissal: stored in `localStorage` so it survives across tabs and
// browser restarts. Optional follow-up: mirror to `user_profiles.metadata`
// for cross-device sync.

import { useEffect, useState } from 'react'
import { X } from 'lucide-react'

import { useKnowledgeIntegrations } from '@/components/knowledge/hooks/useKnowledgeData'
import { useAuth } from '@/components/auth/hooks/use-auth'

const DISMISS_KEY = 'velion.connector-consent-prompt.dismissed'
const FIRST_VALUE_DELAY_MS = 90_000 // 90s on dashboard before the prompt shows
const MICROSOFT_PROVIDERS = new Set(['microsoft', 'microsoft365', 'office365'])

function dismissedInLocalStorage(): boolean {
  if (typeof window === 'undefined') return false
  return window.localStorage.getItem(DISMISS_KEY) === '1'
}

function setDismissedInLocalStorage(): void {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(DISMISS_KEY, '1')
}

export function ConnectorConsentPrompt(): React.JSX.Element | null {
  const [dismissed, setDismissed] = useState<boolean>(() => dismissedInLocalStorage())
  const [firstValueElapsed, setFirstValueElapsed] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const { user } = useAuth()
  const { data: integrations } = useKnowledgeIntegrations()

  // Kick off the first-value timer once the component mounts. We deliberately
  // don't depend on user/integration state — the timer counts dashboard
  // visibility, not "ready to ask".
  useEffect(() => {
    const handle = setTimeout(() => setFirstValueElapsed(true), FIRST_VALUE_DELAY_MS)
    return () => clearTimeout(handle)
  }, [])

  if (dismissed || !firstValueElapsed) return null
  if (!user?.id || !user?.email) return null
  if (!integrations) return null

  // Look up the org from the integrations payload (the API already resolves
  // the active org server-side). When there's no org yet, we don't have a
  // sensible scope to attach the connect-session to.
  const orgId = integrations.orgId
  if (!orgId) return null

  // Has the user already connected a Microsoft source? If so, we have
  // nothing to prompt for.
  const hasMicrosoft = (integrations.connections ?? []).some((c) =>
    MICROSOFT_PROVIDERS.has(String(c.provider).toLowerCase()),
  )
  if (hasMicrosoft) return null

  const dismiss = (): void => {
    setDismissedInLocalStorage()
    setDismissed(true)
  }

  const connect = async (): Promise<void> => {
    if (submitting) return
    setSubmitting(true)
    try {
      const response = await fetch('/api/oauth/initiate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({
          org_id: orgId,
          user_id: user.id,
          user_email: user.email,
          provider: 'microsoft',
          sources: ['sharePoint', 'oneDrive', 'teams', 'outlook'],
        }),
      })
      if (!response.ok) {
        // Surfacing the error is the route handler's job; this component is
        // best-effort UX. Re-enable the button so the user can retry.
        setSubmitting(false)
        return
      }
      const json = (await response.json()) as {
        authorization_url?: string | null
        connect_link?: string | null
      }
      const target = json.authorization_url || json.connect_link
      if (target) {
        window.location.href = target
      } else {
        // Connect link came back empty — record dismissal so we don't keep
        // re-prompting on a misconfigured upstream.
        dismiss()
      }
    } catch {
      setSubmitting(false)
    }
  }

  return (
    <div
      role="dialog"
      aria-labelledby="connector-consent-title"
      aria-describedby="connector-consent-body"
      className="pointer-events-auto fixed bottom-6 right-6 z-40 flex w-[min(420px,calc(100vw-2rem))] flex-col gap-3 rounded-xl border border-slate-200 bg-white p-4 text-sm shadow-lg dark:border-slate-800 dark:bg-slate-900 dark:text-slate-100"
      data-testid="connector-consent-prompt"
    >
      <button
        type="button"
        onClick={dismiss}
        className="absolute right-2 top-2 rounded p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-300 dark:hover:bg-slate-800 dark:hover:text-slate-100"
        aria-label="Lukk varselet"
      >
        <X size={16} strokeWidth={1.5} />
      </button>

      <div className="flex flex-col gap-1">
        <h2 id="connector-consent-title" className="font-medium">
          Koble til Microsoft 365 for bedre svar
        </h2>
        <p id="connector-consent-body" className="text-xs text-slate-600 dark:text-slate-400">
          Velion kan finne dokumenter, samtaler og e-poster på tvers av SharePoint,
          OneDrive, Teams og Outlook. Du kan koble til når som helst — vi
          spør bare hvis vi tror det vil hjelpe.
        </p>
      </div>

      <div className="flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={dismiss}
          className="rounded px-3 py-1.5 text-xs text-slate-600 transition hover:bg-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-300 dark:text-slate-300 dark:hover:bg-slate-800"
        >
          Kanskje senere
        </button>
        <button
          type="button"
          onClick={connect}
          disabled={submitting}
          className="rounded bg-slate-900 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-slate-800 focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-700 disabled:cursor-not-allowed disabled:opacity-60 dark:bg-white dark:text-slate-900 dark:hover:bg-slate-200"
          data-testid="connector-consent-connect"
        >
          {submitting ? 'Åpner Microsoft...' : 'Koble til Microsoft 365'}
        </button>
      </div>
    </div>
  )
}
