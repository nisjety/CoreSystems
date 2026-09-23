/**
 * The "Artefakter" tab: a real artifact/canvas renderer.
 *
 * Left: every artifact in the conversation (title, kind badge, revision count).
 * Right: the selected artifact rendered by kind — code with line numbers and
 * basic highlighting, Markdown documents through the existing chat Markdown
 * renderer, untrusted HTML in a sandboxed iframe, images, and generated binaries
 * as a download card. A `‹ v2/3 ›` stepper walks the revision history of a
 * repeatedly re-emitted artifact and always opens on the newest revision.
 */
import {
  AlertCircle,
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
} from '@/shared/icons'
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
  ChatMarkdown,
  DiffView,
} from './ChatMessages'
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
  type IconComponent,
} from './chat-types'
import { sandboxHtmlDocument } from '@/shared/lib/sandbox-html'
import { useI18n } from '@/shared/i18n'

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

export function ArtifactsPanel(props: { items: ArtifactPanelItem[] }) {
  const i18n = useI18n()
  const [requestedId, setRequestedId] = createSignal<string | null>(null)

  // The selection follows the newest artifact until the user picks one, and
  // falls back to the newest again if the picked artifact leaves the list
  // (thread switch, new chat).
  const selected = createMemo(() => {
    const items = props.items
    if (items.length === 0) return null
    const requested = requestedId()
    return items.find((item) => item.artifact.id === requested) ?? items[items.length - 1]
  })

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
                onSelect={() => setRequestedId(item.artifact.id)}
              />
            )}
          </For>
        </div>
        <Show when={selected()}>
          {(item) => <ArtifactViewer item={item()} />}
        </Show>
      </div>
    </Show>
  )
}

function ArtifactListEntry(props: { item: ArtifactPanelItem; selected: boolean; onSelect: () => void }) {
  const renderKind = createMemo(() => artifactRenderKind(props.item.artifact))
  const versionCount = createMemo(() => artifactVersions(props.item.artifact).length)
  const i18n = useI18n()
  const title = createMemo(() => artifactDisplayTitle(props.item, renderKind(), i18n.tr))

  return (
    <button
      type="button"
      role="tab"
      aria-selected={props.selected ? 'true' : 'false'}
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

export function ArtifactViewer(props: { item: ArtifactPanelItem }) {
  const i18n = useI18n()
  const [versionCursor, setVersionCursor] = createSignal<number | null>(null)
  const [copyState, setCopyState] = createSignal<'copied' | 'failed' | 'idle'>('idle')
  const [showSource, setShowSource] = createSignal(false)
  /**
   * "What changed in this revision" — the one patch this app can genuinely
   * produce, because `ChatArtifact.history` retains every version's full content
   * client-side. No backend work and no new contract; the data was already here.
   */
  const [showDiff, setShowDiff] = createSignal(false)
  const [imageFailed, setImageFailed] = createSignal(false)

  const artifact = () => props.item.artifact
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
  const missing = createMemo(() => artifactContentMissing(displayed().content))
  const fileMeta = createMemo(() => artifactFileMeta(displayed(), renderKind(), props.item.file))
  const title = createMemo(() => artifactDisplayTitle({ ...props.item, artifact: displayed() }, renderKind(), i18n.tr))

  // Reset the revision cursor, the HTML source toggle, and any image failure
  // when the panel switches to a different artifact — a new selection must open
  // on the newest revision in preview mode. Keyed on the id alone so a version
  // streaming in for the artifact the user is already reading does NOT yank
  // their revision cursor forward.
  createEffect(
    () => artifact().id,
    () => {
      setVersionCursor(null)
      setShowSource(false)
      setShowDiff(false)
      setImageFailed(false)
      setCopyState('idle')
    },
  )

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

  return (
    <section class="verevon-chat-artifact-view" aria-label={i18n.tr(`Artefakt ${title()}`, `Artifact ${title()}`)}>
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
              <span>v{revision().version} · {revisionIndex() + 1}/{versions().length}</span>
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
                  ? 'Vis versjonen'
                  : `Vis endringer (+${diff().stat.added} −${diff().stat.removed})`}
              </button>
            )}
          </Show>
          <Show when={renderKind() === 'html' && !missing()}>
            <button
              type="button"
              class="verevon-chat-artifact-action"
              onClick={() => setShowSource((value) => !value)}
            >
              {showSource() ? <Eye size={14} /> : <Code2 size={14} />}
              {showSource() ? 'Vis forhåndsvisning' : 'Vis kode'}
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
              <Switch fallback="Kopier">
                <Match when={copyState() === 'copied'}>Kopiert</Match>
                <Match when={copyState() === 'failed'}>Kunne ikke kopiere</Match>
              </Switch>
            </button>
          </Show>
          <Show when={fileMeta()}>
            {(meta) => (
              <a
                class="verevon-chat-artifact-action verevon-chat-artifact-action--primary"
                href={meta().href}
                download={meta().downloadName}
              >
                <Download size={14} />
                Last ned
              </a>
            )}
          </Show>
        </div>
      </header>

      <div class="verevon-chat-artifact-view__body">
        <Show when={!missing()} fallback={<ArtifactLoadFailure kind={artifact().kind} />}>
          <Show
            when={!(showDiff() && revisionDiff())}
            fallback={<DiffView result={revisionDiff()!} />}
          >
          <Switch fallback={<ArtifactPlainText content={displayed().content} />}>
            <Match when={renderKind() === 'code'}>
              <ArtifactCodeView content={displayed().content} language={artifactLanguage(displayed())} />
            </Match>
            <Match when={renderKind() === 'document'}>
              <div class="verevon-chat-artifact-document">
                <ChatMarkdown content={displayed().content} />
              </div>
            </Match>
            <Match when={renderKind() === 'html'}>
              <Show
                when={!showSource()}
                fallback={<ArtifactCodeView content={displayed().content} language="html" />}
              >
                <ArtifactHtmlPreview html={displayed().content} title={title()} />
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
      </div>
    </section>
  )
}

/**
 * Code with a line-number gutter and basic, dependency-free highlighting (see
 * chat-code-highlight.ts — this project ships no highlighter library).
 */
export function ArtifactCodeView(props: { content: string; language: string }) {
  const lines = createMemo(() => highlightCode(props.content, props.language))
  return (
    <div class="verevon-chat-artifact-code">
      <pre>
        <code>
          <For each={lines()}>
            {(line) => (
              <span class="verevon-chat-artifact-code__line">
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
          Last ned
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
        <p>Verevon meldte om et artefakt av typen «{props.kind}», men innholdet kom aldri fram. Prøv å generere det på nytt.</p>
      </div>
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
