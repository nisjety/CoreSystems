import type {
  BrowserFrame,
  BrowserObservation,
  BrowserSession,
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
  profileLabel: string
  renderMode: BrowserSurfaceMode
  screenshotArtifactId?: string | null
  sessionId?: string
  sourceLabel: string
  status: BrowserSession['status']
  title: string
  url: string
  viewport: {
    height: number
    width: number
  }
}

const fallbackViewport = { width: 1280, height: 800 }

export function browserSessionFromPreview(preview: ScrapePreview): BrowserSessionViewModel {
  const mode = preview.browserSession?.session.renderMode ?? 'readability_fallback'
  const observation = preview.browserSession?.observation ?? null
  const session = preview.browserSession?.session
  const frameUrl = resolveFrameUrl(session?.frame)

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
    profileLabel: profileLabel(session),
    renderMode: mode,
    screenshotArtifactId: observation?.screenshot_artifact_id ?? null,
    sessionId: session?.id,
    sourceLabel: frameUrl ? 'Chromium frame' : sourceLabel(mode),
    status: session?.status ?? 'degraded',
    title: observation?.title || session?.title || preview.title,
    url: observation?.url || session?.url || preview.url,
    viewport: session?.viewport ?? fallbackViewport,
  }
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
    viewport: session.viewport ?? previous?.viewport,
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

function resolveFrameUrl(frame?: BrowserFrame | null): string | null {
  const value = frame?.url?.trim()
  if (!value) return null
  if (value.startsWith('/')) return `${gatewayBaseUrl()}${value}`
  if (/^https?:\/\//i.test(value)) return value
  return null
}
