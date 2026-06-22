import {
  AlertCircle,
  CheckCircle2,
  Clock3,
  Square,
} from 'lucide-solid'
import {
  type JSX,
} from 'solid-js'
import {
  createId,
} from './chat-normalizers'
import {
  type AgentTaskStep,
  type AgentTaskStepSection,
  type ArtifactPanelItem,
  type ChatArtifact,
  type ChatGroundingFact,
  type ChatGroundingGraph,
  type ChatGroundingGraphNode,
  type ChatGroundingSource,
  type ChatKnowledgeGrounding,
  type ChatTurn,
  type EvidenceSource,
  type GeneratedFile,
  type GeneratedImagePreview,
  type MarkdownBlock,
  PROSE_ARTIFACT_KINDS,
  type TaskStepStatus,
} from './chat-types'

export function collectArtifactItems(turns: ChatTurn[]): ArtifactPanelItem[] {
  const byId = new Map<string, ArtifactPanelItem>()
  for (const turn of turns) {
    for (const artifact of turn.artifacts ?? []) {
      const existing = byId.get(artifact.id)
      const item = {
        artifact,
        file: selectGeneratedFileForArtifact(artifact, turn),
        turn,
      }
      if (!existing || artifact.version >= existing.artifact.version) byId.set(artifact.id, item)
    }
  }
  return [...byId.values()]
}

export function collectArtifacts(turns: ChatTurn[]): ChatArtifact[] {
  return collectArtifactItems(turns).map((item) => item.artifact)
}

export function selectGeneratedFileForArtifact(artifact: ChatArtifact, turn: ChatTurn): GeneratedFile | undefined {
  const imageFiles = (turn.files ?? []).filter(isGeneratedImageFile)
  if (imageFiles.length === 0) return undefined
  const exact = imageFiles.find((file) => file.id === artifact.id || file.url === artifact.content)
  if (exact) return exact
  if (!isImageArtifact(artifact)) return undefined
  const imageArtifacts = (turn.artifacts ?? []).filter(isImageArtifact)
  const imageIndex = imageArtifacts.findIndex((item) => item.id === artifact.id)
  if (imageFiles.length === imageArtifacts.length && imageIndex >= 0) return imageFiles[imageIndex]
  return imageFiles.length === 1 ? imageFiles[0] : undefined
}

export function collectEvidenceSources(turns: ChatTurn[]): EvidenceSource[] {
  const seen = new Set<string>()
  const result: EvidenceSource[] = []
  for (const turn of turns) {
    for (const source of turn.grounding?.sources ?? []) {
      const key = `knowledge:${source.documentId || source.id}`
      if (seen.has(key)) continue
      seen.add(key)
      result.push(source)
    }
    for (const citation of turn.citations ?? []) {
      const key = `web:${citation.url || citation.id}`
      if (seen.has(key)) continue
      seen.add(key)
      result.push({ ...citation, kind: 'web' })
    }
  }
  return result
}

export function groupTaskSteps(steps: AgentTaskStep[]): AgentTaskStepSection[] {
  const sections = new Map<string, AgentTaskStepSection>()
  for (const step of steps) {
    const id = step.turnId ?? 'session'
    const existing = sections.get(id)
    if (existing) {
      existing.steps = [...existing.steps, step]
      continue
    }
    sections.set(id, {
      id,
      title: step.turnTitle ? `Answer: ${step.turnTitle}` : 'Session activity',
      createdAt: step.createdAt,
      steps: [step],
    })
  }
  return [...sections.values()]
}

export function collectLatestGrounding(turns: ChatTurn[]): ChatKnowledgeGrounding | null {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const grounding = turns[index]?.grounding
    if (grounding) return grounding
  }
  return null
}

export function selectLatestImageArtifact(turns: ChatTurn[]): ChatArtifact | null {
  const images = collectArtifacts(turns).filter((artifact) => artifact.kind.toLowerCase() === 'image')
  return images.at(-1) ?? null
}

export function buildGeneratedImagePreviews(
  files: GeneratedFile[],
  artifacts: ChatArtifact[],
  content: string,
): GeneratedImagePreview[] {
  const imageFiles = files.filter(isGeneratedImageFile)
  const imageArtifacts = artifacts.filter(isImageArtifact)
  const filePreviews = imageFiles.map((file, index) => {
    const pairedArtifact = imageArtifacts.find((artifact) => artifact.id === file.id) ?? (
      imageArtifacts.length === imageFiles.length || imageArtifacts.length === 1
        ? imageArtifacts[index] ?? imageArtifacts[0]
        : undefined
    )
    const title = generatedImageTitle(pairedArtifact?.title, content, file.name)
    return {
      id: `file:${file.id}`,
      title,
      src: file.url,
      downloadName: generatedImageDownloadName(title, file, file.url),
      size: file.size,
      artifactId: pairedArtifact?.id,
    }
  })
  const pairedArtifactIds = filePreviews
    .map((preview) => preview.artifactId)
    .filter((id): id is string => Boolean(id))
  const artifactPreviews = imageArtifacts
    .filter((artifact) => !pairedArtifactIds.includes(artifact.id))
    .map((artifact) => {
      const src = imageArtifactSrc(artifact.content)
      const title = generatedImageTitle(artifact.title, content)
      return {
        id: `artifact:${artifact.id}`,
        title,
        src,
        downloadName: generatedImageDownloadName(title, undefined, src),
        size: 0,
        artifactId: artifact.id,
      }
    })
  return [...filePreviews, ...artifactPreviews]
}

export function isGeneratedImageFile(file: GeneratedFile): boolean {
  return file.mime.toLowerCase().startsWith('image/') || isImageUrl(file.url) || imageFileExtension(file.name) !== null
}

export function isImageArtifact(artifact: ChatArtifact): boolean {
  return artifact.kind.toLowerCase() === 'image' || inferArtifactKind(artifact.content) === 'image'
}

export function buildArtifactImageSpecs(item: ArtifactPanelItem, dimensions: string | null): Array<{ label: string; value: string }> {
  const src = imageArtifactSrc(item.artifact.content)
  const mime = item.file?.mime || dataUrlMime(src)
  const estimatedSize = item.file?.size ?? estimateDataUrlBytes(src)
  const model = item.turn.modelUsed ?? item.turn.model
  return [
    { label: 'Prompt', value: generatedImageTitle(item.artifact.title, item.turn.content, item.file?.name) },
    { label: 'Dimensions', value: dimensions ?? 'Loading...' },
    { label: 'File size', value: estimatedSize > 0 ? formatBytes(estimatedSize) : 'Not reported' },
    { label: 'Format', value: imageFormatLabel(mime, item.file?.name) },
    { label: 'MIME type', value: mime ?? 'Not reported' },
    { label: 'Model', value: model ? prettyModel(model) : 'Not reported' },
    { label: 'Quality', value: imageQualityLabel(item.turn) },
    { label: 'Version', value: `v${Math.max(1, item.artifact.version)}` },
    { label: 'Created', value: formatRelative(item.turn.createdAt) },
    ...(item.turn.latencyMs != null ? [{ label: 'Latency', value: formatLatency(item.turn.latencyMs) }] : []),
  ]
}

export function generatedImageTitle(artifactTitle?: string, content?: string, fileName?: string): string {
  return (
    meaningfulImageTitle(artifactTitle) ??
    extractGeneratedImagePrompt(content) ??
    meaningfulImageTitle(fileName ? titleFromFileName(fileName) : undefined) ??
    'Generated image'
  )
}

export function imageGenerationDisplayContent(content: string): string {
  const trimmed = content.trim()
  return /^(?:i\s+)?generated an image artifact:\s*\S+\s+for prompt:\s*.+$/i.test(trimmed) ? '' : content
}

export function meaningfulImageTitle(value?: string): string | null {
  const title = value?.trim()
  if (!title) return null
  return isGenericImageTitle(title) ? null : title
}

export function isGenericImageTitle(value: string): boolean {
  const normalized = value.trim().toLowerCase()
  return normalized === 'image' ||
    normalized === 'artifact' ||
    normalized === 'generated image' ||
    normalized === 'generated-image' ||
    normalized === 'generated-image.png'
}

export function extractGeneratedImagePrompt(content?: string): string | null {
  const prompt = content?.match(/\bfor prompt:\s*(.+)$/i)?.[1]?.trim()
  return prompt ? prompt.replace(/^["']|["']$/g, '') : null
}

export function titleFromFileName(name: string): string {
  return name
    .replace(/\.[a-z0-9]+$/i, '')
    .replace(/[-_]+/g, ' ')
    .trim()
}

export function generatedImageDownloadName(title: string, file?: GeneratedFile, src?: string): string {
  return `${slugFileName(title)}.${imageDownloadExtension(file, src)}`
}

export function slugFileName(value: string): string {
  const slug = value
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80)
  return slug || 'generated-image'
}

export function imageDownloadExtension(file?: GeneratedFile, src?: string): string {
  const mimeExtension = imageMimeExtension(file?.mime)
  if (mimeExtension) return mimeExtension
  const fileExtension = file ? imageFileExtension(file.name) : null
  if (fileExtension) return fileExtension
  const dataUrlExtension = src?.match(/^data:image\/([^;,]+)/i)?.[1]?.toLowerCase()
  if (dataUrlExtension) return normalizeImageExtension(dataUrlExtension)
  return 'png'
}

export function imageFormatLabel(mime?: string | null, fileName?: string): string {
  const extension = imageMimeExtension(mime ?? undefined) ?? (fileName ? imageFileExtension(fileName) : null)
  return extension ? extension.toUpperCase() : 'Not reported'
}

export function imageQualityLabel(turn: ChatTurn): string {
  for (const call of turn.toolCalls ?? []) {
    const args = objectValue(call.args)
    const quality = stringValue(args?.quality) ?? stringValue(args?.image_quality)
    if (quality) return capitalize(quality)
  }
  return 'Not reported'
}

export function dataUrlMime(src: string): string | null {
  return src.match(/^data:([^;,]+)/i)?.[1]?.toLowerCase() ?? null
}

export function estimateDataUrlBytes(src: string): number {
  const base64 = src.match(/^data:[^,]+;base64,(.+)$/i)?.[1]
  if (!base64) return 0
  const padding = base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0
  return Math.max(0, Math.floor((base64.length * 3) / 4) - padding)
}

export function imageMimeExtension(mime?: string): string | null {
  const subtype = mime?.toLowerCase().match(/^image\/([^;]+)/)?.[1]
  return subtype ? normalizeImageExtension(subtype) : null
}

export function imageFileExtension(name: string): string | null {
  const extension = name.match(/\.([a-z0-9]+)$/i)?.[1]?.toLowerCase()
  return extension && ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'avif'].includes(extension)
    ? normalizeImageExtension(extension)
    : null
}

export function normalizeImageExtension(extension: string): string {
  if (extension === 'jpeg') return 'jpg'
  if (extension === 'svg+xml') return 'svg'
  const sanitized = extension.replace(/[^a-z0-9]/g, '')
  return sanitized || 'png'
}

export function isImageUrl(url: string): boolean {
  return /^data:image\//i.test(url) || /^blob:/i.test(url) || /^https?:\/\/\S+\.(?:png|jpe?g|gif|webp|svg|avif)(?:[?#]\S*)?$/i.test(url)
}

export function normalizeGrounding(value: unknown): ChatKnowledgeGrounding | undefined {
  const root = objectValue(value)
  if (!root) return undefined
  const sources = Array.isArray(root.sources)
    ? root.sources.map(normalizeGroundingSource).filter((source): source is ChatGroundingSource => Boolean(source))
    : []
  const facts = Array.isArray(root.facts)
    ? root.facts.map(normalizeGroundingFact).filter((fact): fact is ChatGroundingFact => Boolean(fact))
    : []
  const graph = normalizeGroundingGraph(root.graph)
  return {
    mode: root.mode === 'hybrid' ? 'hybrid' : 'retrieve',
    query: stringValue(root.query) ?? '',
    traceId: stringValue(root.traceId) ?? stringValue(root.trace_id),
    lowConfidence: Boolean(root.lowConfidence ?? root.low_confidence),
    factCount: numberValue(root.factCount) ?? numberValue(root.fact_count) ?? facts.length,
    sourceCount: numberValue(root.sourceCount) ?? numberValue(root.source_count) ?? sources.length,
    facts,
    sources,
    graph,
  }
}

export function normalizeGroundingSource(value: unknown): ChatGroundingSource | null {
  const source = objectValue(value)
  if (!source) return null
  return {
    id: stringValue(source.id) ?? createId('source'),
    kind: 'knowledge',
    title: stringValue(source.title) ?? 'Knowledge source',
    snippet: stringValue(source.snippet) ?? '',
    provider: stringValue(source.provider) ?? 'knowledge',
    sourceType: stringValue(source.sourceType) ?? stringValue(source.source_type) ?? 'document',
    documentId: stringValue(source.documentId) ?? stringValue(source.document_id) ?? '',
    href: stringValue(source.href) ?? '#',
    score: numberValue(source.score) ?? 0,
  }
}

export function normalizeGroundingFact(value: unknown): ChatGroundingFact | null {
  const fact = objectValue(value)
  if (!fact) return null
  return {
    knowledgeId: stringValue(fact.knowledgeId) ?? stringValue(fact.knowledge_id) ?? '',
    documentId: stringValue(fact.documentId) ?? stringValue(fact.document_id) ?? '',
    text: stringValue(fact.text) ?? '',
    score: numberValue(fact.score) ?? 0,
    sourceTitle: stringValue(fact.sourceTitle) ?? stringValue(fact.source_title) ?? '',
    sourceType: stringValue(fact.sourceType) ?? stringValue(fact.source_type) ?? '',
    provider: stringValue(fact.provider) ?? '',
    chunkIndex: numberValue(fact.chunkIndex) ?? numberValue(fact.chunk_index) ?? 0,
  }
}

export function normalizeGroundingGraph(value: unknown): ChatGroundingGraph | undefined {
  const graph = objectValue(value)
  if (!graph) return undefined
  const nodes = Array.isArray(graph.nodes)
    ? graph.nodes.map((item) => {
        const node = objectValue(item)
        if (!node) return null
        return {
          id: stringValue(node.id) ?? createId('node'),
          label: stringValue(node.label) ?? stringValue(node.name) ?? 'Node',
          kind: stringValue(node.kind) ?? 'entity',
        }
      }).filter((node): node is ChatGroundingGraphNode => Boolean(node))
    : []
  const summaries = Array.isArray(graph.communitySummaries)
    ? graph.communitySummaries.filter((item): item is string => typeof item === 'string')
    : Array.isArray(graph.community_summaries)
      ? graph.community_summaries.filter((item): item is string => typeof item === 'string')
      : []
  return {
    traceId: stringValue(graph.traceId) ?? stringValue(graph.trace_id),
    communitySummaries: summaries,
    edgeCount: numberValue(graph.edgeCount) ?? numberValue(graph.edge_count) ?? 0,
    nodes,
  }
}

export function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' ? value as Record<string, unknown> : undefined
}

export function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

export function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

export function normalizeTaskStatus(status?: string): TaskStepStatus {
  if (status === 'done' || status === 'active' || status === 'waiting' || status === 'error' || status === 'stopped') return status
  if (status === 'running') return 'active'
  if (status === 'failed') return 'error'
  return 'active'
}

export function inferArtifactKind(content: string) {
  if (content.startsWith('data:image') || /^https?:\/\/\S+\.(?:png|jpe?g|gif|webp|svg)/i.test(content)) return 'image'
  return 'text'
}

export function artifactTitle(kind: string) {
  if (kind === 'image') return 'Generated image'
  if (PROSE_ARTIFACT_KINDS.has(kind)) return 'Generated document'
  return 'Artifact'
}

export function imageArtifactSrc(content: string) {
  if (/^(data:|blob:|https?:\/\/)/i.test(content)) return content
  return `data:image/png;base64,${content}`
}

export function isValidUrl(url: string): boolean {
  try {
    new URL(url)
    return true
  } catch {
    return false
  }
}

export function domId(value: string): string {
  return value.replace(/[^A-Za-z0-9_-]+/g, '-')
}

export function hostname(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

export function createChatTitle(turns: ChatTurn[]) {
  const first = turns.find((turn) => turn.role === 'user')
  if (!first) return 'Velion Chat'
  return createPreview(first.content, 48)
}

export function createPreview(content: string, max = 34) {
  const clean = content.replace(/\s+/g, ' ').trim()
  return clean.length > max ? `${clean.slice(0, max - 1)}...` : clean
}

export function truncateText(content: string, max: number): string {
  const clean = content.replace(/\s+/g, ' ').trim()
  return clean.length > max ? `${clean.slice(0, max - 1)}...` : clean
}

export function capitalize(value: string): string {
  return value ? `${value[0]?.toUpperCase() ?? ''}${value.slice(1)}` : value
}

export function shouldShowDateDivider(previous: ChatTurn | undefined, current: ChatTurn) {
  return !previous || dayKey(previous.createdAt) !== dayKey(current.createdAt)
}

export function dayKey(value: string) {
  return new Date(value).toDateString()
}

export function formatDayLabel(value: string) {
  const date = new Date(value)
  const today = new Date()
  const yesterday = new Date(today)
  yesterday.setDate(today.getDate() - 1)
  if (date.toDateString() === today.toDateString()) return 'Today'
  if (date.toDateString() === yesterday.toDateString()) return 'Yesterday'
  return new Intl.DateTimeFormat('en', { weekday: 'long', month: 'short', day: 'numeric' }).format(date)
}

export function formatRelative(value: string) {
  const timestamp = new Date(value).getTime()
  if (Number.isNaN(timestamp)) return ''
  const minutes = Math.max(0, Math.round((Date.now() - timestamp) / 60000))
  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric' }).format(new Date(value))
}

export function formatTime(value: string) {
  return new Intl.DateTimeFormat('en', { hour: '2-digit', minute: '2-digit' }).format(new Date(value))
}

export function formatLatency(ms: number) {
  if (ms < 1000) return `${Math.round(ms)} ms`
  return `${(ms / 1000).toFixed(1)} s`
}

export function prettyModel(model: string) {
  const lower = model.toLowerCase()
  if (lower.includes('gpt-image-1')) return 'GPT Image 1'
  if (lower.includes('gpt-image')) return 'GPT Image'
  if (lower.includes('gpt-4o-mini')) return 'GPT-4o Mini'
  if (lower.includes('gpt-4.1')) return 'GPT-4.1'
  if (lower.includes('gpt-4o')) return 'GPT-4o'
  if (lower.includes('claude')) return 'Claude Sonnet'
  if (lower.includes('reason')) return 'Velion Reasoner'
  return model.length > 22 ? `${model.slice(0, 22)}...` : model
}

export function formatBytes(bytes: number): string {
  if (!bytes || bytes < 0) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function formatToolArgs(args: unknown): string {
  if (args == null) return ''
  if (typeof args === 'string') return args
  try {
    return JSON.stringify(args, null, 2)
  } catch {
    return String(args)
  }
}

export function readAloud(text: string) {
  if (!('speechSynthesis' in window)) return
  const clean = text.trim()
  if (!clean) return
  window.speechSynthesis.cancel()
  const utterance = new SpeechSynthesisUtterance(clean)
  utterance.lang = 'nb-NO'
  window.speechSynthesis.speak(utterance)
}

export function getTaskStepIcon(status: TaskStepStatus): { className: string; node: JSX.Element } {
  if (status === 'done') return { className: 'is-done', node: <CheckCircle2 size={14} /> }
  if (status === 'active') return { className: 'is-active', node: <Clock3 size={14} /> }
  if (status === 'waiting') return { className: 'is-waiting', node: <AlertCircle size={14} /> }
  if (status === 'error') return { className: 'is-error', node: <AlertCircle size={14} /> }
  return { className: 'is-stopped', node: <Square size={11} /> }
}

export function parseMarkdownBlocks(markdown: string): MarkdownBlock[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const blocks: MarkdownBlock[] = []
  let index = 0

  while (index < lines.length) {
    const line = lines[index] ?? ''
    if (!line.trim()) {
      index += 1
      continue
    }

    const fence = /^```(.*)$/.exec(line.trim())
    if (fence) {
      const code: string[] = []
      const lang = fence[1]?.trim() ?? ''
      index += 1
      while (index < lines.length && !/^```/.test(lines[index]?.trim() ?? '')) {
        code.push(lines[index] ?? '')
        index += 1
      }
      if (index < lines.length) index += 1
      blocks.push({ kind: 'code', lang, text: code.join('\n') })
      continue
    }

    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      blocks.push({ kind: 'hr' })
      index += 1
      continue
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line)
    if (heading) {
      const marks = heading[1] ?? ''
      const text = heading[2] ?? ''
      blocks.push({
        kind: 'heading',
        level: marks.length <= 1 ? 1 : marks.length === 2 ? 2 : 3,
        text,
      })
      index += 1
      continue
    }

    if (/^\s*>\s?/.test(line)) {
      const quoted: string[] = []
      while (index < lines.length && /^\s*>\s?/.test(lines[index] ?? '')) {
        quoted.push((lines[index] ?? '').replace(/^\s*>\s?/, ''))
        index += 1
      }
      blocks.push({ kind: 'quote', text: quoted.join('\n') })
      continue
    }

    const list = parseList(lines, index)
    if (list) {
      blocks.push(list.block)
      index = list.next
      continue
    }

    const paragraph: string[] = []
    while (index < lines.length && lines[index]?.trim() && !isMarkdownBlockStart(lines[index] ?? '')) {
      paragraph.push(lines[index] ?? '')
      index += 1
    }
    blocks.push({ kind: 'paragraph', text: paragraph.join('\n').trim() })
  }

  return blocks
}

export function parseList(lines: string[], start: number): { block: Extract<MarkdownBlock, { kind: 'list' }>; next: number } | null {
  const first = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(lines[start] ?? '')
  if (!first) return null
  const ordered = /\d+[.)]/.test(first[2] ?? '')
  const items: string[] = []
  let index = start
  while (index < lines.length) {
    const match = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(lines[index] ?? '')
    if (!match || /\d+[.)]/.test(match[2] ?? '') !== ordered) break
    items.push(match[3] ?? '')
    index += 1
  }
  return { block: { kind: 'list', ordered, items }, next: index }
}

export function isMarkdownBlockStart(line: string) {
  return /^\s*```/.test(line)
    || /^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)
    || /^#{1,6}\s+/.test(line)
    || /^\s*>\s?/.test(line)
    || /^(\s*)([-*+]|\d+[.)])\s+/.test(line)
}

export function parseInline(text: string): Array<string | JSX.Element> {
  const nodes: Array<string | JSX.Element> = []
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g
  let cursor = 0
  for (const match of text.matchAll(pattern)) {
    if (match.index == null) continue
    if (match.index > cursor) nodes.push(text.slice(cursor, match.index))
    const token = match[0]
    if (token.startsWith('`')) {
      nodes.push(<code>{token.slice(1, -1)}</code>)
    } else if (token.startsWith('**')) {
      nodes.push(<strong>{token.slice(2, -2)}</strong>)
    } else if (token.startsWith('*')) {
      nodes.push(<em>{token.slice(1, -1)}</em>)
    } else {
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token)
      nodes.push(link ? <a href={link[2]} target="_blank" rel="noopener noreferrer">{link[1]}</a> : token)
    }
    cursor = match.index + token.length
  }
  if (cursor < text.length) nodes.push(text.slice(cursor))
  return nodes
}
