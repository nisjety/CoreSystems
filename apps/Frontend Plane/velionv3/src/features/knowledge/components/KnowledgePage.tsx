import { A } from '@solidjs/router'
import {
  ArrowUpRight,
  Blocks,
  ChevronDown,
  FilePlus2,
  FileText,
  GitBranch,
  Globe2,
  Grid2X2,
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
import { KnowledgeDiagnosticsPanel } from '@/features/knowledge/components/KnowledgeDiagnosticsPanel'
import { KnowledgeOperatingMapCanvas } from '@/features/knowledge/components/KnowledgeOperatingMapCanvas'
import { executeAction } from '@/shared/actions/action-client'
import {
  loadKnowledgeSources,
  type LiveKnowledgeCollection,
  type LiveKnowledgeFile,
  type LiveKnowledgeFolder,
  type LiveKnowledgeGraphNode,
  type LiveKnowledgeIntegration,
  type LiveKnowledgeMetric,
  type LiveKnowledgePayload,
  type LiveKnowledgeSource,
  type LiveKnowledgeSourceType,
  type LiveKnowledgeWebSource,
} from '@/shared/api/knowledge-live-client'
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

type CrawlStartResult = {
  createdAt: string
  id: string
  status: string
  target: string
}

const CONNECT_PROVIDERS = [
  { id: 'microsoft', label: 'Microsoft 365', detail: 'SharePoint, OneDrive, Teams, Outlook', sources: ['sharepoint', 'onedrive', 'teams', 'outlook'] },
  { id: 'google', label: 'Google Workspace', detail: 'Drive and docs', sources: ['google_drive', 'documents'] },
  { id: 'notion', label: 'Notion', detail: 'Pages and databases', sources: ['pages', 'databases'] },
  { id: 'github', label: 'GitHub', detail: 'Repos, README, issues', sources: ['repos', 'readme', 'issues'] },
  { id: 'slack', label: 'Slack', detail: 'Channels and thread history', sources: ['channels'] },
] as const

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
  blue: 'knowledge-folder-card__art--blue',
  gray: 'knowledge-folder-card__art--gray',
  green: 'knowledge-folder-card__art--green',
  warm: 'knowledge-folder-card__art--warm',
}

const integrationStatusClass: Record<LiveKnowledgeIntegration['status'], string> = {
  Connected: 'knowledge-status--connected',
  Review: 'knowledge-status--review',
  Syncing: 'knowledge-status--syncing',
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
  const [activeView, setActiveView] = createSignal<KnowledgeView>('overview')
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
        message: error instanceof Error ? error.message : 'Knowledge workspace could not be loaded.',
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
        message: error instanceof Error ? error.message : 'Operating Map could not be loaded.',
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
      setNotice({
        tone: failures.length > 0 ? 'warn' : 'good',
        message: failures.length > 0
          ? `Started ${result.integrationStarted + result.finspoStarted} syncs, but ${failures.length} sources still need review.`
          : `Started ${result.integrationStarted + result.finspoStarted} source syncs.`,
      })
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'Knowledge sync could not be started.',
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
      setNotice({
        tone: 'good',
        message: `Imports-core queued ${result.totalItems || files.length} file${files.length === 1 ? '' : 's'} for ingestion.`,
      })
      setAddSourceOpen(false)
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'File upload could not be started.',
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
          ? 'SharePoint drive registered and sync started in Finspo.'
          : 'SharePoint drive registered. Sync can be started from Knowledge.',
      })
      setAddSourceOpen(false)
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'SharePoint source could not be registered.',
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
        message: `${provider.label} authorization opened in a new window. Return here after approval to refresh the workspace.`,
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
        message: error instanceof Error ? error.message : 'Connection flow could not be started.',
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
        message: `Started a website crawl for ${result.target}. Track run ${result.id} in Ingestions while pages flow into Knowledge.`,
      })
      setAddSourceOpen(false)
      await loadKnowledgeWorkspace()
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'The website crawl could not be started.',
      })
    } finally {
      setBusyAction(null)
    }
  }

  async function handleGenerateOperatingMap() {
    setBusyAction('operating-map')
    setNotice(null)
    setOperatingMapEvents(['Generating Operating Map proposal from Knowledge evidence...'])
    try {
      const result = await generateOperatingMap(activeOrgId)
      setOperatingMapEvents((events) => [...events, `Proposal ${result.proposal.id} created.`])
      if (result.runId) {
        await streamOperatingMapRunEvents(activeOrgId, result.runId, {
          onEvent: (event) => setOperatingMapEvents((events) => [...events, event.detail]),
        }).catch((error) => {
          setOperatingMapEvents((events) => [...events, error instanceof Error ? error.message : 'Operating Map event stream failed.'])
        })
      }
      await loadOperatingMapWorkspace()
      setNotice({ tone: 'good', message: 'Operating Map proposal is ready for review.' })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'Operating Map generation could not be started.',
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
          ? 'Operating Map accepted and published into Knowledge wiki.'
          : 'Operating Map proposal rejected.',
      })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'Operating Map proposal could not be reviewed.',
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
      setNotice({ tone: 'good', message: `${blueprint.name} blueprint suggestion saved for Agents review.` })
    } catch (error) {
      setNotice({
        tone: 'warn',
        message: error instanceof Error ? error.message : 'Agent blueprint could not be queued.',
      })
    } finally {
      setBusyAction(null)
    }
  }

  return (
    <div class="knowledge-page-surface">
      <div class="knowledge-page-container">
        <WorkspaceHeader
          activeView={activeView()}
          collections={liveKnowledge()?.collections ?? []}
          selectedCollectionId={selectedCollectionId()}
          syncing={busyAction() === 'sync'}
          onActiveViewChange={setActiveView}
          onAddSource={() => setAddSourceOpen(true)}
          onCollectionChange={setSelectedCollectionId}
          onSync={() => void handleSync()}
        />

        <Show when={notice()}>
          {(currentNotice) => <NoticeBanner notice={currentNotice()} />}
        </Show>

        <Show when={loading() && !liveKnowledge()}>
          <section class="velion-panel knowledge-loading-panel">Loading knowledge workspace...</section>
        </Show>

        <Show when={!loading() && !liveKnowledge()}>
          <EmptyPanel
            title="Knowledge workspace unavailable"
            description="The page could not load live data from Data Plane v2 and the ingestion services."
          />
        </Show>

        <Show when={visibleKnowledge()}>
          <Show when={activeView() === 'overview'}>
            <OverviewCanvas
              liveKnowledge={visibleKnowledge()!}
              searchQuery={searchQuery()}
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
          providers={CONNECT_PROVIDERS}
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
  selectedCollectionId: string
  syncing: boolean
  onActiveViewChange: (view: KnowledgeView) => void
  onAddSource: () => void
  onCollectionChange: (collectionId: string) => void
  onSync: () => void
}) {
  return (
    <header class="knowledge-header">
      <div class="knowledge-header__main">
        <div class="knowledge-header__select-wrap">
          <VelionSelect
            aria-label="Select knowledge collection"
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
        <p class="knowledge-header__copy">
          Overview of folders, integrations, files, and retrieval health for this knowledge space.
        </p>
      </div>

      <div class="knowledge-header__actions">
        <SegmentedView activeView={props.activeView} onActiveViewChange={props.onActiveViewChange} />
        <A href="/ingestions" class="button button--secondary button--md knowledge-link-button">
          <ArrowUpRight class="size-4" />
          Ingestions
        </A>
        <Button size="md" onClick={props.onSync} disabled={props.syncing}>
          <RefreshCw class={cn('size-4', props.syncing && 'knowledge-spin')} />
          Sync
        </Button>
        <Button variant="primary" size="md" onClick={props.onAddSource}>
          <FilePlus2 class="size-4" />
          Add source
        </Button>
      </div>
    </header>
  )
}

function SegmentedView(props: {
  activeView: KnowledgeView
  onActiveViewChange: (view: KnowledgeView) => void
}) {
  const views: Array<{ Icon: KnowledgeIcon; id: KnowledgeView; label: string }> = [
    { id: 'overview', label: 'Overview', Icon: Grid2X2 },
    { id: 'operating-map', label: 'AI Map', Icon: MapIcon },
    { id: 'graph', label: 'Graph', Icon: Network },
    { id: 'chunks', label: 'Chunks', Icon: Table2 },
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
  liveKnowledge: LiveKnowledgePayload
  onSearchChange: (query: string) => void
  searchQuery: string
}) {
  return (
    <main class="knowledge-main-stack">
      <section>
        <SectionHeader title="Folders" description="Browse the strongest source groups and where their files come from." />
        <div class="knowledge-folder-grid">
          <Show
            when={props.liveKnowledge.folders.length > 0}
            fallback={<EmptyPanel title="No source groups yet" description="Connect an integration or import files to start building grouped knowledge folders." />}
          >
            <For each={props.liveKnowledge.folders}>
              {(folder) => <FolderCard folder={folder} />}
            </For>
          </Show>
        </div>
      </section>

      <section>
        <SectionHeader title="Integrations" description="Connected source systems feeding this knowledge space." />
        <div class="knowledge-integration-grid">
          <Show
            when={props.liveKnowledge.integrations.length > 0}
            fallback={<EmptyPanel title="No integrations connected" description="Start a workspace connection from Add source to pull in live knowledge." />}
          >
            <For each={props.liveKnowledge.integrations}>
              {(integration) => <IntegrationCard integration={integration} />}
            </For>
          </Show>
        </div>
      </section>

      <WebSourcesPanel webSources={props.liveKnowledge.webSources} />

      <KnowledgeDiagnosticsPanel
        dataPlane={props.liveKnowledge.dataPlane}
        diagnostics={props.liveKnowledge.diagnostics}
      />

      <Show when={props.liveKnowledge.sources.length > 0}>
        <LiveSourceInspector liveKnowledge={props.liveKnowledge} />
      </Show>

      <section class="knowledge-files-metrics-grid">
        <FilesTable files={props.liveKnowledge.files} searchQuery={props.searchQuery} onSearchChange={props.onSearchChange} />
        <MetricPanel metrics={props.liveKnowledge.metricCards} />
      </section>
    </main>
  )
}

function LiveSourceInspector(props: { liveKnowledge: LiveKnowledgePayload }) {
  return (
    <section class="velion-panel knowledge-source-inspector">
      <div class="knowledge-source-inspector__header">
        <div>
          <h2>Source evidence</h2>
          <p>Documents, chunks, graph links, and source-system sync state from the live knowledge stack.</p>
        </div>
        <span>
          {props.liveKnowledge.graph.available
            ? `${props.liveKnowledge.graph.nodeCount} nodes · ${props.liveKnowledge.graph.edgeCount} edges`
            : `${props.liveKnowledge.dataPlane.documentCount} Data Plane documents`}
        </span>
      </div>
      <div class="knowledge-source-evidence-grid">
        <For each={props.liveKnowledge.sources.slice(0, 6)}>
          {(source) => (
            <article class="knowledge-source-evidence-card">
              <div class="knowledge-source-evidence-card__heading">
                <div>
                  <h3>{source.title}</h3>
                  <p>{source.provider}</p>
                </div>
                <span>{source.status}</span>
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

function FolderCard(props: { folder: LiveKnowledgeFolder }) {
  return (
    <button type="button" class="knowledge-folder-card">
      <div class={cn('knowledge-folder-card__art', folderToneClass[props.folder.tone])}>
        <div class="knowledge-folder-card__shine" />
        <div class="knowledge-folder-card__label">
          {props.folder.connections[0] ?? 'Velion'}
          <br />
          Source Group
        </div>
      </div>

      <div class="knowledge-folder-card__base">
        <div class="knowledge-folder-card__tab" />
        <div class="knowledge-folder-card__tab-angle" />
      </div>

      <div class="knowledge-folder-card__title">
        <h3>{props.folder.title}</h3>
        <p>{props.folder.subtitle}</p>
      </div>

      <div class="knowledge-folder-card__stats">
        <div>
          <strong>{props.folder.primaryValue}</strong>
          <span>{props.folder.primaryLabel}</span>
        </div>
        <p>{props.folder.secondaryValue} {props.folder.secondaryLabel}</p>
      </div>
    </button>
  )
}

function IntegrationCard(props: { integration: LiveKnowledgeIntegration }) {
  return (
    <article class="velion-panel knowledge-integration-card">
      <div class="knowledge-integration-card__top">
        <span>{props.integration.name.slice(0, 1)}</span>
        <small class={cn('knowledge-status-pill', integrationStatusClass[props.integration.status])}>
          {props.integration.status}
        </small>
      </div>
      <h3>{props.integration.name}</h3>
      <div class="knowledge-integration-card__stats">
        <div>
          <p>Coverage</p>
          <strong>{props.integration.documents}</strong>
        </div>
        <div>
          <p>Freshness</p>
          <strong>{props.integration.freshness}</strong>
        </div>
      </div>
      <Show when={props.integration.detail}>
        <p class="knowledge-integration-card__detail">{props.integration.detail}</p>
      </Show>
    </article>
  )
}

function WebSourcesPanel(props: { webSources: LiveKnowledgeWebSource[] }) {
  return (
    <section>
      <SectionHeader title="Tracked web sources" description="Quarry-backed website targets that can refresh into the knowledge workspace." />
      <div class="knowledge-web-grid">
        <Show
          when={props.webSources.length > 0}
          fallback={<EmptyPanel title="No tracked websites yet" description="Start a Quarry crawl from Add source to move website content into the ingestion and knowledge stack." />}
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
  return (
    <section class="velion-panel knowledge-files-panel">
      <div class="knowledge-files-panel__header">
        <div>
          <h2>Files</h2>
          <p>Latest files available to retrieval.</p>
        </div>
        <label class="knowledge-files-search">
          <Search class="size-4" />
          <VelionInput
            aria-label="Search files and sources"
            value={props.searchQuery}
            onInput={(event) => props.onSearchChange(event.currentTarget.value)}
            placeholder="Search files and sources..."
          />
        </label>
      </div>

      <div class="knowledge-table-scroll">
        <table class="knowledge-files-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Added By</th>
              <th>Source</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            <Show
              when={props.files.length > 0}
              fallback={(
                <tr>
                  <td colSpan={4}>No retrieval files match the current filters.</td>
                </tr>
              )}
            >
              <For each={props.files}>
                {(file) => {
                  const Icon = sourceTypeIcon[file.type]
                  return (
                    <tr>
                      <td>
                        <span class="knowledge-file-name">
                          <Dynamic component={Icon} class="size-4" />
                          {file.name}
                        </span>
                      </td>
                      <td>{file.addedBy}</td>
                      <td>{file.source}</td>
                      <td>{file.updated}</td>
                    </tr>
                  )
                }}
              </For>
            </Show>
          </tbody>
        </table>
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
  return (
    <main class="knowledge-chunks-layout">
      <section class="velion-panel knowledge-chunks-sources">
        <h2>Sources</h2>
        <div>
          <Show
            when={props.sources.length > 0}
            fallback={<p>No chunk-backed documents yet.</p>}
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
  const nodeById = createMemo(() => new Map(props.graph.nodes.map((node) => [node.id, node])))

  return (
    <section class="velion-panel knowledge-graph-panel" aria-label="RAGGraph relationship map">
      <div class="knowledge-graph-panel__header">
        <div>
          <h2>RAGGraph relationship map</h2>
          <p>Entity relationships grounded in source chunks from Data Plane v2.</p>
        </div>
        <GitBranch class="size-5" />
      </div>

      <div class="knowledge-graph-canvas">
        <div class="knowledge-graph-grid" aria-hidden="true" />
        <svg class="knowledge-graph-svg" viewBox="0 0 640 420" role="img" aria-label="Knowledge source graph">
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
                  aria-label={`Select ${node.label}`}
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
  const chunkEvidence = () => props.relatedSources
    .flatMap((source) => source.chunksPreview.map((chunk) => ({ ...chunk, sourceTitle: source.title })))
    .slice(0, 4)

  return (
    <Show
      keyed
      when={props.selectedNode}
      fallback={(
      <EmptyPanel
        title="No graph node selected"
        description="Choose a node in the graph to inspect related retrieval sources and chunk evidence."
      />
      )}
    >
      {(node) => (
        <section class="velion-panel knowledge-graph-inspector">
          <h2>{node.label}</h2>
          <p>
            {formatGraphGroup(node.group)} · {node.sourceRefs.length} linked chunk reference{node.sourceRefs.length === 1 ? '' : 's'}.
          </p>

          <div class="knowledge-tag-row">
            <Show
              when={props.relatedSources.length > 0}
              fallback={<span>No document previews were resolved for this node yet.</span>}
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
  return (
    <Show
      keyed
      when={props.source}
      fallback={(
      <EmptyPanel
        title="No chunk source selected"
        description="Choose a document to inspect the chunks currently available to retrieval."
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
