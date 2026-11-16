import { createSignal, For, Show } from 'solid-js'

import { Loader2, Sparkles } from '@/shared/icons'

import {
  createPersonalAgent,
  getPersonalAgentsFeature,
  listPersonalAgents,
  setPrimaryPersonalAgent,
  type PersonalAgent,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { SpaceAgentAccessDialog } from '@/features/spaces/components/SpaceAgentAccessDialog'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'

const MIN_NAME_LENGTH = 2
const MAX_NAME_LENGTH = 60
const MAX_INSTRUCTIONS_LENGTH = 4000

/** A fresh id for one create attempt. `randomUUID` exists only in a secure context, so a fallback is kept. */
function newAttemptId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  return `agent-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`
}

/**
 * The caller's own agents (WS17-S5): create one, see the list, choose the primary one, and open each
 * agent's access. The whole section is hidden while the gateway does not report
 * `features.personal_agents`. Presentation only: the gateway and Control decide every change.
 */
export function PersonalAgentsSection() {
  const i18n = useI18n()
  const [featureOn] = createResource(() => getPersonalAgentsFeature())
  const available = () => featureOn() === true
  const [list, { refetch }] = createResource(
    () => (available() ? true : undefined),
    () => listPersonalAgents(),
  )

  const [name, setName] = createSignal('')
  const [instructions, setInstructions] = createSignal('')
  const [creating, setCreating] = createSignal(false)
  const [createError, setCreateError] = createSignal<string | null>(null)
  // One attempt id per request content: a retry of the same request reuses it, an edited request gets a new one.
  let attempt: { readonly key: string; readonly signature: string } | undefined

  const trimmedName = () => name().trim()
  const canCreate = () =>
    !creating() &&
    trimmedName().length >= MIN_NAME_LENGTH &&
    trimmedName().length <= MAX_NAME_LENGTH &&
    instructions().length <= MAX_INSTRUCTIONS_LENGTH

  async function create(event: Event): Promise<void> {
    event.preventDefault()
    if (!canCreate()) return
    const signature = JSON.stringify([trimmedName(), instructions().trim()])
    const key = attempt && attempt.signature === signature ? attempt.key : newAttemptId()
    attempt = { key, signature }
    setCreating(true)
    setCreateError(null)
    try {
      await createPersonalAgent({ name: trimmedName(), instructions: instructions(), clientRequestId: key })
      attempt = undefined
      setName('')
      setInstructions('')
      await refetch()
    } catch (error) {
      setCreateError(
        translateApiError(error, i18n.tr, {
          no: 'Agenten kunne ikke opprettes. Ingenting ble satt opp.',
          en: 'The agent could not be created. Nothing was set up.',
        }),
      )
    } finally {
      setCreating(false)
    }
  }

  const [primaryBusy, setPrimaryBusy] = createSignal(false)
  const [primaryError, setPrimaryError] = createSignal<string | null>(null)
  const [accessRef, setAccessRef] = createSignal<string | undefined>(undefined)

  const primarySubjectId = () => list()?.primarySubjectId ?? null
  const isPrimary = (agent: PersonalAgent) => primarySubjectId() === agent.subjectId

  async function choosePrimary(agent: PersonalAgent | null): Promise<void> {
    if (primaryBusy()) return
    setPrimaryBusy(true)
    setPrimaryError(null)
    try {
      await setPrimaryPersonalAgent(agent ? agent.agentRef : null)
      await refetch()
    } catch (error) {
      setPrimaryError(
        translateApiError(error, i18n.tr, {
          no: 'Den primære agenten kunne ikke endres. Ingenting er endret.',
          en: 'The primary agent could not be changed. Nothing was changed.',
        }),
      )
    } finally {
      setPrimaryBusy(false)
    }
  }

  const statusLabel = (status: PersonalAgent['status']) =>
    status === 'draft' ? i18n.tr('Utkast', 'Draft') : i18n.tr('Aktiv', 'Active')

  const visibilityLabel = (visibility: PersonalAgent['visibility']) =>
    visibility === 'org' ? i18n.tr('Synlig for organisasjonen', 'Visible to the organization') : i18n.tr('Bare du', 'Only you')

  const roomsLabel = (count: number) => {
    if (count === 0) return i18n.tr('Ikke i noe rom', 'In no room')
    return i18n.tr(`I ${count} rom`, `In ${count} ${count === 1 ? 'room' : 'rooms'}`)
  }

  return (
    <Show when={available()}>
      <SectionHeader
        title={i18n.tr('Personlige agenter', 'Personal agents')}
        description={i18n.tr(
          'Agenter du eier. Du bestemmer hvem som kan se dem og legge dem til i rom. En agent i et rom får bare det rommet og Control gir den; den arver ingenting fra deg.',
          'Agents you own. You choose who can see them and add them to rooms. An agent in a room gets only what the room and Control grant it; it inherits nothing from you.',
        )}
      />

      <form
        class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
        onSubmit={(event) => void create(event)}
      >
        <label for="personal-agent-name" class="verevon-settings-field">
          <span class="verevon-settings-label">{i18n.tr('Navn', 'Name')}</span>
          <span class="verevon-settings-input-wrap">
            <input
              id="personal-agent-name"
              type="text"
              class="verevon-settings-input"
              value={name()}
              maxlength={MAX_NAME_LENGTH}
              onInput={(event) => setName(event.currentTarget.value)}
            />
          </span>
          <span class="verevon-settings-help">
            {trimmedName().length} / {MAX_NAME_LENGTH} {i18n.tr('tegn', 'characters')}
          </span>
        </label>

        <label for="personal-agent-instructions" class="verevon-settings-field">
          <span class="verevon-settings-label">{i18n.tr('Instrukser (valgfritt)', 'Instructions (optional)')}</span>
          <span class="verevon-settings-input-wrap">
            <textarea
              id="personal-agent-instructions"
              class="verevon-settings-input verevon-settings-textarea"
              rows={5}
              value={instructions()}
              maxlength={MAX_INSTRUCTIONS_LENGTH}
              onInput={(event) => setInstructions(event.currentTarget.value)}
            />
          </span>
          <span class="verevon-settings-help">
            {instructions().length} / {MAX_INSTRUCTIONS_LENGTH} {i18n.tr('tegn', 'characters')}
          </span>
        </label>

        <Show when={createError()}>
          {(message) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <div>
          <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={!canCreate()}>
            <Show
              when={creating()}
              fallback={<><Sparkles size={14} aria-hidden="true" /> {i18n.tr('Opprett agent', 'Create agent')}</>}
            >
              <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Oppretter …', 'Creating…')}
            </Show>
          </SettingsButton>
        </div>
      </form>

      <Show when={list.loading}>
        <p class="verevon-settings-subnote" role="status">{i18n.tr('Laster agenter …', 'Loading agents…')}</p>
      </Show>

      <Show when={list.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {translateApiError(list.error, i18n.tr, {
            no: 'Agentene dine kunne ikke hentes. Prøv igjen om litt.',
            en: 'Your agents could not be loaded. Try again shortly.',
          })}
        </p>
      </Show>

      <Show when={primaryError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={list.error ? undefined : list()}>
        <Show
          when={list()?.agents.length}
          fallback={
            <p class="verevon-settings-subnote">
              {i18n.tr('Du har ingen personlige agenter ennå.', 'You have no personal agents yet.')}
            </p>
          }
        >
          <ul class="verevon-settings-divided-list">
            <For each={list()?.agents ?? []}>
              {(agent) => (
                <li>
                  <div>
                    <strong>{agent.name.trim() || i18n.tr('Agent uten navn', 'Agent with no name')}</strong>
                    <p class="verevon-settings-subnote">
                      {statusLabel(agent.status)} · {visibilityLabel(agent.visibility)} · {roomsLabel(agent.rooms.length)}
                      <Show when={isPrimary(agent)}>
                        {' · '}
                        {i18n.tr('Primær agent', 'Primary agent')}
                      </Show>
                    </p>
                  </div>
                  <div>
                    <SettingsButton settingsSize="sm" onClick={() => setAccessRef(agent.agentRef)}>
                      {i18n.tr('Tilgang', 'Access')}
                    </SettingsButton>
                    <SettingsButton
                      settingsSize="sm"
                      disabled={primaryBusy()}
                      onClick={() => void choosePrimary(isPrimary(agent) ? null : agent)}
                    >
                      {isPrimary(agent)
                        ? i18n.tr('Fjern som primær', 'Clear primary')
                        : i18n.tr('Sett som primær', 'Set as primary')}
                    </SettingsButton>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>

      <SpaceAgentAccessDialog
        agentRef={accessRef()}
        open={() => accessRef() !== undefined}
        onClose={() => setAccessRef(undefined)}
        onSaved={() => void refetch()}
      />
    </Show>
  )
}
