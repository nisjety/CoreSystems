import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { requestJson } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
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

export type BrowserSiteLevel = 'none' | 'read' | 'act'
export type BrowserSiteRule = 'block' | 'read' | 'act'
export type BrowserSiteMode = 'open' | 'allowlist'

export interface BrowserSiteChoice {
  host: string
  level: BrowserSiteLevel
  expires_at: string
  updated_at: string
}

export interface BrowserSiteChoices {
  organization_id: string
  user_id: string
  revision: number
  prefs: BrowserSiteChoice[]
}

export interface BrowserSiteDecision {
  host: string
  effective: 'deny' | 'ask' | 'read' | 'act'
  cap: BrowserSiteLevel
  source: 'org_block' | 'org_cap' | 'user_pref' | 'default'
}

export interface BrowserSiteExplanation {
  organization_id: string
  user_id: string
  revision: number
  mode: BrowserSiteMode
  decision: BrowserSiteDecision
  org_rule: string
  user_level: string
  user_expires_at: string | null
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

function base(orgId: string): string {
  return `/api/v1/orgs/${encodeURIComponent(orgId)}/browser-sites`
}

function hostPath(orgId: string, host: string): string {
  return `${base(orgId)}/me/${encodeURIComponent(host)}`
}

export function getMyBrowserSiteChoices(orgId: string): Promise<BrowserSiteChoices> {
  return requestJson<BrowserSiteChoices>(`${base(orgId)}/me`)
}

/** Sets the caller's choice for one host, against the revision the caller read. */
export function setMyBrowserSiteChoice(
  orgId: string,
  host: string,
  expectedRevision: number,
  level: BrowserSiteLevel,
): Promise<BrowserSiteChoice> {
  return requestJson<BrowserSiteChoice>(hostPath(orgId, host), {
    method: 'PUT',
    body: JSON.stringify({ expected_revision: expectedRevision, level }),
  })
}

export function forgetMyBrowserSiteChoice(orgId: string, host: string): Promise<void> {
  return requestJson<void>(hostPath(orgId, host), { method: 'DELETE' })
}

export function explainBrowserSite(orgId: string, host: string): Promise<BrowserSiteExplanation> {
  return requestJson<BrowserSiteExplanation>(`${base(orgId)}/explain?host=${encodeURIComponent(host)}`)
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
      return tr('Du blir spurt før første bruk', 'You will be asked before first use')
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

  const [newHost, setNewHost] = createSignal('')
  const [newLevel, setNewLevel] = createSignal<BrowserSiteLevel>('read')
  const [probe, setProbe] = createSignal('')
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
