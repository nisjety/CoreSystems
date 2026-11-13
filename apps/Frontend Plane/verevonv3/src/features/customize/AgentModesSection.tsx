import { createMemo, createSignal, For, Show } from 'solid-js'
import { Lock, PauseCircle, ShieldCheck } from '@/shared/icons'
import { ApiError, requestJson } from '@/shared/api/http'
import type { AgentModeId, AutonomyRung } from '@/shared/api/chat-client'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  SectionHeader,
  SettingsButton,
  SettingsSelect,
  SettingsTextarea,
  ToggleRow,
} from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Agent modes and the organisation's agent policy (WS25-S11): the strictest mode agents may run in,
 * the highest autonomy rung they may be granted, lockdown, the agent-run pause (F-t3-083) and the MCP
 * allow-list. capability-core is the system of record (DEC-14), read and written through the gateway's
 * `/api/v1/org/agent-policy` routes.
 * A saved policy takes effect only where the deployment has switched policy enforcement on, so the copy
 * says so instead of implying the cap already holds.
 *
 * Members see the same values read-only with a locked badge: only an organisation admin may write, and
 * the section does not offer a control it would then have refused.
 */

type I18nText = { tr: (noText: string, enText: string) => string }

/** The agent modes, strictest first: the order of api/contracts/agent-modes.json. */
const AGENT_MODE_ORDER: readonly AgentModeId[] = ['lockdown', 'read_only', 'ask_first', 'auto']
const AGENT_MODE_STRICTNESS: Readonly<Record<AgentModeId, number>> = { lockdown: 3, read_only: 2, ask_first: 1, auto: 0 }

const AUTONOMY_RUNGS: readonly AutonomyRung[] = ['read_only', 'workspace_write', 'danger_full_access']
const ALLOW_LIST_MODES = ['none', 'approved_only', 'any'] as const
const MCP_TOOL_APPROVALS = ['approved_only', 'trust_annotations'] as const

type AllowListMode = (typeof ALLOW_LIST_MODES)[number]
type McpToolApproval = (typeof MCP_TOOL_APPROVALS)[number]

/** The settings the section writes: every key of the closed schema the admin can set, as the BFF takes them. */
type OrgAgentPolicySettings = {
  mode_cap: AgentModeId
  rung_cap: AutonomyRung
  lockdown: boolean
  /** The agents-paused switch (F-t3-083), stored under the key `agents.paused`. */
  'agents.paused': boolean
  'ext.mcp_servers': { mode: AllowListMode; ids: string[] }
  mcp_tool_approval: McpToolApproval
}

/** The one write the pause control makes: `pause` turns agent runs off for the organisation, `resume` turns them on. */
type PauseChange = 'pause' | 'resume'

/** The stored document. A key it does not hold reads as its schema default, as the server answers it. */
type OrgAgentPolicyDocument = {
  org_id: string
  revision: number
  digest: string
  settings: Record<string, unknown>
}

type EffectiveSetting = {
  key: string
  kind: string
  status: string
  value: unknown
  source: string
  fail_closed_value: unknown
}

type EffectiveOrgAgentPolicy = {
  org_id: string
  revision: number
  digest: string
  settings: EffectiveSetting[]
}

/** The schema defaults (org-agent-policy-schema.json): what an org with no row is governed by. */
const SCHEMA_DEFAULTS: OrgAgentPolicySettings = {
  mode_cap: 'auto',
  rung_cap: 'danger_full_access',
  lockdown: false,
  'agents.paused': false,
  'ext.mcp_servers': { mode: 'any', ids: [] },
  mcp_tool_approval: 'trust_annotations',
}

const POLICY_PATH = '/api/v1/org/agent-policy'
const SERVER_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,199}$/
const MAX_SERVER_IDS = 64

/** The modes a cap admits: the cap itself and every stricter mode. The composer offers exactly these. */
export function agentModesWithinCap(cap: AgentModeId): AgentModeId[] {
  return AGENT_MODE_ORDER.filter((mode) => AGENT_MODE_STRICTNESS[mode] >= AGENT_MODE_STRICTNESS[cap])
}

function isAgentModeId(value: unknown): value is AgentModeId {
  return typeof value === 'string' && Object.hasOwn(AGENT_MODE_STRICTNESS, value)
}

/** The name a person sees for a mode, in the active locale. The composer and the section share it. */
export function agentModeLabel(mode: AgentModeId, i18n: I18nText): string {
  switch (mode) {
    case 'lockdown':
      return i18n.tr('Låst ned: ingen agentkjøringer', 'Lockdown: no agent runs')
    case 'read_only':
      return i18n.tr('Kun lesing: planlegg, ingen skriving', 'Read only: plan, no writes')
    case 'ask_first':
      return i18n.tr('Spør først: godkjenning før risikable verktøy', 'Ask first: approval before risky tools')
    case 'auto':
      return i18n.tr('Auto: innenfor plattformens grenser', 'Auto: within platform limits')
  }
}

function agentModeSummary(mode: AgentModeId, i18n: I18nText): string {
  switch (mode) {
    case 'lockdown':
      return i18n.tr('Ingen agentkjøringer (Do). Vanlig chat fungerer som før.', 'No agent runs (Do). Plain chat works as before.')
    case 'read_only':
      return i18n.tr('Agenten planlegger og leser, men skriver ikke. Skrivende verktøy nektes.', 'The agent plans and reads but does not write. Writing tools are refused.')
    case 'ask_first':
      return i18n.tr('Agenten kan kjøre, men spør før risikable verktøy.', 'The agent can run but asks before risky tools.')
    case 'auto':
      return i18n.tr('Agenten kjører uten å spørre, innenfor plattformens grenser.', 'The agent runs without asking, within platform limits.')
  }
}

function isAutonomyRung(value: unknown): value is AutonomyRung {
  return typeof value === 'string' && (AUTONOMY_RUNGS as readonly string[]).includes(value)
}

function autonomyRungLabel(rung: AutonomyRung, i18n: I18nText): string {
  switch (rung) {
    case 'read_only':
      return i18n.tr('Kun lesing', 'Read only')
    case 'workspace_write':
      return i18n.tr('Skriv i arbeidsområdet', 'Write in the workspace')
    case 'danger_full_access':
      return i18n.tr('Full tilgang', 'Full access')
  }
}

function isAllowListMode(value: unknown): value is AllowListMode {
  return typeof value === 'string' && (ALLOW_LIST_MODES as readonly string[]).includes(value)
}

function allowListModeLabel(mode: AllowListMode, i18n: I18nText): string {
  switch (mode) {
    case 'none':
      return i18n.tr('Ingen servere', 'No servers')
    case 'approved_only':
      return i18n.tr('Bare godkjente servere', 'Approved servers only')
    case 'any':
      return i18n.tr('Alle servere', 'Any server')
  }
}

function isMcpToolApproval(value: unknown): value is McpToolApproval {
  return typeof value === 'string' && (MCP_TOOL_APPROVALS as readonly string[]).includes(value)
}

function mcpToolApprovalLabel(approval: McpToolApproval, i18n: I18nText): string {
  switch (approval) {
    case 'approved_only':
      return i18n.tr('Bare godkjente verktøy', 'Approved tools only')
    case 'trust_annotations':
      return i18n.tr('Stol på verktøyets merknader', 'Trust tool annotations')
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * The agents-paused switch as the server enforces it (F-t3-083): a key the document omits is off, and a value this
 * build cannot read is on (the schema's fail-closed value). The section never shows the organisation running when
 * the server would refuse its agents.
 */
function readAgentsPaused(raw: Record<string, unknown>): boolean {
  const value = raw['agents.paused']
  if (value === undefined) return SCHEMA_DEFAULTS['agents.paused']
  return typeof value === 'boolean' ? value : true
}

/** Reads a stored document into the typed settings; anything malformed reads as its schema default. */
function readSettings(raw: Record<string, unknown>): OrgAgentPolicySettings {
  const list = raw['ext.mcp_servers']
  const allowList = isRecord(list) && isAllowListMode(list.mode) && Array.isArray(list.ids)
    ? { mode: list.mode, ids: list.ids.filter((id): id is string => typeof id === 'string') }
    : SCHEMA_DEFAULTS['ext.mcp_servers']
  return {
    mode_cap: isAgentModeId(raw.mode_cap) ? raw.mode_cap : SCHEMA_DEFAULTS.mode_cap,
    rung_cap: isAutonomyRung(raw.rung_cap) ? raw.rung_cap : SCHEMA_DEFAULTS.rung_cap,
    lockdown: typeof raw.lockdown === 'boolean' ? raw.lockdown : SCHEMA_DEFAULTS.lockdown,
    'agents.paused': readAgentsPaused(raw),
    'ext.mcp_servers': allowList,
    mcp_tool_approval: isMcpToolApproval(raw.mcp_tool_approval) ? raw.mcp_tool_approval : SCHEMA_DEFAULTS.mcp_tool_approval,
  }
}

/** Server ids as typed: one per line or comma separated, trimmed, blanks and duplicates dropped. */
export function parseServerIds(text: string): string[] {
  const seen = new Set<string>()
  for (const part of text.split(/[\n,]/)) {
    const id = part.trim()
    if (id) seen.add(id)
  }
  return [...seen]
}

/** The first thing wrong with the server ids, in the active locale, or null. Checked before a save. */
function serverIdProblem(ids: readonly string[], i18n: I18nText): string | null {
  if (ids.length > MAX_SERVER_IDS) {
    return i18n.tr(`Høyst ${MAX_SERVER_IDS} MCP-servere kan godkjennes.`, `At most ${MAX_SERVER_IDS} MCP servers can be approved.`)
  }
  const invalid = ids.find((id) => !SERVER_ID_PATTERN.test(id))
  if (invalid === undefined) return null
  return i18n.tr(
    `«${invalid}» er ikke en gyldig server-ID. Bruk bokstaver, tall, _, ., : og -.`,
    `"${invalid}" is not a valid server ID. Use letters, digits, _, ., : and -.`,
  )
}

// The policy API answers 404 while ORG_AGENT_POLICY_API is off, so that answer is cached for the page:
// the deployment does not change under a session. Any other failure is not cached, so a later call retries.
let availability: Promise<boolean> | undefined

/** Whether this deployment serves the organisation agent policy at all. Rejects on a failure other than "off". */
export function orgAgentPolicyAvailable(): Promise<boolean> {
  availability ??= getOrgAgentPolicy().then(
    () => true,
    (error: unknown) => {
      if (error instanceof ApiError && error.status === 404) return false
      availability = undefined
      throw error
    },
  )
  return availability
}

export function __resetOrgAgentPolicyAvailabilityForTests(): void {
  availability = undefined
}

function getOrgAgentPolicy(): Promise<OrgAgentPolicyDocument> {
  return requestJson<OrgAgentPolicyDocument>(POLICY_PATH)
}

function getEffectiveOrgAgentPolicy(): Promise<EffectiveOrgAgentPolicy> {
  return requestJson<EffectiveOrgAgentPolicy>(`${POLICY_PATH}/effective`)
}

/** Replaces the whole document at `revision`. The server takes a missing key as its default, so send all of them. */
function saveOrgAgentPolicy(revision: number, settings: Record<string, unknown>): Promise<OrgAgentPolicyDocument> {
  return requestJson<OrgAgentPolicyDocument>(POLICY_PATH, {
    method: 'PUT',
    body: JSON.stringify({ revision, settings }),
  })
}

/**
 * The mode cap the composer limits its picker to, or undefined where the policy is off or unreadable, so no
 * picker is offered and the server's own posture stands. Never throws.
 */
export async function loadEffectiveAgentModeCap(): Promise<AgentModeId | undefined> {
  try {
    if (!(await orgAgentPolicyAvailable())) return undefined
    const effective = await getEffectiveOrgAgentPolicy()
    const cap = effective.settings.find((setting) => setting.key === 'mode_cap')?.value
    return isAgentModeId(cap) ? cap : undefined
  } catch {
    return undefined
  }
}

type PolicyDraft = { settings: OrgAgentPolicySettings; idsText: string }

function saveFailure(error: unknown, i18n: I18nText): string {
  if (error instanceof ApiError && error.status === 409) {
    return i18n.tr(
      'Policyen er endret etter at du åpnet den. Last siden på nytt og prøv igjen.',
      'The policy changed after you opened it. Reload the page and try again.',
    )
  }
  return translateApiError(error, i18n.tr, {
    no: 'Kunne ikke lagre agentpolicyen.',
    en: 'Could not save the agent policy.',
  })
}

function settingLabel(key: string, i18n: I18nText): string {
  switch (key) {
    case 'mode_cap':
      return i18n.tr('Høyeste agentmodus', 'Highest agent mode')
    case 'rung_cap':
      return i18n.tr('Høyeste tillatelsesnivå', 'Highest permission level')
    case 'lockdown':
      return i18n.tr('Lås ned agenter', 'Lock down agents')
    case 'agents.paused':
      return i18n.tr('Agentkjøringer på pause', 'Agent runs paused')
    case 'ext.mcp_servers':
      return i18n.tr('MCP-servere agenten kan bruke', 'MCP servers the agent may use')
    case 'mcp_tool_approval':
      return i18n.tr('Godkjenning av MCP-verktøy', 'Approval of MCP tools')
    case 'web.mode':
      return i18n.tr('Nettilgang for agentene', 'Web access for agents')
    case 'web.paid_providers':
      return i18n.tr('Betalte søketjenester', 'Paid search services')
    default:
      return key
  }
}

function effectiveValueText(setting: EffectiveSetting, i18n: I18nText): string {
  if (setting.status === 'reserved') return i18n.tr('Ikke i bruk ennå', 'Not in use yet')
  switch (setting.key) {
    case 'mode_cap':
      return isAgentModeId(setting.value) ? agentModeLabel(setting.value, i18n) : String(setting.value)
    case 'rung_cap':
      return isAutonomyRung(setting.value) ? autonomyRungLabel(setting.value, i18n) : String(setting.value)
    case 'lockdown':
    case 'agents.paused':
      return setting.value === true ? i18n.tr('På', 'On') : i18n.tr('Av', 'Off')
    case 'ext.mcp_servers': {
      if (!isRecord(setting.value) || !isAllowListMode(setting.value.mode)) return String(setting.value)
      const count = Array.isArray(setting.value.ids) ? setting.value.ids.length : 0
      return setting.value.mode === 'approved_only'
        ? i18n.tr(`Bare godkjente (${count})`, `Approved only (${count})`)
        : allowListModeLabel(setting.value.mode, i18n)
    }
    case 'mcp_tool_approval':
      return isMcpToolApproval(setting.value) ? mcpToolApprovalLabel(setting.value, i18n) : String(setting.value)
    default:
      return String(setting.value)
  }
}

function effectiveSourceText(source: string, i18n: I18nText): string {
  return source === 'org_policy'
    ? i18n.tr('Organisasjonen', 'Organisation')
    : i18n.tr('Plattformstandard', 'Platform default')
}

function effectiveStatusText(status: string, i18n: I18nText): string {
  return status === 'reserved'
    ? i18n.tr('Reservert, ikke i bruk', 'Reserved, not in use')
    : i18n.tr('Håndheves der håndheving er på', 'Enforced where enforcement is on')
}

function LockedBadge(props: { label: string }) {
  return (
    <span class="verevon-settings-subnote" title={props.label}>
      <Lock size={12} aria-hidden="true" /> {props.label}
    </span>
  )
}

export function AgentModesSection() {
  const i18n = useI18n()
  const session = getSession()
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [stored, { refetch: refetchStored }] = createResource(() => getOrgAgentPolicy())
  const [effective, { refetch: refetchEffective }] = createResource(() => getEffectiveOrgAgentPolicy())

  // `undefined` until an admin edits: the stored document is then what is shown.
  const [draft, setDraft] = createSignal<PolicyDraft | undefined>(undefined)
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [problem, setProblem] = createSignal<string | null>(null)
  // The agents-paused switch writes on its own once the person confirms it, and a save of the draft never writes it.
  const [pauseChange, setPauseChange] = createSignal<PauseChange | null>(null)
  const [pauseNote, setPauseNote] = createSignal<string | null>(null)
  const [pauseProblem, setPauseProblem] = createSignal<string | null>(null)

  const shown = createMemo<PolicyDraft | undefined>(() => {
    const edited = draft()
    if (edited) return edited
    const doc = stored()
    if (!doc) return undefined
    const settings = readSettings(doc.settings)
    return { settings, idsText: settings['ext.mcp_servers'].ids.join('\n') }
  })

  const edit = (change: Partial<OrgAgentPolicySettings>) => {
    const base = shown()
    if (!base) return
    setSaved(false)
    setDraft({ ...base, settings: { ...base.settings, ...change } })
  }

  const editIds = (idsText: string) => {
    const base = shown()
    if (!base) return
    setSaved(false)
    setDraft({ ...base, idsText })
  }

  const save = async () => {
    const base = shown()
    const doc = stored()
    if (!isAdmin() || !base || !doc || saving()) return
    const ids = parseServerIds(base.idsText)
    const wrong = serverIdProblem(ids, i18n)
    if (wrong) {
      setProblem(wrong)
      return
    }
    const mode = base.settings['ext.mcp_servers'].mode
    const settings: OrgAgentPolicySettings = {
      ...base.settings,
      'ext.mcp_servers': { mode, ids: mode === 'approved_only' ? ids : [] },
    }
    setSaving(true)
    setProblem(null)
    setSaved(false)
    try {
      // Keys this section does not edit (the web switches) ride along, so a save here never resets them. The
      // agents-paused switch is written by its own control, so the stored value rides along too, as it is held.
      await saveOrgAgentPolicy(doc.revision, { ...doc.settings, ...settings, 'agents.paused': doc.settings['agents.paused'] })
      setDraft(undefined)
      setSaved(true)
    } catch (error) {
      setProblem(saveFailure(error, i18n))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetchStored(), refetchEffective()])
  }

  /**
   * Writes the agents-paused switch alone, at the revision the page holds, once the person has confirmed it. The other
   * settings go back as they are stored, so edits not yet saved stay unsaved. An open draft takes the new switch, so
   * the switch shows what is held.
   */
  const setPaused = async (paused: boolean) => {
    const doc = stored()
    if (!isAdmin() || !doc || saving()) return
    setPauseChange(null)
    setPauseNote(null)
    setPauseProblem(null)
    setSaving(true)
    try {
      await saveOrgAgentPolicy(doc.revision, { ...doc.settings, 'agents.paused': paused })
      const edited = draft()
      if (edited) setDraft({ ...edited, settings: { ...edited.settings, 'agents.paused': paused } })
      setPauseNote(paused
        ? i18n.tr('Agentkjøringene er satt på pause.', 'Agent runs are paused.')
        : i18n.tr('Agentkjøringene er gjenopptatt.', 'Agent runs are resumed.'))
    } catch (error) {
      setPauseProblem(saveFailure(error, i18n))
    } finally {
      setSaving(false)
    }
    await Promise.allSettled([refetchStored(), refetchEffective()])
  }

  const allowListOptions = () => ALLOW_LIST_MODES.map((mode) => ({ value: mode, label: allowListModeLabel(mode, i18n) }))

  return (
    <section class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Agentmodus', 'Agent mode')}
        description={i18n.tr(
          'Den strengeste modusen agentene i organisasjonen kan kjøre i, den høyeste tillatelsen de kan få, og hvilke MCP-servere de kan bruke. Ingen kan velge en mildere modus i chatten enn grensen her.',
          'The strictest mode agents in the organisation may run in, the highest permission they may be granted, and the MCP servers they may use. Nobody can pick a looser mode in chat than the cap here.',
        )}
      />

      <Show when={!isAdmin()}>
        <p class="verevon-settings-subnote">
          <ShieldCheck size={14} aria-hidden="true" />{' '}
          {i18n.tr(
            'Bare organisasjonsadministratorer kan endre agentpolicyen. Du ser verdiene som gjelder for deg.',
            'Only organisation administrators can change the agent policy. You see the values that apply to you.',
          )}
        </p>
      </Show>

      <Show when={stored.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste agentpolicyen.', 'Could not load the agent policy.')}
        </p>
      </Show>

      <Show when={stored.loading && !shown()}>
        <p class="verevon-settings-subnote" role="status" aria-busy="true">
          {i18n.tr('Laster agentpolicyen…', 'Loading the agent policy…')}
        </p>
      </Show>

      <Show when={shown()}>
        {(current) => (
          <>
            <Show when={!isAdmin() && current().settings['agents.paused']}>
              <p class="verevon-settings-status-message" role="status">
                <PauseCircle size={14} aria-hidden="true" />{' '}
                {i18n.tr(
                  'Agentkjøringene er satt på pause for organisasjonen. Vanlig chat fungerer som før. Be en organisasjonsadministrator om å gjenoppta dem.',
                  'Agent runs are paused for your organisation. Plain chat works as before. Ask an organisation administrator to resume them.',
                )}
              </p>
            </Show>
            <div class="verevon-settings-panel">
              <ToggleRow
                title={i18n.tr('Pause alle agentkjøringer i organisasjonen', 'Pause all agent runs in this organisation')}
                description={i18n.tr(
                  'Stopper nye agentkjøringer og fortsettelser for alle. Vanlig chat blir ikke berørt.',
                  'Stops new agent runs and continuations for everyone. Plain chat is not affected.',
                )}
                enabled={current().settings['agents.paused']}
                disabled={!isAdmin() || saving()}
                onChange={(checked) => {
                  setPauseNote(null)
                  setPauseProblem(null)
                  setPauseChange(checked ? 'pause' : 'resume')
                }}
              />
              <p class="verevon-settings-subnote">
                {isAdmin()
                  ? i18n.tr(
                    'Pausen lagres så snart du har bekreftet. Den gjelder der håndhevingen av agentpolicyen er slått på.',
                    'The pause is saved as soon as you confirm it. It applies where enforcement of the agent policy is on.',
                  )
                  : i18n.tr(
                    'Den gjelder der håndhevingen av agentpolicyen er slått på.',
                    'It applies where enforcement of the agent policy is on.',
                  )}
              </p>
              <Show when={isAdmin() ? pauseChange() : null}>
                {(change) => (
                  <div class="verevon-settings-actions">
                    <p>
                      {change() === 'pause'
                        ? i18n.tr(
                          'Nye agentkjøringer og fortsettelser blir avvist for alle i organisasjonen til pausen slås av. Vanlig chat blir ikke berørt.',
                          'New agent runs and continuations are refused for everyone in the organisation until the pause is turned off. Plain chat is not affected.',
                        )
                        : i18n.tr(
                          'Medlemmene kan starte agentkjøringer igjen, innenfor de andre innstillingene i agentpolicyen.',
                          'Members can start agent runs again, within the other settings of the agent policy.',
                        )}
                    </p>
                    <div>
                      <SettingsButton settingsSize="sm" disabled={saving()} onClick={() => setPauseChange(null)}>
                        {i18n.tr('Avbryt', 'Cancel')}
                      </SettingsButton>{' '}
                      <SettingsButton
                        variant={change() === 'pause' ? undefined : 'primary'}
                        danger={change() === 'pause'}
                        settingsSize="sm"
                        disabled={saving()}
                        onClick={() => void setPaused(change() === 'pause')}
                      >
                        {change() === 'pause'
                          ? i18n.tr('Pause agentkjøringer', 'Pause agent runs')
                          : i18n.tr('Gjenoppta agentkjøringer', 'Resume agent runs')}
                      </SettingsButton>
                    </div>
                  </div>
                )}
              </Show>
              <Show when={isAdmin() ? pauseProblem() : null}>
                {(message) => (
                  <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                    {message()}
                  </p>
                )}
              </Show>
              <Show when={pauseNote()}>
                {(message) => (
                  <p class="verevon-settings-status-message" role="status">
                    {message()}
                  </p>
                )}
              </Show>
            </div>
          </>
        )}
      </Show>

      <Show when={shown()}>
        {(current) => (
          <div class="verevon-settings-panel">
            <Show when={!isAdmin()}>
              <LockedBadge label={i18n.tr('Låst for medlemmer', 'Locked for members')} />
            </Show>
            <SettingsSelect
              id="customize-mode-cap"
              label={settingLabel('mode_cap', i18n)}
              value={current().settings.mode_cap}
              disabled={!isAdmin() || saving()}
              options={AGENT_MODE_ORDER.map((mode) => ({ value: mode, label: agentModeLabel(mode, i18n) }))}
              onChange={(event) => edit({ mode_cap: event.currentTarget.value as AgentModeId })}
            />
            <SettingsSelect
              id="customize-rung-cap"
              label={settingLabel('rung_cap', i18n)}
              value={current().settings.rung_cap}
              disabled={!isAdmin() || saving()}
              options={AUTONOMY_RUNGS.map((rung) => ({ value: rung, label: autonomyRungLabel(rung, i18n) }))}
              onChange={(event) => edit({ rung_cap: event.currentTarget.value as AutonomyRung })}
            />
            <ToggleRow
              title={settingLabel('lockdown', i18n)}
              description={i18n.tr(
                'Stopper agentkjøringer for hele organisasjonen. Vanlig chat blir ikke berørt.',
                'Stops agent runs for the whole organisation. Plain chat is not affected.',
              )}
              enabled={current().settings.lockdown}
              disabled={!isAdmin() || saving()}
              onChange={(checked) => edit({ lockdown: checked })}
            />
            <SettingsSelect
              id="customize-mcp-mode"
              label={settingLabel('ext.mcp_servers', i18n)}
              value={current().settings['ext.mcp_servers'].mode}
              disabled={!isAdmin() || saving()}
              options={allowListOptions()}
              onChange={(event) => edit({
                'ext.mcp_servers': { ...current().settings['ext.mcp_servers'], mode: event.currentTarget.value as AllowListMode },
              })}
            />
            <Show when={current().settings['ext.mcp_servers'].mode === 'approved_only'}>
              <Show
                when={isAdmin()}
                fallback={
                  <p class="verevon-settings-subnote">
                    {current().idsText.trim() === ''
                      ? i18n.tr('Ingen servere er godkjent ennå.', 'No servers are approved yet.')
                      : current().idsText.split('\n').join(', ')}
                  </p>
                }
              >
                <SettingsTextarea
                  id="customize-mcp-ids"
                  label={i18n.tr('Godkjente server-IDer, én per linje', 'Approved server IDs, one per line')}
                  value={current().idsText}
                  onInput={(event) => editIds(event.currentTarget.value)}
                />
              </Show>
            </Show>
            <SettingsSelect
              id="customize-mcp-approval"
              label={settingLabel('mcp_tool_approval', i18n)}
              value={current().settings.mcp_tool_approval}
              disabled={!isAdmin() || saving()}
              options={MCP_TOOL_APPROVALS.map((approval) => ({ value: approval, label: mcpToolApprovalLabel(approval, i18n) }))}
              onChange={(event) => edit({ mcp_tool_approval: event.currentTarget.value as McpToolApproval })}
            />
          </div>
        )}
      </Show>

      <Show when={isAdmin()}>
        <div class="verevon-settings-actions">
          <p>
            {problem() ?? (saved() ? i18n.tr('Agentpolicyen er lagret.', 'The agent policy is saved.') : '')}
          </p>
          <div>
            <SettingsButton variant="primary" disabled={!draft() || saving()} onClick={() => void save()}>
              {i18n.tr('Lagre agentpolicy', 'Save agent policy')}
            </SettingsButton>
          </div>
        </div>
      </Show>

      <details class="verevon-settings-panel">
        <summary>{i18n.tr('Hva gjør hver modus, og hva gjelder nå?', 'What does each mode do, and what applies now?')}</summary>
        <ul>
          <For each={AGENT_MODE_ORDER}>
            {(mode) => (
              <li>
                <strong>{agentModeLabel(mode, i18n)}</strong> <span>{agentModeSummary(mode, i18n)}</span>
              </li>
            )}
          </For>
        </ul>
        <Show when={effective()}>
          {(view) => (
            <table>
              <thead>
                <tr>
                  <th scope="col">{i18n.tr('Innstilling', 'Setting')}</th>
                  <th scope="col">{i18n.tr('Verdi', 'Value')}</th>
                  <th scope="col">{i18n.tr('Kilde', 'Source')}</th>
                  <th scope="col">{i18n.tr('Status', 'Status')}</th>
                </tr>
              </thead>
              <tbody>
                <For each={view().settings}>
                  {(setting) => (
                    <tr>
                      <td>{settingLabel(setting.key, i18n)}</td>
                      <td>{effectiveValueText(setting, i18n)}</td>
                      <td>{effectiveSourceText(setting.source, i18n)}</td>
                      <td>{effectiveStatusText(setting.status, i18n)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          )}
        </Show>
      </details>
    </section>
  )
}
