import { createSignal, For, Show } from 'solid-js'

import {
  getSpaceToolPolicy,
  setSpaceAgentToolPolicy,
  setSpaceToolPolicy,
  type SpaceAgent,
  type SpaceMergedToolPolicy,
  type SpaceToolLevel,
  type SpaceToolPolicyEntry,
} from '@/shared/api/spaces-client'
import { hasErrorCode } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  TOOL_POLICY_LEVELS,
  TOOL_POLICY_WILDCARD,
  blankRow,
  draftMatchesStored,
  draftProblem,
  draftProblemMessage,
  draftRows,
  entriesFromDraft,
  mergedRows,
  rowOverriddenByCeiling,
  toolLevelHint,
  toolLevelLabel,
  toolSourceLabel,
  type ToolPolicyDraftRow,
} from '../lib/space-tool-policy'
import './space-tool-policy.css'

/**
 * The per-tool policy editor (WS10-S5b). Entry point: the Agent tab. `layer="agent"` edits one
 * bound agent's rules; `layer="ceiling"` edits the room ceiling that applies to every agent here.
 *
 * Control stores both layers and merges them tighten-only (allow < ask < block, the stricter layer
 * wins). The table at the top is Control's merged answer with the layer that decided each tool, so
 * a rule that the ceiling overrides says so instead of showing a level that will not apply. This
 * component decides nothing: Control checks the caller is a manager or owner and compares the
 * layer revision, and the gateway hides the editor when the rollout flag is off. "Allow" is the
 * absence of an extra rule; it never waives the usual approvals.
 */
export function SpaceAgentPolicyEditor(props: {
  readonly spaceRef: string
  readonly layer: 'agent' | 'ceiling'
  /** The agent whose layer is edited; required for `layer="agent"`. */
  readonly agent?: SpaceAgent
  readonly onSaved?: () => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [loading, setLoading] = createSignal(false)
  const [saving, setSaving] = createSignal(false)
  const [loaded, setLoaded] = createSignal(false)
  const [error, setError] = createSignal('')
  const [notice, setNotice] = createSignal('')
  const [rows, setRows] = createSignal<ToolPolicyDraftRow[]>([])
  const [stored, setStored] = createSignal<readonly SpaceToolPolicyEntry[]>([])
  const [revision, setRevision] = createSignal(0)
  const [merged, setMerged] = createSignal<SpaceMergedToolPolicy | null>(null)

  const subjectId = () => props.agent?.subject_id?.trim() || undefined
  const bindingRef = () => props.agent?.binding_ref?.trim() || undefined
  const isCeiling = () => props.layer === 'ceiling'

  const apply = (layerEntries: readonly SpaceToolPolicyEntry[], layerRevision: number, next: SpaceMergedToolPolicy) => {
    setStored(layerEntries)
    setRevision(layerRevision)
    setRows(draftRows(layerEntries))
    setMerged(next)
    setLoaded(true)
  }

  async function load() {
    setLoading(true)
    setError('')
    try {
      const view = await getSpaceToolPolicy(props.spaceRef, isCeiling() ? undefined : subjectId())
      const layer = isCeiling() ? view.ceiling : view.agents.find((item) => item.agentSubjectId === subjectId())
      apply(layer?.entries ?? [], layer?.revision ?? 0, view.merged)
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Verktøyreglene kunne ikke hentes. Ingenting er endret.',
          en: 'The tool rules could not be loaded. Nothing was changed.',
        }),
      )
    } finally {
      setLoading(false)
    }
  }

  const toggle = () => {
    const next = !open()
    setOpen(next)
    if (next && !loaded() && !loading()) void load()
  }

  const problem = () => draftProblem(rows())
  const unchanged = () => draftMatchesStored(rows(), stored())

  const updateRow = (id: number, patch: Partial<Pick<ToolPolicyDraftRow, 'toolKey' | 'level'>>) =>
    setRows((current) => current.map((row) => (row.id === id ? { ...row, ...patch } : row)))
  const removeRow = (id: number) => setRows((current) => current.filter((row) => row.id !== id))
  const addRow = () =>
    setRows((current) => [...current, blankRow(current.some((row) => row.toolKey.trim() === TOOL_POLICY_WILDCARD) ? '' : TOOL_POLICY_WILDCARD)])

  async function save() {
    if (saving() || problem() !== null) return
    setSaving(true)
    setError('')
    setNotice('')
    const entries = entriesFromDraft(rows())
    try {
      const result = isCeiling()
        ? await setSpaceToolPolicy(props.spaceRef, entries, revision())
        : await setSpaceAgentToolPolicy(props.spaceRef, bindingRef() ?? '', entries, revision())
      apply(result.policy.entries, result.policy.revision, result.merged)
      setNotice(
        result.projection === 'pending'
          ? i18n.tr(
              'Lagret. Visningen i rommet oppdateres om litt.',
              'Saved. The room view will catch up shortly.',
            )
          : i18n.tr('Lagret.', 'Saved.'),
      )
      props.onSaved?.()
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Verktøyreglene kunne ikke lagres. Ingenting er endret.',
          en: 'The tool rules could not be saved. Nothing was changed.',
        }),
      )
      // Someone else changed the layer: show what is stored now rather than overwrite it blind.
      if (hasErrorCode(err, 'tool_policy_revision_conflict')) await load()
    } finally {
      setSaving(false)
    }
  }

  const title = () =>
    isCeiling()
      ? i18n.tr('Verktøyregler for hele rommet', 'Tool rules for the whole room')
      : i18n.tr('Verktøyregler for denne agenten', 'Tool rules for this agent')

  return (
    <div class="verevon-tool-policy">
      <button
        type="button"
        class="verevon-tool-policy__toggle"
        aria-expanded={open()}
        onClick={toggle}
      >
        {open() ? i18n.tr('Skjul verktøyregler', 'Hide tool rules') : title()}
      </button>

      <Show when={open()}>
        <div class="verevon-tool-policy__panel">
          <p class="verevon-tool-policy__note">
            {isCeiling()
              ? i18n.tr(
                  'Reglene her gjelder alle agenter i rommet. Den strengeste regelen vinner, og en agents egne regler kan ikke løsne dem.',
                  'These rules apply to every agent in the room. The strictest rule wins, and an agent\'s own rules cannot loosen them.',
                )
              : i18n.tr(
                  'Reglene her gjelder bare denne agenten. Rommets tak gjelder i tillegg, og den strengeste regelen vinner.',
                  'These rules apply to this agent only. The room ceiling applies as well, and the strictest rule wins.',
                )}{' '}
            {i18n.tr(
              '«Tillat» er ingen ekstra regel: vanlige godkjenninger gjelder fortsatt.',
              '"Allow" is no extra rule: the usual approvals still apply.',
            )}
          </p>

          <Show when={loading()}>
            <p class="verevon-tool-policy__status" role="status">
              {i18n.tr('Henter regler …', 'Loading rules…')}
            </p>
          </Show>

          <Show when={loaded()}>
            <Show when={merged()}>
              {(view) => (
                <div>
                  <h4 class="verevon-tool-policy__heading">
                    {i18n.tr('Slik gjelder det nå', 'What applies now')}
                  </h4>
                  <Show
                    when={mergedRows(view()).length > 0}
                    fallback={
                      <p class="verevon-tool-policy__note">
                        {i18n.tr('Ingen verktøyregler er satt.', 'No tool rules are set.')}
                      </p>
                    }
                  >
                    <ul class="verevon-tool-policy__merged">
                      <For each={mergedRows(view())}>
                        {(row) => (
                          <li>
                            <span class="verevon-tool-policy__key">
                              {row.toolKey === TOOL_POLICY_WILDCARD
                                ? i18n.tr('* (alle verktøy)', '* (all tools)')
                                : row.toolKey}
                            </span>
                            <strong>{toolLevelLabel(row.level, i18n.tr)}</strong>
                            <span class="verevon-tool-policy__source">
                              {toolSourceLabel(row.source, i18n.tr)}
                            </span>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                </div>
              )}
            </Show>

            <div>
              <h4 class="verevon-tool-policy__heading">{title()}</h4>
              <ul class="verevon-tool-policy__rows">
                <For each={rows()} keyed={false}>
                  {(row) => (
                    <li class="verevon-tool-policy__row">
                      <input
                        type="text"
                        class="verevon-tool-policy__input"
                        value={row().toolKey}
                        maxLength={200}
                        spellcheck={false}
                        aria-label={i18n.tr('Verktøynavn', 'Tool name')}
                        placeholder={i18n.tr('verktøynavn eller *', 'tool name or *')}
                        disabled={saving()}
                        onInput={(event) => updateRow(row().id, { toolKey: event.currentTarget.value })}
                      />
                      <select
                        class="verevon-tool-policy__select"
                        value={row().level}
                        aria-label={i18n.tr('Nivå', 'Level')}
                        disabled={saving()}
                        onChange={(event) =>
                          updateRow(row().id, { level: event.currentTarget.value as SpaceToolLevel })
                        }
                      >
                        <For each={TOOL_POLICY_LEVELS}>
                          {(level) => <option value={level}>{toolLevelLabel(level, i18n.tr)}</option>}
                        </For>
                      </select>
                      <button
                        type="button"
                        class="verevon-tool-policy__button"
                        disabled={saving()}
                        onClick={() => removeRow(row().id)}
                      >
                        {i18n.tr('Fjern', 'Remove')}
                      </button>
                      <p class="verevon-tool-policy__row-note">{toolLevelHint(row().level, i18n.tr)}</p>
                      <Show when={!isCeiling() && merged() && rowOverriddenByCeiling(merged()!, row())}>
                        <p class="verevon-tool-policy__row-note">
                          {i18n.tr(
                            'Rommets tak er strengere og gjelder i stedet for dette nivået.',
                            'The room ceiling is stricter and applies instead of this level.',
                          )}
                        </p>
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
              <Show when={rows().length === 0}>
                <p class="verevon-tool-policy__note">
                  {isCeiling()
                    ? i18n.tr('Rommet har ingen egne verktøyregler.', 'The room has no tool rules of its own.')
                    : i18n.tr('Agenten har ingen egne verktøyregler.', 'This agent has no tool rules of its own.')}
                </p>
              </Show>
            </div>

            <Show when={problem()}>
              {(issue) => (
                <p class="verevon-tool-policy__error" role="alert">
                  {draftProblemMessage(issue(), i18n.tr)}
                </p>
              )}
            </Show>

            <div class="verevon-tool-policy__actions">
              <button type="button" class="verevon-tool-policy__button" disabled={saving()} onClick={addRow}>
                {i18n.tr('Legg til regel', 'Add rule')}
              </button>
              <button
                type="button"
                class="verevon-tool-policy__button"
                disabled={saving() || problem() !== null || unchanged() || (!isCeiling() && !bindingRef())}
                onClick={() => void save()}
              >
                {saving() ? i18n.tr('Lagrer …', 'Saving…') : i18n.tr('Lagre', 'Save')}
              </button>
              <button
                type="button"
                class="verevon-tool-policy__button"
                disabled={saving() || loading() || unchanged()}
                onClick={() => setRows(draftRows(stored()))}
              >
                {i18n.tr('Angre endringer', 'Discard changes')}
              </button>
            </div>
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class="verevon-tool-policy__error" role="alert">
                {message()}
              </p>
            )}
          </Show>
          <Show when={notice()}>
            {(message) => (
              <p class="verevon-tool-policy__status" role="status">
                {message()}
              </p>
            )}
          </Show>
        </div>
      </Show>
    </div>
  )
}
