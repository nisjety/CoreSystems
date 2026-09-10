/**
 * Artifact/canvas domain logic for the chat surface.
 *
 * Deliberately JSX-free and a LEAF in the module graph — it imports only
 * `./chat-types`, never `./chat-media-markdown` or `./chat-normalizers` (those
 * two already form a cycle between themselves). That keeps every rule here
 * directly unit-testable without pulling in the renderer.
 *
 * The backend contract this implements (model-gateway SSE):
 *   artifact: { id, kind, title, content, version }
 * where `kind` is free-form and `content` is either source text, Markdown, a
 * complete HTML document, or a `data:` URI (base64) for generated binaries.
 * The same `id` re-emitted with a higher `version` is an UPDATE of the same
 * artifact — that is how canvas iteration works — so every revision is kept in
 * `ChatArtifact.history` and the top-level fields always mirror the newest one.
 */
import {
  type ChatArtifact,
  type ChatArtifactVersion,
  type GeneratedFile,
  PROSE_ARTIFACT_KINDS,
} from './chat-types'

/** Which viewer renders an artifact. Derived from `kind`, then from `content`. */
export type ArtifactRenderKind = 'binary' | 'code' | 'document' | 'html' | 'image' | 'pdf' | 'text'

export type ParsedDataUri = {
  /** Payload byte length: decoded for base64, UTF-8 encoded for percent-escaped. */
  bytes: number
  /** Raw payload exactly as it appeared after the comma. */
  data: string
  isBase64: boolean
  /** Lowercased media type; `text/plain` when the URI omits it (RFC 2397). */
  mime: string
}

export type ArtifactFileMeta = {
  bytes: number
  downloadName: string
  /** `href` for an `<a download>` — a data URI or an https URL. */
  href: string
  mime: string
  mimeLabel: string
}

/** `kind` values that mean "a complete HTML document to preview". */
export const HTML_ARTIFACT_KINDS = new Set(['html', 'htm', 'webpage', 'website'])

/**
 * `kind` values whose `content` is source code. Includes the generic `code`
 * plus the language names the model tends to emit as the kind itself.
 */
export const CODE_ARTIFACT_KINDS = new Set([
  'bash', 'c', 'code', 'cpp', 'cs', 'csharp', 'css', 'dart', 'dockerfile', 'go',
  'graphql', 'java', 'javascript', 'js', 'json', 'jsx', 'kotlin', 'lua', 'patch',
  'php', 'py', 'python', 'r', 'rb', 'ruby', 'rs', 'rust', 'scala', 'script', 'sh',
  'shell', 'source', 'sql', 'swift', 'toml', 'ts', 'tsx', 'typescript', 'xml',
  'yaml', 'yml', 'zsh',
])

/**
 * `kind` values whose `content` is not viewable text — a generated file the
 * user downloads (`content` is a `data:` URI).
 */
export const BINARY_ARTIFACT_KINDS = new Set([
  'archive', 'attachment', 'binary', 'doc', 'docx', 'download', 'file', 'pdf',
  'ppt', 'pptx', 'spreadsheet', 'xls', 'xlsx', 'zip',
])

const DATA_URI_PATTERN = /^data:([^;,]*)((?:;[^;,]*)*),([\s\S]*)$/i

const IMAGE_CONTENT_PATTERN = /^(?:data:image\/|blob:)|^https?:\/\/\S+\.(?:png|jpe?g|gif|webp|svg|avif)(?:[?#]\S*)?$/i

/**
 * Extension → language id, used to recover the language of a `code` artifact
 * from its title (the backend puts the filename there, e.g. `"solve.py"`).
 */
const EXTENSION_LANGUAGES: Record<string, string> = {
  bash: 'bash', c: 'c', cc: 'cpp', cjs: 'javascript', cpp: 'cpp', cs: 'csharp',
  css: 'css', dart: 'dart', go: 'go', graphql: 'graphql', h: 'c', hpp: 'cpp',
  htm: 'html', html: 'html', java: 'java', js: 'javascript', json: 'json',
  jsx: 'javascript', kt: 'kotlin', lua: 'lua', md: 'markdown', mjs: 'javascript',
  php: 'php', py: 'python', r: 'r', rb: 'ruby', rs: 'rust', scala: 'scala',
  sh: 'shell', sql: 'sql', svg: 'xml', swift: 'swift', toml: 'toml',
  ts: 'typescript', tsx: 'typescript', txt: 'text', xml: 'xml', yaml: 'yaml',
  yml: 'yaml', zsh: 'shell',
}

/** Norwegian labels for the office/document MIME types the backend generates. */
const MIME_LABELS: Record<string, string> = {
  'application/json': 'JSON-fil',
  'application/msword': 'Word-dokument (DOC)',
  'application/pdf': 'PDF-dokument',
  'application/vnd.ms-excel': 'Excel-regneark (XLS)',
  'application/vnd.ms-powerpoint': 'PowerPoint-presentasjon (PPT)',
  'application/vnd.oasis.opendocument.spreadsheet': 'OpenDocument-regneark (ODS)',
  'application/vnd.oasis.opendocument.text': 'OpenDocument-tekst (ODT)',
  'application/vnd.openxmlformats-officedocument.presentationml.presentation': 'PowerPoint-presentasjon (PPTX)',
  'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet': 'Excel-regneark (XLSX)',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document': 'Word-dokument (DOCX)',
  'application/xml': 'XML-fil',
  'application/zip': 'ZIP-arkiv',
  'text/csv': 'CSV-fil',
  'text/html': 'HTML-dokument',
  'text/markdown': 'Markdown-dokument',
  'text/plain': 'Tekstfil',
}

const MIME_EXTENSIONS: Record<string, string> = {
  'application/json': 'json',
  'application/msword': 'doc',
  'application/pdf': 'pdf',
  'application/vnd.ms-excel': 'xls',
  'application/vnd.ms-powerpoint': 'ppt',
  'application/vnd.oasis.opendocument.spreadsheet': 'ods',
  'application/vnd.oasis.opendocument.text': 'odt',
  'application/vnd.openxmlformats-officedocument.presentationml.presentation': 'pptx',
  'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet': 'xlsx',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document': 'docx',
  'application/xml': 'xml',
  'application/zip': 'zip',
  'text/csv': 'csv',
  'text/html': 'html',
  'text/markdown': 'md',
  'text/plain': 'txt',
}

/**
 * Viewer descriptors, in both languages. These were Norwegian-only and
 * rendered straight into the panel header, which is acceptance criterion 9's
 * defect in visible text rather than in an attribute — the static guard in
 * chat-localization.test.ts only reads naming attributes, so nothing would
 * have caught it.
 */
const RENDER_KIND_LABELS: Record<ArtifactRenderKind, { no: string; en: string }> = {
  binary: { no: 'Fil', en: 'File' },
  code: { no: 'Kode', en: 'Code' },
  document: { no: 'Dokument', en: 'Document' },
  html: { no: 'Nettside', en: 'Web page' },
  image: { no: 'Bilde', en: 'Image' },
  pdf: { no: 'PDF', en: 'PDF' },
  text: { no: 'Tekst', en: 'Text' },
}

const RENDER_KIND_MIMES: Record<ArtifactRenderKind, string> = {
  binary: 'application/octet-stream',
  pdf: 'application/pdf',
  code: 'text/plain',
  document: 'text/markdown',
  html: 'text/html',
  image: 'image/png',
  text: 'text/plain',
}

/**
 * `src` for an image artifact. Content is either a full URI (data:/blob:/https:)
 * or bare base64 image bytes, which the backend emits as PNG — the single place
 * that assumption lives, shared by the renderers and the download control so the
 * two can never disagree about what an image artifact points at.
 */
export function imageArtifactSrc(content: string): string {
  if (/^(data:|blob:|https?:\/\/)/i.test(content)) return content
  return `data:image/png;base64,${content}`
}

/** True when an artifact's `content` is itself renderable image bytes/URL. */
export function looksLikeImageContent(content: string): boolean {
  return IMAGE_CONTENT_PATTERN.test(content.trim())
}

/**
 * True when `content` is a PDF the browser can actually open: an explicit
 * `application/pdf` data URI, a blob URL, or an http(s) URL ending in `.pdf`.
 *
 * Deliberately narrower than `looksLikeImageContent`, which wraps bare base64
 * in a data URI. Bare base64 on a `pdf` artifact could equally be prose, and
 * guessing wrong would put an empty viewer where a working download used to
 * be — so an unrecognised body keeps its file card.
 */
export function looksLikePdfContent(content: string): boolean {
  const value = content.trim()
  if (/^data:application\/pdf\b/i.test(value)) return true
  if (/^blob:/i.test(value)) return true
  return /^https?:\/\/\S+\.pdf(?:[?#]\S*)?$/i.test(value)
}

function normalizeKind(kind: string): string {
  return kind.trim().toLowerCase()
}

/**
 * Kind → renderer. Explicit kinds win; an unknown or missing kind falls back to
 * what the content actually is, so a `kind`-less artifact still renders (that is
 * the pre-existing behavior for text/markdown artifacts and must not regress).
 */
export function artifactRenderKind(artifact: ChatArtifact): ArtifactRenderKind {
  const kind = normalizeKind(artifact.kind)
  if (kind === 'image' || kind === 'screenshot' || kind === 'screen') return 'image'
  if (HTML_ARTIFACT_KINDS.has(kind)) return 'html'
  if (kind === 'document' || kind === 'markdown' || PROSE_ARTIFACT_KINDS.has(kind)) return 'document'
  if (CODE_ARTIFACT_KINDS.has(kind)) return 'code'
  // A pdf artifact the browser can open is a viewer, not a download. Checked
  // before the binary set, which still owns every unopenable body.
  if (kind === 'pdf' && looksLikePdfContent(artifact.content)) return 'pdf'
  if (BINARY_ARTIFACT_KINDS.has(kind)) return 'binary'
  if (looksLikeImageContent(artifact.content)) return 'image'
  if (parseDataUri(artifact.content)) return 'binary'
  return 'text'
}

/**
 * Descriptor for the viewer header. `tr` is passed in rather than the hook
 * being called here: this module is pure and has several non-component
 * callers, and a `useI18n()` inside it would be a hook call outside a
 * component root.
 */
export function artifactRenderKindLabel(
  kind: ArtifactRenderKind,
  tr: (noText: string, enText: string) => string,
): string {
  const label = RENDER_KIND_LABELS[kind]
  return tr(label.no, label.en)
}

/** Text-based kinds can be copied to the clipboard verbatim. */
export function isCopyableRenderKind(kind: ArtifactRenderKind): boolean {
  return kind === 'code' || kind === 'document' || kind === 'html' || kind === 'text'
}

/**
 * Language id for the code viewer. The backend carries it in the title
 * (`"solve.py"`); when the title has no usable extension the `kind` itself is
 * often the language (`kind: "python"`).
 */
export function artifactLanguage(artifact: ChatArtifact): string {
  const extension = artifact.title.trim().toLowerCase().match(/\.([a-z0-9+]+)$/)?.[1]
  const fromTitle = extension ? EXTENSION_LANGUAGES[extension] : undefined
  if (fromTitle) return fromTitle
  const kind = normalizeKind(artifact.kind)
  if (kind && kind !== 'code' && kind !== 'source' && kind !== 'script') {
    return EXTENSION_LANGUAGES[kind] ?? kind
  }
  return ''
}

/**
 * Parses a `data:` URI without ever throwing. Returns `null` for anything that
 * is not a well-formed data URI (no `data:` scheme, or no comma separating the
 * header from the payload) so callers can render an honest failure state
 * instead of a broken download link.
 */
export function parseDataUri(value: string): ParsedDataUri | null {
  const match = DATA_URI_PATTERN.exec(value.trim())
  if (!match) return null
  const params = (match[2] ?? '').split(';').map((param) => param.trim().toLowerCase())
  const isBase64 = params.includes('base64')
  const data = match[3] ?? ''
  return {
    bytes: dataPayloadBytes(data, isBase64),
    data,
    isBase64,
    mime: match[1]?.trim().toLowerCase() || 'text/plain',
  }
}

/** Byte length of a data-URI payload; 0 when the URI is malformed or empty. */
export function dataUriByteSize(value: string): number {
  return parseDataUri(value)?.bytes ?? 0
}

function dataPayloadBytes(data: string, isBase64: boolean): number {
  if (!data) return 0
  if (isBase64) {
    const body = data.replace(/[\s=]/g, '')
    const padding = data.endsWith('==') ? 2 : data.endsWith('=') ? 1 : 0
    return Math.max(0, Math.floor(((body.length + padding) * 3) / 4) - padding)
  }
  try {
    return new TextEncoder().encode(decodeURIComponent(data)).length
  } catch {
    // Percent-escapes can be malformed (a truncated stream); measuring the raw
    // text is a lower bound, which beats throwing inside a render.
    return new TextEncoder().encode(data).length
  }
}

/** Human-readable, Norwegian label for a MIME type (xlsx/docx/pdf/…). */
export function friendlyMimeLabel(mime: string | null | undefined): string {
  const normalized = mime?.trim().toLowerCase().split(';')[0] ?? ''
  if (!normalized) return 'Ukjent filtype'
  const known = MIME_LABELS[normalized]
  if (known) return known
  const [type, subtype = ''] = normalized.split('/')
  if (type === 'image' && subtype) return `${subtype.replace('+xml', '').toUpperCase()}-bilde`
  if (type === 'audio' && subtype) return `${subtype.toUpperCase()}-lyd`
  if (type === 'video' && subtype) return `${subtype.toUpperCase()}-video`
  if (type === 'text' && subtype) return `${subtype.toUpperCase()}-tekst`
  return normalized
}

/** Extension for a download filename, inferred from the MIME type. */
export function mimeFileExtension(mime: string | null | undefined): string | null {
  const normalized = mime?.trim().toLowerCase().split(';')[0] ?? ''
  if (!normalized) return null
  const known = MIME_EXTENSIONS[normalized]
  if (known) return known
  const subtype = normalized.split('/')[1]
  if (!subtype) return null
  const sanitized = subtype.replace('+xml', '').replace(/[^a-z0-9]/g, '')
  return sanitized || null
}

function artifactSlug(value: string): string {
  const slug = value
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80)
  return slug || 'artefakt'
}

/**
 * Filename for the Download control. A title that already looks like a filename
 * is used verbatim (the backend puts real filenames there); otherwise the title
 * is slugified and given an extension derived from the MIME type or the kind.
 */
export function artifactDownloadName(
  artifact: ChatArtifact,
  renderKind: ArtifactRenderKind,
  file?: GeneratedFile,
): string {
  if (file?.name.trim()) return file.name.trim()
  const title = artifact.title.trim()
  if (/\.[a-z0-9]{1,8}$/i.test(title)) return title
  const mime = file?.mime ?? parseDataUri(artifact.content)?.mime ?? RENDER_KIND_MIMES[renderKind]
  const extension = mimeFileExtension(mime) ??
    (renderKind === 'code' ? codeExtension(artifactLanguage(artifact)) : 'txt')
  return `${artifactSlug(title || artifact.kind)}.${extension}`
}

function codeExtension(language: string): string {
  for (const [extension, mapped] of Object.entries(EXTENSION_LANGUAGES)) {
    if (mapped === language) return extension
  }
  return 'txt'
}

/**
 * Everything the Download control and the binary viewer need, or `null` when
 * the artifact carries nothing downloadable (malformed/empty `content` and no
 * paired attachment) — the caller must then render "kunne ikke lastes".
 */
export function artifactFileMeta(
  artifact: ChatArtifact,
  renderKind: ArtifactRenderKind,
  file?: GeneratedFile,
): ArtifactFileMeta | null {
  if (file?.url) {
    const parsed = parseDataUri(file.url)
    const mime = file.mime || parsed?.mime || 'application/octet-stream'
    return {
      bytes: file.size > 0 ? file.size : parsed?.bytes ?? 0,
      downloadName: artifactDownloadName(artifact, renderKind, file),
      href: file.url,
      mime,
      mimeLabel: friendlyMimeLabel(mime),
    }
  }
  const content = artifact.content.trim()
  if (!content) return null
  const parsed = parseDataUri(content)
  if (parsed) {
    return {
      bytes: parsed.bytes,
      downloadName: artifactDownloadName(artifact, renderKind),
      href: content,
      mime: parsed.mime,
      mimeLabel: friendlyMimeLabel(parsed.mime),
    }
  }
  if (/^https?:\/\//i.test(content)) {
    return {
      bytes: 0,
      downloadName: artifactDownloadName(artifact, renderKind),
      href: content,
      mime: RENDER_KIND_MIMES[renderKind],
      mimeLabel: friendlyMimeLabel(RENDER_KIND_MIMES[renderKind]),
    }
  }
  if (renderKind === 'image') {
    // Bare base64 image bytes: resolve them through the same convention the
    // renderer uses so the download matches exactly what is on screen.
    const src = imageArtifactSrc(content)
    const parsed = parseDataUri(src)
    if (!parsed) return null
    return {
      bytes: parsed.bytes,
      downloadName: artifactDownloadName(artifact, renderKind),
      href: src,
      mime: parsed.mime,
      mimeLabel: friendlyMimeLabel(parsed.mime),
    }
  }
  // A binary artifact whose content is neither a data URI nor a URL has no bytes
  // to hand over. Offering a download of the literal string would deliver a
  // corrupt file, so report nothing and let the caller say "kunne ikke lastes".
  if (renderKind === 'binary') return null
  // Plain text (code / Markdown / HTML): synthesize a data URI so `<a download>`
  // works without a Blob URL and its revoke lifecycle.
  const mime = RENDER_KIND_MIMES[renderKind]
  return {
    bytes: new TextEncoder().encode(artifact.content).length,
    downloadName: artifactDownloadName(artifact, renderKind),
    href: `data:${mime};charset=utf-8,${encodeURIComponent(artifact.content)}`,
    mime,
    mimeLabel: friendlyMimeLabel(mime),
  }
}

// ── Version history ──────────────────────────────────────────────────────────

function versionOf(artifact: ChatArtifact): ChatArtifactVersion {
  return { content: artifact.content, title: artifact.title, version: artifact.version }
}

export function isChatArtifactVersion(value: unknown): value is ChatArtifactVersion {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.content === 'string' &&
    typeof record.title === 'string' &&
    typeof record.version === 'number'
  )
}

/**
 * Revision list for an artifact, oldest → newest. Artifacts restored from an
 * older thread snapshot have no `history`, so the current fields stand in as a
 * single revision — the viewer never has to special-case that.
 */
export function artifactVersions(artifact: ChatArtifact): ChatArtifactVersion[] {
  const history = (artifact.history ?? []).filter(isChatArtifactVersion)
  if (history.length === 0) return [versionOf(artifact)]
  return [...history].sort((left, right) => left.version - right.version)
}

/** The revision the viewer shows by default: the newest one. */
export function latestArtifactVersion(artifact: ChatArtifact): ChatArtifactVersion {
  const versions = artifactVersions(artifact)
  return versions[versions.length - 1] ?? versionOf(artifact)
}

/** Revision matching `version`, or the newest one when there is no such revision. */
export function artifactVersionAt(artifact: ChatArtifact, version: number | null): ChatArtifactVersion {
  if (version == null) return latestArtifactVersion(artifact)
  return artifactVersions(artifact).find((entry) => entry.version === version) ??
    latestArtifactVersion(artifact)
}

/**
 * Folds a newly streamed artifact into the one already held for that id.
 *
 * A repeat `id` is an UPDATE, never a new list entry: the revision is appended
 * to `history` and the top-level fields mirror the newest revision so every
 * existing reader (message chips, inline image previews, the agent-screen
 * figure) keeps seeing the current content with no change.
 *
 * `version` is the revision key. Re-emitting the SAME version replaces that
 * revision's content rather than appending — that is how a progressively filled
 * or corrected revision behaves, and it means the backend MUST increment
 * `version` for each rewrite it wants the user to be able to step back to.
 * An out-of-order older revision is recorded in history but does not regress
 * what the panel shows.
 */
export function mergeArtifactVersion(existing: ChatArtifact | undefined, incoming: ChatArtifact): ChatArtifact {
  const revision = versionOf(incoming)
  if (!existing) return { ...incoming, history: [revision] }

  const previous = artifactVersions(existing)
  const history = [...previous.filter((entry) => entry.version !== incoming.version), revision]
    .sort((left, right) => left.version - right.version)
  const newest = history[history.length - 1] ?? revision
  return {
    ...existing,
    ...incoming,
    content: newest.content,
    history,
    title: newest.title,
    version: newest.version,
  }
}

/** Finds the artifact with this id anywhere in a set of per-turn artifact lists. */
export function findArtifactById(lists: Array<ChatArtifact[] | undefined>, id: string): ChatArtifact | undefined {
  let found: ChatArtifact | undefined
  for (const list of lists) {
    for (const artifact of list ?? []) {
      if (artifact.id !== id) continue
      // Later turns win: the newest carrier of the id holds the fullest history.
      if (!found || artifact.version >= found.version) found = artifact
    }
  }
  return found
}

/**
 * An artifact the backend announced but whose payload never arrived (or was
 * lost when a thread snapshot was trimmed). The viewer must say
 * "kunne ikke lastes" instead of rendering an empty box.
 */
export function artifactContentMissing(content: string): boolean {
  return content.trim().length === 0
}
