import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { requestJson } from '@/shared/api/http'
import { browserSitesPath as base, explainBrowserSite, setMyBrowserSiteChoice, type BrowserSiteChoice, type BrowserSiteExplanation, type BrowserSiteLevel, type BrowserSiteMode } from '@/shared/api/browser-sites-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { canonicalBrowserHost } from '@/features/settings/components/BrowserPolicySection'
import {
  SectionHeader,
  SettingsButton,
  SettingsSelect,
} from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'

/**
 * WS21-S6: the member's own remembered choices for the agent browser, and why one site is or is
 * not open. org-core stores and explains the choice; the browser broker pulls it when it mints a
 * grant, and Quarry enforces. This section only lists, sets and forgets the caller's own choices
 * through the gateway, and explains the decision for one host.
 *
 * It is shown only in a build with `VITE_BROWSER_SITES` set to the literal `true`. The gateway
 * routes exist only when `BROWSER_DO_ENABLED` is on, so with either switch off nothing here is
 * reachable.
 */

export { setMyBrowserSiteChoice } from '@/shared/api/browser-sites-client'
export type { BrowserSiteChoice, BrowserSiteLevel, BrowserSiteMode } from '@/shared/api/browser-sites-client'
export type BrowserSiteRule = 'block' | 'read' | 'act'

export interface BrowserSiteChoices {
  organization_id: string
  user_id: string
  revision: number
  prefs: BrowserSiteChoice[]
}

export interface BrowserSiteRuleRow {
  host: string
  rule: BrowserSiteRule
  note?: string
}

export interface BrowserSitePolicy {
  organization_id: string
  revision: number
  mode: BrowserSiteMode
  rules: BrowserSiteRuleRow[]
  updated_by: string
  updated_at: string | null
}

type Tr = (noText: string, enText: string) => string

/** Whether the browser-site settings are built in (`VITE_BROWSER_SITES`, the literal `true`). */
export function browserSitesEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_SITES === 'true'
}

function hostPath(orgId: string, host: string): string {
  return `${base(orgId)}/me/${encodeURIComponent(host)}`
}

export function getMyBrowserSiteChoices(orgId: string): Promise<BrowserSiteChoices> {
  return requestJson<BrowserSiteChoices>(`${base(orgId)}/me`)
}

export function forgetMyBrowserSiteChoice(orgId: string, host: string): Promise<void> {
  return requestJson<void>(hostPath(orgId, host), { method: 'DELETE' })
}

export function getBrowserSitePolicy(orgId: string): Promise<BrowserSitePolicy> {
  return requestJson<BrowserSitePolicy>(base(orgId))
}

/** Replaces the whole org policy. A stale `expected_revision` is refused with 409, never overwritten. */
export function putBrowserSitePolicy(
  orgId: string,
  body: { expected_revision: number; mode: BrowserSiteMode; rules: BrowserSiteRuleRow[] },
): Promise<BrowserSitePolicy> {
  return requestJson<BrowserSitePolicy>(base(orgId), {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function levelLabel(level: string, tr: Tr): string {
  switch (level) {
    case 'act':
      return tr('Lesing og handling', 'Reading and acting')
    case 'read':
      return tr('Bare lesing', 'Reading only')
    case 'none':
      return tr('Ikke tillatt', 'Not allowed')
    default:
      return level
  }
}

export function decisionLabel(effective: string, tr: Tr): string {
  switch (effective) {
    case 'act':
      return tr('Lesing og handling er tillatt', 'Reading and acting are allowed')
    case 'read':
      return tr('Bare lesing er tillatt', 'Only reading is allowed')
    case 'deny':
      return tr('Nettstedet er ikke tillatt', 'The site is not allowed')
    case 'ask':
      return tr('Ingen tilgang valgt ennå', 'No access choice made yet')
    default:
      return effective
  }
}

export function sourceLabel(source: string, tr: Tr): string {
  switch (source) {
    case 'org_block':
      return tr('Blokkert av administrator', 'Blocked by an administrator')
    case 'org_cap':
      return tr('Begrenset av administrator', 'Limited by an administrator')
    case 'user_pref':
      return tr('Ditt eget valg', 'Your own choice')
    case 'default':
      return tr('Ingen valg ennå', 'No choice yet')
    default:
      return source
  }
}

export function BrowserSitesSection() {
  const i18n = useI18n()
  const tr = i18n.tr
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  const [choices, { refetch }] = createResource(
    () => (browserSitesEnabled() && orgId() ? orgId() : undefined),
    (id) => getMyBrowserSiteChoices(id),
  )

  const requestedHost = typeof window === 'undefined' ? '' : new URLSearchParams(window.location.search).get('host') ?? ''
  const canonicalHost = canonicalBrowserHost(requestedHost)
  const reviewHost = canonicalHost.ok && canonicalHost.host.includes('.') ? canonicalHost.host : ''
  const [newHost, setNewHost] = createSignal(reviewHost)
  const [newLevel, setNewLevel] = createSignal<BrowserSiteLevel>('read')
  const [probe, setProbe] = createSignal(reviewHost)
  const [explanation, setExplanation] = createSignal<BrowserSiteExplanation | null>(null)
  const [problem, setProblem] = createSignal<string | null>(null)
  const [busy, setBusy] = createSignal(false)

  const failure = (err: unknown) =>
    translateApiError(err, tr, {
      no: 'Valget kunne ikke lagres. Prøv igjen.',
      en: 'The choice could not be saved. Try again.',
    })

  const change = async (action: () => Promise<unknown>) => {
    setBusy(true)
    setProblem(null)
    try {
      await action()
      await refetch()
    } catch (err) {
      setProblem(failure(err))
      await refetch().catch(() => undefined)
    } finally {
      setBusy(false)
    }
  }

  const revision = () => choices()?.revision ?? 0

  const handleAddChoice = () => {
    if (!newHost().trim()) return
    void change(async () => {
      const host = newHost().trim().toLowerCase()
      await setMyBrowserSiteChoice(orgId(), host, revision(), newLevel())
      setNewHost('')
    })
  }

  const handleForget = (host: string) => {
    void change(() => forgetMyBrowserSiteChoice(orgId(), host))
  }

  const handleExplain = async () => {
    const host = probe().trim()
    if (!host) return
    setProblem(null)
    try {
      setExplanation(await explainBrowserSite(orgId(), host))
    } catch (err) {
      setExplanation(null)
      setProblem(failure(err))
    }
  }

  const levelOptions = () => [
    { value: 'read', label: levelLabel('read', tr) },
    { value: 'act', label: levelLabel('act', tr) },
    { value: 'none', label: levelLabel('none', tr) },
  ]

  return (
    <Show when={browserSitesEnabled()}>
      <div data-testid="browser-sites-section">
        <SectionHeader
          title={tr('Nettsider for agenten', 'Sites for the agent')}
          description={tr(
            'Nettsidene agenten kan åpne for deg, og hva du har valgt. Administratoren kan begrense dem; valget ditt kan aldri gi mer tilgang enn administratoren tillater.',
            'The sites the agent may open for you, and what you chose. An administrator can limit them; your choice can never give more access than the administrator allows.',
          )}
        />

        {/*
          Capability overview (the settings hub). Browsing is the only
          user-settable level; the other capabilities are governed by the
          session, the run grants or approvals — never set here.
        */}
        <div data-testid="browser-sites-capabilities">
          <SectionHeader
            title={tr('Hva agenten kan gjøre', 'What the agent may do')}
            description={tr(
              'Nye nettsider krever at du blir spurt først, med mindre du allerede har valgt. Valget varer inntil 90 dager.',
              'New sites ask you first unless you already chose. A choice lasts up to 90 days.',
            )}
          />
          <ul data-testid="browser-sites-capability-list">
            <li>
              <span>{tr('Nettlesing', 'Browsing')}</span>
              <span>{tr('Spør først for nye nettsider', 'Ask first for new sites')}</span>
            </li>
            <li>
              <span>{tr('Nedlastinger', 'Downloads')}</span>
              <span>{tr('Krever godkjenning hver gang', 'Asks every time')}</span>
            </li>
            <li>
              <span>{tr('Opplastinger', 'Uploads')}</span>
              <span>{tr('Ikke tillatt for assistentkjøringer', 'Not allowed for assistant runs')}</span>
            </li>
            <li>
              <span>{tr('Opptak', 'Recording')}</span>
              <span>{tr('Aldri', 'Never')}</span>
            </li>
            <li>
              <span>{tr('Lagret profil', 'Saved profile')}</span>
              <span>{tr('Krever godkjenning', 'Asks for approval')}</span>
            </li>
          </ul>
        </div>

        <Show when={problem()}>
          {(message) => (
            <p role="alert" class="verevon-settings-label">
              {message()}
            </p>
          )}
        </Show>

        <Show
          when={(choices()?.prefs.length ?? 0) > 0}
          fallback={
            <p class="verevon-settings-label">
              {tr('Ingen nettsider er husket ennå.', 'No sites are remembered yet.')}
            </p>
          }
        >
          <ul data-testid="browser-sites-choices">
            <For each={choices()?.prefs ?? []}>
              {(pref) => (
                <li>
                  <span>{pref.host}</span> <span>{levelLabel(pref.level, tr)}</span>
                  <SettingsButton
                    type="button"
                    variant="secondary"
                    settingsSize="xs"
                    disabled={busy()}
                    onClick={() => handleForget(pref.host)}
                  >
                    {tr('Glem', 'Forget')}
                  </SettingsButton>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <div>
          <label for="browser-sites-new-host" class="verevon-settings-label">
            {tr('Legg til nettside', 'Add a site')}
          </label>
          <VerevonInput
            id="browser-sites-new-host"
            value={newHost()}
            placeholder="example.com"
            onInput={(event) => setNewHost(event.currentTarget.value)}
          />
          <SettingsSelect
            id="browser-sites-new-level"
            label={tr('Valg', 'Choice')}
            value={newLevel()}
            options={levelOptions()}
            onChange={(event) => setNewLevel(event.currentTarget.value as BrowserSiteLevel)}
          />
          <SettingsButton type="button" disabled={busy() || !newHost().trim()} onClick={handleAddChoice}>
            {tr('Lagre valg', 'Save choice')}
          </SettingsButton>
        </div>

        <div>
          <label for="browser-sites-probe" class="verevon-settings-label">
            {tr('Hvorfor åpnes eller åpnes ikke en side?', 'Why is a site open or not?')}
          </label>
          <VerevonInput
            id="browser-sites-probe"
            value={probe()}
            placeholder="example.com"
            onInput={(event) => setProbe(event.currentTarget.value)}
          />
          <SettingsButton type="button" variant="secondary" onClick={() => void handleExplain()}>
            {tr('Forklar', 'Explain')}
          </SettingsButton>
          <Show when={explanation()}>
            {(answer) => (
              <p data-testid="browser-sites-explanation" class="verevon-settings-label">
                {decisionLabel(answer().decision.effective, tr)}. {sourceLabel(answer().decision.source, tr)}.
              </p>
            )}
          </Show>
        </div>
      </div>
    </Show>
  )
}
