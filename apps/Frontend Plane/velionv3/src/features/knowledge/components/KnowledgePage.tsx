import { A } from '@solidjs/router'
import {
  ArrowUpRight,
  Blocks,
  Bookmark,
  ChevronDown,
  Clock3,
  Download,
  FilePlus2,
  FileText,
  Filter,
  Folder,
  Grid2X2,
  List,
  Mail,
  MoreHorizontal,
  Pencil,
  Sparkles,
  Trash2,
  GitBranch,
  Globe2,
  Link2,
  Map as MapIcon,
  Network,
  RefreshCw,
  Search,
  Table2,
  type LucideProps,
} from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { KnowledgeAddSourceModal } from '@/features/knowledge/components/KnowledgeAddSourceModal'
import { KnowledgeOperatingMapCanvas } from '@/features/knowledge/components/KnowledgeOperatingMapCanvas'
import { PrivacyBadge } from '@/features/knowledge/components/PrivacyBadge'
import { ShareDialog } from '@/features/knowledge/components/ShareDialog'
import { isGateOpen } from '@/shared/context/ownership-gate'
import { translateApiError, useI18n } from '@/shared/i18n'
import { executeAction } from '@/shared/actions/action-client'
import {
  loadKnowledgeSources,
  type LiveKnowledgeCollection,
  type LiveKnowledgeFile,
  type LiveKnowledgeFolder,
  type LiveKnowledgeGraphNode,
  type LiveKnowledgeMetric,
  type LiveKnowledgePayload,
  type LiveKnowledgeSource,
  type LiveKnowledgeSourceType,
  type LiveKnowledgeWebSource,
} from '@/shared/api/knowledge-live-client'
// Crawl-completion signal reused from the Home dashboard's Crawl composer
// (KnowledgeComposer.tsx) — same SSE run-event stream, same terminal-status
// detection. Wiring it here closes the gap where a crawl started from THIS
// page refetched the workspace once at crawl-start (before any page had
// finished ingesting) and then never again, leaving "Tracked web sources"
// stuck on stale/empty data until a manual reload.
import { streamCrawlRunEvents } from '@/shared/api/knowledge-client'
import {
  generateOperatingMap,
  loadOperatingMap,
  reviewOperatingMapProposal,
  streamOperatingMapRunEvents,
  type OperatingMapAgentBlueprint,
  type OperatingMapProposal,
  type OperatingMapSnapshot,
} from '@/shared/api/operating-map-client'
import { getSessionContext } from '@/shared/api/auth-client'
import { requestForm, requestJson } from '@/shared/api/http'
import { Button } from '@/shared/ui/Button'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { VelionSegmented, VelionSegmentedButton } from '@/shared/ui/velion/VelionSegmented'
import { VelionSelect } from '@/shared/ui/velion/VelionSelect'
import { cn } from '@/shared/lib/cn'

type KnowledgeView = 'overview' | 'operating-map' | 'graph' | 'chunks'
type KnowledgeLayout = 'grid' | 'list'
type KnowledgeIcon = Component<LucideProps>

type Notice = {
  message: string
  tone: 'good' | 'warn'
}

type SyncResult = {
  finspoFailures: string[]
  finspoStarted: number
  integrationFailures: string[]
  integrationStarted: number
}

type UploadResult = {
  failedItems: number
  id: string
  processedItems: number
  sourceType: string
  status: string
  totalItems: number
}

type SharePointCreateResult = {
  id: string
  syncStarted: boolean
}

// Matches the gateway's actual /api/v1/knowledge/crawl response shape
// (apps/gateway/src/domains/knowledge/quarry.rs::start_crawl) — it echoes the
// normalized job, never the submitted URL, so callers must keep the URL the
// user typed (the request input) to reference it afterward instead of
// expecting the response to hand it back.
type CrawlStartResult = {
  id: string
  status: string
}

// Provider labels are brand names (kept as-is); `detail` copy is localized, so
// this is built from a `tr` function at render time instead of a static const.
function buildConnectProviders(tr: (noText: string, enText: string) => string) {
  return [
    { id: 'microsoft', label: 'Microsoft 365', detail: tr('SharePoint, OneDrive, Teams, Outlook', 'SharePoint, OneDrive, Teams, Outlook'), sources: ['sharepoint', 'onedrive', 'teams', 'outlook'] },
    { id: 'google', label: 'Google Workspace', detail: tr('Disk og dokumenter', 'Drive and docs'), sources: ['google_drive', 'documents'] },
    { id: 'notion', label: 'Notion', detail: tr('Sider og databaser', 'Pages and databases'), sources: ['pages', 'databases'] },
    { id: 'github', label: 'GitHub', detail: tr('Repos, README og saker', 'Repos, README, issues'), sources: ['repos', 'readme', 'issues'] },
    { id: 'slack', label: 'Slack', detail: tr('Kanaler og trådhistorikk', 'Channels and thread history'), sources: ['channels'] },
  ] as const
}

const sourceTypeIcon: Record<LiveKnowledgeSourceType, KnowledgeIcon> = {
  Docs: FileText,
  Notion: Link2,
  PDF: FileText,
  URL: Globe2,
}

const graphToneClass: Record<LiveKnowledgeGraphNode['tone'], string> = {
  core: 'knowledge-graph-node--core',
  policy: 'knowledge-graph-node--policy',
  product: 'knowledge-graph-node--product',
  risk: 'knowledge-graph-node--risk',
  support: 'knowledge-graph-node--support',
}

const folderToneClass: Record<LiveKnowledgeFolder['tone'], string> = {
  blue: 'knowledge-folder-card__visual--blue',
  gray: 'knowledge-folder-card__visual--gray',
  green: 'knowledge-folder-card__visual--green',
  warm: 'knowledge-folder-card__visual--warm',
}

function filterKnowledgePayload(
  liveKnowledge: LiveKnowledgePayload,
  collectionId: string,
  searchQuery: string,
): LiveKnowledgePayload {
  const normalizedQuery = searchQuery.trim().toLowerCase()
  const providerScope = collectionId.startsWith('provider:') ? collectionId.slice('provider:'.length) : null
  const webScope = collectionId === 'web'

  const matchesQuery = (...values: Array<string | undefined>) =>
    normalizedQuery.length === 0 || values.some((value) => value?.toLowerCase().includes(normalizedQuery))
  const matchesProvider = (providerKey: string) =>
    collectionId === 'all' ||
    (providerScope ? providerKey === providerScope : false) ||
    (webScope ? providerKey === 'web' : false)

  const folders = liveKnowledge.folders.filter((folder) =>
    matchesProvider(folder.providerKey) &&
    matchesQuery(folder.title, folder.subtitle, ...folder.connections),
  )
  const integrations = liveKnowledge.integrations.filter((integration) =>
    (collectionId === 'all' || (providerScope ? integration.providerKey === providerScope : false)) &&
    matchesQuery(integration.name, integration.detail, integration.providerKey, integration.documents),
  )
  const files = liveKnowledge.files.filter((file) =>
    matchesProvider(file.providerKey) &&
    matchesQuery(file.name, file.addedBy, file.source, file.updated),
  )
  const sources = liveKnowledge.sources.filter((source) =>
    matchesProvider(source.providerKey) &&
    matchesQuery(
      source.title,
      source.description,
      source.provider,
      source.category,
      ...source.tags,
      ...source.related,
    ),
  )
  const webSources = liveKnowledge.webSources.filter((source) =>
    matchesProvider(source.providerKey) &&
    matchesQuery(source.name, source.url, source.kind, source.status),
  )

  const allowedSourceIds = new Set(sources.map((source) => source.id))
  const graphNodes = liveKnowledge.graph.nodes.filter((node) => {
    const providerMatch = collectionId === 'all'
      ? true
      : node.sourceIds.some((sourceId) => allowedSourceIds.has(sourceId))
    return providerMatch && matchesQuery(node.label, node.group, ...node.sourceRefs)
  })
  const allowedNodeIds = new Set(graphNodes.map((node) => node.id))
  const graphLinks = liveKnowledge.graph.links.filter((link) =>
    allowedNodeIds.has(link.from) && allowedNodeIds.has(link.to) && matchesQuery(link.label, ...link.sourceRefs),
  )

  return {
    ...liveKnowledge,
    files,
    folders,
    integrations,
    sources,
    webSources,
    graph: {
      ...liveKnowledge.graph,
      nodes: graphNodes,
      links: graphLinks,
      nodeCount: graphNodes.length,
      edgeCount: graphLinks.length,
      available: graphNodes.length > 0 || graphLinks.length > 0,
    },
  }
}

export default function KnowledgePage() {
  const i18n = useI18n()
  const [activeView, setActiveView] = createSignal<KnowledgeView>('overview')
  const [overviewLayout, setOverviewLayout] = createSignal<KnowledgeLayout>('grid')
  const [selectedCollectionId, setSelectedCollectionId] = createSignal('all')
  const [selectedSourceId, setSelectedSourceId] = createSignal<string | null>(null)
  const [selectedGraphNodeId, setSelectedGraphNodeId] = createSignal<string | null>(null)
  const [searchQuery, setSearchQuery] = createSignal('')
  const [liveKnowledge, setLiveKnowledge] = createSignal<LiveKnowledgePayload | null>(null)
  const [operatingMap, setOperatingMap] = createSignal<OperatingMapSnapshot | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [operatingMapLoading, setOperatingMapLoading] = createSignal(true)
  const [busyAction, setBusyAction] = createSignal<string | null>(null)
  const [operatingMapEvents, setOperatingMapEvents] = createSignal<string[]>([])
  const [notice, setNotice] = createSignal<Notice | null>(null)
  const [addSourceOpen, setAddSourceOpen] = createSignal(false)
  // Resolved once on mount and stable for the page lifetime — a plain value, not
  // reactive UI state, so handlers and the connect-poll can read it freely.
  let activeOrgId = ''
  let activeUserId = ''
  // Tracks the in-flight crawl-completion SSE subscription (see
  // handleStartWebsiteCrawl) so it's cancelled if the user navigates away
  // before the crawl finishes — otherwise the stream would keep running
  // against an unmounted page.
  let crawlStatusAbort: AbortController | null = null
  onCleanup(() => crawlStatusAbort?.abort())

  const visibleKnowledge = createMemo(() => {
    const payload = liveKnowledge()
    return payload ? filterKnowledgePayload(payload, selectedCollectionId(), searchQuery()) : null
  })
  const selectedSource = createMemo(() =>
    visibleKnowledge()?.sources.find((source) => source.id === selectedSourceId()) ?? visibleKnowledge()?.sources[0] ?? null,
  )
  const selectedGraphNode = createMemo(() =>
    visibleKnowledge()?.graph.nodes.find((node) => node.id === selectedGraphNodeId()) ?? visibleKnowledge()?.graph.nodes[0] ?? null,
  )
  const relatedGraphSources = createMemo(() => {
    const knowledge = visibleKnowledge()
    const node = selectedGraphNode()
    return knowledge && node
      ? knowledge.sources.filter((source) => node.sourceIds.includes(source.id))
      : []
  })

  onMount(() => {
    const requestedSourceId = new URLSearchParams(window.location.search).get('source')?.trim() ?? ''
    if (
      requestedSourceId.length > 0 &&
      requestedSourceId.length <= 256 &&
      /^[A-Za-z0-9_-]+$/.test(requestedSourceId)
    ) {
      setSelectedSourceId(requestedSourceId)
      setActiveView('chunks')
    }
    const controller = new AbortController()
    void (async () => {
      try {
        const ctx = await getSessionContext()
        activeOrgId = ctx.orgId ?? ctx.orgs?.[0]?.id ?? ''
        activeUserId = ctx.userId
      } catch {
        // Leave org empty; loadKnowledgeSources resolves it from the session itself.
      }
      await Promise.all([
        loadKnowledgeWorkspace(controller.signal),
        loadOperatingMapWorkspace(controller.signal),
      ])
    })()
    onCleanup(() => controller.abort())
  })

  createEffect(() => {
    const payload = liveKnowledge()
    if (!payload) return
    if (!payload.collections.some((collection) => collection.id === selectedCollectionId())) {
      setSelectedCollectionId(payload.collections[0]?.id ?? 'all')
    }
  })

  createEffect(() => {
    const knowledge = visibleKnowledge()
    if (!knowledge) return
    if (!selectedSourceId() || !knowledge.sources.some((source) => source.id === selectedSourceId())) {
      setSelectedSourceId(knowledge.sources[0]?.id ?? null)
    }
    if (!selectedGraphNodeId() || !knowledge.graph.nodes.some((node) => node.id === selectedGraphNodeId())) {
      setSelectedGraphNodeId(knowledge.graph.nodes[0]?.id ?? null)
    }
  })

  async function loadKnowledgeWorkspace(signal?: AbortSignal) {
    setLoading(true)
    try {
      const nextKnowledge = await loadKnowledgeSources(signal, activeOrgId)
      setLiveKnowledge(nextKnowledge)
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return
      setLiveKnowledge(null)
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Kunnskapsområdet kunne ikke lastes.', en: 'Knowledge workspace could not be loaded.' }),
      })
    } finally {
      setLoading(false)
    }
  }

  async function loadOperatingMapWorkspace(signal?: AbortSignal) {
    setOperatingMapLoading(true)
    try {
      setOperatingMap(await loadOperatingMap(activeOrgId, signal))
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return
      setOperatingMap(null)
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Operating Map kunne ikke lastes.', en: 'Operating Map could not be loaded.' }),
      })
    } finally {
      setOperatingMapLoading(false)
    }
  }

  async function handleSync() {
    setBusyAction('sync')
    setNotice(null)
    try {
      const result = await requestJson<SyncResult>('/api/v1/knowledge/sync', {
        method: 'POST',
        body: JSON.stringify({}),
        headers: { 'x-velion-org-id': activeOrgId },
      })
      const failures = [...result.integrationFailures, ...result.finspoFailures]
      const startedCount = result.integrationStarted + result.finspoStarted
      setNotice({
        tone: failures.length > 0 ? 'warn' : 'good',
        message: failures.length > 0
          ? i18n.tr(
              `Startet ${startedCount} synkroniseringer, men ${failures.length} kilder trenger fortsatt gjennomgang.`,
              `Started ${startedCount} syncs, but ${failures.length} sources still need review.`,
            )
          : i18n.tr(`Startet ${startedCount} kildesynkroniseringer.`, `Started ${startedCount} source syncs.`),
      })
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Synkroniseringen kunne ikke startes.', en: 'Knowledge sync could not be started.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleUploadFiles(files: File[]) {
    setBusyAction('upload')
    setNotice(null)
    try {
      const result = await uploadKnowledgeFiles(files, activeOrgId)
      const queuedCount = result.totalItems || files.length
      setNotice({
        tone: 'good',
        message: i18n.tr(
          `Imports-core la ${queuedCount} fil${files.length === 1 ? '' : 'er'} i kø for innhenting.`,
          `Imports-core queued ${queuedCount} file${files.length === 1 ? '' : 's'} for ingestion.`,
        ),
      })
      setAddSourceOpen(false)
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Filopplastingen kunne ikke startes.', en: 'File upload could not be started.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleRegisterSharePoint(input: {
    driveId: string
    driveName: string
    driveType: string
    siteId: string
    siteWebUrl: string
    tenantId: string
  }) {
    setBusyAction('sharepoint')
    setNotice(null)
    try {
      const result = await requestJson<SharePointCreateResult>('/api/v1/knowledge/sharepoint', {
        method: 'POST',
        body: JSON.stringify(input),
        headers: { 'x-velion-org-id': activeOrgId },
      })
      setNotice({
        tone: 'good',
        message: result.syncStarted
          ? i18n.tr('SharePoint-stasjonen er registrert, og synkronisering er startet i Finspo.', 'SharePoint drive registered and sync started in Finspo.')
          : i18n.tr('SharePoint-stasjonen er registrert. Synkronisering kan startes fra Kunnskap.', 'SharePoint drive registered. Sync can be started from Knowledge.'),
      })
      setAddSourceOpen(false)
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'SharePoint-kilden kunne ikke registreres.', en: 'SharePoint source could not be registered.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleConnectProvider(provider: {
    detail: string
    id: string
    label: string
    sources: readonly string[]
  }) {
    setBusyAction('connect')
    setNotice(null)
    try {
      const session = await requestJson<{ connectUrl?: string; redirectUrl?: string }>(
        `/api/v1/integrations/providers/${encodeURIComponent(provider.id)}/connect-session`,
        {
          method: 'POST',
          body: JSON.stringify({
            selectedSources: [...provider.sources],
            bundles: ['onboarding'],
          }),
        },
      )
      const connectUrl = session.connectUrl ?? session.redirectUrl
      if (!connectUrl) throw new Error('The integration service did not return an authorization URL.')
      const authWindow = window.open(connectUrl, '_blank', popupFeatures())
      if (!authWindow) throw new Error('The authorization window was blocked by the browser.')
      setNotice({
        tone: 'good',
        message: i18n.tr(
          `${provider.label}-autorisering ble åpnet i et nytt vindu. Kom tilbake hit etter godkjenning for å oppdatere arbeidsområdet.`,
          `${provider.label} authorization opened in a new window. Return here after approval to refresh the workspace.`,
        ),
      })
      setAddSourceOpen(false)
      const closePoll = window.setInterval(() => {
        if (!authWindow.closed) return
        window.clearInterval(closePoll)
        void loadKnowledgeWorkspace()
      }, 1_000)
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Tilkoblingen kunne ikke startes.', en: 'Connection flow could not be started.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleStartWebsiteCrawl(input: { maxPages?: number; url: string }) {
    setBusyAction('crawl')
    setNotice(null)
    try {
      const result = await requestJson<CrawlStartResult>('/api/v1/knowledge/crawl', {
        method: 'POST',
        body: JSON.stringify(input),
      })
      setNotice({
        tone: 'good',
        message: i18n.tr(
          `Startet en gjennomsøking av nettstedet for ${input.url}. Følg kjøring ${result.id} i Innhenting mens sidene flyter inn i Kunnskap.`,
          `Started a website crawl for ${input.url}. Track run ${result.id} in Ingestions while pages flow into Knowledge.`,
        ),
      })
      setAddSourceOpen(false)
      // Immediate refetch only shows that a crawl started — the pipeline
      // finishes asynchronously in Quarry, well after this call returns, so
      // it alone left "Tracked web sources" (and everything else on this
      // page) stuck on stale/empty data until a manual reload. Subscribe to
      // the same run-event stream the Home dashboard's Crawl composer
      // already uses and refetch again once the run reaches a terminal
      // status — no new polling primitive, just reusing the existing SSE
      // completion signal instead of a blind setInterval.
      await loadKnowledgeWorkspace()
      crawlStatusAbort?.abort()
      crawlStatusAbort = new AbortController()
      void streamCrawlRunEvents(
        activeOrgId,
        result.id,
        { onDone: () => void loadKnowledgeWorkspace() },
        crawlStatusAbort.signal,
      ).catch(() => {
        // Best-effort completion refresh — a stream failure here (e.g. the
        // job finished before the SSE connection was established) must
        // never surface as a page-level error. Manual "Sync" and the next
        // page load remain the fallback refresh paths.
      })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Gjennomsøkingen av nettstedet kunne ikke startes.', en: 'The website crawl could not be started.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleGenerateOperatingMap() {
    setBusyAction('operating-map')
    setNotice(null)
    setOperatingMapEvents([i18n.tr('Genererer Operating Map-forslag fra Kunnskap-bevis …', 'Generating Operating Map proposal from Knowledge evidence...')])
    try {
      const result = await generateOperatingMap(activeOrgId)
      setOperatingMapEvents((events) => [...events, i18n.tr(`Forslag ${result.proposal.id} opprettet.`, `Proposal ${result.proposal.id} created.`)])
      if (result.runId) {
        await streamOperatingMapRunEvents(activeOrgId, result.runId, {
          onEvent: (event) => setOperatingMapEvents((events) => [...events, event.detail]),
          onError: (error) => {
            setOperatingMapEvents((events) => [...events, translateApiError(error, i18n.tr, { no: 'Hendelsesstrømmen for Operating Map feilet.', en: 'Operating Map event stream failed.' })])
          },
        }).catch((error) => {
          setOperatingMapEvents((events) => [...events, translateApiError(error, i18n.tr, { no: 'Hendelsesstrømmen for Operating Map feilet.', en: 'Operating Map event stream failed.' })])
        })
      }
      await loadOperatingMapWorkspace()
      setNotice({ tone: 'good', message: i18n.tr('Operating Map-forslaget er klart for gjennomgang.', 'Operating Map proposal is ready for review.') })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Generering av Operating Map kunne ikke startes.', en: 'Operating Map generation could not be started.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleReviewOperatingMapProposal(proposal: OperatingMapProposal, decision: 'accept' | 'reject') {
    setBusyAction(`operating-map-${decision}`)
    setNotice(null)
    try {
      await reviewOperatingMapProposal(activeOrgId, proposal.id, decision)
      await loadOperatingMapWorkspace()
      setNotice({
        tone: 'good',
        message: decision === 'accept'
          ? i18n.tr('Operating Map er godkjent og publisert i Kunnskap-wikien.', 'Operating Map accepted and published into Knowledge wiki.')
          : i18n.tr('Operating Map-forslaget ble avvist.', 'Operating Map proposal rejected.'),
      })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Forslaget til Operating Map kunne ikke behandles.', en: 'Operating Map proposal could not be reviewed.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleCreateAgentBlueprint(blueprint: OperatingMapAgentBlueprint) {
    setBusyAction(`blueprint-${blueprint.id}`)
    setNotice(null)
    try {
      const version = operatingMap()?.currentVersion
      if (!version?.id) {
        throw new Error('Accept an Operating Map before creating an agent blueprint suggestion.')
      }
      await executeAction(
        'operating_map.create_agent_blueprint',
        { type: 'human', userId: activeUserId, orgId: activeOrgId },
        {
          versionId: version.id,
          blueprintId: blueprint.id,
          role: normalizeBlueprintRole(blueprint.role),
          sourceWorkflowId: blueprint.sourceWorkflowId,
          name: blueprint.name,
          payload: {
            source: 'operating-map',
            mapId: version.mapId,
            sourceWorkflowId: blueprint.sourceWorkflowId,
          },
        },
      )
      await loadOperatingMapWorkspace()
      setNotice({ tone: 'good', message: i18n.tr(`${blueprint.name}-agentmal-forslag lagret for gjennomgang i Agenter.`, `${blueprint.name} blueprint suggestion saved for Agents review.`) })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: translateApiError(error, i18n.tr, { no: 'Agent-malen kunne ikke legges i kø.', en: 'Agent blueprint could not be queued.' }),
      })
    } finally {
      setBusyAction(null)
    }
  }

  return (
    <div class="knowledge-page-surface knowledge-page-surface--docs">
      <div class="knowledge-page-container">
        <WorkspaceHeader
          activeView={activeView()}
          collections={liveKnowledge()?.collections ?? []}
          layout={overviewLayout()}
          searchQuery={searchQuery()}
          selectedCollectionId={selectedCollectionId()}
          syncing={busyAction() === 'sync'}
          onActiveViewChange={setActiveView}
          onAddSource={() => setAddSourceOpen(true)}
          onCollectionChange={setSelectedCollectionId}
          onLayoutChange={setOverviewLayout}
          onSearchChange={setSearchQuery}
          onSync={() => void handleSync()}
        />

        <Show when={notice()}>
          {(currentNotice) => <NoticeBanner notice={currentNotice()} />}
        </Show>

        <Show when={loading() && !liveKnowledge()}>
          <section class="velion-panel knowledge-loading-panel">{i18n.tr('Laster kunnskapsområdet …', 'Loading knowledge workspace...')}</section>
        </Show>

        <Show when={!loading() && !liveKnowledge()}>
          <EmptyPanel
            title={i18n.tr('Kunnskapsområdet er utilgjengelig', 'Knowledge workspace unavailable')}
            description={i18n.tr(
              'Siden kunne ikke laste sanntidsdata fra Data Plane v2 og innhentingstjenestene.',
              'The page could not load live data from Data Plane v2 and the ingestion services.',
            )}
          />
        </Show>

        <Show when={visibleKnowledge()}>
          <Show when={activeView() === 'overview'}>
            <OverviewCanvas
              layout={overviewLayout()}
              liveKnowledge={visibleKnowledge()!}
              searchQuery={searchQuery()}
              onAddSource={() => setAddSourceOpen(true)}
              onSearchChange={setSearchQuery}
            />
          </Show>
          <Show when={activeView() === 'operating-map'}>
            <KnowledgeOperatingMapCanvas
              busy={busyAction()?.startsWith('operating-map') || busyAction()?.startsWith('blueprint') || false}
              events={operatingMapEvents()}
              liveKnowledge={visibleKnowledge()!}
              loading={operatingMapLoading()}
              operatingMap={operatingMap()}
              onCreateBlueprint={(blueprint) => void handleCreateAgentBlueprint(blueprint)}
              onGenerate={() => void handleGenerateOperatingMap()}
              onReviewProposal={(proposal, decision) => void handleReviewOperatingMapProposal(proposal, decision)}
            />
          </Show>
          <Show when={activeView() === 'graph'}>
            <GraphCanvas
              graph={visibleKnowledge()!.graph}
              relatedSources={relatedGraphSources()}
              selectedNode={selectedGraphNode()}
              onSelectNode={setSelectedGraphNodeId}
            />
          </Show>
          <Show when={activeView() === 'chunks'}>
            <ChunksCanvas
              selectedSource={selectedSource()}
              sources={visibleKnowledge()!.sources}
              onSelectSource={setSelectedSourceId}
            />
          </Show>
        </Show>
      </div>

      <Show when={addSourceOpen()}>
        <KnowledgeAddSourceModal
          busy={busyAction() !== null}
          providers={buildConnectProviders(i18n.tr)}
          onClose={() => setAddSourceOpen(false)}
          onConnectProvider={handleConnectProvider}
          onRegisterSharePoint={handleRegisterSharePoint}
          onStartWebsiteCrawl={handleStartWebsiteCrawl}
          onUploadFiles={handleUploadFiles}
        />
      </Show>
    </div>
  )
}

function WorkspaceHeader(props: {
  activeView: KnowledgeView
  collections: LiveKnowledgeCollection[]
  layout: KnowledgeLayout
  searchQuery: string
  selectedCollectionId: string
  syncing: boolean
  onActiveViewChange: (view: KnowledgeView) => void
  onAddSource: () => void
  onCollectionChange: (collectionId: string) => void
  onLayoutChange: (layout: KnowledgeLayout) => void
  onSearchChange: (query: string) => void
  onSync: () => void
}) {
  const i18n = useI18n()
  let collectionSelect: HTMLSelectElement | undefined

  return (
    <header class="knowledge-header knowledge-header--docs">
      <div class="knowledge-header__main">
        <span class="knowledge-header__eyebrow">{i18n.tr('Kunnskapsbase', 'Knowledge base')}</span>
        <div class="knowledge-header__title-row">
          <h1 class="knowledge-header__title">{i18n.tr('Kunnskap', 'Knowledge')}</h1>
          <div class="knowledge-header__select-wrap">
            <VelionSelect
              ref={(element) => { collectionSelect = element }}
              aria-label={i18n.tr('Velg kunnskapssamling', 'Select knowledge collection')}
              value={props.selectedCollectionId}
              onChange={(event) => props.onCollectionChange(event.currentTarget.value)}
              class="knowledge-header__select"
            >
              <For each={props.collections}>
                {(collection) => <option value={collection.id}>{collection.label}</option>}
              </For>
            </VelionSelect>
            <ChevronDown class="knowledge-header__chevron" strokeWidth={2} />
          </div>
          <span class="knowledge-header__scope">{i18n.tr('Arbeidsområdebibliotek', 'Workspace library')}</span>
        </div>
        <p class="knowledge-header__copy">
          {i18n.tr(
            'Ett rolig sted for kildesamlinger, delte filer og bevisene Velion bruker for å svare.',
            'One calm place for source collections, shared files, and the evidence Velion uses to answer.',
          )}
        </p>
      </div>

      <div class="knowledge-header__actions">
        <label class="knowledge-toolbar-search">
          <Search class="size-4" aria-hidden="true" />
          <VelionInput
            aria-label={i18n.tr('Søk i kunnskapsbasen', 'Search knowledge base')}
            value={props.searchQuery}
            onInput={(event) => props.onSearchChange(event.currentTarget.value)}
            placeholder={i18n.tr('Søk', 'Search')}
          />
          <kbd>⌘ K</kbd>
        </label>
        <button
          type="button"
          class="knowledge-toolbar-button"
          aria-label={i18n.tr('Filtrer kunnskap', 'Filter knowledge')}
          title={i18n.tr('Filtrer etter samling', 'Filter by collection')}
          onClick={() => collectionSelect?.focus()}
        >
          <Filter class="size-4" />
          <span>{i18n.tr('Filter', 'Filter')}</span>
        </button>
        <div class="knowledge-toolbar-view" role="group" aria-label={i18n.tr('Visningsstil for kunnskap', 'Knowledge view style')}>
          <button
            type="button"
            class={cn('knowledge-toolbar-view__button', props.layout === 'grid' && 'knowledge-toolbar-view__button--active')}
            aria-label={i18n.tr('Rutenettvisning', 'Grid view')}
            aria-pressed={props.layout === 'grid'}
            onClick={() => props.onLayoutChange('grid')}
          >
            <Grid2X2 class="size-4" />
          </button>
          <button
            type="button"
            class={cn('knowledge-toolbar-view__button', props.layout === 'list' && 'knowledge-toolbar-view__button--active')}
            aria-label={i18n.tr('Listevisning', 'List view')}
            aria-pressed={props.layout === 'list'}
            onClick={() => props.onLayoutChange('list')}
          >
            <List class="size-4" />
          </button>
        </div>
        <SegmentedView activeView={props.activeView} onActiveViewChange={props.onActiveViewChange} />
        <A href="/ingestions" class="button button--secondary button--md knowledge-link-button">
          <ArrowUpRight class="size-4" />
          {i18n.tr('Innhenting', 'Ingestions')}
        </A>
        <Button size="md" onClick={props.onSync} disabled={props.syncing}>
          <RefreshCw class={cn('size-4', props.syncing && 'knowledge-spin')} />
          {i18n.tr('Synkroniser', 'Sync')}
        </Button>
        <Button variant="primary" size="md" onClick={props.onAddSource}>
          <FilePlus2 class="size-4" />
          {i18n.tr('Legg til kilde', 'Add source')}
        </Button>
      </div>
    </header>
  )
}

function SegmentedView(props: {
  activeView: KnowledgeView
  onActiveViewChange: (view: KnowledgeView) => void
}) {
  const i18n = useI18n()
  const views: Array<{ Icon: KnowledgeIcon; id: KnowledgeView; label: string }> = [
    { id: 'overview', label: i18n.tr('Oversikt', 'Overview'), Icon: Grid2X2 },
    { id: 'operating-map', label: i18n.tr('AI-kart', 'AI Map'), Icon: MapIcon },
    { id: 'graph', label: i18n.tr('Graf', 'Graph'), Icon: Network },
    { id: 'chunks', label: i18n.tr('Utdrag', 'Chunks'), Icon: Table2 },
  ]

  return (
    <VelionSegmented>
      <For each={views}>
        {(view) => (
          <VelionSegmentedButton
            selected={props.activeView === view.id}
            onClick={() => props.onActiveViewChange(view.id)}
          >
            <Dynamic component={view.Icon} class="size-4" />
            {view.label}
          </VelionSegmentedButton>
        )}
      </For>
    </VelionSegmented>
  )
}

function OverviewCanvas(props: {
  layout: KnowledgeLayout
  liveKnowledge: LiveKnowledgePayload
  onAddSource: () => void
  onSearchChange: (query: string) => void
  searchQuery: string
}) {
  const i18n = useI18n()
  const collectionCards = createMemo(() => buildKnowledgeCollectionCards(props.liveKnowledge, i18n.tr))

  return (
    <main class={cn('knowledge-main-stack knowledge-dashboard', props.layout === 'list' && 'knowledge-dashboard--list')}>
      <div class="knowledge-dashboard__body">
        <div class="knowledge-dashboard__primary">
          <Show when={props.liveKnowledge.dataPlane.documentsTruncated}>
            <div class="knowledge-muted-copy" role="status">
              {i18n.tr(
                `Viser ${props.liveKnowledge.dataPlane.loadedDocumentCount ?? props.liveKnowledge.files.length} av ${props.liveKnowledge.dataPlane.documentCount} dokumenter i arbeidsområdet. Bruk søk eller en mappe for å avgrense visningen.`,
                `Showing ${props.liveKnowledge.dataPlane.loadedDocumentCount ?? props.liveKnowledge.files.length} of ${props.liveKnowledge.dataPlane.documentCount} workspace documents. Use search or a folder to narrow the view.`,
              )}
            </div>
          </Show>

          <section class="knowledge-dashboard-section knowledge-dashboard-section--collections">
            <SectionHeader title={i18n.tr('Integrasjoner', 'Integrations')} description={i18n.tr('Tilkoblede kildesystemer som mater dette kunnskapsområdet.', 'Connected source systems feeding this knowledge space.')} />
            <div class="knowledge-doc-card-grid">
              <Show
                when={collectionCards().length > 0}
                fallback={(
                  <EmptyPanel
                    title={i18n.tr('Ingen integrasjoner tilkoblet', 'No integrations connected')}
                    description={i18n.tr('Start en arbeidsområdetilkobling fra Legg til kilde for å hente inn sanntidskunnskap.', 'Start a workspace connection from Add source to pull in live knowledge.')}
                  />
                )}
              >
                <For each={collectionCards()}>
                  {(collection) => <KnowledgeCollectionCard collection={collection} onConnect={props.onAddSource} />}
                </For>
              </Show>
            </div>
          </section>

          <section class="knowledge-dashboard-section knowledge-dashboard-section--shortcuts">
            <SectionHeader title={i18n.tr('Mapper', 'Folders')} description={i18n.tr('Snarveier til kildegruppene arbeidsområdet ditt bruker mest.', 'Shortcuts into the source groups your workspace uses most.')} />
            <div class="knowledge-shortcut-grid">
              <Show
                when={props.liveKnowledge.folders.length > 0}
                fallback={(
                  <EmptyPanel
                    title={i18n.tr('Ingen kildegrupper ennå', 'No source groups yet')}
                    description={i18n.tr('Koble til en integrasjon eller importer filer for å begynne å bygge grupperte kunnskapsmapper.', 'Connect an integration or import files to start building grouped knowledge folders.')}
                  />
                )}
              >
                <For each={props.liveKnowledge.folders}>
                  {(folder) => <FolderCard folder={folder} />}
                </For>
              </Show>
            </div>
          </section>

          <KnowledgePulsePanel dataPlane={props.liveKnowledge.dataPlane} />

          <WebSourcesPanel webSources={props.liveKnowledge.webSources} />

          <Show when={props.liveKnowledge.sources.length > 0}>
            <LiveSourceInspector liveKnowledge={props.liveKnowledge} />
          </Show>
        </div>

        <aside class="knowledge-dashboard__aside">
          <FilesTable files={props.liveKnowledge.files} searchQuery={props.searchQuery} onSearchChange={props.onSearchChange} />
          <MetricPanel metrics={props.liveKnowledge.metricCards} />
        </aside>
      </div>
    </main>
  )
}

function LiveSourceInspector(props: { liveKnowledge: LiveKnowledgePayload }) {
  const i18n = useI18n()
  // The document being shared (null = dialog closed). Only the id + visibility
  // are needed; the ShareDialog itself is gated by the honesty gate.
  const [shareTarget, setShareTarget] = createSignal<{
    id: string
    visibility?: 'private' | 'org' | 'shared'
  } | null>(null)

  return (
    <section class="velion-panel knowledge-source-inspector">
      <div class="knowledge-source-inspector__header">
        <div>
          <h2>{i18n.tr('Kildebevis', 'Source evidence')}</h2>
          <p>{i18n.tr(
            'Dokumenter, utdrag, graf-koblinger og synkroniseringsstatus for kildesystemer fra den levende kunnskapsstakken.',
            'Documents, chunks, graph links, and source-system sync state from the live knowledge stack.',
          )}</p>
        </div>
        <span>
          {props.liveKnowledge.graph.available
            ? i18n.tr(
                `${props.liveKnowledge.graph.nodeCount} noder · ${props.liveKnowledge.graph.edgeCount} kanter`,
                `${props.liveKnowledge.graph.nodeCount} nodes · ${props.liveKnowledge.graph.edgeCount} edges`,
              )
            : i18n.tr(
                `${props.liveKnowledge.dataPlane.documentCount} dokumenter i arbeidsområdet`,
                `${props.liveKnowledge.dataPlane.documentCount} workspace documents`,
              )}
        </span>
      </div>
      <div class="knowledge-source-evidence-grid">
        <For each={props.liveKnowledge.sources.slice(0, 6)}>
          {(source) => (
            <article class="knowledge-source-evidence-card">
              <div class="knowledge-source-evidence-card__heading">
                <div>
                  <div class="knowledge-source-evidence-card__title-row">
                    <h3>{source.title}</h3>
                    <PrivacyBadge visibility={source.visibility} />
                  </div>
                  <p>{source.provider}</p>
                </div>
                <div class="knowledge-source-evidence-card__actions">
                  {/* Phase 6 freshness chip: 'Indexed' (embedded_at set) → Ready;
                      Pending review / Re-indexing surface as the live indexing state. */}
                  <span class="knowledge-status-chip" data-status={source.status}>
                    {source.status === 'Indexed' ? i18n.tr('Klar', 'Ready') : source.status}
                  </span>
                  <Show when={isGateOpen()}>
                    <button
                      type="button"
                      class="knowledge-source-share-button"
                      onClick={() =>
                        setShareTarget({ id: source.id, visibility: source.visibility })
                      }
                    >
                      {i18n.tr('Del', 'Share')}
                    </button>
                  </Show>
                </div>
              </div>
              <p class="knowledge-source-evidence-card__copy">{source.description}</p>
              <div class="knowledge-tag-row">
                <For each={source.tags.concat(source.related).slice(0, 4)}>
                  {(tag) => <span>{tag}</span>}
                </For>
              </div>
            </article>
          )}
        </For>
      </div>
      <Show when={shareTarget()}>
        {(target) => (
          <ShareDialog
            docId={target().id}
            visibility={target().visibility}
            onClose={() => setShareTarget(null)}
          />
        )}
      </Show>
    </section>
  )
}

function SectionHeader(props: { title: string; description: string }) {
  return (
    <div class="knowledge-section-header">
      <h2>{props.title}</h2>
      <p>{props.description}</p>
    </div>
  )
}

type DashboardCollection = {
  description: string
  Icon: KnowledgeIcon
  id: string
  meta: string
  actionLabel?: string
  title: string
  tone: 'ink' | 'soft' | 'warm'
}

function buildKnowledgeCollectionCards(
  payload: LiveKnowledgePayload,
  tr: (noText: string, enText: string) => string,
): DashboardCollection[] {
  const integrationCards = payload.integrations.map((integration, index) => ({
    description: integration.detail || tr(`${integration.documents} tilgjengelig for henting.`, `${integration.documents} available to retrieval.`),
    Icon: providerIcon(integration.providerKey),
    id: integration.id,
    meta: `${integration.documents} · ${integration.freshness}`,
    actionLabel: tr('Administrer', 'Manage'),
    title: integration.name,
    tone: index % 3 === 1 ? 'soft' : index % 3 === 2 ? 'warm' : 'ink',
  } satisfies DashboardCollection))
  const sourceCards = payload.sources.slice(0, 6).map((source, index) => ({
    description: source.description || tr(`${source.provider}-kilde koblet til Kunnskap.`, `${source.provider} source connected to Knowledge.`),
    Icon: sourceTypeIcon[source.type],
    id: source.id,
    meta: tr(`${source.provider} · ${source.chunks} utdrag`, `${source.provider} · ${source.chunks} chunks`),
    actionLabel: undefined,
    title: source.title,
    tone: index % 3 === 1 ? 'soft' : index % 3 === 2 ? 'warm' : 'ink',
  } satisfies DashboardCollection))

  return [...integrationCards, ...sourceCards].slice(0, 6)
}

function providerIcon(providerKey: string): KnowledgeIcon {
  if (providerKey === 'web') return Globe2
  if (providerKey === 'notion') return Blocks
  if (providerKey === 'microsoft') return FileText
  if (providerKey === 'google') return Grid2X2
  return Sparkles
}

function KnowledgeCollectionCard(props: {
  collection: DashboardCollection
  onConnect: () => void
}) {
  return (
    <article class={cn('knowledge-doc-card', `knowledge-doc-card--${props.collection.tone}`)}>
      <div class="knowledge-doc-card__icon" aria-hidden="true">
        <props.collection.Icon class="size-4" strokeWidth={1.8} />
      </div>
      <div class="knowledge-doc-card__body">
        <h3>{props.collection.title}</h3>
        <p>{props.collection.description}</p>
      </div>
      <div class="knowledge-doc-card__footer">
        <span>{props.collection.meta}</span>
        <Show when={props.collection.actionLabel}>
          <button type="button" onClick={() => props.onConnect()}>{props.collection.actionLabel}</button>
        </Show>
      </div>
    </article>
  )
}

function FolderCard(props: { folder: LiveKnowledgeFolder }) {
  const i18n = useI18n()
  const linkedSources = () => props.folder.connections.length
  const linkedSourcesLabel = () => i18n.tr(`${linkedSources()} tilknyttede kilder`, `${linkedSources()} linked sources`)

  return (
    <article class="knowledge-folder-card">
      <div class={cn('knowledge-folder-card__visual', folderToneClass[props.folder.tone])}>
        <div class="knowledge-folder-card__visual-glow" aria-hidden="true" />
        <div class="knowledge-folder-card__sheet-stack" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
        <div class="knowledge-folder-card__folder-shape" aria-hidden="true">
          <span class="knowledge-folder-card__folder-tab" />
          <span class="knowledge-folder-card__folder-face" />
        </div>
        <div class="knowledge-folder-card__connection-badges" aria-label={linkedSourcesLabel()}>
          <For each={props.folder.connections.slice(0, 3)}>
            {(connection) => <span title={connection}>{connection.slice(0, 1).toUpperCase()}</span>}
          </For>
        </div>
      </div>

      <div class="knowledge-folder-card__body">
        <div class="knowledge-folder-card__heading">
          <div>
            <h3>{props.folder.title}</h3>
            <p>{props.folder.subtitle}</p>
          </div>
          <span class="knowledge-folder-card__arrow" aria-hidden="true">↗</span>
        </div>

        <div class="knowledge-folder-card__stats">
          <span class="knowledge-folder-card__stat-primary">
            <Folder class="size-4" strokeWidth={1.7} />
            <strong>{props.folder.primaryValue}</strong>
            <small>{props.folder.primaryLabel}</small>
          </span>
          <span class="knowledge-folder-card__stat-secondary">
            {props.folder.secondaryValue} {props.folder.secondaryLabel}
          </span>
        </div>

        <div class="knowledge-folder-card__linked-meter" aria-label={linkedSourcesLabel()}>
          <span>{i18n.tr('Tilknyttede kilder', 'Linked sources')}</span>
          <div>
            <For each={[0, 1, 2, 3, 4]}>
              {(segment) => <i class={segment < linkedSources() ? 'knowledge-folder-card__meter-segment--active' : ''} />}
            </For>
          </div>
        </div>
      </div>
    </article>
  )
}

function KnowledgePulsePanel(props: {
  dataPlane: LiveKnowledgePayload['dataPlane']
}) {
  const i18n = useI18n()
  const coverage = () => {
    if (props.dataPlane.documentCount <= 0) return 0
    return Math.min(100, Math.round((props.dataPlane.indexedCount / props.dataPlane.documentCount) * 100))
  }

  return (
    <section class="knowledge-pulse-grid" aria-label={i18n.tr('Kunnskapspuls', 'Knowledge pulse')}>
      <article class="velion-panel knowledge-pulse-card knowledge-pulse-card--activity">
        <div class="knowledge-pulse-card__header">
          <div>
            <span class="knowledge-card-eyebrow">{i18n.tr('Arbeidsområdepuls', 'Workspace pulse')}</span>
            <h2>{i18n.tr('Dokumentasjonsengasjement', 'Documentation engagement')}</h2>
          </div>
          <span class="knowledge-pulse-card__status"><span /> {i18n.tr('Sanntidsøyeblikksbilde', 'Live snapshot')}</span>
        </div>
        <div class="knowledge-pulse-card__activity">
          <div class="knowledge-pulse-card__activity-copy">
            <strong>{props.dataPlane.indexedCount}</strong>
            <span>{i18n.tr('dokumenter klare for henting', 'documents ready for retrieval')}</span>
            <p>{i18n.tr('Historisk visning og redigeringstelemetri vises her når aktivitetsstrømmen er koblet til.', 'Historical view and edit telemetry will appear here once the activity feed is connected.')}</p>
          </div>
          <div class="knowledge-pulse-card__empty-graph" aria-label={i18n.tr('Ingen aktivitetstelemetri tilgjengelig', 'No activity telemetry available')}>
            <span>{i18n.tr('Ingen aktivitetstelemetri ennå', 'No activity telemetry yet')}</span>
          </div>
        </div>
        <div class="knowledge-pulse-card__axis" aria-hidden="true">
          <span>{i18n.tr('Kilder', 'Sources')}</span><span>{i18n.tr('Synk', 'Sync')}</span><span>{i18n.tr('Indeks', 'Index')}</span><span>{i18n.tr('Svar', 'Answers')}</span>
        </div>
      </article>

      <article class="velion-panel knowledge-pulse-card knowledge-pulse-card--coverage">
        <div class="knowledge-pulse-card__header">
          <div>
            <span class="knowledge-card-eyebrow">{i18n.tr('Dekning', 'Coverage')}</span>
            <h2>{i18n.tr('Indeksdekning', 'Index coverage')}</h2>
          </div>
          <MoreHorizontal class="size-4" aria-hidden="true" />
        </div>
        <div class="knowledge-pulse-donut" style={{ background: `conic-gradient(#171717 ${coverage()}%, #e7e6e1 0)` }}>
          <div>
            <strong>{coverage()}%</strong>
            <span>{i18n.tr('indeksert', 'indexed')}</span>
          </div>
        </div>
        <p class="knowledge-pulse-card__footnote">
          {i18n.tr(
            `${props.dataPlane.indexedCount} av ${props.dataPlane.documentCount} dokumenter er klare.`,
            `${props.dataPlane.indexedCount} of ${props.dataPlane.documentCount} documents are ready.`,
          )}
        </p>
      </article>
    </section>
  )
}

function WebSourcesPanel(props: { webSources: LiveKnowledgeWebSource[] }) {
  const i18n = useI18n()
  return (
    <section>
      <SectionHeader
        title={i18n.tr('Sporede nettkilder', 'Tracked web sources')}
        description={i18n.tr('Quarry-baserte nettstedsmål som kan oppdateres inn i kunnskapsområdet.', 'Quarry-backed website targets that can refresh into the knowledge workspace.')}
      />
      <div class="knowledge-web-grid">
        <Show
          when={props.webSources.length > 0}
          fallback={(
            <EmptyPanel
              title={i18n.tr('Ingen sporede nettsteder ennå', 'No tracked websites yet')}
              description={i18n.tr('Start en Quarry-gjennomsøking fra Legg til kilde for å flytte nettstedsinnhold inn i innhentings- og kunnskapsstakken.', 'Start a Quarry crawl from Add source to move website content into the ingestion and knowledge stack.')}
            />
          )}
        >
          <For each={props.webSources}>
            {(source) => (
              <article class="velion-panel knowledge-web-card">
                <div class="knowledge-web-card__top">
                  <div>
                    <h3>{source.name}</h3>
                    <a href={source.url} target="_blank" rel="noreferrer">{source.url}</a>
                  </div>
                  <span class={cn('knowledge-status-pill', webStatusClass(source.status))}>
                    {formatGraphGroup(source.status)}
                  </span>
                </div>
                <div class="knowledge-web-card__meta">
                  <span>{source.kind}</span>
                  <span>{source.updated}</span>
                </div>
              </article>
            )}
          </For>
        </Show>
      </div>
    </section>
  )
}

function FilesTable(props: {
  files: LiveKnowledgeFile[]
  onSearchChange: (query: string) => void
  searchQuery: string
}) {
  const i18n = useI18n()
  return (
    <section class="velion-panel knowledge-files-panel knowledge-files-panel--archive">
      <div class="knowledge-files-panel__header">
        <div>
          <h2><span class="sr-only">{i18n.tr('Filer', 'Files')}</span><span aria-hidden="true">{i18n.tr('Sprint-arkiver', 'Sprint Archives')}</span></h2>
          <p>{i18n.tr('Nyeste filer tilgjengelig for henting.', 'Latest files available to retrieval.')}</p>
        </div>
        <div class="knowledge-archive-actions" aria-hidden="true">
          <span><Bookmark class="size-4" /></span>
          <span><Pencil class="size-4" /></span>
          <span><Trash2 class="size-4" /></span>
          <span><Mail class="size-4" /></span>
          <span><Download class="size-4" /></span>
        </div>
        <label class="knowledge-files-search">
          <Search class="size-4" />
          <VelionInput
            aria-label={i18n.tr('Søk i filer og kilder', 'Search files and sources')}
            value={props.searchQuery}
            onInput={(event) => props.onSearchChange(event.currentTarget.value)}
            placeholder={i18n.tr('Søk i filer og kilder …', 'Search files and sources...')}
          />
        </label>
      </div>

      <div class="knowledge-archive-list">
        <Show
          when={props.files.length > 0}
          fallback={<p class="knowledge-archive-empty">{i18n.tr('Ingen filer for henting samsvarer med gjeldende filtre.', 'No retrieval files match the current filters.')}</p>}
        >
          <For each={props.files}>
            {(file) => {
              const Icon = sourceTypeIcon[file.type]
              return (
                <article class="knowledge-archive-file">
                  <div class="knowledge-archive-file__icon"><Dynamic component={Icon} class="size-4" /></div>
                  <div class="knowledge-archive-file__body">
                    <h3>{file.name}</h3>
                    <p>{i18n.tr(`Delt av ${file.addedBy}`, `Shared by ${file.addedBy}`)}</p>
                    <p>{i18n.tr(`${file.source} · Oppdatert ${file.updated}`, `${file.source} · Updated ${file.updated}`)}</p>
                  </div>
                  <Clock3 class="knowledge-archive-file__clock size-4" aria-hidden="true" />
                </article>
              )
            }}
          </For>
        </Show>
      </div>
    </section>
  )
}

function MetricPanel(props: { metrics: LiveKnowledgeMetric[] }) {
  return (
    <aside class="knowledge-metric-panel">
      <For each={props.metrics}>
        {(metric) => (
          <article class="velion-panel knowledge-metric-card">
            <div class="knowledge-metric-card__top">
              <div>
                <h2>{metric.label}</h2>
                <strong>{metric.value}</strong>
              </div>
              <div class="knowledge-mini-bars" aria-hidden="true">
                <For each={[0, 1, 2, 3]}>
                  {(bar) => (
                    <span class={cn(bar === 2 && metric.tone === 'good' ? 'knowledge-mini-bars__bar--good' : '', bar === 2 && metric.tone === 'warn' ? 'knowledge-mini-bars__bar--warn' : '')} />
                  )}
                </For>
              </div>
            </div>
            <p class={cn('knowledge-metric-delta', metric.tone === 'good' ? 'knowledge-metric-delta--good' : 'knowledge-metric-delta--warn')}>
              {metric.delta}
            </p>
          </article>
        )}
      </For>
    </aside>
  )
}

function GraphCanvas(props: {
  graph: LiveKnowledgePayload['graph']
  relatedSources: LiveKnowledgeSource[]
  selectedNode: LiveKnowledgeGraphNode | null
  onSelectNode: (nodeId: string) => void
}) {
  return (
    <main class="knowledge-graph-layout">
      <GraphPanel graph={props.graph} selectedNode={props.selectedNode} onSelectNode={props.onSelectNode} />
      <GraphInspectorPanel selectedNode={props.selectedNode} relatedSources={props.relatedSources} />
    </main>
  )
}

function ChunksCanvas(props: {
  selectedSource: LiveKnowledgeSource | null
  sources: LiveKnowledgeSource[]
  onSelectSource: (sourceId: string) => void
}) {
  const i18n = useI18n()
  return (
    <main class="knowledge-chunks-layout">
      <section class="velion-panel knowledge-chunks-sources">
        <h2>{i18n.tr('Kilder', 'Sources')}</h2>
        <div>
          <Show
            when={props.sources.length > 0}
            fallback={<p>{i18n.tr('Ingen dokumenter med utdrag ennå.', 'No chunk-backed documents yet.')}</p>}
          >
            <For each={props.sources}>
              {(source) => (
                <button
                  type="button"
                  aria-pressed={source.id === props.selectedSource?.id}
                  onClick={() => props.onSelectSource(source.id)}
                  class={cn('knowledge-chunk-source-button', source.id === props.selectedSource?.id && 'knowledge-chunk-source-button--active')}
                >
                  <span>{source.title}</span>
                  <small>{source.chunks}</small>
                </button>
              )}
            </For>
          </Show>
        </div>
      </section>
      <ChunksPanel source={props.selectedSource} />
    </main>
  )
}

function GraphPanel(props: {
  graph: LiveKnowledgePayload['graph']
  selectedNode: LiveKnowledgeGraphNode | null
  onSelectNode: (nodeId: string) => void
}) {
  const i18n = useI18n()
  const nodeById = createMemo(() => new Map(props.graph.nodes.map((node) => [node.id, node])))

  return (
    <section class="velion-panel knowledge-graph-panel" aria-label={i18n.tr('RAGGraph-relasjonskart', 'RAGGraph relationship map')}>
      <div class="knowledge-graph-panel__header">
        <div>
          <h2>{i18n.tr('RAGGraph-relasjonskart', 'RAGGraph relationship map')}</h2>
          <p>{i18n.tr('Entitetsrelasjoner forankret i kildeutdrag fra Data Plane v2.', 'Entity relationships grounded in source chunks from Data Plane v2.')}</p>
        </div>
        <GitBranch class="size-5" />
      </div>

      <div class="knowledge-graph-canvas">
        <div class="knowledge-graph-grid" aria-hidden="true" />
        <svg class="knowledge-graph-svg" viewBox="0 0 640 420" role="img" aria-label={i18n.tr('Kunnskapskildegraf', 'Knowledge source graph')}>
          <For each={props.graph.links}>
            {(link) => {
              const from = () => nodeById().get(link.from)
              const to = () => nodeById().get(link.to)
              const selected = () => link.from === props.selectedNode?.id || link.to === props.selectedNode?.id
              return (
                <Show when={from() && to()}>
                  <line
                    x1={from()!.x}
                    y1={from()!.y}
                    x2={to()!.x}
                    y2={to()!.y}
                    stroke={selected() ? '#111111' : '#B8B9B1'}
                    stroke-width={selected() ? link.strength : 1}
                    stroke-opacity={selected() ? 0.82 : 0.48}
                  />
                </Show>
              )
            }}
          </For>
        </svg>

        <div class="knowledge-graph-node-layer">
          <For each={props.graph.nodes}>
            {(node) => {
              const active = () => node.id === props.selectedNode?.id
              return (
                <button
                  type="button"
                  aria-label={i18n.tr(`Velg ${node.label}`, `Select ${node.label}`)}
                  onClick={() => props.onSelectNode(node.id)}
                  class={cn('knowledge-graph-node-button', active() && 'knowledge-graph-node-button--active')}
                  style={{
                    left: `${(node.x / 640) * 100}%`,
                    top: `${(node.y / 420) * 100}%`,
                    width: `${node.radius * 2}px`,
                    height: `${node.radius * 2}px`,
                  }}
                >
                  <span class={cn('knowledge-graph-node-dot', graphToneClass[node.tone])} />
                  <span class="knowledge-graph-node-label">{node.label}</span>
                </button>
              )
            }}
          </For>
        </div>
      </div>
    </section>
  )
}

function GraphInspectorPanel(props: {
  relatedSources: LiveKnowledgeSource[]
  selectedNode: LiveKnowledgeGraphNode | null
}) {
  const i18n = useI18n()
  const chunkEvidence = () => props.relatedSources
    .flatMap((source) => source.chunksPreview.map((chunk) => ({ ...chunk, sourceTitle: source.title })))
    .slice(0, 4)

  return (
    <Show
      keyed
      when={props.selectedNode}
      fallback={(
      <EmptyPanel
        title={i18n.tr('Ingen grafnode valgt', 'No graph node selected')}
        description={i18n.tr('Velg en node i grafen for å inspisere relaterte hentekilder og utdragsbevis.', 'Choose a node in the graph to inspect related retrieval sources and chunk evidence.')}
      />
      )}
    >
      {(node) => (
        <section class="velion-panel knowledge-graph-inspector">
          <h2>{node.label}</h2>
          <p>
            {i18n.tr(
              `${formatGraphGroup(node.group)} · ${node.sourceRefs.length} tilknyttet utdragsreferanse${node.sourceRefs.length === 1 ? '' : 'r'}.`,
              `${formatGraphGroup(node.group)} · ${node.sourceRefs.length} linked chunk reference${node.sourceRefs.length === 1 ? '' : 's'}.`,
            )}
          </p>

          <div class="knowledge-tag-row">
            <Show
              when={props.relatedSources.length > 0}
              fallback={<span>{i18n.tr('Ingen dokumentforhåndsvisninger ble funnet for denne noden ennå.', 'No document previews were resolved for this node yet.')}</span>}
            >
              <For each={props.relatedSources}>
                {(source) => <span>{source.title}</span>}
              </For>
            </Show>
          </div>

          <div class="knowledge-chunk-list">
            <For each={chunkEvidence()}>
              {(chunk) => (
                <ChunkPreviewCard
                  id={chunk.id}
                  score={chunk.score}
                  sourceTitle={chunk.sourceTitle}
                  text={chunk.text}
                  title={chunk.title}
                />
              )}
            </For>
          </div>
        </section>
      )}
    </Show>
  )
}

function ChunksPanel(props: { source: LiveKnowledgeSource | null }) {
  const i18n = useI18n()
  return (
    <Show
      keyed
      when={props.source}
      fallback={(
      <EmptyPanel
        title={i18n.tr('Ingen utdragskilde valgt', 'No chunk source selected')}
        description={i18n.tr('Velg et dokument for å inspisere utdragene som for øyeblikket er tilgjengelige for henting.', 'Choose a document to inspect the chunks currently available to retrieval.')}
      />
      )}
    >
      {(source) => (
        <section class="velion-panel knowledge-chunks-panel">
          <h2>{source.title}</h2>
          <p>{source.description}</p>
          <div class="knowledge-tag-row">
            <For each={source.tags.concat(source.related).slice(0, 4)}>
              {(tag) => <span>{tag}</span>}
            </For>
          </div>
          <div class="knowledge-chunk-list">
            <For each={source.chunksPreview}>
              {(chunk) => (
                <ChunkPreviewCard
                  id={chunk.id}
                  score={chunk.score}
                  text={chunk.text}
                  title={chunk.title}
                />
              )}
            </For>
          </div>
        </section>
      )}
    </Show>
  )
}

function ChunkPreviewCard(props: {
  id: string
  score: string
  sourceTitle?: string
  text: string
  title: string
}) {
  return (
    <article class="knowledge-chunk-card">
      <div class="knowledge-chunk-card__meta">
        <span>
          <Blocks class="size-4" />
          {props.sourceTitle ?? props.id}
        </span>
        <small class={props.score.startsWith('#') ? '' : 'knowledge-chunk-score'}>{props.score}</small>
      </div>
      <h3>{props.title}</h3>
      <p>{props.text}</p>
    </article>
  )
}

function NoticeBanner(props: { notice: Notice }) {
  return (
    <section class={cn('knowledge-notice', props.notice.tone === 'good' ? 'knowledge-notice--good' : 'knowledge-notice--warn')}>
      {props.notice.message}
    </section>
  )
}

function EmptyPanel(props: {
  title: string
  description: string
}) {
  return (
    <section class="velion-panel knowledge-empty-panel">
      <h2>{props.title}</h2>
      <p>{props.description}</p>
    </section>
  )
}

function webStatusClass(status: string) {
  if (status === 'active') return 'knowledge-status--connected'
  if (status === 'running' || status === 'queued') return 'knowledge-status--syncing'
  return 'knowledge-status--review'
}

function popupFeatures() {
  return [
    'width=980',
    'height=760',
    'scrollbars=yes',
    'resizable=yes',
    'status=no',
    'toolbar=no',
    'location=no',
    'copyhistory=no',
    'menubar=no',
    'directories=no',
  ].join(',')
}

async function uploadKnowledgeFiles(files: File[], orgId: string) {
  const body = new FormData()
  for (const file of files) {
    body.append('files', file, file.name)
  }
  // imports-core multipart upload via the gateway (plural `imports`); requestForm
  // preserves the multipart boundary and unwraps the gateway `{ data }` envelope.
  // The gateway scopes the import to the active org via the x-velion-org-id header.
  return requestForm<UploadResult>('/api/v1/knowledge/imports/upload', body, {
    headers: { 'x-velion-org-id': orgId },
  })
}

function formatGraphGroup(group: string) {
  return group.replace(/[_:-]+/g, ' ').replace(/\b\w/g, (character) => character.toUpperCase())
}

function normalizeBlueprintRole(role: string): 'service' | 'sales' | 'ecommerce' | 'chatbot' | 'workflow' {
  if (role === 'service' || role === 'sales' || role === 'ecommerce' || role === 'chatbot' || role === 'workflow') {
    return role
  }
  return 'workflow'
}
