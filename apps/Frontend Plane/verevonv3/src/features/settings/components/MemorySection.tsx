import { Loader2, Trash2 } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  deleteMemory,
  listMemories,
  type MemoryEntry,
  type MemoryProvenance,
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

export function MemorySection() {
  const i18n = useI18n()
  const [memories, { refetch }] = createResource(() => listMemories())
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [confirmId, setConfirmId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const list = createMemo(() => memories()?.memories ?? [])
  const degraded = createMemo(() => memories()?.degraded ?? false)

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
    </>
  )
}
