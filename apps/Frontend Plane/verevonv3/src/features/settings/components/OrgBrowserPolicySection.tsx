import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { ApiError } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import {
  SectionHeader,
  SettingsButton,
  SettingsSelect,
} from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import {
  browserSitesEnabled,
  getBrowserSitePolicy,
  putBrowserSitePolicy,
  type BrowserSiteMode,
  type BrowserSiteRule,
  type BrowserSiteRuleRow,
} from '@/features/settings/components/BrowserSitesSection'

/**
 * WS21-S6: the organisation's browser site policy, for owners and admins. org-core stores and
 * resolves it; the browser broker reads the answer when it mints a grant, and Quarry enforces.
 * This section edits the whole policy set through the gateway, from the revision it was read at.
 *
 * `open` reaches every site that is not blocked, and asks the member first. `allowlist` reaches
 * only the sites listed with read or act. A `block` rule wins over everything.
 *
 * Shown only in a build with `VITE_BROWSER_SITES` set to the literal `true`, and only to an owner
 * or admin. The gateway checks the same role and answers 403 to anyone else.
 */

type Tr = (noText: string, enText: string) => string

/** The limits org-core enforces; checked here only to answer before a save. */
const MAX_RULES = 500
const MAX_NOTE_CHARS = 200
const HOST_PATTERN = /^[A-Za-z0-9.-]{1,253}$/

/** The first thing wrong with the draft, or null. org-core still answers every semantic rule. */
export function policyProblem(rules: readonly BrowserSiteRuleRow[], tr: Tr): string | null {
  if (rules.length > MAX_RULES) {
    return tr(
      `En policy kan ha høyst ${MAX_RULES} nettsteder.`,
      `A policy can hold at most ${MAX_RULES} sites.`,
    )
  }
  for (const rule of rules) {
    if (!HOST_PATTERN.test(rule.host.trim())) {
      return tr(
        `«${rule.host}» er ikke et domenenavn. Skriv bare bokstaver, tall, punktum og bindestrek.`,
        `"${rule.host}" is not a host name. Use letters, digits, dots and hyphens only.`,
      )
    }
    if ((rule.note ?? '').trim().length > MAX_NOTE_CHARS) {
      return tr(
        `Notatet er lengre enn ${MAX_NOTE_CHARS} tegn.`,
        `The note is longer than ${MAX_NOTE_CHARS} characters.`,
      )
    }
  }
  return null
}

/** The rule as it is saved: a trimmed host, and a note only when one was written. */
export function normalizeRule(rule: BrowserSiteRuleRow): BrowserSiteRuleRow {
  const note = (rule.note ?? '').trim()
  return note
    ? { host: rule.host.trim(), rule: rule.rule, note }
    : { host: rule.host.trim(), rule: rule.rule }
}

/** A stale editor's save is refused with 409; it is never written over the newer policy. */
function isRevisionConflict(err: unknown): boolean {
  return err instanceof ApiError && err.status === 409
}

export function OrgBrowserPolicySection() {
  const i18n = useI18n()
  const tr = i18n.tr
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [policy, { refetch }] = createResource(
    () => (browserSitesEnabled() && isAdmin() && orgId() ? orgId() : undefined),
    (id) => getBrowserSitePolicy(id),
  )

  // `undefined` until the admin edits: the stored policy is then what is shown.
  const [draftMode, setDraftMode] = createSignal<BrowserSiteMode | undefined>(undefined)
  const [draftRules, setDraftRules] = createSignal<BrowserSiteRuleRow[] | undefined>(undefined)
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [problem, setProblem] = createSignal<string | null>(null)

  const mode = (): BrowserSiteMode => draftMode() ?? policy()?.mode ?? 'open'
  const rules = (): BrowserSiteRuleRow[] => draftRules() ?? policy()?.rules ?? []

  const edit = (next: BrowserSiteRuleRow[]) => {
    setSaved(false)
    setDraftRules(next)
  }

  const updateRule = (index: number, patch: Partial<BrowserSiteRuleRow>) =>
    edit(rules().map((rule, position) => (position === index ? { ...rule, ...patch } : rule)))

  const removeRule = (index: number) =>
    edit(rules().filter((_, position) => position !== index))

  const save = async () => {
    const current = policy()
    if (!current) return
    const draftProblem = policyProblem(rules(), tr)
    if (draftProblem) {
      setProblem(draftProblem)
      return
    }
    setSaving(true)
    setProblem(null)
    try {
      await putBrowserSitePolicy(orgId(), {
        expected_revision: current.revision,
        mode: mode(),
        rules: rules().map(normalizeRule),
      })
      setDraftMode(undefined)
      setDraftRules(undefined)
      setSaved(true)
      await refetch()
    } catch (err) {
      setProblem(
        isRevisionConflict(err)
          ? tr(
              'Policyen er endret av en annen administrator. Last inn siden på nytt; utkastet ditt er ikke lagret.',
              'Another administrator changed the policy. Reload the page; your draft was not saved.',
            )
          : translateApiError(err, tr, {
              no: 'Policyen kunne ikke lagres. Prøv igjen.',
              en: 'The policy could not be saved. Try again.',
            }),
      )
    } finally {
      setSaving(false)
    }
  }

  const ruleOptions = (): Array<{ value: BrowserSiteRule; label: string }> => [
    { value: 'read', label: tr('Bare lesing', 'Reading only') },
    { value: 'act', label: tr('Lesing og handling', 'Reading and acting') },
    { value: 'block', label: tr('Blokkert', 'Blocked') },
  ]

  const modeOptions = () => [
    { value: 'open', label: tr('Åpen: spør før første bruk', 'Open: ask before first use') },
    { value: 'allowlist', label: tr('Bare listede nettsteder', 'Listed sites only') },
  ]

  return (
    <Show when={browserSitesEnabled() && isAdmin()}>
      <div data-testid="org-browser-policy-section">
        <SectionHeader
          title={tr('Nettleserpolicy for organisasjonen', 'Organisation browser policy')}
          description={tr(
            'Hvilke nettsider agenten kan åpne for medlemmene. En blokkering gjelder alle, og medlemmets eget valg kan ikke gå forbi den.',
            'Which sites the agent may open for members. A block applies to everyone, and a member’s own choice cannot go past it.',
          )}
        />

        <Show when={policy()}>
          <SettingsSelect
            id="org-browser-policy-mode"
            label={tr('Modus', 'Mode')}
            value={mode()}
            options={modeOptions()}
            onChange={(event) => {
              setSaved(false)
              setDraftMode(event.currentTarget.value as BrowserSiteMode)
            }}
          />

          <ul data-testid="org-browser-policy-rules">
            <For each={rules()}>
              {(rule, index) => (
                <li>
                  <VerevonInput
                    id={`org-browser-policy-host-${index()}`}
                    value={rule.host}
                    placeholder="example.com"
                    onInput={(event) => updateRule(index(), { host: event.currentTarget.value })}
                  />
                  <SettingsSelect
                    id={`org-browser-policy-rule-${index()}`}
                    label={tr('Regel', 'Rule')}
                    value={rule.rule}
                    options={ruleOptions()}
                    onChange={(event) =>
                      updateRule(index(), { rule: event.currentTarget.value as BrowserSiteRule })
                    }
                  />
                  <VerevonInput
                    id={`org-browser-policy-note-${index()}`}
                    value={rule.note ?? ''}
                    placeholder={tr('Notat (valgfritt)', 'Note (optional)')}
                    onInput={(event) => updateRule(index(), { note: event.currentTarget.value })}
                  />
                  <SettingsButton
                    type="button"
                    variant="secondary"
                    settingsSize="xs"
                    onClick={() => removeRule(index())}
                  >
                    {tr('Fjern', 'Remove')}
                  </SettingsButton>
                </li>
              )}
            </For>
          </ul>

          <SettingsButton
            type="button"
            variant="secondary"
            disabled={saving()}
            onClick={() => edit([...rules(), { host: '', rule: 'read' }])}
          >
            {tr('Legg til nettside', 'Add a site')}
          </SettingsButton>

          <SettingsButton type="button" disabled={saving()} onClick={() => void save()}>
            {tr('Lagre nettleserpolicy', 'Save browser policy')}
          </SettingsButton>

          <Show when={problem()}>
            {(message) => (
              <p role="alert" class="verevon-settings-label">
                {message()}
              </p>
            )}
          </Show>
          <Show when={saved()}>
            <p class="verevon-settings-label">{tr('Policyen er lagret.', 'The policy is saved.')}</p>
          </Show>
        </Show>
      </div>
    </Show>
  )
}
