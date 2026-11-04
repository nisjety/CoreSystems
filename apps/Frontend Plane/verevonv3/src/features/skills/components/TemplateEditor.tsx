import { createSignal, For, Show } from 'solid-js'
import { getActionDescriptor } from '@/shared/actions/action-registry'
import { createTemplate } from '@/shared/api/authored-kinds-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { skillLintRefusal, type SkillLintFinding } from '@/features/skills/skill-lint'
import { authoredLintMessage } from '@/features/skills/authored-lint'

/**
 * A new `/` template of the person's own (EP-11 S6). A template is text the composer puts in the
 * draft when it is picked, with `{{input}}` filled from the rest of the line; it is never sent by
 * itself and never injected as a skill. The server binds the owner to the session and lints the
 * text, so a refusal is shown here as the server named it.
 */
export function TemplateEditor(props: { onSaved?: (templateId: string) => void; onCancel?: () => void }) {
  const i18n = useI18n()
  const [name, setName] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [content, setContent] = createSignal('')
  const [submitting, setSubmitting] = createSignal(false)
  const [findings, setFindings] = createSignal<SkillLintFinding[]>([])
  const [errorMessage, setErrorMessage] = createSignal<string | null>(null)

  const save = async (event: SubmitEvent) => {
    event.preventDefault()
    if (submitting()) return
    const input = {
      name: name().trim(),
      description: description().trim() || undefined,
      content: content().trim(),
    }
    if (!getActionDescriptor('templates.create')?.inputSchema.safeParse(input).success) {
      setErrorMessage(
        i18n.tr(
          'Navnet må ha 1–64 tegn, og teksten må ha 1–32 000 tegn.',
          'The name must be 1–64 characters and the text 1–32,000 characters.',
        ),
      )
      return
    }
    setSubmitting(true)
    setErrorMessage(null)
    setFindings([])
    try {
      const created = await createTemplate(input)
      props.onSaved?.(created.id)
    } catch (err) {
      const refused = skillLintRefusal(err)
      if (refused.length > 0) {
        setFindings(refused)
        return
      }
      setErrorMessage(translateApiError(err, i18n.tr, { no: 'Kunne ikke lagre malen.', en: 'Could not save the template.' }))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <form class="verevon-schedule-dialog__dialog" aria-label={i18n.tr('Ny mal', 'New template')} onSubmit={(event) => void save(event)}>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Navn', 'Name')}</span>
        <input value={name()} maxlength={64} required disabled={submitting()} onInput={(event) => setName(event.currentTarget.value)} />
      </label>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Beskrivelse (valgfritt)', 'Description (optional)')}</span>
        <input value={description()} maxlength={300} disabled={submitting()} onInput={(event) => setDescription(event.currentTarget.value)} />
      </label>
      <label class="verevon-schedule-dialog__field">
        <span>{i18n.tr('Tekst', 'Text')}</span>
        <textarea value={content()} maxlength={32000} required rows={6} disabled={submitting()} onInput={(event) => setContent(event.currentTarget.value)} />
      </label>
      <p class="verevon-schedule-dialog__note">
        {i18n.tr(
          'Skriv {{input}} der teksten etter /navn skal stå. Malen havner i utkastet, og ingenting sendes før du sender.',
          'Write {{input}} where the text after /name goes. The template lands in the draft, and nothing is sent until you send.',
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
          {submitting() ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Lagre mal', 'Save template')}
        </button>
      </footer>
    </form>
  )
}
