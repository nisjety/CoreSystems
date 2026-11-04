import { createMemo, createSignal, Show } from 'solid-js'
import { ShieldCheck } from '@/shared/icons'
import { ApiError, requestJson } from '@/shared/api/http'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton, SettingsSelect } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * The organisation's web access (Wave 3 EP-9 W2): whether its agents may use the public web at all, and whether a
 * paid search provider may serve them. Both are keys of the organisation agent policy that capability-core holds
 * (DEC-14), read and written through the gateway's `/api/v1/org/agent-policy` routes, the routes the agent mode
 * section uses.
 *
 * A write replaces the whole document, so this section sends every setting as it is stored and changes only its two
 * switches. A saved switch takes effect only where the deployment has switched policy enforcement on, and the copy
 * says so instead of implying the switch already holds.
 *
 * Members see the switches read-only with a locked badge, as the agent mode section shows them.
 */

type I18nText = { tr: (noText: string, enText: string) => string }

const WEB_MODES = ['off', 'live'] as const
const PAID_PROVIDER_SWITCHES = ['off', 'on'] as const

type WebMode = (typeof WEB_MODES)[number]
type PaidProviderSwitch = (typeof PAID_PROVIDER_SWITCHES)[number]

/** The two switches this section writes. */
type WebSwitches = { webMode: WebMode; paidProviders: PaidProviderSwitch }

/** The stored document: every setting the server holds, as it answers. */
type OrgAgentPolicyDocument = {
  org_id: string
  revision: number
  digest: string
  settings: Record<string, unknown>
}

const POLICY_PATH = '/api/v1/org/agent-policy'

/**
 * Reads the switches the way the server enforces them: a key the document omits takes its default (live, on), and a
 * value this build does not read takes the fail-closed value (off). The section never shows a switch looser than the
 * one in force.
 */
function readWebSwitches(settings: Record<string, unknown>): WebSwitches {
  const webMode: WebMode = !('web.mode' in settings) || settings['web.mode'] === 'live' ? 'live' : 'off'
  const paidProviders: PaidProviderSwitch =
    !('web.paid_providers' in settings) || settings['web.paid_providers'] === 'on' ? 'on' : 'off'
  return { webMode, paidProviders }
}

function getOrgAgentPolicy(): Promise<OrgAgentPolicyDocument> {
  return requestJson<OrgAgentPolicyDocument>(POLICY_PATH)
}

/** Replaces the whole document at `revision`: every setting but the two switches goes back as it is stored. */
function saveWebSwitches(doc: OrgAgentPolicyDocument, switches: WebSwitches): Promise<OrgAgentPolicyDocument> {
  return requestJson<OrgAgentPolicyDocument>(POLICY_PATH, {
    method: 'PUT',
    body: JSON.stringify({
      revision: doc.revision,
      settings: { ...doc.settings, 'web.mode': switches.webMode, 'web.paid_providers': switches.paidProviders },
    }),
  })
}

function saveFailure(error: unknown, i18n: I18nText): string {
  if (error instanceof ApiError && error.status === 409) {
    return i18n.tr(
      'Policyen er endret etter at du åpnet den. Last siden på nytt og prøv igjen.',
      'The policy changed after you opened it. Reload the page and try again.',
    )
  }
  return translateApiError(error, i18n.tr, {
    no: 'Kunne ikke lagre nettilgangen.',
    en: 'Could not save web access.',
  })
}

function webModeLabel(mode: WebMode, i18n: I18nText): string {
  return mode === 'live'
    ? i18n.tr('På: agentene kan søke på nettet og hente sider', 'On: agents may search the web and fetch pages')
    : i18n.tr('Av: agentene bruker ikke nettet', 'Off: agents do not use the web')
}

function paidProvidersLabel(value: PaidProviderSwitch, i18n: I18nText): string {
  return value === 'on'
    ? i18n.tr('Tillatt: betalte søketjenester kan brukes', 'Allowed: paid search services may be used')
    : i18n.tr('Ikke tillatt: bare gratis søketjenester', 'Not allowed: free search services only')
}

export function OrgWebAccessSection() {
  const i18n = useI18n()
  const session = getSession()
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [stored, { refetch }] = createResource(() => getOrgAgentPolicy())

  // `undefined` until an admin edits: the stored switches are then what is shown.
  const [draft, setDraft] = createSignal<WebSwitches | undefined>(undefined)
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [problem, setProblem] = createSignal<string | null>(null)

  const shown = createMemo<WebSwitches | undefined>(() => {
    const edited = draft()
    if (edited) return edited
    const doc = stored()
    return doc ? readWebSwitches(doc.settings) : undefined
  })

  const edit = (change: Partial<WebSwitches>) => {
    const base = shown()
    if (!base) return
    setSaved(false)
    setDraft({ ...base, ...change })
  }

  const save = async () => {
    const doc = stored()
    const next = shown()
    if (!isAdmin() || !doc || !next || saving()) return
    setSaving(true)
    setProblem(null)
    setSaved(false)
    try {
      await saveWebSwitches(doc, next)
      setDraft(undefined)
      setSaved(true)
    } catch (error) {
      setProblem(saveFailure(error, i18n))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetch()])
  }

  return (
    <section class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Nettilgang', 'Web access')}
        description={i18n.tr(
          'Om agentene i organisasjonen kan bruke nettet, og om betalte søketjenester kan brukes for dem. Når nettilgangen er av, får agentene ingen nettverktøy, og en chat sier hvorfor.',
          'Whether the agents in your organisation may use the public web, and whether paid search services may serve them. With web access off, agents are offered no web tools, and a chat says why.',
        )}
      />

      <Show when={!isAdmin()}>
        <p class="verevon-settings-subnote">
          <ShieldCheck size={14} aria-hidden="true" />{' '}
          {i18n.tr(
            'Bare organisasjonsadministratorer kan endre nettilgangen. Du ser verdiene som gjelder for deg.',
            'Only organisation administrators can change web access. You see the values that apply to you.',
          )}
        </p>
      </Show>

      <Show when={stored.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste nettilgangen.', 'Could not load web access.')}
        </p>
      </Show>

      <Show when={stored.loading && !shown()}>
        <p class="verevon-settings-subnote" role="status" aria-busy="true">
          {i18n.tr('Laster nettilgangen…', 'Loading web access…')}
        </p>
      </Show>

      <Show when={shown()}>
        {(current) => (
          <div class="verevon-settings-panel">
            <SettingsSelect
              id="customize-web-mode"
              label={i18n.tr('Nettilgang for agentene', 'Web access for agents')}
              value={current().webMode}
              disabled={!isAdmin() || saving()}
              options={WEB_MODES.map((mode) => ({ value: mode, label: webModeLabel(mode, i18n) }))}
              onChange={(event) => edit({ webMode: event.currentTarget.value as WebMode })}
            />
            <SettingsSelect
              id="customize-web-paid"
              label={i18n.tr('Betalte søketjenester', 'Paid search services')}
              value={current().paidProviders}
              disabled={!isAdmin() || saving()}
              options={PAID_PROVIDER_SWITCHES.map((value) => ({ value, label: paidProvidersLabel(value, i18n) }))}
              onChange={(event) => edit({ paidProviders: event.currentTarget.value as PaidProviderSwitch })}
            />
          </div>
        )}
      </Show>

      <Show when={isAdmin()}>
        <div class="verevon-settings-actions">
          <p>{problem() ?? (saved() ? i18n.tr('Nettilgangen er lagret.', 'Web access is saved.') : '')}</p>
          <div>
            <SettingsButton variant="primary" disabled={!draft() || saving()} onClick={() => void save()}>
              {i18n.tr('Lagre nettilgang', 'Save web access')}
            </SettingsButton>
          </div>
        </div>
      </Show>

      <p class="verevon-settings-subnote">
        {i18n.tr(
          'Lagrede verdier gjelder bare der plattformen har slått på håndheving av agentpolicyen.',
          'A saved value applies only where the deployment has switched policy enforcement on.',
        )}
      </p>
    </section>
  )
}
