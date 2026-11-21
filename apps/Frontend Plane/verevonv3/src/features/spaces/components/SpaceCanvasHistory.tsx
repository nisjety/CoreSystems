import { createEffect, createSignal, For, Show } from 'solid-js'

import {
  getSpaceCanvasVersion,
  listSpaceCanvasVersions,
  type SpaceCanvasHistoryWindow,
  type SpaceCanvasVersion,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { canvasErrorMessage } from '@/shared/i18n/errors'

import {
  CANVAS_HISTORY_PAGE,
  canvasChangeNote,
  canvasCompareOrder,
  canvasVersionMeta,
  toggleCompareSelection,
} from '../lib/space-canvas'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { SpaceCanvasDiff } from './SpaceCanvasDiff'

/**
 * The version history of the room canvas (WS19-S7): the versions newest first, without text, in pages of 50, and a
 * comparison of any two versions, older first. A person who may write sees Restore on every version but the head. The
 * list is read when it opens and again whenever the head on screen changes, so a save or a restore shows at once.
 *
 * D21m: a member with a history window sees only the versions inside it (plus the current head) and a note that says
 * so. Restore is not offered there: Control refuses a restore under any window. Comparison stays, on listed versions only.
 *
 * Presentation only. The gateway and the owning planes decide what may be read; the restore itself is the panel's
 * write, and this component only asks the person to confirm it.
 */
export interface SpaceCanvasHistoryProps {
  readonly spaceRef: string
  readonly roster?: () => readonly SpaceRosterMember[]
  /** The head on screen. A new head reloads the list. */
  readonly headVersionId: string | null
  /** Whether this person may restore: the gateway's `can_write`, and a signed-in actor. */
  readonly canRestore: boolean
  /** A restore is in flight, so the restore buttons wait. */
  readonly restoring: boolean
  /** The person confirmed a restore of this version. */
  readonly onRestore: (version: SpaceCanvasVersion) => void
  readonly onClose: () => void
}

type Comparison = { readonly older: SpaceCanvasVersion; readonly newer: SpaceCanvasVersion }
type LoadStatus = 'loading' | 'ready' | 'error'

export function SpaceCanvasHistory(props: SpaceCanvasHistoryProps) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [versions, setVersions] = createSignal<readonly SpaceCanvasVersion[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [historyWindow, setHistoryWindow] = createSignal<SpaceCanvasHistoryWindow | null>(null)
  const [status, setStatus] = createSignal<LoadStatus>('loading')
  const [failure, setFailure] = createSignal<unknown>(null)
  const [moreBusy, setMoreBusy] = createSignal(false)
  const [moreFailed, setMoreFailed] = createSignal(false)
  const [picked, setPicked] = createSignal<readonly SpaceCanvasVersion[]>([])
  const [comparison, setComparison] = createSignal<Comparison | null>(null)
  const [comparing, setComparing] = createSignal(false)
  const [comparisonFailure, setComparisonFailure] = createSignal<unknown>(null)

  // Each list read takes a new call id; an answer that lands after a newer read (or after unmount) is dropped.
  let callId = 0
  let comparisonId = 0
  // The room the picks and the texts belong to. A Space's versions are never compared with another Space's.
  let shownRoom = ''
  // The text of each version read for a comparison. A version never changes, so its text is read once.
  const texts = new Map<string, SpaceCanvasVersion>()

  const loadFirst = async (spaceRef: string) => {
    const id = ++callId
    if (spaceRef !== shownRoom) {
      shownRoom = spaceRef
      texts.clear()
      setPicked([])
      setComparison(null)
      setComparisonFailure(null)
    }
    setStatus('loading')
    try {
      const page = await listSpaceCanvasVersions(spaceRef, { limit: CANVAS_HISTORY_PAGE })
      if (id !== callId) return
      setVersions(page.versions)
      setNextCursor(page.nextCursor)
      setHistoryWindow(page.window ?? null)
      setMoreFailed(false)
      setFailure(null)
      setStatus('ready')
    } catch (error) {
      if (id !== callId) return
      setFailure(error)
      setStatus('error')
    }
  }

  // Read when the history opens, and again whenever the head on screen changes.
  createEffect(
    () => `${props.spaceRef}\u0000${props.headVersionId ?? ''}`,
    () => {
      void loadFirst(props.spaceRef)
    },
  )

  const loadMore = async () => {
    const cursor = nextCursor()
    if (!cursor || moreBusy()) return
    const id = callId
    setMoreBusy(true)
    setMoreFailed(false)
    try {
      const page = await listSpaceCanvasVersions(props.spaceRef, { limit: CANVAS_HISTORY_PAGE, cursor })
      if (id !== callId) return
      setVersions((current) => {
        const seen = new Set(current.map((version) => version.versionId))
        return [...current, ...page.versions.filter((version) => !seen.has(version.versionId))]
      })
      setNextCursor(page.nextCursor)
    } catch {
      if (id === callId) setMoreFailed(true)
    } finally {
      setMoreBusy(false)
    }
  }

  const textOf = async (spaceRef: string, version: SpaceCanvasVersion): Promise<SpaceCanvasVersion> => {
    const known = texts.get(version.versionId)
    if (known) return known
    const full = await getSpaceCanvasVersion(spaceRef, version.versionId)
    texts.set(version.versionId, full)
    return full
  }

  const isPicked = (version: SpaceCanvasVersion) => picked().some((item) => item.versionId === version.versionId)

  // Picking a second version reads both texts and shows the difference, older first. A third pick drops the oldest.
  const togglePick = (version: SpaceCanvasVersion) => {
    const next = toggleCompareSelection(picked(), version)
    const [first, second] = next
    setPicked(next)
    setComparison(null)
    setComparisonFailure(null)
    if (!first || !second) return
    const id = ++comparisonId
    const [older, newer] = canvasCompareOrder(first, second)
    const spaceRef = props.spaceRef
    setComparing(true)
    void Promise.all([textOf(spaceRef, older), textOf(spaceRef, newer)])
      .then(([olderText, newerText]) => {
        if (id === comparisonId) setComparison({ older: olderText, newer: newerText })
      })
      .catch((error: unknown) => {
        if (id === comparisonId) setComparisonFailure(error)
      })
      .finally(() => {
        if (id === comparisonId) setComparing(false)
      })
  }

  const confirmRestore = (version: SpaceCanvasVersion) => {
    if (props.restoring) return
    const ok = window.confirm(
      tr(
        `Gjenopprette versjon ${version.versionNo}? Teksten blir den nyeste versjonen. Ingen versjon blir fjernet.`,
        `Restore version ${version.versionNo}? Its text becomes the newest version. No version is removed.`,
      ),
    )
    if (ok) props.onRestore(version)
  }

  const versionNoOf = (versionId: string): number | undefined =>
    versions().find((version) => version.versionId === versionId)?.versionNo

  return (
    <section class="verevon-space-canvas__history" aria-labelledby="space-canvas-history-title">
      <div class="verevon-space-canvas__toolbar">
        <h3 id="space-canvas-history-title">{tr('Versjoner', 'Versions')}</h3>
        <button type="button" class="verevon-space-secondary-action" onClick={() => props.onClose()}>
          {tr('Lukk', 'Close')}
        </button>
      </div>

      <Show when={status() === 'loading' && versions().length === 0}>
        <p class="verevon-space-inline-status" role="status">
          {tr('Laster versjonene …', 'Loading the versions…')}
        </p>
      </Show>

      <Show when={status() === 'error'}>
        <div class="verevon-space-projection-error" role="alert">
          <p>{canvasErrorMessage(failure(), tr)}</p>
          <button type="button" class="verevon-space-secondary-action" onClick={() => void loadFirst(props.spaceRef)}>
            {tr('Prøv igjen', 'Try again')}
          </button>
        </div>
      </Show>

      <Show when={historyWindow()}>
        {(windowed) => (
          <p class="verevon-space-canvas__hint" role="note">
            {windowed().startsAt
              ? tr(
                  `Du ser versjoner fra ${formatWhenWithTime(windowed().startsAt as string, { locale: i18n.locale() })}, slik historikken i rommet er delt med deg.`,
                  `You see versions from ${formatWhenWithTime(windowed().startsAt as string, { locale: i18n.locale() })}, as the room's history is shared with you.`,
                )
              : tr(
                  'Du ser bare versjonene som historikken i rommet er delt med deg for.',
                  "You see only the versions the room's history is shared with you for.",
                )}{' '}
            {windowed().hiddenBefore
              ? tr('Eldre versjoner er skjult. ', 'Older versions are hidden. ')
              : ''}
            {tr(
              'Du kan se og lagre det nåværende lerretet, men ikke gjenopprette versjoner.',
              'You can view and save the current canvas, but not restore versions.',
            )}
          </p>
        )}
      </Show>

      <Show when={status() === 'ready' && versions().length === 0}>
        <p class="verevon-space-canvas__hint">{tr('Lerretet har ingen versjoner ennå.', 'The canvas has no versions yet.')}</p>
      </Show>

      <ol class="verevon-space-canvas__history-list">
        <For each={versions()}>
          {(version) => (
            <li class="verevon-space-canvas__history-row">
              <p class="verevon-space-canvas__meta">
                {canvasVersionMeta(
                  version,
                  props.roster?.() ?? [],
                  formatWhenWithTime(version.createdAt, { locale: i18n.locale() }),
                  tr,
                )}
                {version.versionId === props.headVersionId ? ` · ${tr('nåværende', 'current')}` : ''}
              </p>
              <Show when={version.summary}>{(summary) => <p>{summary()}</p>}</Show>
              <Show when={canvasChangeNote(version, versionNoOf, tr)}>
                {(note) => <p class="verevon-space-canvas__meta">{note()}</p>}
              </Show>
              <div class="verevon-space-canvas__toolbar">
                <label class="verevon-space-canvas__pick">
                  <input type="checkbox" checked={isPicked(version)} onChange={() => togglePick(version)} />
                  {tr('Velg for sammenligning', 'Select to compare')}
                </label>
                <Show when={props.canRestore && historyWindow() === null && version.versionId !== props.headVersionId}>
                  <button
                    type="button"
                    class="verevon-space-secondary-action"
                    disabled={props.restoring}
                    onClick={() => confirmRestore(version)}
                  >
                    {tr('Gjenopprett', 'Restore')}
                  </button>
                </Show>
              </div>
            </li>
          )}
        </For>
      </ol>

      <Show when={nextCursor()}>
        <div class="verevon-space-canvas__toolbar">
          <Show when={moreFailed()}>
            <p class="verevon-space-projection-error" role="alert">
              {tr('Vi fikk ikke hentet flere versjoner.', 'We could not load more versions.')}
            </p>
          </Show>
          <button type="button" class="verevon-space-secondary-action" disabled={moreBusy()} onClick={() => void loadMore()}>
            {tr('Last flere', 'Load more')}
          </button>
        </div>
      </Show>

      <div class="verevon-space-canvas__compare" aria-live="polite">
        <Show when={picked().length < 2}>
          <p class="verevon-space-canvas__hint">
            {tr('Velg to versjoner for å se forskjellen.', 'Select two versions to see the difference.')}
          </p>
        </Show>
        <Show when={comparing()}>
          <p class="verevon-space-inline-status" role="status">
            {tr('Laster sammenligningen …', 'Loading the comparison…')}
          </p>
        </Show>
        <Show when={comparisonFailure()}>
          {(error) => <p class="verevon-space-canvas__error" role="alert">{canvasErrorMessage(error(), tr)}</p>}
        </Show>
        <Show when={comparison()}>
          {(pair) => (
            <>
              <p class="verevon-space-canvas__meta">
                {tr(
                  `Versjon ${pair().older.versionNo} til versjon ${pair().newer.versionNo}`,
                  `Version ${pair().older.versionNo} to version ${pair().newer.versionNo}`,
                )}
              </p>
              <SpaceCanvasDiff before={pair().older.content ?? ''} after={pair().newer.content ?? ''} />
            </>
          )}
        </Show>
      </div>
    </section>
  )
}
