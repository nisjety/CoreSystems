/**
 * Artifact viewer registry (WS07-S10, conflict C21 / X26).
 *
 * One module that says which kinds of artifact may be drawn on which surface,
 * and how much to trust the content. Pure and JSX-free: the per-kind components
 * stay in `features/chat/components/ChatArtifactPanel.tsx` (files do not move),
 * this table only answers `viewerFor(renderKind, surface)`.
 *
 *   - `chat`: the existing conversation panel, behaviour unchanged.
 *   - `room`: a Space room. Room data is shared with other people and was never
 *     typed by the viewer, so the room allows only `document`, `code` and `html`;
 *     `pdf`, `image`, `binary` and anything unknown have no room viewer (the
 *     PDF branch is an unsandboxed iframe and must never see room data).
 *
 * Trust classes:
 *   - `text`: rendered as inert text or through the safe-link Markdown renderer.
 *   - `untrusted_markup`: markup that may carry script. The ONLY renderer for it
 *     is the opaque-origin `srcdoc` iframe fed by `sandboxHtmlDocument`; no new
 *     origin and no permissive web-view iframe. On the room surface it also
 *     needs an explicit per-artifact click (`requiresActivation`).
 *   - `binary`: opaque bytes, an addressable source or a download card.
 *
 * A viewer registered from outside (WS14 office viewers, sibling plans) must
 * declare `surfaces` and `trust`. An `untrusted_markup` entry for the room that
 * does not require activation is refused, so a later registration cannot
 * quietly turn the room into an auto-preview surface.
 */

export type ViewerSurface = 'chat' | 'room'
export type ViewerTrust = 'text' | 'untrusted_markup' | 'binary'

/** Mirrors `ArtifactRenderKind` in the chat module without importing from `features`. */
export type ViewerRenderKind = 'binary' | 'code' | 'document' | 'html' | 'image' | 'pdf' | 'text'

/** The part of an artifact a lazily loaded viewer receives. */
export type ViewerArtifact = { id: string; kind: string; title: string; content: string; version?: number }

export type ViewerEntry = {
  renderKinds: readonly string[]
  surfaces: readonly ViewerSurface[]
  trust: ViewerTrust
  /** Room only in practice: draw nothing until the reader clicks "Run preview". */
  requiresActivation?: boolean
  /** Slot for lazily loaded viewers (DOCX/XLSX, registered by WS14). Unused by the built-ins. */
  load?: () => Promise<(props: { artifact: ViewerArtifact }) => unknown>
}

const BUILT_IN_VIEWERS: readonly ViewerEntry[] = [
  { renderKinds: ['document'], surfaces: ['chat', 'room'], trust: 'text' },
  { renderKinds: ['code'], surfaces: ['chat', 'room'], trust: 'text' },
  // Chat keeps its auto-preview; the room draws HTML only after a click.
  { renderKinds: ['html'], surfaces: ['chat'], trust: 'untrusted_markup' },
  { renderKinds: ['html'], surfaces: ['room'], trust: 'untrusted_markup', requiresActivation: true },
  { renderKinds: ['pdf'], surfaces: ['chat'], trust: 'binary' },
  { renderKinds: ['image'], surfaces: ['chat'], trust: 'binary' },
  { renderKinds: ['binary'], surfaces: ['chat'], trust: 'binary' },
  // Plain-text fallback for an unrecognised kind; chat only, the room mapper never yields it.
  { renderKinds: ['text'], surfaces: ['chat'], trust: 'text' },
]

const registered: ViewerEntry[] = []

function entryAcceptable(entry: ViewerEntry): boolean {
  if (entry.renderKinds.length === 0 || entry.surfaces.length === 0) return false
  if (entry.trust === 'untrusted_markup' && entry.surfaces.includes('room') && !entry.requiresActivation) return false
  return true
}

/**
 * Registers an extra viewer for kinds no built-in entry claims. Returns the
 * function that removes it again. An entry that is malformed, that tries to
 * override a built-in kind, or that would auto-preview untrusted markup in the
 * room is ignored (the returned remover is then a no-op).
 */
export function registerViewer(entry: ViewerEntry): () => void {
  const claimed = new Set(BUILT_IN_VIEWERS.flatMap((viewer) => viewer.renderKinds))
  if (!entryAcceptable(entry) || entry.renderKinds.some((kind) => claimed.has(kind))) return () => {}
  registered.push(entry)
  return () => {
    const index = registered.indexOf(entry)
    if (index >= 0) registered.splice(index, 1)
  }
}

/**
 * The viewer entry that may draw `renderKind` on `surface`, or `undefined` when
 * none may (the caller then shows a "preview not available" card).
 */
export function viewerFor(renderKind: string, surface: ViewerSurface): ViewerEntry | undefined {
  const matches = (entry: ViewerEntry) => entry.renderKinds.includes(renderKind) && entry.surfaces.includes(surface)
  return BUILT_IN_VIEWERS.find(matches) ?? registered.find(matches)
}

/** True when `renderKind` has any viewer on `surface`. */
export function surfaceAllows(renderKind: string, surface: ViewerSurface): boolean {
  return viewerFor(renderKind, surface) !== undefined
}

/**
 * Identity of one activation: an artifact id is only unique inside its thread
 * (two threads can both emit "report"), and a revision can change the markup, so
 * consent is bound to the id, the version and a hash of the exact content shown.
 */
export function viewerActivationKey(artifact: { id: string; version?: number; content: string }): string {
  let hash = 0x811c9dc5
  for (let index = 0; index < artifact.content.length; index += 1) {
    hash ^= artifact.content.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return `${artifact.id}\u0000${artifact.version ?? 0}\u0000${artifact.content.length}\u0000${hash.toString(16)}`
}
