import { createMemo, createSignal, For, Show } from 'solid-js'
import { Pencil, Plus, Trash2 } from '@/shared/icons'
import { requestJson } from '@/shared/api/http'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  SectionHeader,
  SettingsButton,
  SettingsField,
  SettingsSelect,
  ToggleRow,
} from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Organisation hook rules (WS25-S11): rules that deny, or ask first about, a tool call in the agent runs of
 * the organisation. A rule only narrows a run. capability-core stores them (DEC-14); the gateway forwards
 * the requests, and only an organisation admin may write them.
 *
 * Several rules can match one call, and the strictest decision applies: a deny beats an ask, and an ask beats
 * an allow. The priority orders the list only, so the test panel below shows the same verdict the engine gives.
 */

type I18nText = { tr: (noText: string, enText: string) => string }

type HookEvent = 'pre_tool' | 'post_tool'
type HookDecision = 'deny' | 'ask'
type HookVerdict = 'deny' | 'ask' | 'allow'

export type HookRule = {
  id: string
  event: HookEvent
  tool_matcher: string
  decision: HookDecision
  reason: string
  priority: number
  enabled: boolean
}

type HookRuleBody = Omit<HookRule, 'id'>

type HookListing = {
  org_id: string
  revision: number
  rules: HookRule[]
}

/** The tool-name grammar of the gateway and of execution-core's hook engine: empty or `*`, a trailing `*`, or a name. */
const MATCHER_PATTERN = /^(\*|[A-Za-z0-9][A-Za-z0-9_.:-]{0,199}\*?)?$/
const MAX_REASON_LENGTH = 500
const HOOKS_PATH = '/api/v1/org/hooks'

function listOrgHookRules(): Promise<HookListing> {
  return requestJson<HookListing>(HOOKS_PATH)
}

function createOrgHookRule(body: HookRuleBody): Promise<HookRule> {
  return requestJson<HookRule>(HOOKS_PATH, { method: 'POST', body: JSON.stringify(body) })
}

function updateOrgHookRule(id: string, body: HookRuleBody): Promise<HookRule> {
  return requestJson<HookRule>(`${HOOKS_PATH}/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

/** Retires a rule. The server keeps it in the history, so it can be added again. */
function retireOrgHookRule(id: string): Promise<unknown> {
  return requestJson(`${HOOKS_PATH}/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

/** Whether a matcher covers a tool: empty or `*` is any tool, a trailing `*` a prefix, anything else an exact name. */
export function hookMatchesTool(matcher: string, tool: string): boolean {
  const trimmed = matcher.trim()
  if (trimmed === '' || trimmed === '*') return true
  if (trimmed.endsWith('*')) return tool.startsWith(trimmed.slice(0, -1))
  return trimmed === tool
}

/** The verdict a call to `tool` meets under the enabled rules for `event`: deny beats ask, ask beats allow, no match allows. */
export function hookVerdict(
  rules: readonly HookRule[],
  event: HookEvent,
  tool: string,
): { verdict: HookVerdict; matching: HookRule[] } {
  const matching = rules.filter((rule) => rule.enabled && rule.event === event && hookMatchesTool(rule.tool_matcher, tool))
  if (matching.some((rule) => rule.decision === 'deny')) return { verdict: 'deny', matching }
  return { verdict: matching.length > 0 ? 'ask' : 'allow', matching }
}

type HookForm = {
  event: HookEvent
  tool: string
  decision: HookDecision
  reason: string
  priority: string
  enabled: boolean
}

const emptyForm = (): HookForm => ({
  event: 'pre_tool',
  tool: '',
  decision: 'deny',
  reason: '',
  priority: '0',
  enabled: true,
})

function formFromRule(rule: HookRule): HookForm {
  return {
    event: rule.event,
    tool: rule.tool_matcher === '*' ? '' : rule.tool_matcher,
    decision: rule.decision,
    reason: rule.reason,
    priority: String(rule.priority),
    enabled: rule.enabled,
  }
}

/** A post-tool rule may only deny: the tool has already acted by the time its result is seen. */
function bodyFromForm(form: HookForm): HookRuleBody {
  const tool = form.tool.trim()
  return {
    event: form.event,
    tool_matcher: tool === '' ? '*' : tool,
    decision: form.event === 'post_tool' ? 'deny' : form.decision,
    reason: form.reason.trim(),
    priority: Number(form.priority.trim()),
    enabled: form.enabled,
  }
}

function formProblem(form: HookForm, i18n: I18nText): string | null {
  if (!MATCHER_PATTERN.test(form.tool.trim())) {
    return i18n.tr(
      'Verktøynavnet kan bare ha bokstaver, tall, _, ., : og -, og en * til slutt.',
      'The tool name may use only letters, digits, _, ., : and -, with an optional * at the end.',
    )
  }
  const reason = form.reason.trim()
  if (reason === '') {
    return i18n.tr('Skriv en begrunnelse. Den vises når regelen griper inn.', 'Write a reason. It is shown when the rule applies.')
  }
  if (reason.length > MAX_REASON_LENGTH) {
    return i18n.tr(`Begrunnelsen kan være høyst ${MAX_REASON_LENGTH} tegn.`, `The reason can be at most ${MAX_REASON_LENGTH} characters.`)
  }
  const priority = form.priority.trim()
  if (!/^-?\d+$/.test(priority) || Number(priority) > 2147483647 || Number(priority) < -2147483648) {
    return i18n.tr('Prioriteten må være et helt tall.', 'The priority must be a whole number.')
  }
  return null
}

function eventLabel(event: HookEvent, i18n: I18nText): string {
  return event === 'pre_tool'
    ? i18n.tr('Før verktøykallet', 'Before the tool call')
    : i18n.tr('Etter verktøykallet', 'After the tool call')
}

function decisionLabel(decision: HookDecision, i18n: I18nText): string {
  return decision === 'deny' ? i18n.tr('Nekt', 'Deny') : i18n.tr('Spør først', 'Ask first')
}

function verdictText(verdict: HookVerdict, i18n: I18nText): string {
  switch (verdict) {
    case 'deny':
      return i18n.tr('Verktøyet nektes.', 'The tool is denied.')
    case 'ask':
      return i18n.tr('Verktøyet krever godkjenning først.', 'The tool asks for approval first.')
    case 'allow':
      return i18n.tr('Verktøyet tillates: ingen regel treffer.', 'The tool is allowed: no rule matches.')
  }
}

function toolText(matcher: string, i18n: I18nText): string {
  return matcher === '' || matcher === '*' ? i18n.tr('Alle verktøy', 'Any tool') : matcher
}

export function HooksSection() {
  const i18n = useI18n()
  const session = getSession()
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const [listing, { refetch }] = createResource(() => listOrgHookRules())

  // The open editor: a new rule when `editingId` is unset, otherwise an edit of that rule.
  const [form, setForm] = createSignal<HookForm | undefined>(undefined)
  const [editingId, setEditingId] = createSignal<string | undefined>(undefined)
  const [saving, setSaving] = createSignal(false)
  const [problem, setProblem] = createSignal<string | null>(null)

  const [testTool, setTestTool] = createSignal('')
  const [testEvent, setTestEvent] = createSignal<HookEvent>('pre_tool')
  const testResult = createMemo(() => {
    const tool = testTool().trim()
    if (tool === '') return undefined
    return hookVerdict(listing()?.rules ?? [], testEvent(), tool)
  })

  const eventOptions = (): Array<{ value: HookEvent; label: string }> => [
    { value: 'pre_tool', label: eventLabel('pre_tool', i18n) },
    { value: 'post_tool', label: eventLabel('post_tool', i18n) },
  ]

  const decisionOptions = (event: HookEvent): Array<{ value: HookDecision; label: string }> =>
    event === 'post_tool'
      ? [{ value: 'deny', label: decisionLabel('deny', i18n) }]
      : [
          { value: 'deny', label: decisionLabel('deny', i18n) },
          { value: 'ask', label: decisionLabel('ask', i18n) },
        ]

  const closeEditor = () => {
    setForm(undefined)
    setEditingId(undefined)
    setProblem(null)
  }

  const openNew = () => {
    setEditingId(undefined)
    setProblem(null)
    setForm(emptyForm())
  }

  const openEdit = (rule: HookRule) => {
    setEditingId(rule.id)
    setProblem(null)
    setForm(formFromRule(rule))
  }

  const patchForm = (change: Partial<HookForm>) => {
    const base = form()
    if (base) setForm({ ...base, ...change })
  }

  const save = async () => {
    const current = form()
    if (!isAdmin() || !current || saving()) return
    const wrong = formProblem(current, i18n)
    if (wrong) {
      setProblem(wrong)
      return
    }
    const id = editingId()
    setSaving(true)
    setProblem(null)
    try {
      if (id === undefined) await createOrgHookRule(bodyFromForm(current))
      else await updateOrgHookRule(id, bodyFromForm(current))
      closeEditor()
    } catch (error) {
      setProblem(translateApiError(error, i18n.tr, { no: 'Kunne ikke lagre regelen.', en: 'Could not save the rule.' }))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetch()])
  }

  const retire = async (rule: HookRule) => {
    if (!isAdmin() || saving()) return
    setSaving(true)
    setProblem(null)
    try {
      await retireOrgHookRule(rule.id)
      if (editingId() === rule.id) closeEditor()
    } catch (error) {
      setProblem(translateApiError(error, i18n.tr, { no: 'Kunne ikke fjerne regelen.', en: 'Could not retire the rule.' }))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetch()])
  }

  return (
    <section class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Hooks', 'Hooks')}
        description={i18n.tr(
          'Regler som nekter eller spør først om et verktøykall i agentkjøringene i organisasjonen. En nekt vinner over en spørring, og en spørring over en tillatelse. Prioritet bestemmer bare rekkefølgen i listen.',
          'Rules that deny, or ask first about, a tool call in the agent runs of the organisation. A deny beats an ask, and an ask beats an allow. Priority only orders the list.',
        )}
      />

      <Show when={!isAdmin()}>
        <p class="verevon-settings-subnote">
          {i18n.tr('Bare organisasjonsadministratorer kan legge til eller endre regler.', 'Only organisation administrators can add or change rules.')}
        </p>
      </Show>

      <Show when={listing.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste hook-reglene.', 'Could not load the hook rules.')}
        </p>
      </Show>

      <Show
        when={(listing()?.rules.length ?? 0) > 0}
        fallback={
          <Show when={listing()}>
            <p class="verevon-settings-subnote">{i18n.tr('Ingen regler ennå.', 'No rules yet.')}</p>
          </Show>
        }
      >
        <table>
          <thead>
            <tr>
              <th scope="col">{i18n.tr('Når', 'When')}</th>
              <th scope="col">{i18n.tr('Verktøy', 'Tool')}</th>
              <th scope="col">{i18n.tr('Beslutning', 'Decision')}</th>
              <th scope="col">{i18n.tr('Begrunnelse', 'Reason')}</th>
              <th scope="col">{i18n.tr('Prioritet', 'Priority')}</th>
              <th scope="col">{i18n.tr('Status', 'Status')}</th>
              <Show when={isAdmin()}>
                <th scope="col">{i18n.tr('Handlinger', 'Actions')}</th>
              </Show>
            </tr>
          </thead>
          <tbody>
            <For each={listing()?.rules ?? []}>
              {(rule) => (
                <tr>
                  <td>{eventLabel(rule.event, i18n)}</td>
                  <td>{toolText(rule.tool_matcher, i18n)}</td>
                  <td>{decisionLabel(rule.decision, i18n)}</td>
                  <td>{rule.reason}</td>
                  <td>{rule.priority}</td>
                  <td>{rule.enabled ? i18n.tr('På', 'On') : i18n.tr('Av', 'Off')}</td>
                  <Show when={isAdmin()}>
                    <td>
                      <SettingsButton settingsSize="xs" disabled={saving()} onClick={() => openEdit(rule)}>
                        <Pencil class="size-3" aria-hidden="true" />
                        {i18n.tr('Rediger', 'Edit')}
                      </SettingsButton>{' '}
                      <SettingsButton settingsSize="xs" danger disabled={saving()} onClick={() => void retire(rule)}>
                        <Trash2 class="size-3" aria-hidden="true" />
                        {i18n.tr('Fjern', 'Retire')}
                      </SettingsButton>
                    </td>
                  </Show>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>

      <Show when={isAdmin() && !form()}>
        <div class="verevon-settings-actions">
          <p />
          <div>
            <SettingsButton variant="primary" disabled={saving()} onClick={openNew}>
              <Plus class="size-4" aria-hidden="true" />
              {i18n.tr('Legg til regel', 'Add rule')}
            </SettingsButton>
          </div>
        </div>
      </Show>

      <Show when={isAdmin() ? form() : undefined}>
        {(current) => (
          <div class="verevon-settings-panel">
            <SettingsSelect
              id="hook-event"
              label={i18n.tr('Når regelen gjelder', 'When the rule applies')}
              value={current().event}
              disabled={saving()}
              options={eventOptions()}
              onChange={(event) => {
                const next = event.currentTarget.value as HookEvent
                patchForm({ event: next, decision: next === 'post_tool' ? 'deny' : current().decision })
              }}
            />
            <SettingsField
              id="hook-tool"
              label={i18n.tr('Verktøy', 'Tool')}
              value={current().tool}
              placeholder={i18n.tr('shell, mcp__server__*, eller tomt for alle', 'shell, mcp__server__*, or empty for any')}
              helpText={i18n.tr('Bruk et eksakt navn, et prefiks med * til slutt, eller la feltet stå tomt for alle verktøy.', 'Use an exact name, a prefix ending in *, or leave it empty for any tool.')}
              onInput={(event) => patchForm({ tool: event.currentTarget.value })}
            />
            <SettingsSelect
              id="hook-decision"
              label={i18n.tr('Beslutning', 'Decision')}
              value={current().event === 'post_tool' ? 'deny' : current().decision}
              disabled={saving()}
              options={decisionOptions(current().event)}
              onChange={(event) => patchForm({ decision: event.currentTarget.value as HookDecision })}
            />
            <SettingsField
              id="hook-reason"
              label={i18n.tr('Begrunnelse', 'Reason')}
              value={current().reason}
              helpText={i18n.tr('Vises når regelen griper inn.', 'Shown when the rule applies.')}
              onInput={(event) => patchForm({ reason: event.currentTarget.value })}
            />
            <SettingsField
              id="hook-priority"
              label={i18n.tr('Prioritet', 'Priority')}
              type="number"
              value={current().priority}
              helpText={i18n.tr('Bestemmer bare rekkefølgen i listen.', 'Only orders the list.')}
              onInput={(event) => patchForm({ priority: event.currentTarget.value })}
            />
            <ToggleRow
              title={i18n.tr('Regelen er på', 'Rule is on')}
              description={i18n.tr('Regelen gjelder bare når den er på.', 'The rule applies only while it is on.')}
              enabled={current().enabled}
              disabled={saving()}
              onChange={(checked) => patchForm({ enabled: checked })}
            />
            <Show when={problem()}>
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">{problem()}</p>
            </Show>
            <div class="verevon-settings-actions">
              <p />
              <div>
                <SettingsButton onClick={closeEditor} disabled={saving()}>
                  {i18n.tr('Avbryt', 'Cancel')}
                </SettingsButton>
                <SettingsButton variant="primary" onClick={() => void save()} disabled={saving()}>
                  {editingId() === undefined ? i18n.tr('Legg til regel', 'Add rule') : i18n.tr('Lagre regel', 'Save rule')}
                </SettingsButton>
              </div>
            </div>
          </div>
        )}
      </Show>

      <div class="verevon-settings-panel">
        <h3>{i18n.tr('Test en regel', 'Test a rule')}</h3>
        <p class="verevon-settings-subnote">
          {i18n.tr('Skriv et verktøynavn for å se hvilke aktive regler som treffer, og hva som skjer.', 'Type a tool name to see which active rules match, and what happens.')}
        </p>
        <SettingsField
          id="hook-test-tool"
          label={i18n.tr('Verktøynavn', 'Tool name')}
          value={testTool()}
          placeholder={i18n.tr('for eksempel shell', 'for example shell')}
          onInput={(event) => setTestTool(event.currentTarget.value)}
        />
        <SettingsSelect
          id="hook-test-event"
          label={i18n.tr('Når', 'When')}
          value={testEvent()}
          options={eventOptions()}
          onChange={(event) => setTestEvent(event.currentTarget.value as HookEvent)}
        />
        <Show when={testResult()}>
          {(result) => (
            <div role="status">
              <p>{verdictText(result().verdict, i18n)}</p>
              <Show when={result().matching.length > 0}>
                <ul>
                  <For each={result().matching}>
                    {(rule) => (
                      <li>
                        {toolText(rule.tool_matcher, i18n)}: {decisionLabel(rule.decision, i18n)}. {rule.reason}
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </div>
          )}
        </Show>
      </div>
    </section>
  )
}
