import './personal-setup-card.css'
import { createMemo, createSignal, createUniqueId, For, Show } from 'solid-js'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { TONE_PRESETS, type TonePreset } from '@/shared/api/ai-settings-client'
import { getMemoryControls, rememberMemory } from '@/shared/api/memory-client'
import { getPersonalSetupState } from '@/shared/api/personal-setup-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getSession } from '@/shared/session/session-store'

const MAX_DISPLAY_NAME_LENGTH = 80
const MAX_ROLE_LENGTH = 200

type Language = 'nb-NO' | 'en-US'

/** The browser's own time zone, prefilled for the person to keep or change. Empty when the browser does not say. */
function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone
  } catch {
    return ''
  }
}

/**
 * The personal first-run card (EP-8-S8): one inline, skippable step above the empty chat. It writes the
 * person's own display name, time zone, language and reply tone. A stated role is saved as a memory only
 * on an open thread, and only when memory is on and the organization allows new memories. The card never
 * blocks the composer, and it is shown only while the step is pending.
 */
export function PersonalSetupCard(props: { threadId?: string | null }) {
  const i18n = useI18n()
  const session = getSession()
  const headingId = createUniqueId()
  const nameId = createUniqueId()
  const zoneId = createUniqueId()
  const languageId = createUniqueId()
  const toneId = createUniqueId()
  const roleToggleId = createUniqueId()
  const roleId = createUniqueId()

  // Offered only when user-core shows the card and the person has not answered yet. A failed read hides it.
  const [setup] = createResource(() => getPersonalSetupState())
  const offered = createMemo(() => !setup.error && setup()?.available === true && setup()?.step === 'pending')

  // Memory is read only when the card is offered and a thread is open, since the role needs one. A
  // read that fails hides the role, which is the safe reading.
  const [controls] = createResource(
    () => offered() && Boolean(props.threadId),
    () => getMemoryControls(),
  )
  const roleOffered = createMemo(
    () =>
      Boolean(props.threadId) &&
      !controls.error &&
      controls()?.state === 'on' &&
      controls()?.generationAllowed === true,
  )

  const actorUserId = () => session.user?.id ?? ''
  const actorOrgId = () => session.activeOrg?.id ?? ''
  const actorReady = () => actorUserId() !== '' && actorOrgId() !== ''
  const actor = (): ActionActor => ({ type: 'human', userId: actorUserId(), orgId: actorOrgId() })

  const [displayName, setDisplayName] = createSignal(session.user?.name ?? '')
  const [timezone, setTimezone] = createSignal(browserTimeZone())
  const [language, setLanguage] = createSignal<Language>(i18n.locale() === 'en' ? 'en-US' : 'nb-NO')
  const [tonePreset, setTonePreset] = createSignal<TonePreset>('default')
  const [rememberRole, setRememberRole] = createSignal(false)
  const [role, setRole] = createSignal('')
  const [submitting, setSubmitting] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  // Set when the step is recorded but the role could not be saved: the card then says so and offers Close.
  const [roleNotSaved, setRoleNotSaved] = createSignal(false)
  const [dismissed, setDismissed] = createSignal(false)

  const languageLabel = (value: Language) =>
    value === 'en-US' ? i18n.tr('Engelsk', 'English') : i18n.tr('Norsk bokmål', 'Norwegian Bokmal')

  const toneLabels = (): Record<TonePreset, string> => ({
    default: i18n.tr('Standard', 'Default'),
    professional: i18n.tr('Profesjonell', 'Professional'),
    friendly: i18n.tr('Vennlig', 'Friendly'),
    direct: i18n.tr('Direkte', 'Direct'),
    warm: i18n.tr('Varm', 'Warm'),
    neutral: i18n.tr('Nøytral', 'Neutral'),
  })

  // The answers go to user-core through the action contract; the gateway checks each one again. The step
  // is recorded last, so a failed save leaves the card open to try again.
  const complete = async () => {
    if (submitting() || !actorReady()) return
    const name = displayName().trim()
    if (name.length > MAX_DISPLAY_NAME_LENGTH) {
      setError(i18n.tr('Visningsnavnet kan være høyst 80 tegn.', 'The display name can be at most 80 characters.'))
      return
    }
    setSubmitting(true)
    setError(null)
    try {
      await executeAction('settings.complete_personal_setup', actor(), {
        displayName: name,
        timezone: timezone().trim(),
        language: language(),
        tonePreset: tonePreset(),
      })
    } catch (err) {
      setError(translateApiError(err, i18n.tr))
      setSubmitting(false)
      return
    }
    // The step is recorded. A role is saved only when the person asked for it and memory is usable; if
    // that save fails, the answers stay saved and the card says so.
    let roleSaved = true
    const content = role().trim()
    const threadId = props.threadId
    if (rememberRole() && roleOffered() && threadId && content) {
      try {
        await rememberMemory(threadId, i18n.tr(`Rolle: ${content}`, `Role: ${content}`))
      } catch {
        roleSaved = false
      }
    }
    setSubmitting(false)
    if (roleSaved) setDismissed(true)
    else setRoleNotSaved(true)
  }

  // Skipping records the choice, so the card never shows again for this version of the step.
  const skip = async () => {
    if (submitting() || !actorReady()) return
    setSubmitting(true)
    setError(null)
    try {
      await executeAction('settings.skip_personal_setup', actor(), {})
      setDismissed(true)
    } catch (err) {
      setError(translateApiError(err, i18n.tr))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Show when={offered() && !dismissed() && actorReady()}>
      <section class="verevon-personal-setup" aria-labelledby={headingId} data-testid="personal-setup-card">
        <Show
          when={!roleNotSaved()}
          fallback={
            <div class="verevon-personal-setup__notice" role="status">
              <p>
                {i18n.tr(
                  'Svarene dine er lagret. Rollen din ble ikke lagret; du kan legge den inn igjen senere.',
                  'Your answers are saved. Your role was not saved; you can add it again later.',
                )}
              </p>
              <button type="button" class="verevon-personal-setup__button" onClick={() => setDismissed(true)}>
                {i18n.tr('Lukk', 'Close')}
              </button>
            </div>
          }
        >
          <form
            class="verevon-personal-setup__form"
            onSubmit={(event) => {
              event.preventDefault()
              void complete()
            }}
          >
            <h2 id={headingId} class="verevon-personal-setup__title">
              {i18n.tr('Litt om deg', 'A little about you')}
            </h2>
            <p class="verevon-personal-setup__lede">
              {i18n.tr(
                'Svarene brukes til å tilpasse chatten. Du kan endre dem i innstillingene når som helst.',
                'Your answers tailor the chat. You can change them in settings at any time.',
              )}
            </p>
            <div class="verevon-personal-setup__fields">
              <div class="verevon-personal-setup__field">
                <label for={nameId}>{i18n.tr('Visningsnavn', 'Display name')}</label>
                <input
                  id={nameId}
                  type="text"
                  autocomplete="name"
                  maxlength={MAX_DISPLAY_NAME_LENGTH}
                  value={displayName()}
                  disabled={submitting()}
                  onInput={(event) => setDisplayName(event.currentTarget.value)}
                />
              </div>
              <div class="verevon-personal-setup__field">
                <label for={zoneId}>{i18n.tr('Tidssone', 'Time zone')}</label>
                <input
                  id={zoneId}
                  type="text"
                  autocomplete="off"
                  spellcheck={false}
                  maxlength={64}
                  value={timezone()}
                  disabled={submitting()}
                  onInput={(event) => setTimezone(event.currentTarget.value)}
                />
              </div>
              <div class="verevon-personal-setup__field">
                <label for={languageId}>{i18n.tr('Språk', 'Language')}</label>
                <select
                  id={languageId}
                  value={language()}
                  disabled={submitting()}
                  onChange={(event) => setLanguage(event.currentTarget.value as Language)}
                >
                  <option value="nb-NO" selected={language() === 'nb-NO'}>
                    {languageLabel('nb-NO')}
                  </option>
                  <option value="en-US" selected={language() === 'en-US'}>
                    {languageLabel('en-US')}
                  </option>
                </select>
              </div>
              <div class="verevon-personal-setup__field">
                <label for={toneId}>{i18n.tr('Tone', 'Tone')}</label>
                <select
                  id={toneId}
                  value={tonePreset()}
                  disabled={submitting()}
                  onChange={(event) => setTonePreset(event.currentTarget.value as TonePreset)}
                >
                  <For each={TONE_PRESETS}>
                    {(preset) => (
                      <option value={preset} selected={tonePreset() === preset}>
                        {toneLabels()[preset]}
                      </option>
                    )}
                  </For>
                </select>
              </div>
            </div>
            <Show when={roleOffered()}>
              <div class="verevon-personal-setup__role" data-testid="personal-setup-role">
                <label class="verevon-personal-setup__check" for={roleToggleId}>
                  <input
                    id={roleToggleId}
                    type="checkbox"
                    checked={rememberRole()}
                    disabled={submitting()}
                    onChange={(event) => setRememberRole(event.currentTarget.checked)}
                  />
                  <span>{i18n.tr('Husk min rolle', 'Remember my role')}</span>
                </label>
                <Show when={rememberRole()}>
                  <div class="verevon-personal-setup__field">
                    <label for={roleId}>{i18n.tr('Rolle', 'Role')}</label>
                    <input
                      id={roleId}
                      type="text"
                      maxlength={MAX_ROLE_LENGTH}
                      value={role()}
                      disabled={submitting()}
                      onInput={(event) => setRole(event.currentTarget.value)}
                    />
                  </div>
                </Show>
              </div>
            </Show>
            <Show when={error()}>
              <p class="verevon-personal-setup__error" role="alert">
                {error()}
              </p>
            </Show>
            <div class="verevon-personal-setup__actions">
              <button
                type="button"
                class="verevon-personal-setup__button"
                disabled={submitting()}
                onClick={() => void skip()}
              >
                {i18n.tr('Hopp over', 'Skip')}
              </button>
              <button
                type="submit"
                class="verevon-personal-setup__button verevon-personal-setup__button--primary"
                disabled={submitting()}
              >
                {i18n.tr('Lagre', 'Save')}
              </button>
            </div>
          </form>
        </Show>
      </section>
    </Show>
  )
}
