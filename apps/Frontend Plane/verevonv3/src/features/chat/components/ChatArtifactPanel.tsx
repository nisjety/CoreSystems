/**
 * The "Artefakter" tab: a real artifact/canvas renderer.
 *
 * Left: every artifact in the conversation (title, kind badge, revision count).
 * Right: the selected artifact rendered by kind — code with line numbers and
 * basic highlighting, Markdown documents through the existing chat Markdown
 * renderer, untrusted HTML in a sandboxed iframe, images, and generated binaries
 * as a download card. A `‹ v2/3 ›` stepper walks the revision history of a
 * repeatedly re-emitted artifact and always opens on the newest revision.
 *
 * With the server's revision history on (EP-3 S3, `artifact_history` in the
 * gateway's composer capabilities), a chat thread's panel also reads every
 * stored revision (action `chat.list_artifact_revisions`), so the stepper
 * survives a reload and another device and names who made each revision. A
 * room and a temporary thread never ask.
 *
 * With the BFF's promote switch on (EP-3 S8, `artifact_promote`), a document
 * artifact in a chat thread can be saved to knowledge (action
 * `chat.save_artifact_to_knowledge`): the revision on screen becomes an
 * independent Data Plane document that stays when the chat is deleted.
 */
import {
  AlertCircle,
  BookOpen,
  Check,
  ChevronLeft,
  ChevronRight,
  Code2,
  Copy,
  Download,
  Eye,
  GitCompare,
  FileCode2,
  FileSpreadsheet,
  FileText,
  Globe,
  Image as ImageIcon,
  Pencil,
  Presentation,
  RotateCcw,
  ScanSearch,
} from '@/shared/icons'
import { Dynamic } from '@solidjs/web'
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from 'solid-js'
import {
  artifactConflictHeadVersion,
  restoreArtifactRevision,
  type SavedArtifactRevision,
} from '@/shared/api/chat-client'
import { deckManifestRenderer } from './ArtifactDeckView'
import { ArtifactEditor } from './ArtifactEditor'
import {
  type ArtifactSelectionRange,
  type ArtifactSelectionRequest,
  quoteSha256,
  SOURCE_VIEW_ATTRIBUTE,
  sourceSelectionRange,
} from './ArtifactSelection'
import { extraArtifactRenderers } from './artifact-renderers'
import { officePdfRenderer } from './ArtifactOfficePreview'
import { ArtifactProvenance } from './ArtifactProvenance'
import {
  type ReviewLineRange,
  type ReviewRunFailureKind,
  ReviewRunCard,
  ReviewRunFailure,
  reviewRunEnabled,
  reviewRunFailureKind,
} from './ReviewRunCard'
import {
  ChatMarkdown,
  DiffView,
  MessageMenu,
  type MessageMenuItem,
} from './ChatMessages'
import { exportChatAnswer, type ExportFormat } from '../lib/chat-export'
import { diffText } from '@/shared/chat-nodes'
import {
  EmptyPanel,
} from './ChatPanels'
import {
  type ArtifactRenderKind,
  artifactContentMissing,
  artifactFileMeta,
  artifactLanguage,
  artifactRenderKind,
  artifactRenderKindLabel,
  artifactVersionAt,
  artifactVersions,
  imageArtifactSrc,
  isCopyableRenderKind,
  mergeArtifactVersion,
  mergeServerRevisions,
} from './chat-artifacts'
import {
  highlightCode,
} from './chat-code-highlight'
import {
  buildArtifactImageSpecs,
  formatBytes,
  formatRelative,
  generatedImageTitle,
} from './chat-media-markdown'
import {
  type ArtifactPanelItem,
  type ChatArtifactVersion,
  type IconComponent,
} from './chat-types'
import {
  type ArtifactRevision,
  getComposerCapabilities,
  listArtifactRevisions,
  runReview,
  type ReviewCard,
  uploadChatDocument,
} from '@/shared/api/chat-client'
import { REVIEW_LABEL_MAX_CHARS } from '@/shared/actions/chat-review'
import { sandboxHtmlDocument } from '@/shared/lib/sandbox-html'
import { useI18n } from '@/shared/i18n'
import { type ViewerSurface, viewerActivationKey, viewerFor } from '@/shared/artifacts/viewer-registry'

/**
 * Room HTML previews the reader has explicitly started, for this page session
 * only (memory, never storage). Keyed on id + version + content hash, so consent
 * covers exactly the markup that was shown.
 */
const [activatedPreviews, setActivatedPreviews] = createSignal<ReadonlySet<string>>(new Set())

extraArtifactRenderers.register(officePdfRenderer)
extraArtifactRenderers.register(deckManifestRenderer)

const KIND_ICONS: Record<ArtifactRenderKind, IconComponent> = {
  binary: FileSpreadsheet,
  code: Code2,
  document: FileText,
  html: Globe,
  image: ImageIcon,
  // A previewable PDF reads as a document, not as an opaque file.
  pdf: FileText,
  text: FileCode2,
}

/** The thread whose stored history the panel may read; only a chat thread passes one. */
export type ArtifactHistorySource = {
  threadId: string
  /** A temporary (zero-retention) thread keeps no history, so the panel never asks for it. */
  temporary: boolean
}

/** Kinds the server stores revisions of; every other kind keeps its streamed history only. */
const STORED_REVISION_KINDS = new Set(['document', 'code', 'html'])

/** The kinds the person may edit as source text (EP-3 S4); the server refuses every other kind. */
const EDITABLE_ARTIFACT_KINDS: ReadonlySet<string> = new Set(['document', 'code', 'html'])

/**
 * What the panel needs to offer Edit and Restore (EP-3 S4). `enabled` is the gateway's `artifact_editing`
 * capability; a temporary or Space thread never edits, whatever the capability says. Absent, the panel is the
 * read-only viewer it always was.
 */
export type ArtifactEditing = {
  enabled: boolean
  threadId: string
  temporary: boolean
  spaceThread: boolean
  /** Told about every revision the person saved or restored, so the conversation's artifact state can fold it in. */
  onSaved?: (revision: SavedArtifactRevision) => void
}

/** A selection the person made in a source view and has not sent yet, with the revision it was made on (EP-3 S6). */
type PendingSelection = ArtifactSelectionRange & { version: number }

/** The selected text as the popover shows it: its first 80 characters, the rest cut. */
function excerpt(text: string): string {
  return text.length > 80 ? `${text.slice(0, 80)}...` : text
}

/**
 * The label a review card carries: the title, then the version it read. The gateway takes at most 120 characters, so the
 * title is cut to leave room for the version, which always stays (F-t3-086).
 */
function reviewRunLabel(title: string, version: number): string {
  const suffix = ` v${version}`
  const room = REVIEW_LABEL_MAX_CHARS - Array.from(suffix).length
  return `${Array.from(title.trim()).slice(0, room).join('')}${suffix}`
}

export function ArtifactsPanel(props: {
  items: ArtifactPanelItem[]
  surface?: ViewerSurface
  history?: ArtifactHistorySource
  editing?: ArtifactEditing
  /** Sends one selection edit as a normal turn (EP-3 S6). Absent, the panel offers no "Ask for a change". */
  onAskAboutSelection?: (request: ArtifactSelectionRequest) => Promise<unknown> | void
}) {
  const i18n = useI18n()
  const [requestedId, setRequestedId] = createSignal<string | null>(null)
  /** The thread to read stored history for, once the gateway says its history is on. */
  const [historyThreadId, setHistoryThreadId] = createSignal<string | null>(null)
  /** The thread a document revision may be saved to knowledge from, once the BFF says promote is on. */
  const [promoteThreadId, setPromoteThreadId] = createSignal<string | null>(null)
  // While the editor is open the other artifacts cannot be picked: switching would drop unsaved text.
  const [editorOpen, setEditorOpen] = createSignal(false)

  createEffect(
    () => {
      const history = props.history
      const eligible = (props.surface ?? 'chat') === 'chat' && history != null && !history.temporary
      return eligible ? history.threadId.trim() : ''
    },
    (threadId) => {
      setHistoryThreadId(null)
      setPromoteThreadId(null)
      if (!threadId) return
      let cancelled = false
      void getComposerCapabilities().then((capabilities) => {
        if (cancelled) return
        if (capabilities.artifactHistory) setHistoryThreadId(threadId)
        if (capabilities.artifactPromote) setPromoteThreadId(threadId)
      })
      return () => { cancelled = true }
    },
  )

  // Whether the gateway may revise a selected range (EP-3 S6, `selection_edit`). Asked only where the panel can send the
  // turn; a temporary chat never asks, because the gateway refuses the reference there.
  const [selectionEditOn, setSelectionEditOn] = createSignal(false)
  createEffect(
    () => props.onAskAboutSelection != null && (props.surface ?? 'chat') === 'chat' && props.history?.temporary !== true,
    (eligible) => {
      setSelectionEditOn(false)
      if (!eligible) return
      let cancelled = false
      void getComposerCapabilities().then((capabilities) => {
        if (!cancelled) setSelectionEditOn(capabilities.selectionEdit)
      })
      return () => { cancelled = true }
    },
  )

  // The selection follows the newest artifact until the user picks one, and
  // falls back to the newest again if the picked artifact leaves the list
  // (thread switch, new chat).
  const selected = createMemo(() => {
    const items = props.items
    if (items.length === 0) return null
    const requested = requestedId()
    return items.find((item) => item.artifact.id === requested) ?? items[items.length - 1]
  })

  /** A chat thread that is not temporary may ask for a review. A room and a temporary thread never do (F-t3-086). */
  const reviewable = () => (props.surface ?? 'chat') === 'chat' && props.history != null && !props.history.temporary

  return (
    <Show
      when={props.items.length > 0}
      fallback={(
        <EmptyPanel
          icon={<FileCode2 size={20} />}
          title={i18n.tr('Ingen artefakter ennå', 'No artifacts yet')}
          subtitle={i18n.tr('Dokumenter, kode, bilder og andre artefakter Verevon lager dukker opp her.', 'Documents, code, images and other artifacts Verevon creates appear here.')}
        />
      )}
    >
      <div class="verevon-chat-artifact-workspace">
        <div class="verevon-chat-artifact-list" role="tablist" aria-label={i18n.tr('Artefakter i samtalen', 'Artifacts in this conversation')}>
          <For each={props.items}>
            {(item) => (
              <ArtifactListEntry
                item={item}
                selected={selected()?.artifact.id === item.artifact.id}
                disabled={editorOpen() && selected()?.artifact.id !== item.artifact.id}
                onSelect={() => setRequestedId(item.artifact.id)}
              />
            )}
          </For>
        </div>
        <Show when={selected()}>
          {(item) => (
            <ArtifactViewer
              item={item()}
              surface={props.surface}
              historyThreadId={historyThreadId()}
              promoteThreadId={promoteThreadId()}
              editing={props.editing}
              onEditorOpenChange={setEditorOpen}
              selectionEdit={selectionEditOn()}
              onAskAboutSelection={props.onAskAboutSelection}
              reviewable={reviewable()}
            />
          )}
        </Show>
      </div>
    </Show>
  )
}

function ArtifactListEntry(props: { item: ArtifactPanelItem; selected: boolean; disabled?: boolean; onSelect: () => void }) {
  const renderKind = createMemo(() => artifactRenderKind(props.item.artifact))
  const versionCount = createMemo(() => artifactVersions(props.item.artifact).length)
  const i18n = useI18n()
  const title = createMemo(() => artifactDisplayTitle(props.item, renderKind(), i18n.tr))

  return (
    <button
      type="button"
      role="tab"
      aria-selected={props.selected ? 'true' : 'false'}
      disabled={props.disabled}
      class={{
        'verevon-chat-artifact-list__item': true,
        'verevon-chat-artifact-list__item--active': props.selected,
      }}
      onClick={() => props.onSelect()}
    >
      <span class="verevon-chat-artifact-list__icon">{kindIcon(renderKind())}</span>
      <span class="verevon-chat-artifact-list__copy">
        <strong>{title()}</strong>
        <span>
          <em>{artifactRenderKindLabel(renderKind(), i18n.tr)}</em>
          <Show when={versionCount() > 1}>
            <small>{i18n.tr(`${versionCount()} versjoner`, `${versionCount()} versions`)}</small>
          </Show>
        </span>
      </span>
    </button>
  )
}

function revisionVersion(revision: ArtifactRevision): ChatArtifactVersion {
  return {
    content: revision.content,
    title: revision.title,
    version: revision.version,
    editorKind: revision.editorKind,
    ...(revision.restoredFromVersion ? { restoredFromVersion: revision.restoredFromVersion } : {}),
  }
}

export function ArtifactViewer(props: {
  item: ArtifactPanelItem
  surface?: ViewerSurface
  /** Set only when the server's history is on for a chat thread that is not temporary. */
  historyThreadId?: string | null
  /** Set only when the BFF's promote switch is on for a chat thread that is not temporary (EP-3 S8). */
  promoteThreadId?: string | null
  editing?: ArtifactEditing
  onEditorOpenChange?: (open: boolean) => void
  /** The gateway's `selection_edit` capability, already narrowed to the surfaces that may ask (EP-3 S6). */
  selectionEdit?: boolean
  onAskAboutSelection?: (request: ArtifactSelectionRequest) => Promise<unknown> | void
  /** Set only for a chat thread that is not temporary: the panel may offer Review on a text revision (F-t3-086). */
  reviewable?: boolean
}) {
  const i18n = useI18n()
  const surface = (): ViewerSurface => props.surface ?? 'chat'
  const [versionCursor, setVersionCursor] = createSignal<number | null>(null)
  /** Revisions read from the server's history, every page loaded so far. */
  const [serverRevisions, setServerRevisions] = createSignal<ChatArtifactVersion[]>([])
  const [olderRevisionsExist, setOlderRevisionsExist] = createSignal(false)
  const [loadingOlder, setLoadingOlder] = createSignal(false)
  const [copyState, setCopyState] = createSignal<'copied' | 'failed' | 'idle'>('idle')
  const [showSource, setShowSource] = createSignal(false)
  /**
   * "What changed in this revision" — the one patch this app can genuinely
   * produce, because `ChatArtifact.history` retains every version's full content
   * client-side. No backend work and no new contract; the data was already here.
   */
  const [showDiff, setShowDiff] = createSignal(false)
  const [imageFailed, setImageFailed] = createSignal(false)
  const [exportFailed, setExportFailed] = createSignal(false)
  /**
   * Revisions the person saved or restored here (EP-3 S4), folded over the conversation's copy so the panel
   * shows them at once; a later turn's revision carries them in its own history.
   */
  const [savedRevisions, setSavedRevisions] = createSignal<SavedArtifactRevision[]>([])
  const [editorOpen, setEditorOpen] = createSignal(false)
  const [restoring, setRestoring] = createSignal(false)
  const [restoreFailure, setRestoreFailure] = createSignal<string | null>(null)
  /** The selection the person made in a source view and has not sent, and the instruction they are writing (EP-3 S6). */
  const [pendingSelection, setPendingSelection] = createSignal<PendingSelection | null>(null)
  const [askText, setAskText] = createSignal('')
  const [asking, setAsking] = createSignal(false)
  const [askFailure, setAskFailure] = createSignal<string | null>(null)
  let bodyRef: HTMLDivElement | undefined
  const [promoting, setPromoting] = createSignal(false)
  /** The outcome of the last "save to knowledge" for this artifact; `documentId` links the copy. */
  const [promoteResult, setPromoteResult] = createSignal<{ ok: true; documentId: string } | { ok: false } | null>(null)
  /** The review of the revision on screen, with the version it read (F-t3-086). Kept in memory only, never stored. */
  const [reviewRun, setReviewRun] = createSignal<{ card: ReviewCard; version: number } | null>(null)
  const [reviewing, setReviewing] = createSignal(false)
  const [reviewFailure, setReviewFailure] = createSignal<ReviewRunFailureKind | null>(null)
  /** The lines a finding points at, chosen in the review card; the code view shades and scrolls to them. */
  const [focusLines, setFocusLines] = createSignal<ReviewLineRange | null>(null)
  /** Bumped when the viewer moves to another artifact, so a review that returns late is dropped, not shown. */
  let reviewGeneration = 0

  const artifact = createMemo(() => savedRevisions().reduce(
    (carried, saved) => mergeArtifactVersion(carried, {
      id: carried.id,
      kind: carried.kind,
      content: saved.content,
      title: saved.title,
      version: saved.version,
    }),
    serverRevisions().length > 0
      ? mergeServerRevisions(props.item.artifact, serverRevisions())
      : props.item.artifact,
  ))
  /** Versions the person wrote: a model's provenance stamp does not describe them. */
  const personVersions = createMemo(() => new Set(savedRevisions().map((saved) => saved.version)))
  const versions = createMemo(() => artifactVersions(artifact()))
  const revision = createMemo(() => artifactVersionAt(artifact(), versionCursor()))
  const revisionIndex = createMemo(() => {
    const index = versions().findIndex((entry) => entry.version === revision().version)
    return index < 0 ? versions().length - 1 : index
  })

  /**
   * The artifact as of the selected revision. Every helper below reads this so
   * stepping back in history changes the rendered content, title, and download
   * without touching the stored artifact.
   */
  const displayed = createMemo(() => ({
    ...artifact(),
    content: revision().content,
    title: revision().title,
    version: revision().version,
  }))
  /** The revision immediately before the one on screen, if there is one. */
  const previousRevision = createMemo(() => {
    const index = revisionIndex()
    return index > 0 ? versions()[index - 1] : undefined
  })
  const revisionDiff = createMemo(() => {
    const previous = previousRevision()
    if (!previous) return null
    return diffText(previous.content, revision().content)
  })
  const renderKind = createMemo(() => artifactRenderKind(displayed()))
  /** A registered extra viewer, if one claims this revision. It takes the body; the header stays on the built-in kind. */
  const extraRenderer = createMemo(() => (surface() === 'chat' ? extraArtifactRenderers.resolve(displayed()) : undefined))
  /** The registry's verdict for this kind on this surface; `undefined` means the surface may not draw it. */
  const viewer = createMemo(() => viewerFor(renderKind(), surface()))
  const previewGated = createMemo(
    () => viewer()?.requiresActivation === true && !activatedPreviews().has(viewerActivationKey(displayed())),
  )
  const missing = createMemo(() => artifactContentMissing(displayed().content))
  const fileMeta = createMemo(() => artifactFileMeta(displayed(), renderKind(), props.item.file))
  const title = createMemo(() => artifactDisplayTitle({ ...props.item, artifact: displayed() }, renderKind(), i18n.tr))
  /**
   * Edit and Restore (EP-3 S4): only with the gateway's capability on, in a chat (never a room), in a thread that
   * keeps history (never temporary, never a Space's), for a text kind whose text arrived.
   */
  const canEdit = createMemo(() => {
    const editing = props.editing
    return editing?.enabled === true
      && !editing.temporary
      && !editing.spaceThread
      && editing.threadId.length > 0
      && surface() === 'chat'
      && EDITABLE_ARTIFACT_KINDS.has(artifact().kind)
      && !artifactContentMissing(artifact().content)
  })
  const onLatest = createMemo(() => revisionIndex() >= versions().length - 1)
  /**
   * Asking about a selection (EP-3 S6): the gateway said selection edits are on, this is a chat that is not temporary,
   * the person is not editing, and the kind is a text kind. Which views show the source is decided by `selectionAllowed`.
   */
  const selectionOffered = createMemo(() => props.selectionEdit === true
    && props.onAskAboutSelection != null
    && surface() === 'chat'
    && !editorOpen()
    && EDITABLE_ARTIFACT_KINDS.has(artifact().kind)
    && !artifactContentMissing(artifact().content))
  /** The body on screen is the artifact's own text: the code view, or the source of an HTML page or a document. */
  const selectionAllowed = createMemo(() => selectionOffered() && (renderKind() === 'code' || showSource()))
  /** Save to knowledge (EP-3 S8): a document whose text arrived, on the chat surface, outside the editor. */
  const canPromote = createMemo(() => Boolean(props.promoteThreadId)
    && surface() === 'chat'
    && artifact().kind === 'document'
    && renderKind() === 'document'
    && !missing()
    && !editorOpen())
  /** Review as a run (F-t3-086): the switch is on, and this is a chat that is not temporary. A room never offers it. */
  const reviewOffered = createMemo(() => reviewRunEnabled() && props.reviewable === true && surface() === 'chat')
  /** Review reads the text on screen: a document, code or HTML revision whose text arrived, never while the editor is open. */
  const canReview = createMemo(() => reviewOffered()
    && !editorOpen()
    && !missing()
    && (renderKind() === 'document' || renderKind() === 'code' || renderKind() === 'html'))
  /** A finding's lines can be shown only in a code view, and only for the revision the review read. */
  const reviewLocatable = createMemo(() => (renderKind() === 'code' || renderKind() === 'html')
    && reviewRun()?.version === displayed().version)

  const openEditor = (open: boolean) => {
    setEditorOpen(open)
    props.onEditorOpenChange?.(open)
  }
  onCleanup(() => props.onEditorOpenChange?.(false))

  const acceptSaved = (saved: SavedArtifactRevision) => {
    setSavedRevisions((current) => [...current, saved])
    setVersionCursor(null)
    setShowDiff(false)
    props.editing?.onSaved?.(saved)
  }

  const restoreShown = async () => {
    const editing = props.editing
    if (!editing || restoring()) return
    setRestoring(true)
    setRestoreFailure(null)
    try {
      acceptSaved(await restoreArtifactRevision({
        threadId: editing.threadId,
        artifactId: artifact().id,
        version: revision().version,
        expectedVersion: artifact().version,
      }))
    } catch (error) {
      setRestoreFailure(artifactConflictHeadVersion(error) != null
        ? i18n.tr(
          'Artefaktet ble endret etter at du åpnet det. Last inn samtalen på nytt og prøv igjen.',
          'The artifact changed after you opened it. Reload the conversation and try again.',
        )
        : i18n.tr('Kunne ikke gjenopprette versjonen. Prøv igjen.', 'Could not restore the version. Please try again.'))
    } finally {
      setRestoring(false)
    }
  }

  /** Copies the revision on screen, not the newest one: that is the text the person chose to keep. */
  const saveToKnowledge = async () => {
    const threadId = props.promoteThreadId
    if (!threadId || promoting()) return
    const shown = displayed()
    setPromoting(true)
    setPromoteResult(null)
    try {
      const saved = await uploadChatDocument(title(), shown.content, {
        origin: 'chat-artifact',
        sourceRef: { threadId, artifactId: shown.id, version: shown.version },
      })
      setPromoteResult({ ok: true, documentId: saved.documentId })
    } catch {
      setPromoteResult({ ok: false })
    } finally {
      setPromoting(false)
    }
  }

  /**
   * Reviews the text on screen as a run (F-t3-086). The text is sent once and only the card comes back. A result that
   * arrives after the person moved to another artifact is dropped, so a card never lands on the wrong text.
   */
  const runArtifactReview = async () => {
    if (reviewing()) return
    const shown = displayed()
    const generation = reviewGeneration
    setReviewing(true)
    setReviewFailure(null)
    try {
      const card = await runReview({
        target: 'artifact_revision',
        label: reviewRunLabel(title(), shown.version),
        material: shown.content,
        brief: '',
        locale: i18n.locale(),
      })
      if (generation !== reviewGeneration) return
      setFocusLines(null)
      if (card) {
        setReviewRun({ card, version: shown.version })
      } else {
        setReviewRun(null)
        setReviewFailure('off')
      }
    } catch (error) {
      if (generation !== reviewGeneration) return
      setReviewFailure(reviewRunFailureKind(error))
    } finally {
      if (generation === reviewGeneration) setReviewing(false)
    }
  }

  /** Drops the review the panel shows, any failure, and the highlighted lines. */
  const closeReviewRun = () => {
    setReviewRun(null)
    setReviewFailure(null)
    setFocusLines(null)
  }

  /** Shows a finding's lines in the code view. An HTML page opens its source first; the code view then scrolls to them. */
  const locateReviewLines = (range: ReviewLineRange) => {
    setShowDiff(false)
    if (renderKind() === 'html') setShowSource(true)
    setFocusLines({ start: range.start, end: range.end })
  }

  // Reset the revision cursor, the HTML source toggle, and any image failure
  // when the panel switches to a different artifact — a new selection must open
  // on the newest revision in preview mode. Keyed on the id alone so a version
  // streaming in for the artifact the user is already reading does NOT yank
  // their revision cursor forward. The key is a memo: a Solid 2 effect runs
  // whenever its compute re-runs, equal value or not, and `artifact()` re-runs
  // on every saved or server revision, which this effect itself clears.
  const artifactId = createMemo(() => artifact().id)
  createEffect(
    artifactId,
    () => {
      setVersionCursor(null)
      setShowSource(false)
      setShowDiff(false)
      setImageFailed(false)
      setCopyState('idle')
      setExportFailed(false)
      setSavedRevisions([])
      setRestoreFailure(null)
      setPromoteResult(null)
      reviewGeneration += 1
      setReviewing(false)
      setReviewRun(null)
      setReviewFailure(null)
      setFocusLines(null)
      openEditor(false)
    },
  )

  // The stored history of this artifact, first page on open. Keyed on the
  // thread and the artifact, so a revision streaming in does not refetch; a
  // failed read leaves the streamed history as it was.
  let historyGeneration = 0
  const loadRevisions = (threadId: string, artifactId: string, beforeVersion?: number) => {
    const generation = historyGeneration
    setLoadingOlder(beforeVersion != null)
    return listArtifactRevisions(threadId, artifactId, beforeVersion != null ? { beforeVersion } : {})
      .then((page) => {
        if (generation !== historyGeneration) return
        setServerRevisions((current) => [...current, ...page.revisions.map(revisionVersion)])
        setOlderRevisionsExist(page.hasMore)
      })
      .catch(() => {
        // The history is a richer view of what streamed, never a precondition for reading it.
      })
      .finally(() => {
        if (generation === historyGeneration) setLoadingOlder(false)
      })
  }
  // A memo, so a new item object for the same artifact (a streamed revision) does not re-run the effect.
  const historyKey = createMemo(() => {
    const threadId = props.historyThreadId ?? ''
    const item = props.item.artifact
    return threadId && STORED_REVISION_KINDS.has(item.kind) ? `${threadId}\n${item.id}` : ''
  })
  createEffect(
    historyKey,
    (key) => {
      historyGeneration += 1
      setServerRevisions([])
      setOlderRevisionsExist(false)
      if (!key) return
      const [threadId = '', artifactId = ''] = key.split('\n')
      void loadRevisions(threadId, artifactId)
    },
  )
  const loadOlderRevisions = () => {
    const threadId = props.historyThreadId
    const oldest = serverRevisions().reduce<number | null>(
      (lowest, entry) => (lowest == null || entry.version < lowest ? entry.version : lowest),
      null,
    )
    if (!threadId || oldest == null || loadingOlder()) return
    void loadRevisions(threadId, props.item.artifact.id, oldest)
  }
  /** `v3 · you`, `v4 · Verevon`, `v5 · restored from v2`; the plain position when no editor is known. */
  const revisionLabel = createMemo(() => {
    const current = revision()
    switch (current.editorKind) {
      case 'model':
        return `v${current.version} · Verevon`
      case 'user':
        return i18n.tr(`v${current.version} · deg`, `v${current.version} · you`)
      case 'restore':
        return current.restoredFromVersion
          ? i18n.tr(
            `v${current.version} · gjenopprettet fra v${current.restoredFromVersion}`,
            `v${current.version} · restored from v${current.restoredFromVersion}`,
          )
          : i18n.tr(`v${current.version} · gjenopprettet`, `v${current.version} · restored`)
      default:
        return `v${current.version} · ${revisionIndex() + 1}/${versions().length}`
    }
  })

  // Word/Excel/PowerPoint/PDF for a document artifact, built in the browser
  // from the revision on screen (lib/chat-export). The same four choices an
  // answer offers, and the ones that matter more here: in Verevon the draft
  // usually lives in this panel while the reply is a one-line note. PDF
  // prints the rendered document below (`documentRef`).
  let documentRef: HTMLDivElement | undefined

  const runExport = (format: ExportFormat) => {
    setExportFailed(false)
    try {
      exportChatAnswer(format, {
        content: displayed().content,
        title: title(),
        createdAt: props.item.turn.createdAt,
        sources: (props.item.turn.citations ?? []).map((citation) => ({ title: citation.title || citation.url, url: citation.url })),
        element: format === 'pdf' ? (documentRef ?? null) : undefined,
        locale: i18n.locale(),
      })
    } catch {
      setExportFailed(true)
    }
  }

  const exportItems = (): MessageMenuItem[] => [
    { label: i18n.tr('Eksporter til Word', 'Export to Word'), icon: <FileText size={16} />, onClick: () => runExport('docx') },
    { label: i18n.tr('Eksporter til Excel', 'Export to Excel'), icon: <FileSpreadsheet size={16} />, onClick: () => runExport('xlsx') },
    { label: i18n.tr('Eksporter til PowerPoint', 'Export to PowerPoint'), icon: <Presentation size={16} />, onClick: () => runExport('pptx') },
    { label: i18n.tr('Lagre som PDF', 'Save as PDF'), icon: <Download size={16} />, onClick: () => runExport('pdf') },
  ]

  let copyTimer: number | undefined
  onCleanup(() => window.clearTimeout(copyTimer))

  const copyContent = async () => {
    window.clearTimeout(copyTimer)
    try {
      await navigator.clipboard.writeText(displayed().content)
      setCopyState('copied')
    } catch {
      // Clipboard access can be denied (no permission, insecure context). Say so
      // rather than flashing a success state for something that did not happen.
      setCopyState('failed')
    }
    copyTimer = window.setTimeout(() => setCopyState('idle'), 1800)
  }

  const stepVersion = (delta: number) => {
    const list = versions()
    const next = list[revisionIndex() + delta]
    if (next) setVersionCursor(next.version)
  }

  /** Reads the selection the person made in the body after a release there; only a source view offers one. */
  const readSourceSelection = () => {
    const body = bodyRef
    if (!body || !body.hasAttribute(SOURCE_VIEW_ATTRIBUTE)) {
      setPendingSelection(null)
      return
    }
    const range = sourceSelectionRange(body, displayed().content)
    setPendingSelection(range ? { ...range, version: revision().version } : null)
  }
  // A selection belongs to the text on screen: moving to another revision, or leaving the source view, drops it.
  // A memo, so a re-read of the same revision (a server page, a saved revision) keeps the selection.
  const selectionKey = createMemo(() => `${revision().version}:${selectionAllowed()}`)
  createEffect(
    selectionKey,
    () => {
      setPendingSelection(null)
    },
  )

  /** Sends the selection and the person's instruction as one turn (EP-3 S6). The selected text is hashed, never sent. */
  const askAboutSelection = async (range: PendingSelection) => {
    const send = props.onAskAboutSelection
    const instruction = askText().trim()
    if (!send || !instruction || asking()) return
    setAsking(true)
    setAskFailure(null)
    try {
      await send({
        artifactId: artifact().id,
        title: title(),
        version: range.version,
        start: range.start,
        end: range.end,
        quoteSha256: await quoteSha256(range.quote),
        instruction,
      })
      setPendingSelection(null)
      setAskText('')
    } catch {
      setAskFailure(i18n.tr('Kunne ikke sende endringen. Prøv igjen.', 'Could not send the change. Please try again.'))
    } finally {
      setAsking(false)
    }
  }

  return (
    <section class="verevon-chat-artifact-view" data-menu-boundary aria-label={i18n.tr(`Artefakt ${title()}`, `Artifact ${title()}`)}>
      <header class="verevon-chat-artifact-view__head">
        <div class="verevon-chat-artifact-view__title">
          <span class="verevon-chat-artifact-view__icon">{kindIcon(renderKind())}</span>
          <div>
            <h2>{title()}</h2>
            <p>
              <em>{artifact().kind}</em>
              <Show when={artifactLanguage(displayed()) && renderKind() === 'code'}>
                <span>{artifactLanguage(displayed())}</span>
              </Show>
              <Show when={fileMeta()}>
                {(meta) => (
                  <>
                    <span>{meta().mimeLabel}</span>
                    <Show when={meta().bytes > 0}>
                      <span>{formatBytes(meta().bytes)}</span>
                    </Show>
                  </>
                )}
              </Show>
              <Show when={props.item.turn.createdAt}>
                <span>{formatRelative(props.item.turn.createdAt, i18n.locale())}</span>
              </Show>
            </p>
          </div>
        </div>
        <div class="verevon-chat-artifact-view__actions">
          <Show when={versions().length > 1}>
            <div class="verevon-chat-artifact-versions" aria-label={i18n.tr('Versjonshistorikk', 'Version history')}>
              <button
                type="button"
                aria-label={i18n.tr('Forrige versjon', 'Previous version')}
                disabled={revisionIndex() <= 0}
                onClick={() => stepVersion(-1)}
              >
                <ChevronLeft size={14} />
              </button>
              <span>{revisionLabel()}</span>
              <button
                type="button"
                aria-label={i18n.tr('Neste versjon', 'Next version')}
                disabled={revisionIndex() >= versions().length - 1}
                onClick={() => stepVersion(1)}
              >
                <ChevronRight size={14} />
              </button>
            </div>
          </Show>
          <Show when={olderRevisionsExist()}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              disabled={loadingOlder()}
              onClick={() => loadOlderRevisions()}
            >
              {i18n.tr('Tidligere versjoner', 'Earlier versions')}
            </button>
          </Show>
          <Show when={canEdit() && !editorOpen() && onLatest()}>
            <button type="button" class="verevon-chat-artifact-action" onClick={() => openEditor(true)}>
              <Pencil size={14} />
              {i18n.tr('Rediger', 'Edit')}
            </button>
          </Show>
          <Show when={canEdit() && !editorOpen() && !onLatest()}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              disabled={restoring()}
              onClick={() => void restoreShown()}
            >
              <RotateCcw size={14} />
              {i18n.tr('Gjenopprett denne versjonen', 'Restore this version')}
            </button>
          </Show>
          {/* The value the accessor receives must be the diff itself, so the
              `missing` guard is a ternary rather than an && (which narrows to `true`). */}
          <Show when={missing() ? null : revisionDiff()}>
            {(diff) => (
              <button
                type="button"
                class="verevon-chat-artifact-action"
                onClick={() => setShowDiff((value) => !value)}
              >
                <GitCompare size={14} />
                {showDiff()
                  ? i18n.tr('Vis versjonen', 'Show version')
                  : i18n.tr(
                    `Vis endringer (+${diff().stat.added} −${diff().stat.removed})`,
                    `Show changes (+${diff().stat.added} −${diff().stat.removed})`,
                  )}
              </button>
            )}
          </Show>
          <Show when={(renderKind() === 'html' || (renderKind() === 'document' && selectionOffered())) && !missing()}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              onClick={() => setShowSource((value) => !value)}
            >
              {showSource() ? <Eye size={14} /> : <Code2 size={14} />}
              {showSource()
                ? i18n.tr('Vis forhåndsvisning', 'Show preview')
                : renderKind() === 'document'
                  ? i18n.tr('Vis kilde', 'Show source')
                  : i18n.tr('Vis kode', 'Show code')}
            </button>
          </Show>
          <Show when={isCopyableRenderKind(renderKind()) && !missing()}>
            <button
              type="button"
              class={{
                'verevon-chat-artifact-action': true,
                'verevon-chat-artifact-action--failed': copyState() === 'failed',
              }}
              onClick={() => void copyContent()}
            >
              <Switch fallback={<Copy size={14} />}>
                <Match when={copyState() === 'copied'}><Check size={14} /></Match>
                <Match when={copyState() === 'failed'}><AlertCircle size={14} /></Match>
              </Switch>
              <Switch fallback={i18n.tr('Kopier', 'Copy')}>
                <Match when={copyState() === 'copied'}>{i18n.tr('Kopiert', 'Copied')}</Match>
                <Match when={copyState() === 'failed'}>{i18n.tr('Kunne ikke kopiere', 'Could not copy')}</Match>
              </Switch>
            </button>
          </Show>
          <Show when={canReview()}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              disabled={reviewing()}
              aria-busy={reviewing() ? 'true' : undefined}
              title={i18n.tr(
                'Gjennomgå denne versjonen. Gjennomgangen lagres ikke.',
                'Review this version. The review is not stored.',
              )}
              onClick={() => void runArtifactReview()}
            >
              <ScanSearch size={14} />
              {reviewing() ? i18n.tr('Gjennomgår…', 'Reviewing…') : i18n.tr('Gjennomgå', 'Review')}
            </button>
          </Show>
          <Show when={canPromote()}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              disabled={promoting()}
              onClick={() => void saveToKnowledge()}
            >
              <BookOpen size={14} />
              {i18n.tr('Lagre kopi i kunnskapsbasen', 'Save a copy to knowledge')}
            </button>
          </Show>
          <Show when={renderKind() === 'document' && !missing()}>
            <MessageMenu
              align="auto"
              direction="down"
              trigger={{
                icon: <FileText size={14} />,
                label: i18n.tr('Eksporter dokumentet', 'Export the document'),
                text: i18n.tr('Eksporter', 'Export'),
                class: 'verevon-chat-artifact-action',
              }}
              items={exportItems()}
            />
          </Show>
          <Show when={viewer() ? fileMeta() : null}>
            {(meta) => (
              <a
                class="verevon-chat-artifact-action verevon-chat-artifact-action--primary"
                href={meta().href}
                download={meta().downloadName}
              >
                <Download size={14} />
                {i18n.tr('Last ned', 'Download')}
              </a>
            )}
          </Show>
        </div>
      </header>
      <Show when={exportFailed()}>
        <p class="verevon-chat-feedback-reasons__note" role="alert">
          {i18n.tr('Eksport mislyktes. Prøv igjen.', 'Export failed. Please try again.')}
        </p>
      </Show>
      <Show when={restoreFailure()}>
        {(message) => <p class="verevon-chat-feedback-reasons__note" role="alert">{message()}</p>}
      </Show>
      <Show when={promoteResult()}>
        {(result) => {
          const outcome = result()
          return outcome.ok
            ? (
              <p class="verevon-chat-feedback-reasons__note" role="status">
                {i18n.tr(
                  'Kopien er lagret i kunnskapsbasen. Den blir værende hvis samtalen slettes, og senere endringer her blir ikke med.',
                  'The copy is saved to knowledge. It stays if the chat is deleted, and later edits here do not carry over.',
                )}
                <Show when={outcome.documentId}>
                  {' '}
                  <a href={`/knowledge?source=${encodeURIComponent(outcome.documentId)}`}>
                    {i18n.tr('Åpne kopien', 'Open the copy')}
                  </a>
                </Show>
              </p>
            )
            : (
              <p class="verevon-chat-feedback-reasons__note" role="alert">
                {i18n.tr('Kunne ikke lagre kopien i kunnskapsbasen. Prøv igjen.', 'Could not save the copy to knowledge. Please try again.')}
              </p>
            )
        }}
      </Show>

      <Show when={pendingSelection()}>
        {(range) => (
          <form
            class="verevon-chat-artifact-ask"
            onSubmit={(event) => {
              event.preventDefault()
              void askAboutSelection(range())
            }}
          >
            <p>
              {i18n.tr('Valgt tekst', 'Selected text')}: <q>{excerpt(range().quote)}</q>
            </p>
            <textarea
              rows={2}
              value={askText()}
              aria-label={i18n.tr('Hva skal endres i det valgte?', 'What should change in the selection?')}
              onInput={(event) => setAskText(event.currentTarget.value)}
            />
            <Show when={askFailure()}>
              {(message) => <p class="verevon-chat-feedback-reasons__note" role="alert">{message()}</p>}
            </Show>
            <div class="verevon-chat-artifact-ask__actions">
              <button
                type="submit"
                class="verevon-chat-artifact-action verevon-chat-artifact-action--primary"
                disabled={asking() || askText().trim().length === 0}
              >
                {i18n.tr('Be om endring', 'Ask for a change')}
              </button>
              <button type="button" class="verevon-chat-artifact-action" onClick={() => setPendingSelection(null)}>
                {i18n.tr('Avbryt', 'Cancel')}
              </button>
            </div>
          </form>
        )}
      </Show>
      <div
        ref={bodyRef}
        class="verevon-chat-artifact-view__body"
        data-artifact-source={selectionAllowed() ? '' : undefined}
        onMouseUp={() => readSourceSelection()}
        onKeyUp={() => readSourceSelection()}
      >
        <Show
          when={!(editorOpen() && props.editing)}
          fallback={(
            <ArtifactEditor
              threadId={props.editing?.threadId ?? ''}
              artifactId={artifact().id}
              title={title()}
              content={artifact().content}
              headVersion={artifact().version}
              newest={() => ({ content: artifact().content, title: artifact().title, version: artifact().version })}
              onSaved={(saved) => {
                acceptSaved(saved)
                openEditor(false)
              }}
              onClose={() => openEditor(false)}
            />
          )}
        >
        <Show when={viewer()} fallback={<ArtifactSurfaceRefusal />}>
        <Show when={!missing()} fallback={<ArtifactLoadFailure kind={artifact().kind} />}>
          <Show
            when={!(showDiff() && revisionDiff())}
            fallback={<DiffView result={revisionDiff()!} />}
          >
          <Switch fallback={<ArtifactPlainText content={displayed().content} />}>
            <Match when={extraRenderer()?.component}>
              {(component) => (
                <Dynamic
                  component={component()}
                  artifact={displayed()}
                  file={props.item.file}
                  fallback={() => (
                    <Show when={fileMeta()}>{(meta) => <ArtifactFileCard meta={meta()} title={title()} />}</Show>
                  )}
                />
              )}
            </Match>
            <Match when={renderKind() === 'code'}>
              <ArtifactCodeView
                content={displayed().content}
                language={artifactLanguage(displayed())}
                focus={reviewLocatable() ? focusLines() : null}
              />
            </Match>
            <Match when={renderKind() === 'document'}>
              <Show
                when={!(showSource() && selectionOffered())}
                fallback={<ArtifactPlainText content={displayed().content} />}
              >
                <div ref={documentRef} class="verevon-chat-artifact-document">
                  <ChatMarkdown content={displayed().content} />
                </div>
              </Show>
            </Match>
            <Match when={renderKind() === 'html'}>
              <Show
                when={!showSource()}
                fallback={(
                  <ArtifactCodeView
                    content={displayed().content}
                    language="html"
                    focus={reviewLocatable() ? focusLines() : null}
                  />
                )}
              >
                <Show
                  when={!previewGated()}
                  fallback={(
                    <ArtifactRunPreviewGate
                      onRun={() => {
                        const key = viewerActivationKey(displayed())
                        setActivatedPreviews((current) => new Set(current).add(key))
                      }}
                    />
                  )}
                >
                  <ArtifactHtmlPreview html={displayed().content} title={title()} />
                </Show>
              </Show>
            </Match>
            <Match when={renderKind() === 'image'}>
              <Show when={!imageFailed()} fallback={<ArtifactLoadFailure kind={artifact().kind} />}>
                <ArtifactImageView
                  item={{ ...props.item, artifact: displayed() }}
                  title={title()}
                  onFailed={() => setImageFailed(true)}
                />
              </Show>
            </Match>
            {/*
                Point 4 of the definition of finished: a generated PDF opens in a
                viewer, the same way an attached one already did. `artifactRenderKind`
                only returns 'pdf' for an addressable source, so `content` is safe to
                use as a frame `src` — an unopenable body still routes to 'binary'
                below and keeps its download card.
            */}
            <Match when={renderKind() === 'pdf'}>
              <div class="verevon-chat-artifact-frame">
                <iframe
                  src={artifact().content.trim()}
                  title={i18n.tr(`Forhåndsvisning av ${title()}`, `Preview of ${title()}`)}
                  referrerpolicy="no-referrer"
                />
              </div>
            </Match>
            <Match when={renderKind() === 'binary'}>
              <Show when={fileMeta()} fallback={<ArtifactLoadFailure kind={artifact().kind} />}>
                {(meta) => <ArtifactFileCard meta={meta()} title={title()} />}
              </Show>
            </Match>
          </Switch>
          </Show>
        </Show>
        </Show>
        </Show>
      </div>
      <Show when={reviewOffered() && !editorOpen()}>
        <Show when={reviewRun()}>
          {(run) => (
            <ReviewRunCard
              card={run().card}
              locatable={reviewLocatable()}
              onLocate={locateReviewLines}
              onClose={closeReviewRun}
            />
          )}
        </Show>
        <Show when={reviewFailure()}>
          {(kind) => <ReviewRunFailure kind={kind()} />}
        </Show>
      </Show>
      <Show when={!personVersions().has(revision().version)}>
        <ArtifactProvenance provenance={(artifact() as { provenance?: unknown }).provenance} />
      </Show>
    </section>
  )
}

/**
 * Code with a line-number gutter and basic, dependency-free highlighting (see
 * chat-code-highlight.ts — this project ships no highlighter library).
 */
export function ArtifactCodeView(props: { content: string; language: string; focus?: ReviewLineRange | null }) {
  const lines = createMemo(() => highlightCode(props.content, props.language))
  let viewRef: HTMLDivElement | undefined
  /** Whether a line is one the chosen review finding points at (F-t3-086). */
  const focused = (number: number) => props.focus != null && number >= props.focus.start && number <= props.focus.end
  // The chosen lines are brought into view when the person chooses them. jsdom has no scrollIntoView, hence the optional call.
  createEffect(
    () => props.focus,
    (focus) => {
      if (!focus) return
      viewRef?.querySelector<HTMLElement>(`[data-line="${focus.start}"]`)?.scrollIntoView?.({ block: 'center' })
    },
  )
  return (
    <div class="verevon-chat-artifact-code" ref={viewRef}>
      <pre>
        <code>
          <For each={lines()}>
            {(line) => (
              <span
                class={{
                  'verevon-chat-artifact-code__line': true,
                  'verevon-chat-artifact-code__line--focus': focused(line.number),
                }}
                data-line={line.number}
              >
                <span class="verevon-chat-artifact-code__gutter" aria-hidden="true">{line.number}</span>
                <span class="verevon-chat-artifact-code__text">
                  <For each={line.tokens}>
                    {(token) => <span class={`verevon-code-${token.type}`}>{token.text}</span>}
                  </For>
                </span>
              </span>
            )}
          </For>
        </code>
      </pre>
    </div>
  )
}

/**
 * Live preview of model-generated HTML.
 *
 * SECURITY REQUIREMENT, not a nicety: `content` is untrusted model output that
 * may contain arbitrary scripts. It is rendered via `srcdoc` inside an iframe
 * with `sandbox="allow-scripts"` and deliberately WITHOUT `allow-same-origin`,
 * which forces a unique opaque origin — the document cannot reach this app's
 * DOM, cookies, localStorage, or same-origin `/api` BFF endpoints (where the
 * first-party Better Auth session cookie lives). Adding `allow-same-origin`
 * alongside `allow-scripts` would let the sandboxed page remove its own sandbox
 * and fully defeat this isolation, so the two must never appear together.
 * `referrerpolicy="no-referrer"` keeps the workspace URL out of any outbound
 * request the preview makes.
 */
export function ArtifactHtmlPreview(props: { html: string; title: string }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-artifact-frame">
      <iframe
        title={i18n.tr(`Forhåndsvisning av ${props.title}`, `Preview of ${props.title}`)}
        sandbox="allow-scripts"
        srcdoc={sandboxHtmlDocument(props.html)}
        referrerpolicy="no-referrer"
        loading="lazy"
      />
    </div>
  )
}

export function ArtifactImageView(props: { item: ArtifactPanelItem; title: string; onFailed: () => void }) {
  const i18n = useI18n()
  const [dimensions, setDimensions] = createSignal<string | null>(null)
  const src = createMemo(() => imageArtifactSrc(props.item.artifact.content))
  const specs = createMemo(() => buildArtifactImageSpecs(props.item, dimensions()))

  return (
    <div class="verevon-chat-artifact-image">
      <img
        src={src()}
        alt={props.title}
        onError={() => props.onFailed()}
        onLoad={(event) => {
          const image = event.currentTarget
          setDimensions(`${image.naturalWidth} x ${image.naturalHeight}px`)
        }}
      />
      <div class="verevon-chat-artifact-specs" aria-label={i18n.tr('Bildespesifikasjoner', 'Image specifications')}>
        <For each={specs()}>
          {(spec) => (
            <div>
              <span>{spec.label}</span>
              <strong>{spec.value}</strong>
            </div>
          )}
        </For>
      </div>
    </div>
  )
}

/** Generated binary (xlsx/docx/pdf/…): identity plus a working download. */
export function ArtifactFileCard(props: {
  meta: { bytes: number; downloadName: string; href: string; mime: string; mimeLabel: string }
  title: string
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-artifact-file">
      <span class="verevon-chat-artifact-file__icon"><FileSpreadsheet size={22} /></span>
      <div class="verevon-chat-artifact-file__meta">
        <strong>{props.meta.downloadName}</strong>
        <span>
          {props.meta.mimeLabel}
          <Show when={props.meta.bytes > 0}> · {formatBytes(props.meta.bytes)}</Show>
        </span>
        <small>{props.meta.mime}</small>
      </div>
      <div class="verevon-chat-artifact-file__actions">
        <a
          class="verevon-chat-artifact-action verevon-chat-artifact-action--primary"
          href={props.meta.href}
          download={props.meta.downloadName}
        >
          <Download size={14} />
          {i18n.tr('Last ned', 'Download')}
        </a>
      </div>
    </div>
  )
}

export function ArtifactPlainText(props: { content: string }) {
  return (
    <div class="verevon-chat-artifact-code verevon-chat-artifact-code--plain">
      <pre><code>{props.content}</code></pre>
    </div>
  )
}

/**
 * The artifact was announced by the backend but its payload is empty or
 * unusable. An honest failure beats an empty box the user cannot interpret.
 */
export function ArtifactLoadFailure(props: { kind: string }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-artifact-failure" role="status">
      <AlertCircle size={18} />
      <div>
        <strong>{i18n.tr('Artefaktet kunne ikke lastes', 'The artifact could not be loaded')}</strong>
        <p>
          {i18n.tr(
            `Verevon meldte om et artefakt av typen «${props.kind}», men innholdet kom aldri fram. Prøv å generere det på nytt.`,
            `Verevon reported an artifact of type «${props.kind}», but its content never arrived. Try generating it again.`,
          )}
        </p>
      </div>
    </div>
  )
}

/**
 * The surface has no viewer for this kind (a room never draws PDF, image,
 * binary or unknown kinds). Deliberately no download link and no content.
 */
function ArtifactSurfaceRefusal() {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-artifact-failure verevon-chat-artifact-failure--neutral" role="status">
      <AlertCircle size={18} />
      <div>
        <strong>{i18n.tr('Forhåndsvisning er ikke tilgjengelig i rommet', 'Preview not available in the room')}</strong>
        <p>
          {i18n.tr(
            'Denne typen artefakt vises ikke i rommet.',
            'This kind of artifact is not shown in the room.',
          )}
        </p>
      </div>
    </div>
  )
}

/**
 * Room HTML is untrusted markup: nothing runs until the reader asks for it. The
 * "Show code" toggle in the header stays available without running anything.
 */
function ArtifactRunPreviewGate(props: { onRun: () => void }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-artifact-gate">
      <Globe size={20} />
      <div>
        <strong>{i18n.tr('Forhåndsvisningen er ikke startet', 'Preview not started')}</strong>
        <p>
          {i18n.tr(
            'Dette er HTML skrevet av en agent. Den kjører isolert, men først når du starter den.',
            'This is HTML written by an agent. It runs in isolation, but only once you start it.',
          )}
        </p>
      </div>
      <button type="button" class="verevon-chat-artifact-action verevon-chat-artifact-action--primary" onClick={() => props.onRun()}>
        <Eye size={14} />
        {i18n.tr('Kjør forhåndsvisning', 'Run preview')}
      </button>
    </div>
  )
}

function kindIcon(kind: ArtifactRenderKind) {
  const Icon = KIND_ICONS[kind]
  return <Icon size={15} />
}

/**
 * Images keep the prompt-derived title the inline previews already use; every
 * other kind shows its own title, falling back to the kind label.
 */
function artifactDisplayTitle(
  item: ArtifactPanelItem,
  kind: ArtifactRenderKind,
  tr: (noText: string, enText: string) => string,
): string {
  if (kind === 'image') {
    return generatedImageTitle(item.artifact.title, item.turn.content, item.file?.name)
  }
  return item.artifact.title.trim() || item.artifact.kind || artifactRenderKindLabel(kind, tr)
}
