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
  FileCode2,
  FileSpreadsheet,
  FileText,
  Globe,
  Image as ImageIcon,
} from 'lucide-solid'
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
} from 'solid-js'
import {
  ChatMarkdown,
} from './ChatMessages'
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

const KIND_ICONS: Record<ArtifactRenderKind, IconComponent> = {
  binary: FileSpreadsheet,
  code: Code2,
  document: FileText,
  html: Globe,
  image: ImageIcon,
  text: FileCode2,
}

export function ArtifactsPanel(props: { items: ArtifactPanelItem[] }) {
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
          title="Ingen artefakter ennå"
          subtitle="Dokumenter, kode, bilder og andre artefakter Velion lager dukker opp her."
        />
      )}
    >
      <div class="velion-chat-artifact-workspace">
        <div class="velion-chat-artifact-list" role="tablist" aria-label="Artefakter i samtalen">
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
  const title = createMemo(() => artifactDisplayTitle(props.item, renderKind()))

  return (
    <button
      type="button"
      role="tab"
      aria-selected={props.selected}
      classList={{
        'velion-chat-artifact-list__item': true,
        'velion-chat-artifact-list__item--active': props.selected,
      }}
      onClick={() => props.onSelect()}
    >
      <span class="velion-chat-artifact-list__icon">{kindIcon(renderKind())}</span>
      <span class="velion-chat-artifact-list__copy">
        <strong>{title()}</strong>
        <span>
          <em>{artifactRenderKindLabel(renderKind())}</em>
          <Show when={versionCount() > 1}>
            <small>{versionCount()} versjoner</small>
          </Show>
        </span>
      </span>
    </button>
  )
}

export function ArtifactViewer(props: { item: ArtifactPanelItem }) {
  const [versionCursor, setVersionCursor] = createSignal<number | null>(null)
  const [copyState, setCopyState] = createSignal<'copied' | 'failed' | 'idle'>('idle')
  const [showSource, setShowSource] = createSignal(false)
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
  const renderKind = createMemo(() => artifactRenderKind(displayed()))
  const missing = createMemo(() => artifactContentMissing(displayed().content))
  const fileMeta = createMemo(() => artifactFileMeta(displayed(), renderKind(), props.item.file))
  const title = createMemo(() => artifactDisplayTitle({ ...props.item, artifact: displayed() }, renderKind()))

  // Reset the revision cursor, the HTML source toggle, and any image failure
  // when the panel switches to a different artifact — a new selection must open
  // on the newest revision in preview mode. Keyed on the id alone so a version
  // streaming in for the artifact the user is already reading does NOT yank
  // their revision cursor forward.
  createEffect(on(() => artifact().id, () => {
    setVersionCursor(null)
    setShowSource(false)
    setImageFailed(false)
    setCopyState('idle')
  }))

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
    <section class="velion-chat-artifact-view" aria-label={`Artefakt ${title()}`}>
      <header class="velion-chat-artifact-view__head">
        <div class="velion-chat-artifact-view__title">
          <span class="velion-chat-artifact-view__icon">{kindIcon(renderKind())}</span>
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
                <span>{formatRelative(props.item.turn.createdAt)}</span>
              </Show>
            </p>
          </div>
        </div>
        <div class="velion-chat-artifact-view__actions">
          <Show when={versions().length > 1}>
            <div class="velion-chat-artifact-versions" aria-label="Versjonshistorikk">
              <button
                type="button"
                aria-label="Forrige versjon"
                disabled={revisionIndex() <= 0}
                onClick={() => stepVersion(-1)}
              >
                <ChevronLeft size={14} />
              </button>
              <span>v{revision().version} · {revisionIndex() + 1}/{versions().length}</span>
              <button
                type="button"
                aria-label="Neste versjon"
                disabled={revisionIndex() >= versions().length - 1}
                onClick={() => stepVersion(1)}
              >
                <ChevronRight size={14} />
              </button>
            </div>
          </Show>
          <Show when={renderKind() === 'html' && !missing()}>
            <button
              type="button"
              class="velion-chat-artifact-action"
              onClick={() => setShowSource((value) => !value)}
            >
              {showSource() ? <Eye size={14} /> : <Code2 size={14} />}
              {showSource() ? 'Vis forhåndsvisning' : 'Vis kode'}
            </button>
          </Show>
          <Show when={isCopyableRenderKind(renderKind()) && !missing()}>
            <button
              type="button"
              classList={{
                'velion-chat-artifact-action': true,
                'velion-chat-artifact-action--failed': copyState() === 'failed',
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
                class="velion-chat-artifact-action velion-chat-artifact-action--primary"
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

      <div class="velion-chat-artifact-view__body">
        <Show when={!missing()} fallback={<ArtifactLoadFailure kind={artifact().kind} />}>
          <Switch fallback={<ArtifactPlainText content={displayed().content} />}>
            <Match when={renderKind() === 'code'}>
              <ArtifactCodeView content={displayed().content} language={artifactLanguage(displayed())} />
            </Match>
            <Match when={renderKind() === 'document'}>
              <div class="velion-chat-artifact-document">
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
            <Match when={renderKind() === 'binary'}>
              <Show when={fileMeta()} fallback={<ArtifactLoadFailure kind={artifact().kind} />}>
                {(meta) => <ArtifactFileCard meta={meta()} title={title()} />}
              </Show>
            </Match>
          </Switch>
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
    <div class="velion-chat-artifact-code">
      <pre>
        <code>
          <For each={lines()}>
            {(line) => (
              <span class="velion-chat-artifact-code__line">
                <span class="velion-chat-artifact-code__gutter" aria-hidden="true">{line.number}</span>
                <span class="velion-chat-artifact-code__text">
                  <For each={line.tokens}>
                    {(token) => <span class={`velion-code-${token.type}`}>{token.text}</span>}
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
  return (
    <div class="velion-chat-artifact-frame">
      <iframe
        title={`Forhåndsvisning av ${props.title}`}
        sandbox="allow-scripts"
        srcdoc={props.html}
        referrerpolicy="no-referrer"
        loading="lazy"
      />
    </div>
  )
}

export function ArtifactImageView(props: { item: ArtifactPanelItem; title: string; onFailed: () => void }) {
  const [dimensions, setDimensions] = createSignal<string | null>(null)
  const src = createMemo(() => imageArtifactSrc(props.item.artifact.content))
  const specs = createMemo(() => buildArtifactImageSpecs(props.item, dimensions()))

  return (
    <div class="velion-chat-artifact-image">
      <img
        src={src()}
        alt={props.title}
        onError={() => props.onFailed()}
        onLoad={(event) => {
          const image = event.currentTarget
          setDimensions(`${image.naturalWidth} x ${image.naturalHeight}px`)
        }}
      />
      <div class="velion-chat-artifact-specs" aria-label="Bildespesifikasjoner">
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
    <div class="velion-chat-artifact-file">
      <span class="velion-chat-artifact-file__icon"><FileSpreadsheet size={22} /></span>
      <div class="velion-chat-artifact-file__meta">
        <strong>{props.meta.downloadName}</strong>
        <span>
          {props.meta.mimeLabel}
          <Show when={props.meta.bytes > 0}> · {formatBytes(props.meta.bytes)}</Show>
        </span>
        <small>{props.meta.mime}</small>
      </div>
      <div class="velion-chat-artifact-file__actions">
        <a
          class="velion-chat-artifact-action velion-chat-artifact-action--primary"
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
    <div class="velion-chat-artifact-code velion-chat-artifact-code--plain">
      <pre><code>{props.content}</code></pre>
    </div>
  )
}

/**
 * The artifact was announced by the backend but its payload is empty or
 * unusable. An honest failure beats an empty box the user cannot interpret.
 */
export function ArtifactLoadFailure(props: { kind: string }) {
  return (
    <div class="velion-chat-artifact-failure" role="status">
      <AlertCircle size={18} />
      <div>
        <strong>Artefaktet kunne ikke lastes</strong>
        <p>Velion meldte om et artefakt av typen «{props.kind}», men innholdet kom aldri fram. Prøv å generere det på nytt.</p>
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
function artifactDisplayTitle(item: ArtifactPanelItem, kind: ArtifactRenderKind): string {
  if (kind === 'image') {
    return generatedImageTitle(item.artifact.title, item.turn.content, item.file?.name)
  }
  return item.artifact.title.trim() || item.artifact.kind || artifactRenderKindLabel(kind)
}
