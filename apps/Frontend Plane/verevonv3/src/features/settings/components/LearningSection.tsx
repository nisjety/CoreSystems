import { Check, Loader2, Trash2, Undo2, X } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { chatThreadHref } from '@/features/chat/lib/chat-thread-deep-link'
import {
  decideLearningProposal,
  listLearningLedger,
  snapshotContent,
  undoLearningEntry,
  type LearningActorKind,
  type LearningLedgerEntry,
  type ProposalDecision,
} from '@/shared/api/learning-client'
import { deleteMemory } from '@/shared/api/memory-client'
import { listSkills, type Skill } from '@/shared/api/skills-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'

/**
 * "Læring" — the owner's learning inbox. Learned skills arrive as proposals that
 * are used only after the owner accepts them; the ledger lists what Verevon learned
 * and changed, with undo. Every row concerns the signed-in person alone: the server
 * takes whose proposals and ledger these are from the session.
 */

type I18n = ReturnType<typeof useI18n>

export type DiffLine = { kind: 'same' | 'added' | 'removed'; text: string }

/** Above this many line pairs the diff is not computed; the two texts are shown whole. */
const MAX_DIFF_CELLS = 250_000

/**
 * A line diff of two texts by longest common subsequence. An empty side has no lines.
 * A pair too large for the table is shown as the whole old text removed and the whole
 * new text added, which is honest about what it did not compare.
 */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = before === '' ? [] : before.split('\n')
  const b = after === '' ? [] : after.split('\n')
  if (a.length * b.length > MAX_DIFF_CELLS) {
    return [
      ...a.map((text): DiffLine => ({ kind: 'removed', text })),
      ...b.map((text): DiffLine => ({ kind: 'added', text })),
    ]
  }
  // common[i * width + j] is the longest common subsequence of a[i..] and b[j..].
  const width = b.length + 1
  const common = new Array<number>((a.length + 1) * width).fill(0)
  const at = (i: number, j: number): number => common[i * width + j] ?? 0
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      common[i * width + j] =
        a[i] === b[j] ? at(i + 1, j + 1) + 1 : Math.max(at(i + 1, j), at(i, j + 1))
    }
  }
  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    const left = a[i] ?? ''
    const right = b[j] ?? ''
    if (left === right) {
      lines.push({ kind: 'same', text: left })
      i++
      j++
    } else if (at(i + 1, j) >= at(i, j + 1)) {
      lines.push({ kind: 'removed', text: left })
      i++
    } else {
      lines.push({ kind: 'added', text: right })
      j++
    }
  }
  for (const text of a.slice(i)) lines.push({ kind: 'removed', text })
  for (const text of b.slice(j)) lines.push({ kind: 'added', text })
  return lines
}

function diffMarker(kind: DiffLine['kind']): string {
  if (kind === 'added') return '+ '
  if (kind === 'removed') return '- '
  return '  '
}

function formatWhen(value: string | null): string {
  if (!value) return '—'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '—'
  return date.toLocaleString('nb-NO', { dateStyle: 'medium', timeStyle: 'short' })
}

function actionLabel(entry: LearningLedgerEntry, i18n: I18n): string {
  if (entry.kind === 'memory') return i18n.tr('Husket', 'Remembered')
  switch (entry.action) {
    case 'created':
      return i18n.tr('Opprettet', 'Created')
    case 'proposed':
      return i18n.tr('Foreslått', 'Proposed')
    case 'updated':
      return i18n.tr('Oppdatert', 'Updated')
    case 'accepted':
      return i18n.tr('Godtatt', 'Accepted')
    case 'rejected':
      return i18n.tr('Avvist', 'Rejected')
    case 'enabled':
      return i18n.tr('Aktivert', 'Enabled')
    case 'disabled':
      return i18n.tr('Deaktivert', 'Disabled')
    case 'edited':
      return i18n.tr('Redigert', 'Edited')
    case 'deleted':
      return i18n.tr('Slettet', 'Deleted')
    case 'restored':
      return i18n.tr('Gjenopprettet', 'Restored')
    case 'promoted':
      return i18n.tr('Kopiert til organisasjonen', 'Copied to the organization')
    default:
      return i18n.tr('Endret', 'Changed')
  }
}

/** Who made the change, or `null` when nobody recorded it: no guess is shown. */
function actorLabel(kind: LearningActorKind, i18n: I18n): string | null {
  switch (kind) {
    case 'user':
      return i18n.tr('Av deg', 'By you')
    case 'system':
    case 'service':
      return i18n.tr('Av Verevon', 'By Verevon')
    case 'unattributed':
      return null
  }
}

/** Why memories are left out of the ledger, or `null` when they are not left out. */
function skipNotice(reason: string, i18n: I18n): string | null {
  switch (reason) {
    case 'user_paused':
      return i18n.tr('Minner vises ikke mens minnet er pauset.', 'Memories are not shown while memory is paused.')
    case 'user_off':
      return i18n.tr('Minner vises ikke mens minnet er av.', 'Memories are not shown while memory is off.')
    case 'org_disabled':
      return i18n.tr(
        'Minner vises ikke fordi organisasjonen har slått av minne.',
        'Memories are not shown because the organization has turned memory off.',
      )
    case 'zdr':
      return i18n.tr(
        'Minner vises ikke i en økt med Zero Data Retention.',
        'Memories are not shown in a session with Zero Data Retention.',
      )
    case 'policy_unavailable':
      return i18n.tr(
        'Minner vises ikke fordi minnepolicyen ikke kan leses akkurat nå.',
        'Memories are not shown because the memory policy cannot be read right now.',
      )
    default:
      return null
  }
}

/** What the review puts before the reason of a proposal the owner asked for with /learn (EP-11 S7). */
const LEARN_REASON_PREFIX = 'learn:'

/**
 * A proposal's reason as shown: whether the owner asked for it with /learn, and the reviewer's
 * sentence without the marker.
 */
export function proposalReason(reason: string): { requested: boolean; text: string } {
  if (!reason.startsWith(LEARN_REASON_PREFIX)) return { requested: false, text: reason }
  return { requested: true, text: reason.slice(LEARN_REASON_PREFIX.length).trim() }
}

/** A proposal waits for its owner alone; the server shows nobody else one. */
function isOwnPendingProposal(skill: Skill, ownerId: string): boolean {
  return ownerId !== '' && skill.review_state === 'proposed' && skill.owner_user_id === ownerId
}

function canUndo(entry: LearningLedgerEntry): boolean {
  if (entry.undone) return false
  // A memory has no undo here: it is forgotten, which is the same delete the memory settings use.
  return entry.kind === 'memory' || entry.undoable
}

export function LearningSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const ownerId = createMemo(() => session.user?.id ?? '')

  const [skills, { refetch: refetchSkills }] = createResource(
    () => orgId() || undefined,
    (id) => listSkills(id).then((result) => result.skills),
  )
  const [ledger, { refetch: refetchLedger }] = createResource(() => listLearningLedger())

  // Pages after the first are kept here; a refresh starts again from the first page.
  const [olderEntries, setOlderEntries] = createSignal<LearningLedgerEntry[]>([])
  const [olderCursor, setOlderCursor] = createSignal<string | undefined>(undefined)
  const [loadingMore, setLoadingMore] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [confirmId, setConfirmId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const entries = createMemo(() => [...(ledger()?.entries ?? []), ...olderEntries()])
  const nextCursor = createMemo(() => {
    const cursor = olderCursor() !== undefined ? olderCursor() : ledger()?.nextCursor
    return cursor ?? ''
  })
  const proposals = createMemo(() =>
    (skills() ?? []).filter((skill) => isOwnPendingProposal(skill, ownerId())),
  )
  const notice = createMemo(() => skipNotice(ledger()?.memorySkipReason ?? '', i18n))

  /**
   * The newest entry that proposed or last changed a skill's content. Its `before` is
   * where the proposal's diff starts, and its thread is the conversation it came from.
   */
  const proposalSource = (skillId: string): LearningLedgerEntry | undefined =>
    entries().find(
      (entry) =>
        entry.kind === 'skill' &&
        entry.skillId === skillId &&
        (entry.action === 'proposed' || entry.action === 'updated'),
    )

  const refresh = async () => {
    setOlderEntries([])
    setOlderCursor(undefined)
    await Promise.all([refetchLedger(), refetchSkills()])
  }

  const handleDecision = async (skill: Skill, decision: ProposalDecision) => {
    if (busyId()) return
    setBusyId(skill.id)
    setActionError(null)
    try {
      await decideLearningProposal(skill.id, decision)
      await refresh()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre avgjørelsen.',
          en: 'Could not save the decision.',
        }),
      )
    } finally {
      setBusyId(null)
    }
  }

  const handleUndo = async (entry: LearningLedgerEntry) => {
    if (busyId()) return
    // Forgetting a memory is destructive, so its first click only arms it, as deleting
    // a memory does in the memory settings.
    if (entry.kind === 'memory' && confirmId() !== entry.entryId) {
      setConfirmId(entry.entryId)
      return
    }
    setBusyId(entry.entryId)
    setActionError(null)
    try {
      if (entry.kind === 'memory') {
        // A declined erase is answered without an error, so the flag is read before
        // the row is said to be gone.
        const result = await deleteMemory(entry.entryId)
        if (!result.deleted) {
          setConfirmId(null)
          setActionError(
            i18n.tr(
              'Minnet ble ikke glemt. Det kan være at det ikke er ditt, eller at det ikke kan slettes nå.',
              'The memory was not forgotten. It may not be yours, or it cannot be erased right now.',
            ),
          )
          await refresh()
          return
        }
      } else {
        await undoLearningEntry(entry.entryId)
      }
      setConfirmId(null)
      await refresh()
    } catch (err) {
      setActionError(
        translateApiError(
          err,
          i18n.tr,
          entry.kind === 'memory'
            ? { no: 'Kunne ikke glemme minnet.', en: 'Could not forget the memory.' }
            : { no: 'Kunne ikke angre endringen.', en: 'Could not undo the change.' },
        ),
      )
    } finally {
      setBusyId(null)
    }
  }

  const handleLoadMore = async () => {
    const cursor = nextCursor()
    if (!cursor || loadingMore()) return
    setLoadingMore(true)
    setActionError(null)
    try {
      const page = await listLearningLedger({ cursor })
      setOlderEntries([...olderEntries(), ...page.entries])
      setOlderCursor(page.nextCursor)
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke laste flere endringer.',
          en: 'Could not load more changes.',
        }),
      )
    } finally {
      setLoadingMore(false)
    }
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Læring', 'Learning')}
        description={i18n.tr(
          'Forslag Verevon har laget fra samtalene dine, og alt Verevon har lært. Et forslag brukes ikke før du godtar det.',
          'Proposals Verevon drew from your conversations, and everything Verevon has learned. A proposal is not used until you accept it.',
        )}
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <h3>{i18n.tr('Forslag', 'Proposals')}</h3>

      <Show when={skills.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste forslagene.', 'Could not load the proposals.')}
        </p>
      </Show>

      <Show
        when={!skills.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster forslag…', 'Loading proposals…')}
          </p>
        }
      >
        <Show
          when={proposals().length > 0}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                {i18n.tr('Ingen forslag venter på deg.', 'No proposals are waiting for you.')}
              </p>
            </div>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={proposals()}>
              {(skill) => {
                const source = () => proposalSource(skill.id)
                const diff = () =>
                  lineDiff(snapshotContent(source()?.beforeJson ?? '') ?? '', skill.content)
                const busy = () => busyId() === skill.id
                const reason = () => proposalReason(skill.proposal_reason ?? '')
                return (
                  <div class="verevon-settings-integration-row">
                    <div>
                      <p>
                        {skill.name}{' '}
                        <span class="verevon-trust-chip">{i18n.tr('Venter på deg', 'Waiting for you')}</span>
                        <Show when={reason().requested}>
                          {' '}
                          <span class="verevon-trust-chip">{i18n.tr('Bedt om av deg', 'Requested by you')}</span>
                        </Show>
                      </p>
                      <Show when={reason().text}>
                        {(text) => <span>{text()}</span>}
                      </Show>
                      <Show when={source()?.threadId}>
                        {(threadId) => (
                          <p>
                            <a href={chatThreadHref(threadId())}>
                              {i18n.tr('Åpne samtalen', 'Open the conversation')}
                            </a>
                          </p>
                        )}
                      </Show>
                      <pre
                        class="verevon-learning-diff"
                        aria-label={i18n.tr('Endringer i forslaget', 'Changes in the proposal')}
                      >
                        <For each={diff()}>
                          {(line) => <span data-kind={line.kind}>{`${diffMarker(line.kind)}${line.text}\n`}</span>}
                        </For>
                      </pre>
                    </div>
                    <div>
                      <SettingsButton
                        settingsSize="sm"
                        variant="primary"
                        disabled={busy()}
                        onClick={() => void handleDecision(skill, 'accept')}
                        aria-label={i18n.tr(`Godta ${skill.name}`, `Accept ${skill.name}`)}
                      >
                        <Show
                          when={busy()}
                          fallback={
                            <>
                              <Check size={14} aria-hidden="true" /> {i18n.tr('Godta', 'Accept')}
                            </>
                          }
                        >
                          <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Lagrer…', 'Saving…')}
                        </Show>
                      </SettingsButton>{' '}
                      <SettingsButton
                        settingsSize="sm"
                        disabled={busy()}
                        onClick={() => void handleDecision(skill, 'reject')}
                        aria-label={i18n.tr(`Avvis ${skill.name}`, `Reject ${skill.name}`)}
                      >
                        <X size={14} aria-hidden="true" /> {i18n.tr('Avvis', 'Reject')}
                      </SettingsButton>
                    </div>
                  </div>
                )
              }}
            </For>
          </div>
        </Show>
      </Show>

      <h3>{i18n.tr('Det Verevon har lært', 'What Verevon learned')}</h3>

      <Show when={notice()}>
        {(message) => (
          <p class="verevon-settings-subnote" role="status">
            {message()}
          </p>
        )}
      </Show>

      <Show when={ledger.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste det Verevon har lært.', 'Could not load what Verevon learned.')}
        </p>
      </Show>

      <Show
        when={!ledger.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster…', 'Loading…')}
          </p>
        }
      >
        <Show
          when={entries().length > 0}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                {i18n.tr('Verevon har ikke lært noe ennå.', 'Verevon has not learned anything yet.')}
              </p>
            </div>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={entries()}>
              {(entry) => {
                const subject = () =>
                  entry.kind === 'memory' ? (snapshotContent(entry.afterJson) ?? '') : entry.skillName
                const actor = actorLabel(entry.actorKind, i18n)
                const busy = () => busyId() === entry.entryId
                const armed = () => confirmId() === entry.entryId
                return (
                  <div class="verevon-settings-integration-row">
                    <div>
                      <p>
                        {actionLabel(entry, i18n)}{' '}
                        <Show when={entry.undone}>
                          <span class="verevon-trust-chip">{i18n.tr('Angret', 'Undone')}</span>
                        </Show>
                      </p>
                      <span>{subject()}</span>
                      <span class="verevon-trust-chips">
                        <Show when={actor}>{(label) => <span class="verevon-trust-chip">{label()}</span>}</Show>
                        <span class="verevon-trust-chip">{formatWhen(entry.createdAt)}</span>
                      </span>
                      <Show when={proposalReason(entry.reason).text}>
                        {(reason) => <span class="verevon-settings-subnote">{reason()}</span>}
                      </Show>
                      <Show when={entry.threadId}>
                        {(threadId) => (
                          <p>
                            <a href={chatThreadHref(threadId())}>
                              {i18n.tr('Åpne samtalen', 'Open the conversation')}
                            </a>
                          </p>
                        )}
                      </Show>
                    </div>
                    <Show when={canUndo(entry)}>
                      <div>
                        <SettingsButton
                          settingsSize="sm"
                          danger={entry.kind === 'memory'}
                          disabled={busy()}
                          onClick={() => void handleUndo(entry)}
                          aria-label={`${entry.kind === 'memory' ? i18n.tr('Glem', 'Forget') : i18n.tr('Angre', 'Undo')} ${subject()}`}
                        >
                          <Show
                            when={busy()}
                            fallback={
                              entry.kind === 'memory' ? (
                                <>
                                  <Trash2 size={14} aria-hidden="true" />{' '}
                                  {armed()
                                    ? i18n.tr('Bekreft glemsel?', 'Confirm forgetting?')
                                    : i18n.tr('Glem', 'Forget')}
                                </>
                              ) : (
                                <>
                                  <Undo2 size={14} aria-hidden="true" /> {i18n.tr('Angre', 'Undo')}
                                </>
                              )
                            }
                          >
                            <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Lagrer…', 'Saving…')}
                          </Show>
                        </SettingsButton>
                      </div>
                    </Show>
                  </div>
                )
              }}
            </For>
          </div>
          <Show when={nextCursor()}>
            <div>
              <SettingsButton
                settingsSize="sm"
                disabled={loadingMore()}
                onClick={() => void handleLoadMore()}
              >
                {loadingMore() ? i18n.tr('Laster…', 'Loading…') : i18n.tr('Vis flere', 'Show more')}
              </SettingsButton>
            </div>
          </Show>
        </Show>
      </Show>
    </>
  )
}
