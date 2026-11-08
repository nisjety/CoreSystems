import { createSignal, For, Show } from 'solid-js'
import { getActionDescriptor } from '@/shared/actions/action-registry'
import { createPreset, type PresetSettings } from '@/shared/api/authored-kinds-client'
import { VEREVON_MODES } from '@/shared/api/chat-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { skillLintRefusal, type SkillLintFinding } from '@/features/skills/skill-lint'
import { authoredLintMessage } from '@/features/skills/authored-lint'

/**
 * A new run preset of the person's own (EP-11 S6): a goal text and the visible run console
 * controls it sets. It offers only controls the person could set by hand (mode and web search
 * here), keeps plan mode on, and carries no persona, tool list or autonomy: those belong to an
 * agent definition (EP-6). The server checks the settings against the same closed schema.
 */

/** The settings this editor sends: only the controls it shows, plan mode always on. */
export function presetSettingsFromControls(controls: { modeId: string; browseWeb: boolean }): PresetSettings {
  return { ...(controls.modeId ? { modeId: controls.modeId } : {}), browseWeb: controls.browseWeb, planMode: true }
}

export function PresetEditor(props: { onSaved?: (presetId: string) => void; onCancel?: () => void }) {
  const i18n = useI18n()
  const [name, setName] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [goal, setGoal] = createSignal('')
  const [modeId, setModeId] = createSignal('')
  const [browseWeb, setBrowseWeb] = createSignal(false)
  const [submitting, setSubmitting] = createSignal(false)
  const [findings, setFindings] = createSignal<SkillLintFinding[]>([])
  const [errorMessage, setErrorMessage] = createSignal<string | null>(null)

  const save = async (event: SubmitEvent) => {
    event.preventDefault()
    if (submitting()) return
    const input = {
      name: name().trim(),
      description: description().trim() || undefined,
      content: goal().trim(),
      settings: presetSettingsFromControls({ modeId: modeId(), browseWeb: browseWeb() }),
    }
    if (!getActionDescriptor('presets.create')?.inputSchema.safeParse(input).success) {
      setErrorMessage(
        i18n.tr(
          'Navnet må ha 1–64 tegn, og målet må ha 1–32 000 tegn.',
          'The name must be 1–64 characters and the goal 1–32,000 characters.',
        ),
      )
      return
    }
    setSubmitting(true)
    setErrorMessage(null)
    setFindings([])
    try {
      const created = await createPreset(input)
      props.onSaved?.(created.id)
    } catch (err) {
      const refused = skillLintRefusal(err)
      if (refused.length > 0) {
        setFindings(refused)
        return
      }
      setErrorMessage(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke lagre forhåndsinnstillingen.', en: 'Could not save the preset.' }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <form class="verevon-schedule-dialog__dialog" aria-label={i18n.tr('Ny forhåndsinnstilling', 'New preset')} onSubmit={(event) => void save(event)}>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Navn', 'Name')}</span>
        <input value={name()} maxlength={64} required disabled={submitting()} onInput={(event) => setName(event.currentTarget.value)} />
      </label>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Beskrivelse (valgfritt)', 'Description (optional)')}</span>
        <input value={description()} maxlength={300} disabled={submitting()} onInput={(event) => setDescription(event.currentTarget.value)} />
      </label>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Mål', 'Goal')}</span>
        <textarea value={goal()} maxlength={32000} required rows={4} disabled={submitting()} onInput={(event) => setGoal(event.currentTarget.value)} />
      </label>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Modus', 'Mode')}</span>
        <select value={modeId()} disabled={submitting()} onChange={(event) => setModeId(event.currentTarget.value)}>
          <option value="">{i18n.tr('Som den er', 'Leave as it is')}</option>
          <For each={VEREVON_MODES}>{(mode) => <option value={mode.id}>{mode.label}</option>}</For>
        </select>
      </label>
      <label class="verevon-schedule-dialog__policy-option">
        <input
          type="checkbox"
          role="switch"
          checked={browseWeb()}
          disabled={submitting()}
          onChange={(event) => setBrowseWeb(event.currentTarget.checked)}
        />
        {i18n.tr('Søk på nettet', 'Search the web')}
      </label>
      <p class="verevon-schedule-dialog__note">
        {i18n.tr(
          'En forhåndsinnstilling fyller bare ut kontrollene. Planlegging er alltid på, og kjøringen starter først når du trykker Kjør.',
          'A preset only fills in the controls. Planning is always on, and the run starts only when you press Run.',
        )}
      </p>
      <For each={findings()}>
        {(finding) => (
          <p class="verevon-schedule-dialog__error" data-lint-code={finding.code}>
            {authoredLintMessage(finding.code, i18n.tr)}
          </p>
        )}
      </For>
      <Show when={errorMessage()}>
        {(message) => (
          <p class="verevon-schedule-dialog__error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <footer>
        <Show when={props.onCancel}>
          <button type="button" disabled={submitting()} onClick={() => props.onCancel?.()}>
            {i18n.tr('Avbryt', 'Cancel')}
          </button>
        </Show>
        <button type="submit" class="verevon-schedule-dialog__submit" disabled={submitting()}>
          {submitting() ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Lagre forhåndsinnstilling', 'Save preset')}
        </button>
      </footer>
    </form>
  )
}
