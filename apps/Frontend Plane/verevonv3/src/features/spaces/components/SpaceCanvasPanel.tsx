import { createEffect, createMemo, createSignal, onCleanup, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import type { ActionActor } from '@/shared/actions/types'
import { ApiError, clientError } from '@/shared/api/http'
import {
  getSpaceCanvas,
  type SpaceCanvas,
  type SpaceCanvasRead,
  type SpaceCanvasVersion,
  type SpaceCanvasWriteResult,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { canvasErrorMessage } from '@/shared/i18n/errors'

import { CANVAS_REFRESH_MS, CANVAS_TAB_ID, canvasVersionMeta } from '../lib/space-canvas'
import {
  CANVAS_MAX_BYTES,
  canvasDraftProblem,
  canvasTextBytes,
  headAfterSave,
  isAmbiguousSaveFailure,
  isStaleSaveRefusal,
  pendingSaveFor,
  saveSpaceCanvasVersion,
  type PendingSave,
} from '../lib/space-canvas-save'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { useActiveSpaceTab } from './SpaceCockpit'
import './space-canvas.css'

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
}

type PanelState =
  | { readonly kind: 'loading' }
  | { readonly kind: 'empty'; readonly canWrite: boolean }
  | { readonly kind: 'view'; readonly canvas: SpaceCanvas; readonly head: SpaceCanvasVersion; readonly canWrite: boolean }
  | { readonly kind: 'unavailable'; readonly error: unknown }

/** The latest version a conflict loaded, with its text; null when the Space has no canvas yet. */
type Latest = { readonly canvas: SpaceCanvas; readonly head: SpaceCanvasVersion } | null

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

type Notice = { readonly kind: 'saved'; readonly versionNo: number } | { readonly kind: 'unchanged' }

function headVersionIdOf(current: PanelState): string | null {
  return current.kind === 'view' ? current.head.versionId : null
}

export function SpaceCanvasPanel(props: SpaceCanvasPanelProps) {
  const i18n = useI18n()
  const tr = i18n.tr
  const active = useActiveSpaceTab()
  const isActive = createMemo(() => (active() as string) === CANVAS_TAB_ID)
  const [state, setState] = createSignal<PanelState>({ kind: 'loading' })
  const [editing, setEditing] = createSignal<EditSession | null>(null)
  const [notice, setNotice] = createSignal<Notice | null>(null)
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
      setEditing(null)
      setNotice(null)
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
              <Show when={view().canWrite && view().head.content !== undefined && actor() !== null}>
                <div class="verevon-space-canvas__toolbar">
                  <button
                    type="button"
                    class="verevon-space-secondary-action"
                    onClick={() => startEdit(view().head.versionId, view().head.content ?? '')}
                  >
                    {tr('Rediger', 'Edit')}
                  </button>
                </div>
              </Show>
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
                <div class="verevon-space-canvas__banner" role="alert">
                  <p class="verevon-space-canvas__banner-title">
                    {tr(
                      'Noen andre har lagret en nyere versjon. Teksten din er fortsatt her.',
                      'Someone else saved a newer version. Your text is still here.',
                    )}
                  </p>
                  <p>
                    {tr(
                      'Les den nyeste versjonen under. Lagre teksten din som en ny versjon over den, eller forkast teksten din.',
                      'Read the latest version below. Save your text as a new version over it, or discard your text.',
                    )}
                  </p>
                  <Show
                    when={conflict().latest}
                    fallback={
                      <p class="verevon-space-canvas__meta">
                        {tr('Lerretet har ingen versjon ennå.', 'The canvas has no version yet.')}
                      </p>
                    }
                  >
                    {(latest) => (
                      <>
                        <p class="verevon-space-canvas__meta">
                          {canvasVersionMeta(
                            latest().head,
                            props.roster?.() ?? [],
                            formatWhenWithTime(latest().head.createdAt, { locale: i18n.locale() }),
                            tr,
                          )}
                        </p>
                        <pre class="verevon-space-canvas__latest">{latest().head.content ?? ''}</pre>
                      </>
                    )}
                  </Show>
                  <div class="verevon-space-canvas__toolbar">
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
                  </div>
                </div>
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
                  {canvasErrorMessage(failure().error, tr)}
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
