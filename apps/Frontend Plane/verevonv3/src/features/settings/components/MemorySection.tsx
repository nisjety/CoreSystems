import { Loader2, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  deleteMemory,
  getMemoryControls,
  listMemories,
  resetMemory,
  setMemoryState,
  type MemoryControls,
  type MemoryEntry,
  type MemoryProvenance,
  type MemoryState,
} from '@/shared/api/memory-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'

/**
 * "What do you remember about me" — lists durable memory entries the system
 * has extracted about the signed-in user, across every conversation (not
 * just the one currently open), with per-item delete.
 *
 * Backed by session-core's `MemoryService` (`ListMemory`/`DeleteMemory`),
 * fronted by model-gateway's `/v1/memories` and this gateway's
 * `/api/v1/memory` route. Deleting is destructive and non-recoverable, so it
 * uses the same two-step "click again to confirm" pattern as the members
 * list above, rather than a native `window.confirm`.
 *
 * A Zero Data Retention caller always sees the degraded-empty state below,
 * never an error: session-core answers `ListMemory` with an empty,
 * `degraded: true` response for a ZDR caller rather than failing, and this
 * component surfaces that as distinct from "you genuinely have zero
 * memories yet".
 */
function formatUpdatedAt(value: string): string {
  if (!value) return '—'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '—'
  return date.toLocaleString('nb-NO', { dateStyle: 'medium', timeStyle: 'short' })
}

/**
 * The provenance badge, or `null` when there is nothing honest to say.
 *
 * Only `inferred` gets a chip. That asymmetry is the point: it marks the rows a
 * user did NOT ask for, which is what they are here to review. Badging `stated`
 * too would bury the distinction in noise, and badging `unknown` would draw
 * attention to a row about which we can say nothing useful.
 */
function provenanceLabel(
  provenance: MemoryProvenance,
  i18n: ReturnType<typeof useI18n>,
): string | null {
  return provenance === 'inferred'
    ? i18n.tr('Utledet av Verevon', 'Inferred by Verevon')
    : null
}

function topicLabel(topic: string, i18n: ReturnType<typeof useI18n>): string {
  switch (topic) {
    case 'USER':
      return i18n.tr('Om deg', 'About you')
    case 'AGENT':
      return i18n.tr('Agent', 'Agent')
    case 'WORKSPACE':
      return i18n.tr('Arbeidsområde', 'Workspace')
    case 'POLICY':
      return i18n.tr('Retningslinje', 'Policy')
    default:
      return i18n.tr('Minne', 'Memory')
  }
}

const MEMORY_STATES: readonly MemoryState[] = ['on', 'paused', 'off']

function stateLabel(state: MemoryState, i18n: ReturnType<typeof useI18n>): string {
  switch (state) {
    case 'on':
      return i18n.tr('På', 'On')
    case 'paused':
      return i18n.tr('Pause', 'Paused')
    case 'off':
      return i18n.tr('Av', 'Off')
  }
}

function stateDescription(state: MemoryState, i18n: ReturnType<typeof useI18n>): string {
  switch (state) {
    case 'on':
      return i18n.tr(
        'Verevon husker og bruker det du deler.',
        'Verevon remembers and uses what you share.',
      )
    case 'paused':
      return i18n.tr(
        'Det som er lagret blir stående, men brukes ikke, og ingenting nytt læres. Det du skriver mens minnet står på pause, blir aldri lagret i ettertid.',
        'What is stored stays, but is not used, and nothing new is learned. What you write while memory is paused is never saved afterwards.',
      )
    case 'off':
      return i18n.tr(
        'Som pause, og det lagrede skjules også fra listen. Ingenting slettes.',
        'As paused, and what is stored is also hidden from the list. Nothing is deleted.',
      )
  }
}

/** The banner for a decision that overrides the user's own choice, or `null`. */
function overrideNotice(controls: MemoryControls, i18n: ReturnType<typeof useI18n>): string | null {
  switch (controls.reason) {
    case 'org_disabled':
      return i18n.tr(
        'Organisasjonen din har slått av minne. Valget ditt her gjelder først når en administrator slår det på igjen.',
        'Your organization has turned memory off. Your choice here only applies once an administrator turns it back on.',
      )
    case 'zdr':
      return i18n.tr(
        'Zero Data Retention er aktiv for denne økten. Minne brukes ikke, og tilstanden kan ikke endres herfra. Du kan fortsatt nullstille.',
        'Zero Data Retention is on for this session. Memory is not used, and the state cannot be changed from here. You can still reset.',
      )
    case 'policy_unavailable':
      return i18n.tr(
        'Organisasjonens minnepolicy kan ikke leses akkurat nå, så minne er satt på pause til den er tilgjengelig igjen.',
        'Your organization\'s memory policy cannot be read right now, so memory is paused until it is available again.',
      )
    default:
      return null
  }
}

export function MemorySection() {
  const i18n = useI18n()
  const [memories, { refetch }] = createResource(() => listMemories())
  const [controls, { refetch: refetchControls, mutate: setControls }] = createResource(() =>
    getMemoryControls(),
  )
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [confirmId, setConfirmId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [stateBusy, setStateBusy] = createSignal(false)
  const [resetStep, setResetStep] = createSignal<'idle' | 'confirm' | 'busy'>('idle')
  const [resetNote, setResetNote] = createSignal<string | null>(null)

  const list = createMemo(() => memories()?.memories ?? [])
  const degraded = createMemo(() => memories()?.degraded ?? false)

  const handleState = async (next: MemoryState) => {
    if (stateBusy() || controls()?.state === next) return
    setStateBusy(true)
    setActionError(null)
    setResetNote(null)
    try {
      setControls(await setMemoryState(next))
      // Off hides the list and the others show it again, so it is read afresh.
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke endre minneinnstillingen.',
          en: 'Could not change the memory setting.',
        }),
      )
      await refetchControls()
    } finally {
      setStateBusy(false)
    }
  }

  // Irreversible, so the first click only arms it: the same two-step pattern as
  // deleting a single memory.
  const handleReset = async () => {
    if (resetStep() === 'busy') return
    if (resetStep() === 'idle') {
      setResetStep('confirm')
      return
    }
    setResetStep('busy')
    setActionError(null)
    try {
      const result = await resetMemory()
      setResetNote(
        result.semanticUnconfirmed > 0
          ? i18n.tr(
              'Minnet er nullstilt. Noen kopier i søkeindeksen kunne ikke bekreftes slettet, men de brukes ikke.',
              'Memory was reset. Some copies in the search index could not be confirmed erased, but they are not used.',
            )
          : i18n.tr('Minnet er nullstilt.', 'Memory was reset.'),
      )
      await Promise.all([refetch(), refetchControls()])
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke nullstille minnet.',
          en: 'Could not reset memory.',
        }),
      )
    } finally {
      setResetStep('idle')
    }
  }

  const handleDelete = async (entry: MemoryEntry) => {
    if (busyId()) return
    if (confirmId() !== entry.memoryId) {
      setConfirmId(entry.memoryId)
      return
    }
    setBusyId(entry.memoryId)
    setActionError(null)
    try {
      await deleteMemory(entry.memoryId)
      setConfirmId(null)
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke slette minnet.',
          en: 'Could not delete the memory.',
        }),
      )
    } finally {
      setBusyId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Minne', 'Memory')}
        description={i18n.tr(
          'Dette er hva Verevon har lagret om deg på tvers av alle samtaler — ikke bare denne. Slett et minne du ikke lenger vil at agenten skal huske.',
          'This is what Verevon has stored about you across every conversation — not just this one. Delete anything you no longer want the agent to remember.',
        )}
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={resetNote()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--success" role="status">
            {message()}
          </p>
        )}
      </Show>

      <Show when={controls.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste minneinnstillingen.', 'Could not load the memory setting.')}
        </p>
      </Show>

      <Show when={controls()}>
        {(current) => (
          <div class="verevon-settings-list-card">
            <div
              role="radiogroup"
              aria-label={i18n.tr('Minne', 'Memory')}
              class="verevon-settings-integration-row"
            >
              <For each={MEMORY_STATES}>
                {(state) => (
                  <SettingsButton
                    role="radio"
                    aria-checked={current().state === state ? 'true' : 'false'}
                    variant={current().state === state ? 'primary' : 'secondary'}
                    settingsSize="sm"
                    disabled={stateBusy() || current().reason === 'zdr'}
                    onClick={() => void handleState(state)}
                  >
                    {stateLabel(state, i18n)}
                  </SettingsButton>
                )}
              </For>
            </div>
            <Show when={current().state}>
              {(state) => <p class="verevon-settings-subnote">{stateDescription(state(), i18n)}</p>}
            </Show>
            <Show when={overrideNotice(current(), i18n)}>
              {(notice) => (
                <p class="verevon-settings-subnote" role="status">
                  {notice()}
                </p>
              )}
            </Show>
            <Show when={current().hiddenRows > 0}>
              <p class="verevon-settings-subnote" role="status">
                {i18n.tr(
                  `${current().hiddenRows} minner er skjult mens minnet er av. De er ikke slettet.`,
                  `${current().hiddenRows} memories are hidden while memory is off. They are not deleted.`,
                )}
              </p>
            </Show>
          </div>
        )}
      </Show>

      <Show when={memories.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste minner.', 'Could not load memories.')}
        </p>
      </Show>

      <Show
        when={!memories.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster minner …', 'Loading memories...')}
          </p>
        }
      >
        <Show when={degraded() && list().length === 0}>
          <p class="verevon-settings-subnote" role="status">
            {i18n.tr(
              'Minne er ikke tilgjengelig akkurat nå — for eksempel fordi Zero Data Retention er aktivert for denne økten, eller minnetjenesten er midlertidig nede. Dette er ikke det samme som at du ikke har noen minner.',
              'Memory is not available right now — for example, Zero Data Retention may be on for this session, or the memory service may be temporarily down. This is different from genuinely having no memories.',
            )}
          </p>
        </Show>

        <Show
          when={list().length > 0}
          fallback={
            <Show when={!degraded()}>
              <div class="verevon-settings-list-card">
                <p class="verevon-settings-empty-row">
                  {i18n.tr(
                    'Verevon har ikke lagret noen minner om deg ennå. Etter hvert som du chatter, kan fakta og preferanser du deler begynne å dukke opp her.',
                    'Verevon has not stored any memories about you yet. As you chat, facts and preferences you share may start showing up here.',
                  )}
                </p>
              </div>
            </Show>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={list()}>
              {(entry) => (
                <div class="verevon-settings-integration-row">
                  <div>
                    <p>
                      {entry.content}{' '}
                      <span class="verevon-trust-chip" aria-label={topicLabel(entry.topic, i18n)}>
                        {topicLabel(entry.topic, i18n)}
                      </span>{' '}
                      <Show when={provenanceLabel(entry.provenance, i18n)}>
                        {(label) => (
                          <span
                            class="verevon-trust-chip"
                            aria-label={i18n.tr(
                              'Dette husket Verevon av seg selv – du ba ikke om det',
                              'Verevon remembered this on its own – you did not ask it to',
                            )}
                          >
                            {label()}
                          </span>
                        )}
                      </Show>
                      <Show when={entry.withheldReason}>
                        <span
                          class="verevon-trust-chip"
                          aria-label={i18n.tr(
                            'Lært i et delt rom før beskyttelsen kom. Ingen samtale husker dette. Du kan beholde eller slette det.',
                            'Learned in a shared room before protection existed. No conversation recalls it. You can keep or delete it.',
                          )}
                        >
                          {i18n.tr('Tilbakeholdt – lært i et delt rom', 'Withheld – learned in a shared room')}
                        </span>
                      </Show>
                    </p>
                    <span>
                      {i18n.tr('Sist oppdatert', 'Last updated')}: {formatUpdatedAt(entry.updatedAt)}
                    </span>
                  </div>
                  <div>
                    <SettingsButton
                      settingsSize="sm"
                      danger
                      disabled={busyId() === entry.memoryId}
                      onClick={() => void handleDelete(entry)}
                      aria-label={
                        confirmId() === entry.memoryId
                          ? `${i18n.tr('Bekreft sletting av minne', 'Confirm deleting memory')}: ${entry.content}`
                          : `${i18n.tr('Slett minne', 'Delete memory')}: ${entry.content}`
                      }
                    >
                      <Show
                        when={busyId() === entry.memoryId}
                        fallback={
                          <>
                            <Trash2 size={14} aria-hidden="true" />{' '}
                            {confirmId() === entry.memoryId
                              ? i18n.tr('Bekreft', 'Confirm')
                              : i18n.tr('Slett', 'Delete')}
                          </>
                        }
                      >
                        <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Sletter…', 'Deleting…')}
                      </Show>
                    </SettingsButton>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <div class="verevon-settings-list-card">
        <div class="verevon-settings-integration-row">
          <div>
            <p>{i18n.tr('Nullstill minne', 'Reset memory')}</p>
            <span>
              <Show
                when={resetStep() === 'idle'}
                fallback={i18n.tr(
                  'Dette sletter alt Verevon har lagret om deg, og kan ikke angres.',
                  'This erases everything Verevon has stored about you and cannot be undone.',
                )}
              >
                {i18n.tr(
                  'Slett alt Verevon har lagret om deg i ett steg.',
                  'Erase everything Verevon has stored about you in one step.',
                )}
              </Show>
            </span>
          </div>
          <div>
            <Show when={resetStep() !== 'idle'}>
              <SettingsButton
                settingsSize="sm"
                disabled={resetStep() === 'busy'}
                onClick={() => setResetStep('idle')}
              >
                {i18n.tr('Avbryt', 'Cancel')}
              </SettingsButton>{' '}
            </Show>
            <SettingsButton
              settingsSize="sm"
              danger
              disabled={resetStep() === 'busy'}
              onClick={() => void handleReset()}
            >
              <Show
                when={resetStep() === 'busy'}
                fallback={
                  <>
                    <Trash2 size={14} aria-hidden="true" />{' '}
                    {resetStep() === 'confirm'
                      ? i18n.tr('Bekreft nullstilling', 'Confirm reset')
                      : i18n.tr('Nullstill', 'Reset')}
                  </>
                }
              >
                <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Nullstiller…', 'Resetting…')}
              </Show>
            </SettingsButton>
          </div>
        </div>
      </div>
    </>
  )
}
