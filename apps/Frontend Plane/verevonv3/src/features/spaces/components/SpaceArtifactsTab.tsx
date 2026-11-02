import { Code2, FileText, Globe } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { ApiError } from '@/shared/api/http'
import {
  getSpaceArtifacts,
  loadSpaceTranscript,
  type SpaceArtifactSummary,
  type SpaceThreadTurn,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { mergeRoomArtifactEntries, type RoomArtifactEntry } from '../lib/room-artifact-entries'
import { roomItems } from '../lib/room-artifact-items'
import type { RoomLiveArtifacts } from '../lib/room-live-artifacts'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { SpaceArtifactSheet } from './SpaceArtifactSheet'
import { useActiveSpaceTab } from './SpaceCockpit'
import { SpaceRestrictedNotice } from './SpaceRestrictedNotice'
import './space-artifacts.css'

/**
 * The room's Artifacts tab (WS07-S12): every agent artifact persisted in this
 * Space's threads, latest version of each, over Session Core's aggregate read.
 *
 * # Lazy on purpose
 *
 * Panels mount eagerly (the cockpit hides, never unmounts), so this reads the
 * aggregate only while its own tab is the active one, and again each time the
 * tab is re-entered. A room nobody opens the Artifacts tab of costs nothing.
 *
 * # The list carries no content
 *
 * Selecting an entry reads that thread's transcript once (through the room's
 * own Control decision, like the timeline) and opens the same per-thread sheet
 * the chips open, on the room surface: document and code as text, HTML only
 * after "Run preview". Nothing here is written to browser storage; what the
 * sender's own stream produced is held in memory until a read carries it.
 *
 * # States
 *
 * loading, empty, restricted (shared reads off for this reader), zero data
 * retention (nothing is kept, so nothing is listed), error with Retry. None of
 * them is dressed as another: a failed read is not an empty room.
 */
export interface SpaceArtifactsTabProps {
  readonly spaceRef: string
  /** The sender's own stream artifacts, shared with the timeline's chips. */
  readonly live?: RoomLiveArtifacts
}

type TabStatus = 'idle' | 'loading' | 'ready' | 'restricted' | 'zdr' | 'disabled' | 'error'

const ARTIFACTS_TAB_ID = 'artefakter'

export function SpaceArtifactsTab(props: SpaceArtifactsTabProps) {
  const i18n = useI18n()
  const active = useActiveSpaceTab()
  const isActive = createMemo(() => (active() as string) === ARTIFACTS_TAB_ID)

  const [status, setStatus] = createSignal<TabStatus>('idle')
  const [aggregate, setAggregate] = createSignal<readonly SpaceArtifactSummary[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [loadingMore, setLoadingMore] = createSignal(false)
  const [moreFailed, setMoreFailed] = createSignal(false)
  let callId = 0
  // Transcripts read for an opened entry, by thread. Cleared on every refresh so
  // a revision persisted since is never hidden behind a stale copy.
  const transcripts = new Map<string, readonly SpaceThreadTurn[]>()

  const statusForError = (error: unknown): TabStatus => {
    if (error instanceof ApiError && error.status === 403) return 'restricted'
    if (error instanceof ApiError && error.status === 404 && error.code === 'space_room_artifacts_disabled') {
      return 'disabled'
    }
    return 'error'
  }

  const loadFirst = async (spaceRef: string) => {
    const id = ++callId
    transcripts.clear()
    setMoreFailed(false)
    // Keep a list already on screen while refreshing; only a cold start shows
    // the loading state.
    if (aggregate().length === 0) setStatus('loading')
    try {
      const page = await getSpaceArtifacts(spaceRef)
      if (id !== callId) return
      if (page.zdr === true) {
        setAggregate([])
        setNextCursor(null)
        setStatus('zdr')
        return
      }
      if (!page.sharedReadAvailable) {
        setAggregate([])
        setNextCursor(null)
        setStatus('restricted')
        return
      }
      setAggregate(page.artifacts)
      setNextCursor(page.nextCursor)
      setStatus('ready')
    } catch (error) {
      if (id !== callId) return
      setAggregate([])
      setNextCursor(null)
      setStatus(statusForError(error))
    }
  }

  // Fetch only while this tab is the active one. The compute side yields the
  // Space ref while active and '' otherwise, so leaving the tab fetches nothing
  // and coming back re-reads.
  createEffect(
    () => (isActive() ? props.spaceRef : ''),
    (spaceRef) => {
      if (spaceRef) void loadFirst(spaceRef)
    },
  )

  const loadMore = async () => {
    const cursor = nextCursor()
    if (!cursor || loadingMore()) return
    const id = callId
    setLoadingMore(true)
    setMoreFailed(false)
    try {
      const page = await getSpaceArtifacts(props.spaceRef, { cursor })
      if (id !== callId) return
      setAggregate((current) => {
        const seen = new Set(current.map((entry) => `${entry.thread_id}\u0000${entry.artifact_id}`))
        return [
          ...current,
          ...page.artifacts.filter((entry) => !seen.has(`${entry.thread_id}\u0000${entry.artifact_id}`)),
        ]
      })
      setNextCursor(page.nextCursor)
    } catch {
      if (id === callId) setMoreFailed(true)
    } finally {
      setLoadingMore(false)
    }
  }

  // Live entries only make sense where the aggregate itself would be shown.
  const entries = createMemo<readonly RoomArtifactEntry[]>(() =>
    status() === 'ready'
      ? mergeRoomArtifactEntries(aggregate(), props.live?.all() ?? new Map())
      : [],
  )

  // The opened entry: which thread, which artifact, and that thread's turns.
  type OpenArtifact = { readonly threadId: string; readonly artifactId: string; readonly turns: readonly SpaceThreadTurn[] }
  const [opened, setOpened] = createSignal<OpenArtifact | undefined>(undefined)
  const [opening, setOpening] = createSignal<string | undefined>(undefined)
  const [openFailure, setOpenFailure] = createSignal<'restricted' | 'missing' | 'error' | undefined>(undefined)
  let openId = 0

  const open = async (entry: RoomArtifactEntry) => {
    const id = ++openId
    setOpenFailure(undefined)
    const cached = transcripts.get(entry.threadId)
    if (cached) {
      setOpened({ threadId: entry.threadId, artifactId: entry.artifactId, turns: cached })
      return
    }
    setOpening(entry.key)
    const load = await loadSpaceTranscript(props.spaceRef, entry.threadId)
    if (id !== openId) return
    setOpening(undefined)
    if (load.state === 'ok') {
      transcripts.set(entry.threadId, load.turns)
      setOpened({ threadId: entry.threadId, artifactId: entry.artifactId, turns: load.turns })
      return
    }
    // The sender's own live copy is still theirs to read when the room's
    // transcript cannot be (it has not been persisted, or was refused).
    if ((props.live?.forThread(entry.threadId).length ?? 0) > 0) {
      setOpened({ threadId: entry.threadId, artifactId: entry.artifactId, turns: [] })
      return
    }
    setOpenFailure(load.state)
  }

  const sheetItems = () => {
    const current = opened()
    if (!current) return []
    const persisted = new Set(
      current.turns.flatMap((turn) => (turn.artifacts ?? []).map((artifact) => `${artifact.id}\u0000${artifact.version}`)),
    )
    const pending = (props.live?.forThread(current.threadId) ?? []).filter(
      (artifact) => !persisted.has(`${artifact.id}\u0000${artifact.version}`),
    )
    return roomItems([...current.turns, { artifacts: pending }])
  }

  const kindLabel = (kind: RoomArtifactEntry['kind']) => {
    switch (kind) {
      case 'code':
        return i18n.tr('Kode', 'Code')
      case 'html':
        return 'HTML'
      default:
        return i18n.tr('Dokument', 'Document')
    }
  }

  const retry = () => {
    void loadFirst(props.spaceRef)
  }

  return (
    <section class="verevon-space-view verevon-space-artifacts" aria-labelledby="space-artifacts-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Artefakter i rommet', 'Artifacts in this Space')}</p>
          <h2 id="space-artifacts-title">{i18n.tr('Artefakter', 'Artifacts')}</h2>
          <p>{i18n.tr(
            'Dokumenter og kode agentene har skrevet i samtalene her, nyeste versjon av hver.',
            'Documents and code agents have written in this Space’s conversations, latest version of each.',
          )}</p>
        </div>
      </div>

      <Show when={status() === 'idle' || status() === 'loading'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter artefakter i rommet …', 'Loading this Space’s artifacts…')}
        </p>
      </Show>

      <Show when={status() === 'restricted'}>
        <SpaceRestrictedNotice variant="room" onRetry={retry} />
      </Show>

      <Show when={status() === 'zdr'}>
        <div class="verevon-space-empty-state" role="status">
          <FileText size={20} aria-hidden="true" />
          <div>
            <h3>{i18n.tr('Ingen artefakter lagres', 'No artifacts are kept')}</h3>
            <p>{i18n.tr(
              'Organisasjoner med null datalagring beholder ikke artefakter. Ingenting er listet.',
              'Organizations with zero data retention do not keep artifacts. Nothing is listed.',
            )}</p>
          </div>
        </div>
      </Show>

      <Show when={status() === 'disabled'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr(
            'Artefaktoversikten er ikke slått på for dette miljøet.',
            'The artifact overview is not turned on for this environment.',
          )}
        </p>
      </Show>

      {/* A failed read is not an empty room. */}
      <Show when={status() === 'error'}>
        <div class="verevon-space-projection-error" role="alert">
          <p>{i18n.tr(
            'Vi fikk ikke hentet artefaktene i rommet. Samtalene er uendret.',
            'We could not load this Space’s artifacts. The conversations are unchanged.',
          )}</p>
          <button type="button" class="verevon-space-secondary-action" onClick={retry}>
            {i18n.tr('Prøv igjen', 'Try again')}
          </button>
        </div>
      </Show>

      <Show when={status() === 'ready'}>
        <Show
          when={entries().length > 0}
          fallback={
            <div class="verevon-space-empty-state" role="status">
              <FileText size={20} aria-hidden="true" />
              <div>
                <h3>{i18n.tr('Ingen artefakter ennå', 'No artifacts yet')}</h3>
                <p>{i18n.tr(
                  'Be en agent skrive et dokument eller kode, så dukker det opp her.',
                  'Ask an agent to write a document or code.',
                )}</p>
              </div>
            </div>
          }
        >
          <Show when={openFailure()}>
            {(failure) => (
              <Show
                when={failure() === 'restricted'}
                fallback={
                  <p class="verevon-space-projection-error" role="alert">
                    {failure() === 'missing'
                      ? i18n.tr('Samtalen finnes ikke lenger for deg.', 'That conversation is no longer available to you.')
                      : i18n.tr('Vi fikk ikke åpnet artefakten. Prøv igjen.', 'We could not open that artifact. Try again.')}
                  </p>
                }
              >
                <SpaceRestrictedNotice variant="thread" />
              </Show>
            )}
          </Show>

          <ul class="verevon-space-artifacts__list">
            <For each={entries()}>
              {(entry) => (
                <li>
                  <button
                    type="button"
                    class="verevon-space-artifacts__entry"
                    aria-busy={opening() === entry.key ? 'true' : 'false'}
                    onClick={() => {
                      void open(entry)
                    }}
                  >
                    <span class="verevon-space-artifacts__icon" aria-hidden="true">
                      <Show
                        when={entry.kind === 'code'}
                        fallback={
                          <Show when={entry.kind === 'html'} fallback={<FileText size={16} />}>
                            <Globe size={16} />
                          </Show>
                        }
                      >
                        <Code2 size={16} />
                      </Show>
                    </span>
                    <span class="verevon-space-artifacts__body">
                      <span class="verevon-space-artifacts__title">
                        {entry.title || i18n.tr('Uten tittel', 'Untitled')}
                      </span>
                      <span class="verevon-space-artifacts__meta">
                        <span>{kindLabel(entry.kind)}</span>
                        <span>v{entry.version}</span>
                        <Show when={entry.versionCount > 1}>
                          <span>{i18n.tr(`${entry.versionCount} versjoner`, `${entry.versionCount} versions`)}</span>
                        </Show>
                        <Show when={entry.agentName}>{(name) => <span>{name()}</span>}</Show>
                        <Show when={entry.sizeChars > 0}>
                          <span>{i18n.tr(`${entry.sizeChars} tegn`, `${entry.sizeChars} chars`)}</span>
                        </Show>
                        <Show when={entry.createdAt}>{(when) => <span>{formatWhenWithTime(when(), { locale: i18n.locale() })}</span>}</Show>
                        <Show when={entry.live}>
                          <span class="verevon-space-artifacts__live">{i18n.tr('Ny', 'New')}</span>
                        </Show>
                      </span>
                    </span>
                  </button>
                </li>
              )}
            </For>
          </ul>

          <Show when={nextCursor()}>
            <div class="verevon-space-artifacts__more">
              <Show when={moreFailed()}>
                <p class="verevon-space-projection-error" role="alert">
                  {i18n.tr('Vi fikk ikke hentet flere artefakter.', 'We could not load more artifacts.')}
                </p>
              </Show>
              <button
                type="button"
                class="verevon-space-secondary-action"
                disabled={loadingMore()}
                onClick={() => {
                  void loadMore()
                }}
              >
                {loadingMore() ? i18n.tr('Henter …', 'Loading…') : i18n.tr('Vis flere', 'Load more')}
              </button>
            </div>
          </Show>
        </Show>
      </Show>

      <Show when={opened()}>
        {(current) => (
          <SpaceArtifactSheet
            items={sheetItems}
            selectedId={current().artifactId}
            onClose={() => setOpened(undefined)}
          />
        )}
      </Show>
    </section>
  )
}
