import { FileText, Upload, X } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { ApiError } from '@/shared/api/http'
import {
  deleteSpaceFile,
  getSpaceFileUpload,
  listSpaceFiles,
  newUploadIdempotencyKey,
  readSpaceFileText,
  uploadSpaceFiles,
  type SpaceFile,
  type SpaceFilesGap,
  type SpaceFileText,
} from '@/shared/api/space-files-client'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import {
  canAddRoomFiles,
  fileExtension,
  formatBytes,
  roomFilesLimits,
  roomFileTextPreviewable,
  vetRoomFiles,
} from '../lib/room-files'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { useActiveSpaceTab } from './SpaceCockpit'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import './space-files.css'

/**
 * The room's Files tab (WS14-S9): the durable library of text-extractable files members kept in
 * this Space, with who added each one.
 *
 * # What a room file is
 *
 * The extracted TEXT of an uploaded document. There are no original bytes, so the tab offers a text
 * preview and no download, and says so. Only the person who added a file can remove it (the server
 * decides; the button is offered to the uploader only).
 *
 * # Lazy on purpose
 *
 * Panels mount eagerly (the cockpit hides, never unmounts), so the library is read only while this
 * tab is the active one and again each time it is re-entered.
 *
 * # States
 *
 * loading, empty, restricted (the reader may not read room files), disabled (the library is off for
 * this environment), zero data retention (nothing is kept), error with Retry, and a section the
 * gateway could not list stated beside the rest. A failed read is not an empty library.
 *
 * Presentation only: the gateway and the owning planes decide who may upload, read or delete.
 */
export interface SpaceFilesPanelProps {
  readonly spaceRef: string
  /** The reader's Control subject, to offer Remove on their own files. */
  readonly viewerSubjectId: () => string
  readonly role: () => string
  readonly archived?: () => boolean
  /** `limits.room_files` from the room context, when the gateway sent it. */
  readonly limits?: () => unknown
}

type PanelStatus = 'idle' | 'loading' | 'ready' | 'restricted' | 'zdr' | 'disabled' | 'error'

type UploadRun = {
  readonly uploadId: string
  readonly total: number
  readonly state: 'running' | 'done'
  readonly failures: readonly { readonly name: string; readonly message: string }[]
}

const FILES_TAB_ID = 'filer'
const POLL_MS = 2_000
const POLL_MAX = 90
const PREVIEW_CHARS = 100_000

export function SpaceFilesPanel(props: SpaceFilesPanelProps) {
  const i18n = useI18n()
  const active = useActiveSpaceTab()
  const isActive = createMemo(() => (active() as string) === FILES_TAB_ID)
  const limits = createMemo(() => roomFilesLimits(props.limits?.()))
  const mayAdd = createMemo(() => canAddRoomFiles(props.role()) && props.archived?.() !== true)

  const [status, setStatus] = createSignal<PanelStatus>('idle')
  const [files, setFiles] = createSignal<readonly SpaceFile[]>([])
  const [gaps, setGaps] = createSignal<readonly SpaceFilesGap[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [loadingMore, setLoadingMore] = createSignal(false)
  const [moreFailed, setMoreFailed] = createSignal(false)
  const [notice, setNotice] = createSignal<string | undefined>(undefined)
  const [uploading, setUploading] = createSignal(false)
  const [run, setRun] = createSignal<UploadRun | undefined>(undefined)
  const [dragging, setDragging] = createSignal(false)
  const [removing, setRemoving] = createSignal<string | undefined>(undefined)
  let callId = 0
  let pollToken = 0
  let fileInput: HTMLInputElement | undefined

  onCleanup(() => {
    pollToken += 1
  })

  const statusForError = (error: unknown): PanelStatus => {
    if (error instanceof ApiError) {
      if (error.status === 403) return 'restricted'
      if (error.status === 404 && error.code === 'room_files_disabled') return 'disabled'
      if (error.status === 412) return 'zdr'
    }
    return 'error'
  }

  const loadFirst = async (spaceRef: string) => {
    const id = ++callId
    setMoreFailed(false)
    // Keep a list already on screen while refreshing; only a cold start shows the loading state.
    if (files().length === 0) setStatus('loading')
    try {
      const page = await listSpaceFiles(spaceRef)
      if (id !== callId) return
      setFiles(page.files)
      setGaps(page.unavailable)
      setNextCursor(page.nextCursor)
      setStatus('ready')
    } catch (error) {
      if (id !== callId) return
      setFiles([])
      setGaps([])
      setNextCursor(null)
      setStatus(statusForError(error))
    }
  }

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
      const page = await listSpaceFiles(props.spaceRef, { cursor })
      if (id !== callId) return
      setFiles((current) => {
        const seen = new Set(current.map((file) => file.document_id))
        return [...current, ...page.files.filter((file) => !seen.has(file.document_id))]
      })
      setNextCursor(page.nextCursor)
    } catch {
      if (id === callId) setMoreFailed(true)
    } finally {
      setLoadingMore(false)
    }
  }

  const failureText = (error: unknown, fallbackNo: string, fallbackEn: string) =>
    translateApiError(error, i18n.tr, { no: fallbackNo, en: fallbackEn })

  // ---- upload -------------------------------------------------------------------------------

  const watchUpload = async (uploadId: string, total: number) => {
    const token = ++pollToken
    setRun({ uploadId, total, state: 'running', failures: [] })
    for (let attempt = 0; attempt < POLL_MAX; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, POLL_MS))
      if (token !== pollToken) return
      try {
        const progress = await getSpaceFileUpload(props.spaceRef, uploadId)
        if (token !== pollToken) return
        if (progress.status === 'queued' || progress.status === 'running') continue
        const failures = progress.items
          .filter((item) => item.errorCode)
          .map((item) => ({
            name: item.sourceName ?? '',
            message: translateApiError({ code: item.errorCode ?? '' }, i18n.tr, {
              no: 'Filen kunne ikke behandles.',
              en: 'The file could not be processed.',
            }),
          }))
        setRun({ uploadId, total, state: 'done', failures })
        void loadFirst(props.spaceRef)
        return
      } catch (error) {
        if (token !== pollToken) return
        setRun(undefined)
        setNotice(failureText(error, 'Vi fikk ikke hentet status for opplastingen.', 'We could not read the upload status.'))
        void loadFirst(props.spaceRef)
        return
      }
    }
    // Still running after the polling window: say so, and show whatever has landed.
    setRun(undefined)
    setNotice(
      i18n.tr(
        'Opplastingen tar lengre tid enn vanlig. Filene vises her når de er ferdige.',
        'The upload is taking longer than usual. Files show here when they are ready.',
      ),
    )
    void loadFirst(props.spaceRef)
  }

  const startUpload = async (incoming: readonly File[]) => {
    if (incoming.length === 0 || uploading() || !mayAdd()) return
    setNotice(undefined)
    const verdict = vetRoomFiles(incoming, limits(), i18n.tr)
    if (verdict.messages.length > 0) setNotice(verdict.messages.join(' '))
    if (verdict.accepted.length === 0) return
    setUploading(true)
    try {
      const accepted = await uploadSpaceFiles(props.spaceRef, verdict.accepted, newUploadIdempotencyKey())
      if (!accepted.uploadId) {
        void loadFirst(props.spaceRef)
        return
      }
      void watchUpload(accepted.uploadId, accepted.files.length || verdict.accepted.length)
    } catch (error) {
      setNotice(failureText(error, 'Filene ble ikke lagret i rommet. Ingenting er endret.', 'The files were not kept in the room. Nothing was changed.'))
    } finally {
      setUploading(false)
    }
  }

  const onPicked = (event: Event & { currentTarget: HTMLInputElement }) => {
    const picked = Array.from(event.currentTarget.files ?? [])
    event.currentTarget.value = ''
    void startUpload(picked)
  }

  const onDrop = (event: DragEvent) => {
    event.preventDefault()
    setDragging(false)
    void startUpload(Array.from(event.dataTransfer?.files ?? []))
  }

  // ---- preview ------------------------------------------------------------------------------

  type Preview =
    | { readonly state: 'loading'; readonly file: SpaceFile }
    | { readonly state: 'ready'; readonly file: SpaceFile; readonly text: SpaceFileText }
    | { readonly state: 'failed'; readonly file: SpaceFile; readonly message: string }
  const [preview, setPreview] = createSignal<Preview | undefined>(undefined)
  let previewId = 0
  let previewOpener: HTMLElement | null = null

  const openPreview = async (file: SpaceFile) => {
    const id = ++previewId
    previewOpener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    setPreview({ state: 'loading', file })
    try {
      const text = await readSpaceFileText(props.spaceRef, file.document_id, { maxChars: PREVIEW_CHARS })
      if (id !== previewId) return
      setPreview({ state: 'ready', file, text })
    } catch (error) {
      if (id !== previewId) return
      setPreview({
        state: 'failed',
        file,
        message: failureText(error, 'Vi fikk ikke åpnet filen. Prøv igjen.', 'We could not open that file. Try again.'),
      })
      // A file removed or hidden since the list was read: refresh so the list tells the truth.
      if (error instanceof ApiError && error.status === 404) void loadFirst(props.spaceRef)
    }
  }

  const closePreview = () => {
    previewId += 1
    setPreview(undefined)
    const opener = previewOpener
    previewOpener = null
    if (opener?.isConnected) queueMicrotask(() => opener.focus())
  }

  // ---- delete -------------------------------------------------------------------------------

  const remove = async (file: SpaceFile) => {
    if (removing()) return
    setRemoving(file.document_id)
    setNotice(undefined)
    try {
      await deleteSpaceFile(props.spaceRef, file.document_id)
      setFiles((current) => current.filter((entry) => entry.document_id !== file.document_id))
    } catch (error) {
      setNotice(failureText(error, 'Filen ble ikke fjernet. Den ligger der fortsatt.', 'The file was not removed. It is still there.'))
      if (error instanceof ApiError && error.status === 404) void loadFirst(props.spaceRef)
    } finally {
      setRemoving(undefined)
    }
  }

  const isMine = (file: SpaceFile) => {
    const me = props.viewerSubjectId()
    return Boolean(me) && file.uploaded_by === me
  }

  const gapText = (gap: SpaceFilesGap) =>
    gap.code === 'files_binding_unavailable'
      ? i18n.tr('Rommet er ikke koblet til et kunnskapsarkiv ennå, så filer kan ikke listes.', 'This room is not connected to a knowledge archive yet, so files cannot be listed.')
      : gap.code === 'files_upstream_unavailable'
        ? i18n.tr('Filene i rommet kunne ikke hentes akkurat nå.', 'This room’s files could not be loaded right now.')
        : gap.reason || i18n.tr('Deler av listen er utilgjengelig.', 'Part of the list is unavailable.')

  const retry = () => {
    void loadFirst(props.spaceRef)
  }

  return (
    <section class="verevon-space-view verevon-space-files" aria-labelledby="space-files-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Filer i rommet', 'Files in this Space')}</p>
          <h2 id="space-files-title">{i18n.tr('Filer', 'Files')}</h2>
          <p>{i18n.tr(
            'Tekstfiler medlemmer har lagt i rommet. Bare teksten lagres, ikke den opprinnelige filen, så det finnes ingen nedlasting.',
            'Text files members have added to this Space. Only the text is kept, never the original file, so there is no download.',
          )}</p>
        </div>
        <Show when={status() === 'ready' && mayAdd()}>
          <button
            type="button"
            class="verevon-space-secondary-action verevon-space-files__add"
            disabled={uploading()}
            onClick={() => fileInput?.click()}
          >
            <Upload size={14} aria-hidden="true" />
            {uploading() ? i18n.tr('Laster opp …', 'Uploading…') : i18n.tr('Legg til filer', 'Add files')}
          </button>
          <input
            ref={fileInput}
            type="file"
            multiple
            hidden
            accept={limits().allowedExtensions.map((extension) => `.${extension}`).join(',')}
            onChange={onPicked}
          />
        </Show>
      </div>

      <Show when={notice()}>
        {(text) => <p class="verevon-space-projection-error" role="alert">{text()}</p>}
      </Show>

      <Show when={run()}>
        {(current) => (
          <div class="verevon-space-files__run" role="status">
            <Show
              when={current().state === 'done'}
              fallback={<p>{i18n.tr(`Behandler ${current().total} fil(er) …`, `Processing ${current().total} file(s)…`)}</p>}
            >
              <Show
                when={current().failures.length > 0}
                fallback={<p>{i18n.tr('Filene er lagt til i rommet.', 'The files were added to the Space.')}</p>}
              >
                <p>{i18n.tr('Noen filer ble ikke lagt til:', 'Some files were not added:')}</p>
                <ul>
                  <For each={current().failures}>
                    {(failure) => <li><strong>{failure.name}</strong> {failure.message}</li>}
                  </For>
                </ul>
              </Show>
              <button type="button" class="verevon-space-secondary-action" onClick={() => setRun(undefined)}>
                {i18n.tr('Lukk', 'Dismiss')}
              </button>
            </Show>
          </div>
        )}
      </Show>

      <Show when={status() === 'idle' || status() === 'loading'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter filer i rommet …', 'Loading this Space’s files…')}
        </p>
      </Show>

      <Show when={status() === 'restricted'}>
        <div class="verevon-space-restricted" role="status">
          <p>{i18n.tr(
            'Du har ikke tilgang til å lese filene i dette rommet.',
            'You do not have access to read this Space’s files.',
          )}</p>
          <button type="button" class="verevon-space-secondary-action" onClick={retry}>
            {i18n.tr('Prøv igjen', 'Try again')}
          </button>
        </div>
      </Show>

      <Show when={status() === 'zdr'}>
        <div class="verevon-space-empty-state" role="status">
          <FileText size={20} aria-hidden="true" />
          <div>
            <h3>{i18n.tr('Ingen filer lagres', 'No files are kept')}</h3>
            <p>{i18n.tr(
              'Organisasjoner med null datalagring beholder ikke filer i rom. Ingenting er listet.',
              'Organizations with zero data retention do not keep room files. Nothing is listed.',
            )}</p>
          </div>
        </div>
      </Show>

      <Show when={status() === 'disabled'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Romfiler er ikke slått på for dette miljøet.', 'Room files are not turned on for this environment.')}
        </p>
      </Show>

      {/* A failed read is not an empty library. */}
      <Show when={status() === 'error'}>
        <div class="verevon-space-projection-error" role="alert">
          <p>{i18n.tr(
            'Vi fikk ikke hentet filene i rommet. Ingenting er endret.',
            'We could not load this Space’s files. Nothing was changed.',
          )}</p>
          <button type="button" class="verevon-space-secondary-action" onClick={retry}>
            {i18n.tr('Prøv igjen', 'Try again')}
          </button>
        </div>
      </Show>

      <Show when={status() === 'ready'}>
        <For each={gaps()}>
          {(gap) => <p class="verevon-space-projection-error" role="status">{gapText(gap)}</p>}
        </For>

        <Show when={mayAdd()}>
          <div
            class="verevon-space-files__drop"
            data-active={dragging() ? 'true' : 'false'}
            onDragOver={(event) => { event.preventDefault(); setDragging(true) }}
            onDragLeave={() => setDragging(false)}
            onDrop={onDrop}
          >
            {i18n.tr(
              `Slipp filer her (maks ${limits().maxFiles} om gangen, ${formatBytes(limits().maxFileBytes)} per fil). Tillatt: ${limits().allowedExtensions.join(', ')}.`,
              `Drop files here (up to ${limits().maxFiles} at a time, ${formatBytes(limits().maxFileBytes)} each). Allowed: ${limits().allowedExtensions.join(', ')}.`,
            )}
          </div>
        </Show>
        <Show when={!mayAdd()}>
          <p class="verevon-space-inline-status">
            {props.archived?.() === true
              ? i18n.tr('Rommet er arkivert, så filer kan ikke legges til.', 'The room is archived, so files cannot be added.')
              : i18n.tr('Rollen din i rommet tillater ikke å legge til filer.', 'Your role in this room does not allow adding files.')}
          </p>
        </Show>

        <Show
          when={files().length > 0}
          fallback={
            <Show when={gaps().length === 0}>
              <div class="verevon-space-empty-state" role="status">
                <FileText size={20} aria-hidden="true" />
                <div>
                  <h3>{i18n.tr('Ingen filer ennå', 'No files yet')}</h3>
                  <p>{i18n.tr(
                    'Filer et medlem legger til vises her for alle i rommet.',
                    'Files a member adds show here for everyone in the Space.',
                  )}</p>
                </div>
              </div>
            </Show>
          }
        >
          <ul class="verevon-space-files__list">
            <For each={files()}>
              {(file) => (
                <li class="verevon-space-files__row">
                  <button
                    type="button"
                    class="verevon-space-files__entry"
                    disabled={!roomFileTextPreviewable()}
                    aria-label={`${i18n.tr('Forhåndsvis', 'Preview')} ${file.file_name ?? ''}`}
                    onClick={() => {
                      void openPreview(file)
                    }}
                  >
                    <span class="verevon-space-files__icon" aria-hidden="true"><FileText size={16} /></span>
                    <span class="verevon-space-files__body">
                      <span class="verevon-space-files__title">{file.file_name ?? i18n.tr('Uten navn', 'Unnamed')}</span>
                      <span class="verevon-space-files__meta">
                        <Show when={file.extension ?? fileExtension(file.file_name ?? '')}>
                          {(extension) => <span>{extension().toUpperCase()}</span>}
                        </Show>
                        <Show when={formatBytes(file.size_bytes)}>{(size) => <span>{size()}</span>}</Show>
                        <Show when={file.text_chars !== null}>
                          <span>{i18n.tr(`${file.text_chars} tegn tekst`, `${file.text_chars} chars of text`)}</span>
                        </Show>
                        <Show when={isMine(file)}>
                          <span>{i18n.tr('Lagt til av deg', 'Added by you')}</span>
                        </Show>
                        <Show when={file.uploaded_at}>
                          {(when) => <span>{formatWhenWithTime(when(), { locale: i18n.locale() })}</span>}
                        </Show>
                      </span>
                    </span>
                  </button>
                  <Show when={isMine(file)}>
                    <SpaceConfirmButton
                      class="verevon-space-files__remove"
                      label={i18n.tr('Fjern', 'Remove')}
                      consequence={i18n.tr(
                        'Filen fjernes for alle i rommet og kan ikke gjenopprettes.',
                        'The file is removed for everyone in the Space and cannot be restored.',
                      )}
                      confirmLabel={`${i18n.tr('Bekreft fjerning av', 'Confirm removing')} ${file.file_name ?? ''}`}
                      disabled={removing() !== undefined}
                      onConfirm={() => {
                        void remove(file)
                      }}
                    />
                  </Show>
                </li>
              )}
            </For>
          </ul>

          <Show when={nextCursor()}>
            <div class="verevon-space-files__more">
              <Show when={moreFailed()}>
                <p class="verevon-space-projection-error" role="alert">
                  {i18n.tr('Vi fikk ikke hentet flere filer.', 'We could not load more files.')}
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

      <Show when={preview()}>
        {(current) => (
          <div
            class="verevon-space-files__sheet"
            role="presentation"
            onClick={(event) => { if (event.target === event.currentTarget) closePreview() }}
          >
            <section
              ref={(element) => queueMicrotask(() => element.focus())}
              class="verevon-space-files__dialog"
              role="dialog"
              aria-modal="true"
              aria-labelledby="space-files-preview-title"
              tabindex="-1"
              onKeyDown={(event) => {
                if (event.key === 'Escape' && !event.defaultPrevented) {
                  event.preventDefault()
                  closePreview()
                }
              }}
            >
              <header class="verevon-space-files__dialog-header">
                <h3 id="space-files-preview-title">{current().file.file_name ?? i18n.tr('Fil', 'File')}</h3>
                <button
                  type="button"
                  class="verevon-space-files__close"
                  aria-label={i18n.tr('Lukk', 'Close')}
                  onClick={closePreview}
                >
                  <X size={16} aria-hidden="true" />
                </button>
              </header>
              <div class="verevon-space-files__dialog-body">
                {(() => {
                  const value = current()
                  if (value.state === 'loading') {
                    return <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter teksten …', 'Loading the text…')}</p>
                  }
                  if (value.state === 'failed') {
                    return <p class="verevon-space-projection-error" role="alert">{value.message}</p>
                  }
                  return (
                    <>
                      <p class="verevon-space-files__note">{i18n.tr(
                        'Uttrukket tekst. Den opprinnelige filen er ikke lagret.',
                        'Extracted text. The original file is not kept.',
                      )}</p>
                      <Show when={value.text.truncated}>
                        <p class="verevon-space-files__note" role="status">{i18n.tr(
                          'Teksten er kuttet; bare starten vises.',
                          'The text is cut off; only the start is shown.',
                        )}</p>
                      </Show>
                      {/* Inert text: extracted file content is never drawn as markup. */}
                      <pre class="verevon-space-files__text">{value.text.text}</pre>
                    </>
                  )
                })()}
              </div>
            </section>
          </div>
        )}
      </Show>
    </section>
  )
}
