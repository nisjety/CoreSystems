import { Loader2, Sparkles } from '@/shared/icons'
import { createMemo, createSignal, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getPersonaInstructions, updatePersonaInstructions } from '@/shared/api/persona-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton, ToggleRow } from '@/features/settings/components/settings-ui'

const MAX_PERSONA_INSTRUCTIONS_LENGTH = 1500

/**
 * Persona spec (chat-wave2, `persona.md`) — the user layer of the
 * authored-instruction hierarchy, structurally a copy of
 * `OrgInstructionsSection` with three differences: no admin gate (a caller
 * always edits their own row), an `enabled` switch alongside the textarea,
 * and the smaller 1500-character cap (a persona is a short style/tone
 * preference, not a policy document).
 *
 * The per-thread override (disabling persona for one conversation) lives in
 * the thread's own "…" menu, not here — this section only owns the global
 * switch and the text.
 */
export function PersonaSection() {
  const i18n = useI18n()

  const [saved, { refetch }] = createResource(() => getPersonaInstructions())

  const [draft, setDraft] = createSignal('')
  const [draftEnabled, setDraftEnabled] = createSignal(true)
  const [dirty, setDirty] = createSignal(false)
  const [submitting, setSubmitting] = createSignal(false)
  const [formError, setFormError] = createSignal<string | null>(null)

  const instructions = createMemo(() => (dirty() ? draft() : saved()?.instructions ?? ''))
  const enabled = createMemo(() => (dirty() ? draftEnabled() : saved()?.enabled ?? true))

  const markDirty = () => {
    if (dirty()) return
    setDraft(saved()?.instructions ?? '')
    setDraftEnabled(saved()?.enabled ?? true)
    setDirty(true)
  }

  const handleInput = (event: InputEvent & { currentTarget: HTMLTextAreaElement }) => {
    markDirty()
    setDraft(event.currentTarget.value)
  }

  const handleToggle = (checked: boolean) => {
    markDirty()
    setDraftEnabled(checked)
  }

  const handleSubmit = async (event: Event) => {
    event.preventDefault()
    if (submitting()) return
    setSubmitting(true)
    setFormError(null)
    try {
      await updatePersonaInstructions(instructions(), enabled())
      setDirty(false)
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre persona-instruksene dine.',
          en: 'Could not save your persona instructions.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Persona', 'Persona')}
        description={i18n.tr(
          'Dine egne, alltid-på instrukser. De legges til i hver ny samtale du starter, under organisasjonens og rommets instrukser — de kan utvide, men ikke overstyre disse. Et rom-agent du eksplisitt nevner med @ beholder likevel sin egen karakter for den meldingen.',
          'Your own always-on instructions. They are added to every new chat you start, beneath the organization\'s and Space\'s instructions — they can add to, but not override, those. A Space agent you explicitly @-mention still keeps its own character for that message.',
        )}
      />

      <Show when={formError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={saved.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste persona-instruksene dine.', 'Could not load your persona instructions.')}
        </p>
      </Show>

      <div class="verevon-settings-divided-list">
        <ToggleRow
          title={i18n.tr('Bruk persona i nye samtaler', 'Use persona in new chats')}
          description={i18n.tr(
            'Slå av for å midlertidig la persona-instruksene ligge urørt uten å slette teksten. En enkelt samtale kan i tillegg skrus av separat fra samtalens eget meny.',
            'Turn off to temporarily leave your persona text in place without deleting it. A single conversation can also be switched off separately from that thread\'s own menu.',
          )}
          info={i18n.tr(
            'Persona-teksten gjelder også i midlertidige samtaler (uten lagring), på samme måte som organisasjonens instrukser — bare den samtale-spesifikke bryteren kan ikke lagres for en midlertidig samtale.',
            'Persona text still applies in temporary (unsaved) chats, the same way organization instructions do — only the per-thread switch cannot be saved for a temporary chat.',
          )}
          enabled={enabled()}
          disabled={submitting() || saved.loading}
          onChange={handleToggle}
        />
      </div>

      <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={handleSubmit}>
        <label for="persona-instructions" class="verevon-settings-field">
          <span class="verevon-settings-label">{i18n.tr('Instrukser', 'Instructions')}</span>
          <span class="verevon-settings-input-wrap">
            <Show
              when={!saved.loading}
              fallback={
                <p class="verevon-settings-subnote" role="status" aria-busy="true">
                  {i18n.tr('Laster …', 'Loading…')}
                </p>
              }
            >
              <textarea
                id="persona-instructions"
                value={instructions()}
                rows={8}
                maxlength={MAX_PERSONA_INSTRUCTIONS_LENGTH}
                placeholder={i18n.tr(
                  'F.eks.\nSvar kort og saklig, uten unødvendige innledninger.\nBruk gjerne punktlister når du forklarer noe med flere trinn.',
                  'E.g.\nAnswer briefly and to the point, without unnecessary preambles.\nPrefer bullet lists when explaining something with multiple steps.',
                )}
                onInput={handleInput}
                class="verevon-settings-input verevon-settings-textarea"
              />
            </Show>
          </span>
          <span class="verevon-settings-help">
            {instructions().length} / {MAX_PERSONA_INSTRUCTIONS_LENGTH} {i18n.tr('tegn', 'characters')}
          </span>
        </label>

        <div>
          <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting() || !dirty()}>
            <Show
              when={submitting()}
              fallback={<><Sparkles size={14} aria-hidden="true" /> {i18n.tr('Lagre persona', 'Save persona')}</>}
            >
              <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Lagrer…', 'Saving…')}
            </Show>
          </SettingsButton>
        </div>
      </form>
    </>
  )
}
