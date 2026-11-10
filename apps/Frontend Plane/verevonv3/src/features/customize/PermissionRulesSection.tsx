import { createMemo, createSignal, For, Show } from 'solid-js'
import { Plus, Trash2 } from '@/shared/icons'
import { requestJson } from '@/shared/api/http'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  SectionHeader,
  SettingsButton,
  SettingsField,
  SettingsSelect,
} from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Organisation permission rules (W5-074-c, F-t3-074): a deny or ask rule on one tool, for one target. The target is a
 * web domain, a mail recipient's domain (never the address), a command prefix, or every target of the tool. A rule
 * only tightens a call: there is no allow. capability-core stores the rules and applies the strictest decision a call
 * meets. The gateway forwards the requests, and only an organisation admin may add or remove a rule.
 */

type I18nText = { tr: (noText: string, enText: string) => string }

export type PermissionRuleDecision = 'deny' | 'ask'
export type PermissionTargetKind = 'domain' | 'recipient' | 'command_prefix' | 'any'

export type PermissionRule = {
  id: string
  tool_pattern: string
  target_kind: PermissionTargetKind
  target_pattern: string
  decision: PermissionRuleDecision
  created_by: string
  created_at: string
}

type PermissionRuleBody = Pick<PermissionRule, 'tool_pattern' | 'target_kind' | 'target_pattern' | 'decision'>

type PermissionRuleListing = {
  org_id: string
  revision: number
  rules: PermissionRule[]
}

const RULES_PATH = '/api/v1/org/permission-rules'
/** The tool-name grammar of the gateway: `*` for any tool, a name, or a name ending in one `*`. */
const TOOL_PATTERN = /^(\*|[A-Za-z0-9][A-Za-z0-9_.:-]{0,199}\*?)$/
const DNS_LABEL = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/
/** A command token is the first word execution-core names: letters, digits and . _ - / +, at most 64 characters. */
const COMMAND_TOKEN = /^[A-Za-z0-9._/+-]{1,64}$/
const MAX_PATTERN_LENGTH = 200

function listPermissionRules(): Promise<PermissionRuleListing> {
  return requestJson<PermissionRuleListing>(RULES_PATH)
}

function createPermissionRule(body: PermissionRuleBody): Promise<PermissionRule> {
  return requestJson<PermissionRule>(RULES_PATH, { method: 'POST', body: JSON.stringify(body) })
}

/** Removes a rule. The server records the removal in the audit log. */
function deletePermissionRule(id: string): Promise<unknown> {
  return requestJson(`${RULES_PATH}/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

/** Whether a domain pattern is valid: `*`, a DNS name, or `*.` and a DNS name (the name and its subdomains). */
export function isDomainPattern(value: string): boolean {
  if (value === '*') return true
  const name = value.startsWith('*.') ? value.slice(2) : value
  return name.length > 0 && name.length <= MAX_PATTERN_LENGTH && name.split('.').every((label) => DNS_LABEL.test(label))
}

/** Whether a command pattern is valid: `*`, or a command token with an optional `*` at the end. */
export function isCommandPattern(value: string): boolean {
  if (value === '*') return true
  const token = value.endsWith('*') ? value.slice(0, -1) : value
  return COMMAND_TOKEN.test(token)
}

type PermissionRuleForm = {
  tool: string
  targetKind: PermissionTargetKind
  target: string
  decision: PermissionRuleDecision
}

const emptyForm = (): PermissionRuleForm => ({
  tool: '',
  targetKind: 'domain',
  target: '',
  decision: 'deny',
})

/** The body the gateway takes. A domain is sent in lower case, and an any-target rule takes `*` whatever the form held. */
function bodyFromForm(form: PermissionRuleForm): PermissionRuleBody {
  const target = form.targetKind === 'any' ? '*' : form.target.trim()
  const isDomainKind = form.targetKind === 'domain' || form.targetKind === 'recipient'
  return {
    tool_pattern: form.tool.trim(),
    target_kind: form.targetKind,
    target_pattern: isDomainKind ? target.toLowerCase() : target,
    decision: form.decision,
  }
}

function formProblem(form: PermissionRuleForm, i18n: I18nText): string | null {
  const tool = form.tool.trim()
  if (!TOOL_PATTERN.test(tool) || tool.length > MAX_PATTERN_LENGTH) {
    return i18n.tr(
      'Verktøynavnet kan bare ha bokstaver, tall, _, ., : og -, og en * til slutt.',
      'The tool name may use only letters, digits, _, ., : and -, with an optional * at the end.',
    )
  }
  const target = form.target.trim()
  if (form.targetKind === 'domain' || form.targetKind === 'recipient') {
    if (isDomainPattern(target.toLowerCase())) return null
    return i18n.tr(
      'Skriv et gyldig domene, for eksempel example.com eller *.example.com. Skriv aldri en e-postadresse.',
      'Write a valid domain, for example example.com or *.example.com. Never write an email address.',
    )
  }
  if (form.targetKind === 'command_prefix' && !isCommandPattern(target)) {
    return i18n.tr(
      'Kommandoprefikset kan bare ha bokstaver, tall, ., _, -, / og +, og en * til slutt.',
      'The command prefix may use only letters, digits, ., _, -, / and +, with an optional * at the end.',
    )
  }
  return null
}

function kindLabel(kind: PermissionTargetKind, i18n: I18nText): string {
  switch (kind) {
    case 'domain':
      return i18n.tr('Nettsted (domene)', 'Web domain')
    case 'recipient':
      return i18n.tr('Mottakerdomene', 'Recipient domain')
    case 'command_prefix':
      return i18n.tr('Kommandoprefiks', 'Command prefix')
    case 'any':
      return i18n.tr('Alle mål', 'Any target')
  }
}

function decisionLabel(decision: PermissionRuleDecision, i18n: I18nText): string {
  return decision === 'deny' ? i18n.tr('Nekt', 'Deny') : i18n.tr('Spør først', 'Ask first')
}

function toolText(pattern: string, i18n: I18nText): string {
  return pattern === '*' ? i18n.tr('Alle verktøy', 'Any tool') : pattern
}

function targetText(rule: PermissionRule, i18n: I18nText): string {
  if (rule.target_kind === 'any') return kindLabel('any', i18n)
  return `${kindLabel(rule.target_kind, i18n)}: ${rule.target_pattern}`
}

function targetHelp(kind: PermissionTargetKind, i18n: I18nText): string {
  switch (kind) {
    case 'domain':
      return i18n.tr(
        'Et domene, for eksempel example.com. *.example.com gjelder også underdomener, og * gjelder alle domener.',
        'A domain, for example example.com. *.example.com also covers its subdomains, and * covers every domain.',
      )
    case 'recipient':
      return i18n.tr(
        'Domenet til mottakeren, ikke adressen, for eksempel example.com.',
        'The domain of the recipient, not the address, for example example.com.',
      )
    case 'command_prefix':
      return i18n.tr(
        'Det første ordet i kommandoen, for eksempel git. En * til slutt, som git*, tar alle kommandoer som starter slik.',
        'The first word of the command, for example git. A * at the end, as in git*, covers every command that starts that way.',
      )
    case 'any':
      return ''
  }
}

function targetKindOptions(i18n: I18nText): Array<{ value: PermissionTargetKind; label: string }> {
  return [
    { value: 'domain', label: kindLabel('domain', i18n) },
    { value: 'recipient', label: kindLabel('recipient', i18n) },
    { value: 'command_prefix', label: kindLabel('command_prefix', i18n) },
    { value: 'any', label: kindLabel('any', i18n) },
  ]
}

function decisionOptions(i18n: I18nText): Array<{ value: PermissionRuleDecision; label: string }> {
  return [
    { value: 'deny', label: decisionLabel('deny', i18n) },
    { value: 'ask', label: decisionLabel('ask', i18n) },
  ]
}

export function PermissionRulesSection() {
  const i18n = useI18n()
  const session = getSession()
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const [listing, { refetch }] = createResource(() => listPermissionRules())

  // The open editor is a new rule, or undefined while the list is shown.
  const [form, setForm] = createSignal<PermissionRuleForm | undefined>(undefined)
  const [saving, setSaving] = createSignal(false)
  const [problem, setProblem] = createSignal<string | null>(null)

  const closeEditor = () => {
    setForm(undefined)
    setProblem(null)
  }

  const openNew = () => {
    setProblem(null)
    setForm(emptyForm())
  }

  const patchForm = (change: Partial<PermissionRuleForm>) => {
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
    setSaving(true)
    setProblem(null)
    try {
      await createPermissionRule(bodyFromForm(current))
      closeEditor()
    } catch (error) {
      setProblem(translateApiError(error, i18n.tr, { no: 'Kunne ikke lagre regelen.', en: 'Could not save the rule.' }))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetch()])
  }

  const remove = async (rule: PermissionRule) => {
    if (!isAdmin() || saving()) return
    setSaving(true)
    setProblem(null)
    try {
      await deletePermissionRule(rule.id)
    } catch (error) {
      setProblem(translateApiError(error, i18n.tr, { no: 'Kunne ikke fjerne regelen.', en: 'Could not remove the rule.' }))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetch()])
  }

  return (
    <section class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Tillatelsesregler', 'Permission rules')}
        description={i18n.tr(
          'Regler som nekter eller spør først om et verktøykall, for et nettsted, et mottakerdomene eller et kommandoprefiks. En regel strammer bare inn: den kan aldri gi tillatelse. Den strengeste beslutningen gjelder.',
          'Rules that deny, or ask first about, one tool call for a web domain, a recipient domain or a command prefix. A rule only tightens a call: it never allows one. The strictest decision applies.',
        )}
      />

      <Show when={!isAdmin()}>
        <p class="verevon-settings-subnote">
          {i18n.tr('Bare organisasjonsadministratorer kan legge til eller fjerne regler.', 'Only organisation administrators can add or remove rules.')}
        </p>
      </Show>

      <Show when={listing.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste tillatelsesreglene.', 'Could not load the permission rules.')}
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
              <th scope="col">{i18n.tr('Verktøy', 'Tool')}</th>
              <th scope="col">{i18n.tr('Mål', 'Target')}</th>
              <th scope="col">{i18n.tr('Beslutning', 'Decision')}</th>
              <Show when={isAdmin()}>
                <th scope="col">{i18n.tr('Handlinger', 'Actions')}</th>
              </Show>
            </tr>
          </thead>
          <tbody>
            <For each={listing()?.rules ?? []}>
              {(rule) => (
                <tr>
                  <td>{toolText(rule.tool_pattern, i18n)}</td>
                  <td>{targetText(rule, i18n)}</td>
                  <td>{decisionLabel(rule.decision, i18n)}</td>
                  <Show when={isAdmin()}>
                    <td>
                      <SettingsButton settingsSize="xs" danger disabled={saving()} onClick={() => void remove(rule)}>
                        <Trash2 class="size-3" aria-hidden="true" />
                        {i18n.tr('Fjern', 'Remove')}
                      </SettingsButton>
                    </td>
                  </Show>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>

      <Show when={problem()}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">{problem()}</p>
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
            <SettingsField
              id="permission-rule-tool"
              label={i18n.tr('Verktøy', 'Tool')}
              value={current().tool}
              placeholder={i18n.tr('web_fetch, mcp__server__*, eller *', 'web_fetch, mcp__server__*, or *')}
              helpText={i18n.tr('Bruk et eksakt navn, et prefiks med * til slutt, eller * for alle verktøy.', 'Use an exact name, a prefix ending in *, or * for every tool.')}
              onInput={(event) => patchForm({ tool: event.currentTarget.value })}
            />
            <SettingsSelect
              id="permission-rule-kind"
              label={i18n.tr('Mål', 'Target')}
              value={current().targetKind}
              disabled={saving()}
              options={targetKindOptions(i18n)}
              onChange={(event) => patchForm({ targetKind: event.currentTarget.value as PermissionTargetKind })}
            />
            <Show when={current().targetKind !== 'any'}>
              <SettingsField
                id="permission-rule-target"
                label={kindLabel(current().targetKind, i18n)}
                value={current().target}
                placeholder={i18n.tr('example.com eller *.example.com', 'example.com or *.example.com')}
                helpText={targetHelp(current().targetKind, i18n)}
                onInput={(event) => patchForm({ target: event.currentTarget.value })}
              />
            </Show>
            <SettingsSelect
              id="permission-rule-decision"
              label={i18n.tr('Beslutning', 'Decision')}
              value={current().decision}
              disabled={saving()}
              options={decisionOptions(i18n)}
              onChange={(event) => patchForm({ decision: event.currentTarget.value as PermissionRuleDecision })}
            />
            <div class="verevon-settings-actions">
              <p />
              <div>
                <SettingsButton onClick={closeEditor} disabled={saving()}>
                  {i18n.tr('Avbryt', 'Cancel')}
                </SettingsButton>
                <SettingsButton variant="primary" onClick={() => void save()} disabled={saving()}>
                  {i18n.tr('Legg til regel', 'Add rule')}
                </SettingsButton>
              </div>
            </div>
          </div>
        )}
      </Show>
    </section>
  )
}
