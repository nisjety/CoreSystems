import { useBeforeLeave } from '@solidjs/router'
import { createEffect, createMemo, createSignal, onCleanup, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import type { ActionActor } from '@/shared/actions/types'
import { ApiError, clientError } from '@/shared/api/http'
import { registerUnsavedWork } from '@/shared/build/unsaved-work'
import {
  getSpaceCanvas,
  getSpaceCanvasVersion,
  type SpaceCanvas,
  type SpaceCanvasRead,
  type SpaceCanvasVersion,
  type SpaceCanvasWriteResult,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { canvasErrorMessage } from '@/shared/i18n/errors'

import {
  CANVAS_CLEAR_CONTENT,
  CANVAS_REFRESH_MS,
  CANVAS_TAB_ID,
  canvasLeavesRoom,
  canvasVersionMeta,
  sameCanvasText,
} from '../lib/space-canvas'
import {
  CANVAS_MAX_BYTES,
  canvasDraftProblem,
  canvasTextBytes,
  headAfterSave,
  isAmbiguousSaveFailure,
  isStaleSaveRefusal,
  pendingRestoreFor,
  pendingSaveFor,
  restoreSpaceCanvasVersion,
  saveSpaceCanvasVersion,
  type PendingRestore,
  type PendingSave,
} from '../lib/space-canvas-save'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { useActiveSpaceTab } from './SpaceCockpit'
import { SpaceCanvasDiff } from './SpaceCanvasDiff'
import { SpaceCanvasHistory } from './SpaceCanvasHistory'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import './space-canvas.css'

/** The typed 404 a save or a restore answers when the room's canvas is gone (D21n), never a head conflict. */
function isCanvasRemoved(error: unknown): boolean {
  return error instanceof ApiError && error.code === 'canvas_not_found'
}

/**
 * The room's Canvas tab (WS19): the one shared Markdown document of this Space. Read by everyone in the room;
 * a member who may write (the gateway says so with `can_write`) sees Edit, or Create canvas in a room with none.
 *
 * # Lazy on purpose
 *
 * Panels mount eagerly (the cockpit hides, it never unmounts), so the canvas is read only while this tab is the
 * active one. While it stays active and the document is visible, the head is checked every 15 seconds with
 * `known_version_id`, so an unchanged head moves no text.
 *
 * # States
 *
 * loading, empty (the room has no canvas yet), view (the head as Markdown, with who saved it and when) and
 * unavailable, which names the reason from the typed error and offers Try again. A failed read is never shown as an
 * empty canvas, and a refresh that fails for a moment keeps the text on screen.
 *
 * # Editing and saving (WS19-S6)
 *
 * An edit is a draft over the head it started from. A save is the `spaces.save_canvas` action, so it has the
 * registry's contract and the gateway's typed refusals. A refusal that means the head moved reloads the latest
 * version and shows the conflict banner: the draft stays, and "save my text as the new version" writes it over the
 * latest. A create that landed before its retry is recognised by its text and is never shown as a failure. A
 * refresh during an edit never replaces the draft; it shows that a newer version exists.
 *
 * # History, diff and restore (WS19-S7)
 *
 * The history (SpaceCanvasHistory) lists the versions, compares any two (SpaceCanvasDiff) and restores an earlier one
 * as the new head through `spaces.restore_canvas_version`, under its own client id prefix. A restore that meets a newer
 * head is read as a save is: the latest version is loaded and, unless it already holds the restored text, the conflict
 * banner shows the difference. The conflict banner's "Review differences" compares the latest version with the text the
 * write would store. Leaving the room with an edited draft asks first.
 *
 * Presentation only: the gateway and the owning planes decide who may read or write.
 */
export interface SpaceCanvasPanelProps {
  readonly spaceRef: string
  /** The room's members, to name who saved a version. Subject ids are never shown. */
  readonly roster?: () => readonly SpaceRosterMember[]
  /** The signed-in member's organization, the actor's org of a save. Without it and `actorId`, no save is offered. */
  readonly orgId?: string
  /** The signed-in member's subject id, the actor of a save. */
  readonly actorId?: string
  /**
   * D22aq: offer "Clear canvas" (a room manager or owner, `canOfferCanvasClear`). Presentation only; the clear is an
   * ordinary save of one newline, decided by Control and wiki-store like any other.
   */
  readonly canClear?: boolean
}

type PanelState =
  | { readonly kind: 'loading' }
  | { readonly kind: 'empty'; readonly canWrite: boolean }
  | { readonly kind: 'view'; readonly canvas: SpaceCanvas; readonly head: SpaceCanvasVersion; readonly canWrite: boolean }
  | { readonly kind: 'unavailable'; readonly error: unknown }

/** The latest version a conflict loaded, with its text; null when the Space has no canvas yet. */
type Latest = { readonly canvas: SpaceCanvas; readonly head: SpaceCanvasVersion } | null

/** A restore that met a newer head: the latest version, and the version the person chose, with its text. */
type RestoreConflict = { readonly latest: Latest; readonly target: SpaceCanvasVersion } | null

/** An open edit: the text it started from, the head the draft is written over, and what the last save said. */
type EditSession = {
  readonly startText: string
  readonly baseVersionId: string | null
  readonly draft: string
  readonly busy: boolean
  /** A newer version was refused or loaded for review: the draft is now written over that version. */
  readonly conflict: { readonly latest: Latest } | null
  /** A save or a load failed in a way the draft survives: the typed failure, shown above the form. */
  readonly failure: { readonly error: unknown } | null
}

type Notice =
  | { readonly kind: 'saved'; readonly versionNo: number }
  | { readonly kind: 'restored'; readonly fromVersionNo: number; readonly versionNo: number }
  | { readonly kind: 'unchanged' }

function headVersionIdOf(current: PanelState): string | null {
  return current.kind === 'view' ? current.head.versionId : null
}

type ConflictBannerProps = {
  readonly title: string
  readonly hint: string
  readonly latest: Latest
  readonly roster: () => readonly SpaceRosterMember[]
  /** The text the person wants the canvas to hold, compared with the latest version. */
  readonly after: () => string
  readonly actions: JSX.Element
}

/**
 * The banner of a write that met a newer head (WS19-S6 for a save, WS19-S7 for a restore): who saved the latest
 * version, and "Review differences" against the text the write would store, shown on demand.
 */
function CanvasConflictBanner(props: ConflictBannerProps) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [reviewing, setReviewing] = createSignal(false)
  return (
    <div class="verevon-space-canvas__banner" role="alert">
      <p class="verevon-space-canvas__banner-title">{props.title}</p>
      <p>{props.hint}</p>
      <Show
        when={props.latest}
        fallback={
          <p class="verevon-space-canvas__meta">{tr('Lerretet har ingen versjon ennå.', 'The canvas has no version yet.')}</p>
        }
      >
        {(latest) => (
          <>
            <p class="verevon-space-canvas__meta">
              {canvasVersionMeta(
                latest().head,
                props.roster(),
                formatWhenWithTime(latest().head.createdAt, { locale: i18n.locale() }),
                tr,
              )}
            </p>
            <div class="verevon-space-canvas__toolbar">
              <button
                type="button"
                class="verevon-space-secondary-action"
                aria-expanded={reviewing() ? 'true' : 'false'}
                onClick={() => {
                  setReviewing((open) => !open)
                }}
              >
                {reviewing() ? tr('Skjul endringene', 'Hide differences') : tr('Se over endringene', 'Review differences')}
              </button>
            </div>
            <Show when={reviewing()}>
              <SpaceCanvasDiff before={latest().head.content ?? ''} after={props.after()} />
            </Show>
          </>
        )}
      </Show>
      <div class="verevon-space-canvas__toolbar">{props.actions}</div>
    </div>
  )
}

export function SpaceCanvasPanel(props: SpaceCanvasPanelProps) {
  const i18n = useI18n()
  const tr = i18n.tr
  const active = useActiveSpaceTab()
  const isActive = createMemo(() => (active() as string) === CANVAS_TAB_ID)
  const [state, setState] = createSignal<PanelState>({ kind: 'loading' })
  const [editing, setEditing] = createSignal<EditSession | null>(null)
  const [notice, setNotice] = createSignal<Notice | null>(null)
  const [historyOpen, setHistoryOpen] = createSignal(false)
  const [restoring, setRestoring] = createSignal(false)
  const [clearing, setClearing] = createSignal(false)
  const [clearFailure, setClearFailure] = createSignal<{ readonly error: unknown; readonly stale: boolean } | null>(null)
  const [restoreFailure, setRestoreFailure] = createSignal<{ readonly error: unknown } | null>(null)
  const [restoreConflict, setRestoreConflict] = createSignal<RestoreConflict>(null)
  const viewState = createMemo(() => {
    const current = state()
    return current.kind === 'view' ? current : undefined
  })
  const emptyState = createMemo(() => {
    const current = state()
    return current.kind === 'empty' ? current : undefined
  })
  const unavailableState = createMemo(() => {
    const current = state()
    return current.kind === 'unavailable' ? current : undefined
  })
  const actor = (): ActionActor | null =>
    props.orgId && props.actorId ? { type: 'human', userId: props.actorId, orgId: props.orgId } : null
  // The gateway's word on this person's write right, once a read has given it. Until then the server decides each save.
  const writeAllowed = createMemo(() => {
    const current = state()
    return current.kind === 'view' || current.kind === 'empty' ? current.canWrite : true
  })
  const draftProblem = createMemo(() => {
    const session = editing()
    return session ? canvasDraftProblem(session.draft) : null
  })
  const draftHint = createMemo((): string | null => {
    switch (draftProblem()) {
      case 'empty':
        return tr('Skriv minst ett tegn for å lagre.', 'Write at least one character to save.')
      case 'too_large':
        return tr(
          'Teksten er større enn grensen på 64 KiB. Forkort den for å lagre.',
          'The text is larger than the 64 KiB limit. Shorten it to save.',
        )
      case 'nul':
        return tr('Teksten inneholder et tegn som ikke kan lagres.', 'The text contains a character that cannot be saved.')
      default:
        return null
    }
  })
  const draftBytes = createMemo(() => {
    const session = editing()
    return session ? canvasTextBytes(session.draft) : 0
  })
  const dirty = createMemo(() => {
    const session = editing()
    return session !== null && session.draft !== session.startText
  })
  // A head newer than the one the draft was written over, seen by a refresh: a notice, never a replaced draft.
  const newerVersion = createMemo(() => {
    const session = editing()
    const current = state()
    if (!session || session.conflict !== null || current.kind === 'loading' || current.kind === 'unavailable') {
      return false
    }
    return headVersionIdOf(current) !== session.baseVersionId
  })
  const canSave = createMemo(() => {
    const session = editing()
    return session !== null && !session.busy && actor() !== null && writeAllowed() && draftProblem() === null
  })

  // Each read takes a new call id; an answer that lands after a newer read (or after unmount) is dropped.
  let callId = 0
  // The room the text on screen belongs to. A page that moves to another room starts from loading, never from the
  // previous room's text, and never saves a draft that was written for the other room.
  let shownRoom = ''
  // The save attempt the open draft is sent under. A retry of the same draft over the same head keeps its id.
  let pendingSave: PendingSave | null = null
  // The restore attempt in flight. A retry of the same version over the same head keeps its id.
  let pendingRestore: PendingRestore | null = null

  onCleanup(() => {
    callId += 1
  })

  // Leaving the page with an edited draft asks first, as the artifact editor does.
  const guardUnload = (event: BeforeUnloadEvent) => {
    if (!dirty()) return
    event.preventDefault()
  }
  window.addEventListener('beforeunload', guardUnload)
  onCleanup(() => window.removeEventListener('beforeunload', guardUnload))

  // Unsaved-work probe (WS07-S3): an edited draft, and a save still in flight, are work a reload would drop. A draft that
  // matches the text it started from is not work, so an open editor with nothing changed does not hold a reload.
  onCleanup(registerUnsavedWork('space.canvas-editor', () => dirty() || editing()?.busy === true))

  // Leaving the room with an edited draft asks first: a move to another room, out of the Spaces, or back or forward.
  // A move within this room keeps the panel and its draft mounted, so it is not asked about. Declining keeps the page.
  useBeforeLeave((event) => {
    if (event.defaultPrevented || !dirty() || !canvasLeavesRoom(event.to, props.spaceRef)) return
    const leave = window.confirm(
      tr(
        'Teksten din i lerretet er ikke lagret. Forlate rommet og forkaste den?',
        'Your canvas text is not saved. Leave the room and discard it?',
      ),
    )
    if (!leave) event.preventDefault()
  })

  const applyRead = (answer: SpaceCanvasRead) => {
    if (answer.unchanged) return
    if (answer.canvas === null) setState({ kind: 'empty', canWrite: answer.canWrite })
    else if (answer.head !== null) {
      setState({ kind: 'view', canvas: answer.canvas, head: answer.head, canWrite: answer.canWrite })
    } else setState({ kind: 'unavailable', error: clientError(503, 'canvas_unavailable') })
  }

  // One read. A refresh (`quiet`) sends the head on screen, keeps it unless the head moved, and keeps it when the
  // read fails for a moment. A cold read, or a refused one, is what the person needs to see.
  const read = async (spaceRef: string, quiet: boolean) => {
    const id = ++callId
    if (spaceRef !== shownRoom) {
      shownRoom = spaceRef
      pendingSave = null
      pendingRestore = null
      setEditing(null)
      setNotice(null)
      setRestoreConflict(null)
      setRestoreFailure(null)
      setHistoryOpen(false)
      setState({ kind: 'loading' })
    } else if (!quiet && state().kind !== 'view') {
      setState({ kind: 'loading' })
    }
    try {
      const answer = await getSpaceCanvas(spaceRef, quiet ? headVersionIdOf(state()) : null)
      if (id === callId) applyRead(answer)
    } catch (error) {
      if (id !== callId) return
      const refused = error instanceof ApiError && error.status >= 400 && error.status < 500
      if (quiet && !refused) return
      setState({ kind: 'unavailable', error })
    }
  }

  // The head as the server holds it now, read after a save so the view carries the authoritative version.
  const refreshAfterSave = async (spaceRef: string) => {
    const id = ++callId
    try {
      const answer = await getSpaceCanvas(spaceRef)
      if (id === callId && spaceRef === props.spaceRef) applyRead(answer)
    } catch {
      // The view the save left on screen stays; the next refresh reads the head again.
    }
  }

  createEffect(
    () => (isActive() ? props.spaceRef : ''),
    (spaceRef) => {
      if (!spaceRef) return undefined
      void read(spaceRef, false)
      const refresh = () => {
        if (document.visibilityState === 'visible') void read(spaceRef, true)
      }
      const timer = setInterval(refresh, CANVAS_REFRESH_MS)
      document.addEventListener('visibilitychange', refresh)
      return () => {
        clearInterval(timer)
        document.removeEventListener('visibilitychange', refresh)
      }
    },
  )

  const startEdit = (baseVersionId: string | null, startText: string) => {
    pendingSave = null
    setNotice(null)
    setEditing({ startText, baseVersionId, draft: startText, busy: false, conflict: null, failure: null })
  }

  const setDraft = (draft: string) => {
    setEditing((current) => (current ? { ...current, draft } : current))
  }

  const cancelEdit = () => {
    const session = editing()
    if (!session || session.busy) return
    if (dirty() && !window.confirm(tr('Forkaste teksten din?', 'Discard your text?'))) return
    pendingSave = null
    setEditing(null)
  }

  // "Load latest and discard my text": the draft goes, and the page reads the head again.
  const discardDraft = () => {
    const session = editing()
    if (!session || session.busy) return
    pendingSave = null
    setEditing(null)
    void read(props.spaceRef, false)
  }

  const noticeText = (current: Notice): string =>
    current.kind === 'saved'
      ? tr(`Lagret som versjon ${current.versionNo}.`, `Saved as version ${current.versionNo}.`)
      : current.kind === 'restored'
        ? tr(
            `Versjon ${current.fromVersionNo} er gjenopprettet som versjon ${current.versionNo}.`,
            `Restored version ${current.fromVersionNo} as version ${current.versionNo}.`,
          )
        : tr(
            'Lerretet har allerede denne teksten, så ingen ny versjon ble lagret.',
            'The canvas already has this text, so no new version was stored.',
          )

  // A save landed: the draft closes, the view shows the version it made, and the head is read again.
  const settleSaved = (spaceRef: string, result: SpaceCanvasWriteResult, content: string) => {
    setEditing(null)
    setNotice(result.unchanged ? { kind: 'unchanged' } : { kind: 'saved', versionNo: result.version.versionNo })
    const shown = headAfterSave(result, content, spaceRef, viewState()?.canvas ?? null)
    if (shown) setState({ kind: 'view', canvas: shown.canvas, head: shown.head, canWrite: true })
    void refreshAfterSave(spaceRef)
  }

  // Reads the latest version and makes it the base of the open draft. When the latest already holds the text that was
  // saved, the save landed and nothing is saved again; otherwise the conflict banner shows the latest version.
  const loadLatest = async (spaceRef: string, savedText: string) => {
    const id = ++callId
    setEditing((current) => (current ? { ...current, busy: true, failure: null } : current))
    try {
      const answer = await getSpaceCanvas(spaceRef)
      if (answer.unchanged) {
        setEditing((current) => (current ? { ...current, busy: false } : current))
        return
      }
      if (id === callId) applyRead(answer)
      if (spaceRef !== props.spaceRef) return
      const latest: Latest =
        answer.canvas !== null && answer.head !== null ? { canvas: answer.canvas, head: answer.head } : null
      if (latest !== null && latest.head.content === savedText) {
        pendingSave = null
        setEditing(null)
        setNotice({ kind: 'saved', versionNo: latest.head.versionNo })
        return
      }
      setEditing((current) =>
        current
          ? {
              ...current,
              baseVersionId: latest?.head.versionId ?? null,
              busy: false,
              failure: null,
              conflict: { latest },
            }
          : current,
      )
    } catch (error) {
      if (spaceRef !== props.spaceRef) return
      setEditing((current) => (current ? { ...current, busy: false, failure: { error } } : current))
    }
  }

  // Sends the draft as a save over the head it was written over. A refusal that means the head moved reloads the
  // latest version; any other failure keeps the draft, and a retry after a dropped connection reuses the save id.
  const saveDraft = async () => {
    const session = editing()
    const who = actor()
    if (!session || !who || session.busy || !writeAllowed() || canvasDraftProblem(session.draft) !== null) return
    const spaceRef = props.spaceRef
    const content = session.draft
    const base = session.baseVersionId
    const attempt = pendingSaveFor(pendingSave, base, content)
    pendingSave = attempt
    setEditing({ ...session, busy: true, failure: null })
    try {
      const result = await saveSpaceCanvasVersion(spaceRef, who, {
        expectedVersionId: base,
        content,
        clientSaveId: attempt.clientSaveId,
      })
      pendingSave = null
      if (spaceRef === props.spaceRef) settleSaved(spaceRef, result, content)
    } catch (error) {
      if (isStaleSaveRefusal(error)) {
        pendingSave = null
        await loadLatest(spaceRef, content)
        return
      }
      // D21n: the canvas was removed under the draft. That is not a conflict: the draft stays, it is no longer written
      // over any version, and the next save creates the canvas with it.
      if (isCanvasRemoved(error)) {
        pendingSave = null
        if (spaceRef !== props.spaceRef) return
        setEditing((current) =>
          current ? { ...current, busy: false, baseVersionId: null, conflict: null, failure: { error } } : current,
        )
        return
      }
      if (!isAmbiguousSaveFailure(error)) pendingSave = null
      if (spaceRef !== props.spaceRef) return
      setEditing((current) => (current ? { ...current, busy: false, failure: { error } } : current))
    }
  }

  const showLatest = () => {
    const session = editing()
    if (!session || session.busy) return
    void loadLatest(props.spaceRef, session.draft)
  }

  // A restore that landed: the notice names both versions, and the head is read again so the view carries its text.
  const settleRestored = (spaceRef: string, result: SpaceCanvasWriteResult, target: SpaceCanvasVersion) => {
    setRestoreConflict(null)
    setNotice(
      result.unchanged
        ? { kind: 'unchanged' }
        : { kind: 'restored', fromVersionNo: target.versionNo, versionNo: result.version.versionNo },
    )
    void refreshAfterSave(spaceRef)
  }

  // The latest version after a restore met a newer head. A latest version that already holds the restored text
  // settles the restore as unchanged; any other goes to the banner, with the text the person chose for the difference.
  const loadLatestForRestore = async (spaceRef: string, target: SpaceCanvasVersion) => {
    const id = ++callId
    try {
      const answer = await getSpaceCanvas(spaceRef)
      if (answer.unchanged) return
      if (id === callId) applyRead(answer)
      if (spaceRef !== props.spaceRef) return
      const latest: Latest =
        answer.canvas !== null && answer.head !== null ? { canvas: answer.canvas, head: answer.head } : null
      if (latest !== null && sameCanvasText(latest.head, target)) {
        setNotice({ kind: 'unchanged' })
        return
      }
      const chosen = await getSpaceCanvasVersion(spaceRef, target.versionId)
      if (spaceRef === props.spaceRef) setRestoreConflict({ latest, target: chosen })
    } catch (error) {
      if (spaceRef === props.spaceRef) setRestoreFailure({ error })
    }
  }

  // Makes an earlier version the new head through `spaces.restore_canvas_version`, under a client id of its own. A
  // refusal that means the head moved is read as a save's is: the latest version is loaded, and the restore is decided
  // there. A restore is never sent through the save action.
  const restoreVersion = async (target: SpaceCanvasVersion, expectedVersionId: string | null) => {
    const who = actor()
    if (!who || !writeAllowed() || restoring()) return
    const spaceRef = props.spaceRef
    const attempt = pendingRestoreFor(pendingRestore, target.versionId, expectedVersionId)
    pendingRestore = attempt
    setRestoring(true)
    setRestoreFailure(null)
    setRestoreConflict(null)
    try {
      const result = await restoreSpaceCanvasVersion(spaceRef, who, {
        versionId: target.versionId,
        expectedVersionId,
        clientSaveId: attempt.clientSaveId,
      })
      pendingRestore = null
      if (spaceRef === props.spaceRef) settleRestored(spaceRef, result, target)
    } catch (error) {
      if (isStaleSaveRefusal(error)) {
        pendingRestore = null
        await loadLatestForRestore(spaceRef, target)
      } else if (isCanvasRemoved(error)) {
        // D21n: there is no canvas to restore into any more; the panel shows the room as it now is.
        pendingRestore = null
        if (spaceRef === props.spaceRef) {
          setHistoryOpen(false)
          void read(spaceRef, false)
        }
      } else {
        if (!isAmbiguousSaveFailure(error)) pendingRestore = null
        if (spaceRef === props.spaceRef) setRestoreFailure({ error })
      }
    } finally {
      setRestoring(false)
    }
  }

  // D22aq: clear the canvas by saving one newline over the head on screen, through the same save action as an edit. A
  // newer head refuses it (nothing is cleared) and the view is read again, so the manager decides on what is there now.
  const clearCanvas = async () => {
    const view = viewState()
    const who = actor()
    if (!view || !who || clearing() || restoring() || !writeAllowed()) return
    const spaceRef = props.spaceRef
    setClearing(true)
    setClearFailure(null)
    try {
      const result = await saveSpaceCanvasVersion(spaceRef, who, {
        expectedVersionId: view.head.versionId,
        content: CANVAS_CLEAR_CONTENT,
        clientSaveId: crypto.randomUUID(),
      })
      if (spaceRef === props.spaceRef) settleSaved(spaceRef, result, CANVAS_CLEAR_CONTENT)
    } catch (error) {
      if (spaceRef !== props.spaceRef) return
      const stale = isStaleSaveRefusal(error)
      setClearFailure({ error, stale })
      if (stale || isCanvasRemoved(error)) void read(spaceRef, false)
    } finally {
      setClearing(false)
    }
  }

  return (
    <section class="verevon-space-view verevon-space-canvas" aria-labelledby="space-canvas-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{tr('Rommets delte dokument', "The room's shared document")}</p>
          <h2 id="space-canvas-title">{tr('Lerret', 'Canvas')}</h2>
          <p>
            {tr(
              'Ett delt dokument for rommet. Medlemmer og agenter skriver det sammen, og hver lagring blir en ny versjon.',
              'One shared document for the room. Members and agents write it together, and each save becomes a new version.',
            )}
          </p>
        </div>
      </div>

      <Show when={notice()}>
        {(current) => <p class="verevon-space-canvas__notice" role="status">{noticeText(current())}</p>}
      </Show>

      <Show when={!editing()}>
        <Show when={state().kind === 'loading'}>
          <p class="verevon-space-inline-status" role="status">
            {tr('Laster lerretet …', 'Loading the canvas…')}
          </p>
        </Show>

        <Show when={emptyState()}>
          {(empty) => (
            <div class="verevon-space-empty-state" role="status">
              <p>{tr('Rommet har ikke et lerret ennå.', 'This room has no canvas yet.')}</p>
              <Show when={empty().canWrite && actor() !== null}>
                <div class="verevon-space-canvas__toolbar">
                  <button type="button" class="verevon-space-primary-action" onClick={() => startEdit(null, '')}>
                    {tr('Opprett lerret', 'Create canvas')}
                  </button>
                </div>
              </Show>
            </div>
          )}
        </Show>

        <Show when={viewState()}>
          {(view) => (
            <>
              <div class="verevon-space-canvas__toolbar">
                <Show when={view().canWrite && view().head.content !== undefined && actor() !== null}>
                  <button
                    type="button"
                    class="verevon-space-secondary-action"
                    onClick={() => startEdit(view().head.versionId, view().head.content ?? '')}
                  >
                    {tr('Rediger', 'Edit')}
                  </button>
                </Show>
                <Show
                  when={
                    props.canClear === true &&
                    view().canWrite &&
                    actor() !== null &&
                    view().head.content !== undefined &&
                    view().head.content !== CANVAS_CLEAR_CONTENT
                  }
                >
                  <SpaceConfirmButton
                    label={clearing() ? tr('Tømmer …', 'Clearing…') : tr('Tøm lerretet', 'Clear canvas')}
                    consequence={tr(
                      'Lerretet blir tomt for alle i rommet. Teksten blir værende i historikken og kan gjenopprettes.',
                      'The canvas becomes empty for everyone in the room. The text stays in the history and can be restored.',
                    )}
                    confirmLabel={tr('Bekreft at lerretet tømmes', 'Confirm clearing the canvas')}
                    disabled={clearing() || restoring()}
                    onConfirm={() => void clearCanvas()}
                  />
                </Show>
                <button
                  type="button"
                  class="verevon-space-secondary-action"
                  aria-expanded={historyOpen() ? 'true' : 'false'}
                  onClick={() => {
                    setHistoryOpen((open) => !open)
                  }}
                >
                  {historyOpen() ? tr('Skjul historikken', 'Hide history') : tr('Historikk', 'History')}
                </button>
              </div>
              <p class="verevon-space-canvas__meta">
                {canvasVersionMeta(
                  view().head,
                  props.roster?.() ?? [],
                  formatWhenWithTime(view().head.createdAt, { locale: i18n.locale() }),
                  tr,
                )}
              </p>
              <div class="verevon-space-canvas__body">
                <ChatMarkdown content={view().head.content ?? ''} />
              </div>
              <Show when={restoreConflict()}>
                {(conflict) => (
                  <CanvasConflictBanner
                    title={tr(
                      'Noen andre har lagret en nyere versjon. Gjenopprettingen ble ikke lagret.',
                      'Someone else saved a newer version. The restore was not saved.',
                    )}
                    hint={tr(
                      'Se over endringene under. Gjenopprett versjonen over den nyeste, eller lukk meldingen.',
                      'Review the differences below. Restore the version over the latest, or dismiss this message.',
                    )}
                    latest={conflict().latest}
                    roster={() => props.roster?.() ?? []}
                    after={() => conflict().target.content ?? ''}
                    actions={
                      <>
                        <button
                          type="button"
                          class="verevon-space-primary-action"
                          disabled={restoring() || conflict().latest === null}
                          onClick={() =>
                            void restoreVersion(conflict().target, conflict().latest?.head.versionId ?? null)
                          }
                        >
                          {tr(
                            `Gjenopprett versjon ${conflict().target.versionNo} over den nyeste`,
                            `Restore version ${conflict().target.versionNo} over the latest`,
                          )}
                        </button>
                        <button
                          type="button"
                          class="verevon-space-secondary-action"
                          onClick={() => setRestoreConflict(null)}
                        >
                          {tr('Lukk', 'Dismiss')}
                        </button>
                      </>
                    }
                  />
                )}
              </Show>
              <Show when={clearFailure()}>
                {(failure) => (
                  <p class="verevon-space-canvas__error" role="alert">
                    {failure().stale
                      ? tr(
                          'Noen andre har lagret en nyere versjon. Lerretet ble ikke tømt; se over den nyeste og prøv igjen.',
                          'Someone else saved a newer version. The canvas was not cleared; review the latest and try again.',
                        )
                      : canvasErrorMessage(failure().error, tr)}
                  </p>
                )}
              </Show>
              <Show when={restoreFailure()}>
                {(failure) => (
                  <p class="verevon-space-canvas__error" role="alert">
                    {canvasErrorMessage(failure().error, tr)}
                  </p>
                )}
              </Show>
              <Show when={restoring()}>
                <p class="verevon-space-canvas__meta" role="status">
                  {tr('Gjenoppretter ...', 'Restoring ...')}
                </p>
              </Show>
              <Show when={historyOpen()}>
                <SpaceCanvasHistory
                  spaceRef={props.spaceRef}
                  roster={props.roster}
                  headVersionId={view().head.versionId}
                  canRestore={view().canWrite && actor() !== null}
                  restoring={restoring()}
                  onRestore={(version) => void restoreVersion(version, view().head.versionId)}
                  onClose={() => setHistoryOpen(false)}
                />
              </Show>
            </>
          )}
        </Show>
      </Show>

      <Show when={editing()}>
        {(session) => (
          <form
            class="verevon-space-canvas__editor"
            aria-labelledby="space-canvas-title"
            onSubmit={(event) => {
              event.preventDefault()
              void saveDraft()
            }}
          >
            <Show when={session().conflict}>
              {(conflict) => (
                <CanvasConflictBanner
                  title={tr(
                    'Noen andre har lagret en nyere versjon. Teksten din er fortsatt her.',
                    'Someone else saved a newer version. Your text is still here.',
                  )}
                  hint={tr(
                    'Se over endringene under. Lagre teksten din som en ny versjon over den, eller forkast teksten din.',
                    'Review the differences below. Save your text as a new version over it, or discard your text.',
                  )}
                  latest={conflict().latest}
                  roster={() => props.roster?.() ?? []}
                  after={() => session().draft}
                  actions={
                    <>
                      <button
                        type="button"
                        class="verevon-space-primary-action"
                        disabled={!canSave()}
                        onClick={() => void saveDraft()}
                      >
                        {tr('Lagre min tekst som ny versjon', 'Save my text as the new version')}
                      </button>
                      <button
                        type="button"
                        class="verevon-space-secondary-action"
                        disabled={session().busy}
                        onClick={discardDraft}
                      >
                        {tr('Last nyeste og forkast min tekst', 'Load latest and discard my text')}
                      </button>
                    </>
                  }
                />
              )}
            </Show>

            <Show when={newerVersion()}>
              <div class="verevon-space-canvas__notice" role="status">
                <p>
                  {tr(
                    'En nyere versjon er lagret mens du redigerer.',
                    'A newer version was saved while you were editing.',
                  )}
                </p>
                <button
                  type="button"
                  class="verevon-space-secondary-action"
                  disabled={session().busy}
                  onClick={showLatest}
                >
                  {tr('Vis nyeste', 'Show latest')}
                </button>
              </div>
            </Show>

            <label class="verevon-space-canvas__label" for="space-canvas-draft">
              {tr('Tekst (Markdown)', 'Text (Markdown)')}
            </label>
            <textarea
              id="space-canvas-draft"
              class="verevon-space-canvas__input"
              rows={14}
              value={session().draft}
              disabled={session().busy}
              aria-describedby="space-canvas-counter"
              onInput={(event) => setDraft(event.currentTarget.value)}
            />
            <p id="space-canvas-counter" class="verevon-space-canvas__counter">
              {`${draftBytes()} / ${CANVAS_MAX_BYTES} ${tr('byte', 'bytes')}`}
            </p>
            <Show when={draftHint()}>{(hint) => <p class="verevon-space-canvas__hint">{hint()}</p>}</Show>
            <Show when={!writeAllowed()}>
              <p class="verevon-space-canvas__hint" role="alert">
                {canvasErrorMessage(clientError(403, 'canvas_write_not_authorized'), tr)}
              </p>
            </Show>
            <Show when={session().failure}>
              {(failure) => (
                <p class="verevon-space-canvas__error" role="alert">
                  {isCanvasRemoved(failure().error)
                    ? tr(
                        'Lerretet ble fjernet mens du redigerte. Teksten din er fortsatt her, og Lagre oppretter et nytt lerret med den.',
                        'The canvas was removed while you were editing. Your text is still here, and Save creates a new canvas with it.',
                      )
                    : canvasErrorMessage(failure().error, tr)}
                </p>
              )}
            </Show>

            <Show when={session().conflict === null}>
              <div class="verevon-space-canvas__toolbar">
                <button type="submit" class="verevon-space-primary-action" disabled={!canSave()}>
                  {session().busy ? tr('Lagrer ...', 'Saving ...') : tr('Lagre', 'Save')}
                </button>
                <button
                  type="button"
                  class="verevon-space-secondary-action"
                  disabled={session().busy}
                  onClick={cancelEdit}
                >
                  {tr('Avbryt', 'Cancel')}
                </button>
              </div>
            </Show>
          </form>
        )}
      </Show>

      <Show when={unavailableState()}>
        {(unavailable) => (
          <div class="verevon-space-projection-error" role="alert">
            <p>{canvasErrorMessage(unavailable().error, tr)}</p>
            <button
              type="button"
              class="verevon-space-secondary-action"
              onClick={() => void read(props.spaceRef, false)}
            >
              {tr('Prøv igjen', 'Try again')}
            </button>
          </div>
        )}
      </Show>
    </section>
  )
}
