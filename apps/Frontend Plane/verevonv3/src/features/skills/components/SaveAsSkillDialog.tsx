import { createSignal, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import { Sparkles, X } from '@/shared/icons'
import { getActionDescriptor } from '@/shared/actions/action-registry'
import { createSkill } from '@/shared/api/skills-client'
import { getSession } from '@/shared/session/session-store'
import { translateApiError, useI18n } from '@/shared/i18n'

/**
 * Save as skill (EP-11 S1): one of the person's own answers, saved as a private skill of theirs.
 * The dialog starts from the answer, and nothing is saved until the person presses the button; the
 * server binds the owner to the session. A temporary chat never shows it.
 */

/** A starting name for a skill: the answer's first heading, else its first line, at most 60 characters. */
export function suggestSkillName(content: string): string {
  const lines = content
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
  const heading = lines.find((line) => /^#{1,6}\s/.test(line))
  return (heading ?? lines[0] ?? '')
    .replace(/^#{1,6}\s+/, '')
    .replace(/[*_`]/g, '')
    .trim()
    .slice(0, 60)
    .trim()
}

export function SaveAsSkillDialog(props: {
  threadId: string
  messageId: string
  /** The answer's text: the skill's starting content, which the person may edit before saving. */
  content: string
  /** A temporary (zero-retention) chat never saves a skill from its answers, so nothing is shown. */
  temporary?: boolean
  onClose: () => void
  onSaved?: (skillId: string) => void
}) {
  const i18n = useI18n()
  const [name, setName] = createSignal(suggestSkillName(props.content))
  const [description, setDescription] = createSignal('')
  const [body, setBody] = createSignal(props.content)
  const [enabled, setEnabled] = createSignal(true)
  const [submitting, setSubmitting] = createSignal(false)
  const [errorMessage, setErrorMessage] = createSignal<string | null>(null)

  const close = () => {
    if (!submitting()) props.onClose()
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.stopPropagation()
      close()
    }
  }

  const save = async (event: SubmitEvent) => {
    event.preventDefault()
    if (submitting()) return
    const input = {
      threadId: props.threadId,
      messageId: props.messageId,
      name: name().trim(),
      description: description().trim() || undefined,
      content: body().trim(),
      enabled: enabled(),
    }
    // The registry's contract bounds the save the way the server does, so a save the server would
    // refuse is never sent.
    const contract = getActionDescriptor('skills.save_from_message')?.inputSchema.safeParse(input)
    if (!contract?.success) {
      setErrorMessage(
        i18n.tr(
          'Navnet må ha 1–64 tegn, og innholdet må ha 1–32 000 tegn.',
          'The name must be 1–64 characters and the content 1–32,000 characters.',
        ),
      )
      return
    }
    const orgId = getSession().activeOrg?.id ?? ''
    if (!orgId) {
      setErrorMessage(i18n.tr('Velg en organisasjon først.', 'Choose an organization first.'))
      return
    }
    setSubmitting(true)
    setErrorMessage(null)
    try {
      const created = await createSkill(orgId, {
        name: input.name,
        description: input.description,
        content: input.content,
        enabled: input.enabled,
        scope: 'user',
      })
      props.onSaved?.(created.id)
      props.onClose()
    } catch (err) {
      setErrorMessage(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre ferdigheten.',
          en: 'Could not save the skill.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Show when={!props.temporary}>
      <Portal>
        <div
          class="verevon-schedule-dialog"
          role="presentation"
          onClick={(event) => {
            if (event.target === event.currentTarget) close()
          }}
        >
          <form
            class="verevon-schedule-dialog__dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="save-as-skill-title"
            onSubmit={(event) => void save(event)}
            onKeyDown={onKeyDown}
          >
            <header>
              <h3 id="save-as-skill-title">
                <Sparkles size={16} aria-hidden="true" /> {i18n.tr('Lagre som ferdighet', 'Save as skill')}
              </h3>
              <button type="button" onClick={close} aria-label={i18n.tr('Lukk', 'Close')}>
                <X size={16} />
              </button>
            </header>

            <label class="verevon-schedule-dialog__field">
              <span>{i18n.tr('Navn', 'Name')}</span>
              <input
                ref={(element) => queueMicrotask(() => element.focus())}
                value={name()}
                maxlength={64}
                required
                disabled={submitting()}
                onInput={(event) => setName(event.currentTarget.value)}
              />
            </label>

            <label class="verevon-schedule-dialog__field">
              <span>{i18n.tr('Beskrivelse (valgfritt)', 'Description (optional)')}</span>
              <input
                value={description()}
                maxlength={300}
                disabled={submitting()}
                onInput={(event) => setDescription(event.currentTarget.value)}
              />
            </label>

            <label class="verevon-schedule-dialog__field">
              <span>{i18n.tr('Innhold', 'Content')}</span>
              <textarea
                value={body()}
                maxlength={32000}
                required
                rows={6}
                disabled={submitting()}
                onInput={(event) => setBody(event.currentTarget.value)}
              />
            </label>

            <p class="verevon-schedule-dialog__note">
              {i18n.tr(
                'Skriv hva som skal gjøres neste gang, ikke hva som skjedde. Hold den kort; lange ferdigheter kuttes ned.',
                'Write what to do next time, not what happened. Keep it short; long skills are cut to fit.',
              )}
            </p>

            <label class="verevon-schedule-dialog__policy-option">
              <input
                type="checkbox"
                role="switch"
                checked={enabled()}
                disabled={submitting()}
                onChange={(event) => setEnabled(event.currentTarget.checked)}
              />
              {i18n.tr('Bruk i mine samtaler', 'Use in my chats')}
            </label>

            <Show when={errorMessage()}>
              {(message) => (
                <p class="verevon-schedule-dialog__error" role="alert">
                  {message()}
                </p>
              )}
            </Show>

            <footer>
              <button type="submit" class="verevon-schedule-dialog__submit" disabled={submitting()}>
                {submitting() ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Lagre ferdighet', 'Save skill')}
              </button>
            </footer>
          </form>
        </div>
      </Portal>
    </Show>
  )
}
