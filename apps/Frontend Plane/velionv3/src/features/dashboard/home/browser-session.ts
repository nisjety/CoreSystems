import type {
  BrowserObservation,
  BrowserSession,
  BrowserTimelineEntry,
  BrowserSessionResponse,
} from '@/shared/api/browser-client'
import { gatewayBaseUrl } from '@/shared/api/config'
import { hostnameOf, type ScrapePreview } from './knowledge-preview'

export type BrowserSurfaceMode = BrowserSession['renderMode']

export type BrowserDomNode = {
  id: string
  kind: string
  selector?: string
  text: string
}

export type BrowserConsoleEntry = {
  level: string
  text: string
}

export type BrowserNetworkEntry = {
  content_type?: string | null
  method: string
  status: number
  url: string
}

export type BrowserTimelineViewEntry = BrowserTimelineEntry & {
  screenshotUrl?: string | null
  visualObservationUrl?: string | null
}

export type BrowserSessionViewModel = {
  capabilities: string[]
  commentAnchors: Array<{
    id: string
    label: string
    x: number
    y: number
  }>
  consoleEntries: BrowserConsoleEntry[]
  domNodes: BrowserDomNode[]
  degradedReason?: string | null
  frameArtifactId?: string | null
  frameMediaType?: string | null
  frameUrl?: string | null
  host: string
  networkEntries: BrowserNetworkEntry[]
  nodeCount?: number | null
  observation?: BrowserObservation | null
  policyDenials: string[]
  profileId: string | null
  profileLabel: string
  profileScope: BrowserSession['profile']['scope']
  profileStorage: BrowserSession['profile']['storage']
  renderMode: BrowserSurfaceMode
  screenshotArtifactId?: string | null
  sessionId?: string
  sourceLabel: string
  status: BrowserSession['status']
  timeline: BrowserTimelineViewEntry[]
  title: string
  url: string
  visualObservationArtifactId?: string | null
  visualObservationUrl?: string | null
  viewport: {
    height: number
    width: number
  }
  zdr: boolean
}

const fallbackViewport = { width: 1280, height: 800 }

export function browserSessionFromPreview(preview: ScrapePreview): BrowserSessionViewModel {
  const mode = preview.browserSession?.session.renderMode ?? 'readability_fallback'
  const observation = preview.browserSession?.observation ?? null
  const session = preview.browserSession?.session
  const frameUrl = resolveBrowserArtifactUrl(session?.frame?.url)
  const timeline = normalizeTimeline(session?.timeline)
  const visualObservationArtifactId =
    session?.visual?.observationArtifactId ?? observation?.visual_observation_artifact_id ?? null
  const visualObservationUrl =
    resolveBrowserArtifactUrl(session?.visual?.observationUrl)
    ?? resolveVisualObservationUrl(session?.id, visualObservationArtifactId)

  return {
    capabilities: session?.capabilities ?? ['annotate', 'dom_select', 'knowledge_ingest'],
    commentAnchors: [
      {
        id: 'comment-1',
        label: '1',
        x: 0.07,
        y: 0.18,
      },
    ],
    consoleEntries: observation?.console_summary ?? [],
    domNodes: observation?.dom_summary
      ? domNodesFromObservation(observation)
      : domNodesFromPreview(preview),
    degradedReason: preview.browserSessionError ?? null,
    frameArtifactId: session?.frame?.artifactId ?? observation?.screenshot_artifact_id ?? null,
    frameMediaType: session?.frame?.mediaType ?? null,
    frameUrl,
    host: hostnameOf(session?.url ?? preview.url),
    networkEntries: observation?.network_summary ?? [],
    nodeCount: observation?.dom_summary?.node_count ?? null,
    observation,
    policyDenials: observation?.policy_denials ?? [],
    profileId: session?.profile.id ?? null,
    profileLabel: profileLabel(session),
    profileScope: session?.profile.scope ?? 'run_scoped',
    profileStorage: session?.profile.storage ?? 'isolated',
    renderMode: mode,
    screenshotArtifactId: observation?.screenshot_artifact_id ?? null,
    sessionId: session?.id,
    sourceLabel: frameUrl ? 'Chromium frame' : sourceLabel(mode),
    status: session?.status ?? 'degraded',
    timeline,
    title: observation?.title || session?.title || preview.title,
    url: observation?.url || session?.url || preview.url,
    visualObservationArtifactId,
    visualObservationUrl,
    viewport: session?.viewport ?? fallbackViewport,
    zdr: session?.zdr ?? false,
  }
}

/**
 * Whether the session's captured visual evidence is ephemeral. Only ZDR runs
 * make evidence ephemeral — for regular sessions Quarry retains artifacts per
 * its normal retention even when the browser profile is isolated (profile and
 * cookie non-persistence is surfaced separately via `profileStorage`). The UI
 * renders an explicit marker for ZDR sessions so they never imply persistence.
 */
export function evidenceIsEphemeral(model: Pick<BrowserSessionViewModel, 'zdr'>): boolean {
  return model.zdr
}

export function attachBrowserSession(
  preview: ScrapePreview,
  browserSession: BrowserSessionResponse | null,
): ScrapePreview {
  if (!browserSession) return preview
  const previous = preview.browserSession?.session
  const session = browserSession.session
  const mergedSession: BrowserSessionResponse['session'] = {
    ...session,
    capabilities: session.capabilities.length > 0
      ? session.capabilities
      : previous?.capabilities ?? session.capabilities,
    leaseId: session.leaseId ?? previous?.leaseId ?? null,
    profile: {
      id: session.profile.id ?? previous?.profile.id ?? null,
      scope: session.profile.id ? session.profile.scope : previous?.profile.scope ?? session.profile.scope,
      storage: session.profile.id ? session.profile.storage : previous?.profile.storage ?? session.profile.storage,
    },
    visual: session.visual ?? previous?.visual ?? null,
    timeline: session.timeline?.length
      ? session.timeline
      : previous?.timeline ?? [],
    viewport: session.viewport ?? previous?.viewport,
    zdr: session.zdr ?? previous?.zdr ?? false,
  }
  const mergedBrowserSession: BrowserSessionResponse = {
    ...browserSession,
    session: mergedSession,
  }

  return {
    ...preview,
    browserSession: mergedBrowserSession,
    browserSessionError: null,
    title: browserSession.observation?.title || mergedSession.title || preview.title,
    url: browserSession.observation?.url || mergedSession.url || preview.url,
  }
}

function domNodesFromObservation(observation: BrowserObservation): BrowserDomNode[] {
  const nodes = observation.dom_summary?.interactive_elements ?? []
  return nodes
    .map((element, index) => ({
      id: `${element.tag}-${index}`,
      kind: element.role || element.tag,
      selector: element.selector,
      text: (element.text || element.selector || element.tag).trim(),
    }))
    .filter((node) => node.text.length > 0)
    .slice(0, 40)
}

function domNodesFromPreview(preview: ScrapePreview): BrowserDomNode[] {
  return preview.blocks.map((block, index) => ({
    id: `block-${index}`,
    kind: block.heading ? 'heading' : 'paragraph',
    text: block.text,
  }))
}

function profileLabel(session?: BrowserSession): string {
  if (!session) return 'Isolert fallback'
  if (session.profile.id) return `${session.profile.id} · ${session.profile.storage}`
  return session.profile.storage === 'persistent' ? 'Persist profile' : 'Ephemeral profile'
}

function sourceLabel(mode: BrowserSurfaceMode): string {
  if (mode === 'chromium') return 'Chromium observation'
  if (mode === 'dom_snapshot') return 'DOM snapshot'
  return 'Readability fallback'
}

function resolveBrowserArtifactUrl(url?: string | null): string | null {
  const value = url?.trim()
  if (!value) return null
  if (value.startsWith('/')) return `${gatewayBaseUrl()}${value}`
  if (/^https?:\/\//i.test(value)) return value
  return null
}

function resolveVisualObservationUrl(sessionId?: string, artifactId?: string | null): string | null {
  const normalizedSessionId = sessionId?.trim()
  const normalizedArtifactId = artifactId?.trim()
  if (!normalizedSessionId || !normalizedArtifactId) return null
  return resolveBrowserArtifactUrl(
    `/api/v1/browser/sessions/${encodeURIComponent(normalizedSessionId)}/artifacts/${encodeURIComponent(normalizedArtifactId)}`,
  )
}

function normalizeTimeline(entries?: BrowserTimelineEntry[] | null): BrowserTimelineViewEntry[] {
  return (entries ?? []).map((entry) => ({
    ...entry,
    screenshotUrl: resolveBrowserArtifactUrl(entry.screenshotUrl),
    visualObservationUrl: resolveBrowserArtifactUrl(entry.visualObservationUrl),
  }))
}

/** Gateway artifact URL for a session-scoped artifact id, or null when either id is unusable. */
export function browserArtifactUrl(sessionId?: string | null, artifactId?: string | null): string | null {
  return resolveVisualObservationUrl(sessionId ?? undefined, artifactId)
}

// --- Timeline detail selection ---------------------------------------------

export type BrowserTimelineDetail = {
  entry: BrowserTimelineViewEntry
  previous: BrowserTimelineViewEntry | null
}

/** The timeline entry for a step plus the preceding entry (for before/after evidence). */
export function timelineDetail(
  timeline: BrowserTimelineViewEntry[],
  step: number | null,
): BrowserTimelineDetail | null {
  if (step === null) return null
  const index = timeline.findIndex((entry) => entry.step === step)
  if (index < 0) return null
  const entry = timeline[index]
  if (!entry) return null
  return { entry, previous: index > 0 ? timeline[index - 1] ?? null : null }
}

export type BrowserTimelineDelta = {
  domNodeDelta: number | null
  titleChanged: boolean
  urlChanged: boolean
}

/**
 * Deterministic delta between two observed timeline entries. Values are
 * computed only from fields the gateway returned; anything unavailable is null.
 */
export function describeTimelineDelta(detail: BrowserTimelineDetail): BrowserTimelineDelta {
  const { entry, previous } = detail
  const currentNodes = entry.domNodeCount ?? null
  const previousNodes = previous?.domNodeCount ?? null
  return {
    domNodeDelta: currentNodes !== null && previousNodes !== null ? currentNodes - previousNodes : null,
    titleChanged: Boolean(previous) && (previous?.title ?? '') !== (entry.title ?? ''),
    urlChanged: Boolean(previous) && (previous?.url ?? '') !== (entry.url ?? ''),
  }
}

// --- Artifact descriptors ----------------------------------------------------

export type BrowserArtifactMediaKind = 'image' | 'json'

export type BrowserArtifactDescriptor = {
  artifactId: string
  kind: 'screenshot' | 'visual_observation'
  label: string
  mediaKind: BrowserArtifactMediaKind
  url: string
}

/** The artifacts the gateway returned for one timeline entry, typed for the viewer. */
export function artifactsForTimelineEntry(entry: BrowserTimelineViewEntry): BrowserArtifactDescriptor[] {
  const artifacts: BrowserArtifactDescriptor[] = []
  if (entry.screenshotArtifactId && entry.screenshotUrl) {
    artifacts.push({
      artifactId: entry.screenshotArtifactId,
      kind: 'screenshot',
      label: 'screenshot.png',
      mediaKind: 'image',
      url: entry.screenshotUrl,
    })
  }
  if (entry.visualObservationArtifactId && entry.visualObservationUrl) {
    artifacts.push({
      artifactId: entry.visualObservationArtifactId,
      kind: 'visual_observation',
      label: 'visual_observation.json',
      mediaKind: 'json',
      url: entry.visualObservationUrl,
    })
  }
  return artifacts
}

// --- Model rationale (AI-suggested steps) -------------------------------------

export type BrowserStepRationale = {
  actionType: string | null
  confidence: number | null
  done: boolean
  goal: string
  modelUsed: string | null
  reason: string | null
  /** Step number of the observation produced by executing the suggested action. */
  step: number
}

export function rationaleForStep(
  rationales: BrowserStepRationale[],
  step: number | null,
): BrowserStepRationale | null {
  if (step === null) return null
  return rationales.find((rationale) => rationale.step === step) ?? null
}

/** Immutably record a rationale for a step, replacing any previous record for that step. */
export function withStepRationale(
  rationales: BrowserStepRationale[],
  rationale: BrowserStepRationale,
): BrowserStepRationale[] {
  return [...rationales.filter((existing) => existing.step !== rationale.step), rationale]
}
