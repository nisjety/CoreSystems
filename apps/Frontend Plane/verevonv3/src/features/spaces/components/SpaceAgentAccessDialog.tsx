import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { Loader2, X } from '@/shared/icons'

import {
  getPersonalAgentAccess,
  updatePersonalAgentAccess,
  type PersonalAgentAddPolicy,
  type PersonalAgentEntry,
  type PersonalAgentEntryRole,
  type PersonalAgentVisibility,
} from '@/shared/api/spaces-client'
import { hasErrorCode } from '@/shared/api/api-error'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/** The most people one agent's access list may name (the limit the gateway and Control enforce). */
const MAX_ENTRIES = 200

/**
 * Who may see one of the caller's agents, who may add it to a room, and who holds manager or
 * collaborator access (WS17-S5). It is opened from the room's agent card and from the personal
 * agents settings. A save sends the whole list the person sees, against the revision they read. A
 * newer revision is refused by Control (409 `agent_state_changed`); the dialog then stops and offers
 * a reload instead of overwriting the other change. Presentation only: the gateway and Control decide
 * who may edit.
 */

export interface SpaceAgentAccessDialogProps {
  /** The agent definition (`agent_ref`). Nothing is read while it is undefined. */
  readonly agentRef: string | undefined
  readonly open: () => boolean
  readonly onClose: () => void
  /** Called after Control has stored the new access. */
  readonly onSaved?: () => void
}

function sameEntries(a: readonly PersonalAgentEntry[], b: readonly PersonalAgentEntry[]): boolean {
  return (
    a.length === b.length &&
    a.every((entry, index) => entry.principalId === b[index]?.principalId && entry.role === b[index]?.role)
  )
}

/** A principal id is the only name a person has here, so it is shortened; the full id is on hover. */
function shortPrincipal(id: string): string {
  return id.length > 16 ? `${id.slice(0, 8)}…${id.slice(-4)}` : id
}

export function SpaceAgentAccessDialog(props: SpaceAgentAccessDialogProps) {
  const i18n = useI18n()
  const [access, { refetch }] = createResource(
    () => (props.open() && props.agentRef ? props.agentRef : undefined),
    (agentRef: string) => getPersonalAgentAccess(agentRef),
  )

  const [visibility, setVisibility] = createSignal<PersonalAgentVisibility>('owner')
  const [addPolicy, setAddPolicy] = createSignal<PersonalAgentAddPolicy>('owner_only')
  const [entries, setEntries] = createSignal<readonly PersonalAgentEntry[]>([])
  const [principalDraft, setPrincipalDraft] = createSignal('')
  const [roleDraft, setRoleDraft] = createSignal<PersonalAgentEntryRole>('collaborator')
  const [entryError, setEntryError] = createSignal<string | null>(null)
  const [saving, setSaving] = createSignal(false)
  const [saveError, setSaveError] = createSignal<string | null>(null)
  const [conflict, setConflict] = createSignal(false)
  const [saved, setSaved] = createSignal(false)

  const editable = () => access()?.role === 'owner' || access()?.role === 'manager'
  const dirty = createMemo(() => {
    const current = access()
    return (
      !!current &&
      (visibility() !== current.visibility ||
        addPolicy() !== current.addPolicy ||
        !sameEntries(entries(), current.entries))
    )
  })

  // Every stored read resets the drafts, so a save only ever sends the list the person is shown.
  createEffect(() => {
    const current = access()
    if (!current) return
    setVisibility(current.visibility)
    setAddPolicy(current.addPolicy)
    setEntries(current.entries.map((entry) => ({ principalId: entry.principalId, role: entry.role })))
    setEntryError(null)
    setSaveError(null)
    setConflict(false)
  })

  function changed(): void {
    setSaved(false)
  }

  function addEntry(): void {
    const principalId = principalDraft().trim()
    setEntryError(null)
    if (!principalId) {
      setEntryError(i18n.tr('Skriv inn id-en til personen.', 'Enter the id of the person.'))
      return
    }
    if (entries().some((entry) => entry.principalId === principalId)) {
      setEntryError(i18n.tr('Personen står allerede på listen.', 'That person is already on the list.'))
      return
    }
    if (entries().length >= MAX_ENTRIES) {
      setEntryError(
        i18n.tr(
          `Listen kan ha høyst ${MAX_ENTRIES} personer.`,
          `The list can hold at most ${MAX_ENTRIES} people.`,
        ),
      )
      return
    }
    setEntries([...entries(), { principalId, role: roleDraft() }])
    setPrincipalDraft('')
    changed()
  }

  function setEntryRole(index: number, role: PersonalAgentEntryRole): void {
    setEntries(entries().map((entry, at) => (at === index ? { ...entry, role } : entry)))
    changed()
  }

  function removeEntry(index: number): void {
    setEntries(entries().filter((_, at) => at !== index))
    changed()
  }

  async function reload(): Promise<void> {
    setConflict(false)
    await refetch()
  }

  async function save(event: Event): Promise<void> {
    event.preventDefault()
    const current = access()
    const agentRef = props.agentRef
    if (!current || !agentRef || saving() || conflict() || !editable() || !dirty()) return
    setSaving(true)
    setSaveError(null)
    setSaved(false)
    let stored = false
    try {
      await updatePersonalAgentAccess(agentRef, {
        visibility: visibility(),
        addPolicy: addPolicy(),
        entries: entries(),
        expectedRevision: current.revision,
      })
      stored = true
    } catch (error) {
      if (hasErrorCode(error, 'agent_state_changed')) {
        // Control holds a newer revision. Nothing is resent; the person reloads and decides again.
        setConflict(true)
      } else {
        setSaveError(
          translateApiError(error, i18n.tr, {
            no: 'Tilgangen kunne ikke lagres. Ingenting er endret.',
            en: 'The access could not be saved. Nothing was changed.',
          }),
        )
      }
    } finally {
      setSaving(false)
    }
    if (stored) {
      setSaved(true)
      void refetch()
      props.onSaved?.()
    }
  }

  return (
    <Show when={props.open()}>
      <div
        class="verevon-space-create-agent"
        role="presentation"
        onClick={(event) => { if (event.target === event.currentTarget) props.onClose() }}
      >
        <section
          class="verevon-space-create-agent__dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby="space-agent-access-title"
        >
          <header>
            <h3 id="space-agent-access-title">{i18n.tr('Tilgang til agenten', 'Agent access')}</h3>
            <button type="button" onClick={() => props.onClose()} aria-label={i18n.tr('Lukk', 'Close')}>
              <X size={16} />
            </button>
          </header>

          <p class="verevon-space-create-agent__note">
            {i18n.tr(
              'Bestem hvem som kan se agenten, og hvem som kan legge den til i rom. En endring lagres bare hvis ingen andre har endret tilgangen siden du åpnet den.',
              'Choose who can see this agent and who can add it to rooms. A change is saved only if nobody else has changed the access since you opened it.',
            )}
          </p>

          <Show when={access.loading}>
            <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter tilgang …', 'Loading access…')}</p>
          </Show>

          <Show when={access.error}>
            <p class="verevon-space-projection-error" role="alert">
              {translateApiError(access.error, i18n.tr, {
                no: 'Tilgangen til agenten kunne ikke hentes.',
                en: 'The access to this agent could not be loaded.',
              })}
            </p>
          </Show>

          <Show when={access()}>
            <form class="verevon-space-agent-access" onSubmit={(event) => void save(event)}>
              <Show when={!editable()}>
                <p class="verevon-space-create-agent__note">
                  {i18n.tr(
                    'Du kan se tilgangen, men bare eieren og lederne kan endre den.',
                    'You can see the access, but only the owner and managers can change it.',
                  )}
                </p>
              </Show>

              <fieldset class="verevon-space-agent-access__group" disabled={!editable() || saving()}>
                <legend>{i18n.tr('Synlighet', 'Visibility')}</legend>
                <label>
                  <input
                    type="radio"
                    name="agent-access-visibility"
                    value="owner"
                    checked={visibility() === 'owner'}
                    onChange={() => { setVisibility('owner'); changed() }}
                  />
                  <span>{i18n.tr('Bare meg', 'Only me')}</span>
                </label>
                <label>
                  <input
                    type="radio"
                    name="agent-access-visibility"
                    value="org"
                    checked={visibility() === 'org'}
                    onChange={() => { setVisibility('org'); changed() }}
                  />
                  <span>{i18n.tr('Hele organisasjonen', 'The whole organization')}</span>
                </label>
              </fieldset>

              <fieldset class="verevon-space-agent-access__group" disabled={!editable() || saving()}>
                <legend>{i18n.tr('Hvem kan legge den til i rom', 'Who can add it to a room')}</legend>
                <label>
                  <input
                    type="radio"
                    name="agent-access-add-policy"
                    value="owner_only"
                    checked={addPolicy() === 'owner_only'}
                    onChange={() => { setAddPolicy('owner_only'); changed() }}
                  />
                  <span>{i18n.tr('Bare eieren', 'Only the owner')}</span>
                </label>
                <label>
                  <input
                    type="radio"
                    name="agent-access-add-policy"
                    value="nobody"
                    checked={addPolicy() === 'nobody'}
                    onChange={() => { setAddPolicy('nobody'); changed() }}
                  />
                  <span>{i18n.tr('Ingen', 'Nobody')}</span>
                </label>
                <label>
                  <input
                    type="radio"
                    name="agent-access-add-policy"
                    value="anyone"
                    checked={addPolicy() === 'anyone'}
                    onChange={() => { setAddPolicy('anyone'); changed() }}
                  />
                  <span>{i18n.tr('Alle som kan se den', 'Anyone who can see it')}</span>
                </label>
              </fieldset>

              <fieldset class="verevon-space-agent-access__group" disabled={!editable() || saving()}>
                <legend>{i18n.tr('Personer med tilgang', 'People with access')}</legend>
                <Show
                  when={entries().length > 0}
                  fallback={
                    <p class="verevon-space-agent-access__empty">
                      {i18n.tr('Ingen er lagt til ennå.', 'No one has been added yet.')}
                    </p>
                  }
                >
                  <ul class="verevon-space-agent-access__list">
                    <For each={entries()}>
                      {(entry, index) => (
                        <li class="verevon-space-agent-access__row">
                          <code title={entry.principalId}>{shortPrincipal(entry.principalId)}</code>
                          <select
                            aria-label={i18n.tr(
                              `Rolle for ${shortPrincipal(entry.principalId)}`,
                              `Role for ${shortPrincipal(entry.principalId)}`,
                            )}
                            onChange={(event) =>
                              setEntryRole(index(), event.currentTarget.value === 'manager' ? 'manager' : 'collaborator')
                            }
                          >
                            <option value="manager" selected={entry.role === 'manager'}>
                              {i18n.tr('Leder', 'Manager')}
                            </option>
                            <option value="collaborator" selected={entry.role === 'collaborator'}>
                              {i18n.tr('Samarbeidspartner', 'Collaborator')}
                            </option>
                          </select>
                          <button type="button" onClick={() => removeEntry(index())}>
                            {i18n.tr('Fjern', 'Remove')}
                          </button>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
                <div class="verevon-space-agent-access__add">
                  <input
                    type="text"
                    value={principalDraft()}
                    maxlength={200}
                    placeholder={i18n.tr('Person-id', 'Person id')}
                    aria-label={i18n.tr('Person-id', 'Person id')}
                    onInput={(event) => setPrincipalDraft(event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault()
                        addEntry()
                      }
                    }}
                  />
                  <select
                    aria-label={i18n.tr('Rolle for ny person', 'Role for the new person')}
                    onChange={(event) =>
                      setRoleDraft(event.currentTarget.value === 'manager' ? 'manager' : 'collaborator')
                    }
                  >
                    <option value="collaborator" selected={roleDraft() === 'collaborator'}>
                      {i18n.tr('Samarbeidspartner', 'Collaborator')}
                    </option>
                    <option value="manager" selected={roleDraft() === 'manager'}>
                      {i18n.tr('Leder', 'Manager')}
                    </option>
                  </select>
                  <button type="button" onClick={() => addEntry()}>
                    {i18n.tr('Legg til', 'Add')}
                  </button>
                </div>
                <Show when={entryError()}>
                  {(message) => <p class="verevon-space-projection-error" role="alert">{message()}</p>}
                </Show>
              </fieldset>

              <Show when={conflict()}>
                <div class="verevon-space-agent-access__conflict" role="alert">
                  <p>
                    {i18n.tr(
                      'Tilgangen er endret av noen andre, så lagringen ble stoppet. Ingenting er overskrevet. Last inn de nyeste innstillingene og gjør endringen på nytt.',
                      'Someone else changed the access, so the save was stopped. Nothing was overwritten. Load the latest settings and make your change again.',
                    )}
                  </p>
                  <button type="button" class="verevon-space-secondary-action" onClick={() => void reload()}>
                    {i18n.tr('Last inn på nytt', 'Reload')}
                  </button>
                </div>
              </Show>

              <Show when={saveError()}>
                {(message) => <p class="verevon-space-projection-error" role="alert">{message()}</p>}
              </Show>

              <Show when={saved()}>
                <p class="verevon-space-inline-status" role="status">
                  {i18n.tr('Tilgangen er lagret.', 'The access is saved.')}
                </p>
              </Show>

              <button
                type="submit"
                class="verevon-space-secondary-action"
                disabled={!editable() || !dirty() || conflict() || saving()}
              >
                <Show
                  when={saving()}
                  fallback={i18n.tr('Lagre tilgang', 'Save access')}
                >
                  <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Lagrer …', 'Saving…')}
                </Show>
              </button>
            </form>
          </Show>
        </section>
      </div>
    </Show>
  )
}
