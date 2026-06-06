"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import {
  ArrowUpRight,
  Blocks,
  ChevronDown,
  FilePlus2,
  GitBranch,
  Grid2X2,
  Network,
  RefreshCw,
  Search,
  Table2,
} from "lucide-react";

import {
  VelionButton,
  VelionInput,
  VelionSegmented,
  VelionSegmentedButton,
  VelionSelect,
} from "@/components/ui/velion-ui";
import { KnowledgeAddSourceModal } from "@/features/knowledge-v2/components/KnowledgeAddSourceModal";
import type {
  LiveKnowledgeCollection,
  LiveKnowledgeFile,
  LiveKnowledgeFolder,
  LiveKnowledgeGraphNode,
  LiveKnowledgeIntegration,
  LiveKnowledgeMetric,
  LiveKnowledgePayload,
  LiveKnowledgeSource,
  LiveKnowledgeWebSource,
} from "@/features/knowledge-v2/lib/knowledge-live";
import { createConnectSession } from "@/features/onboarding-v2/lib/onboarding-api";
import { sourceTypeIcon } from "@/features/knowledge-v2/lib/knowledge-data";
import { apiGet, apiSend } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

type KnowledgeView = "overview" | "graph" | "chunks";

type Notice = {
  message: string;
  tone: "good" | "warn";
};

type SyncResult = {
  finspoFailures: string[];
  finspoStarted: number;
  integrationFailures: string[];
  integrationStarted: number;
};

type UploadResult = {
  failedItems: number;
  id: string;
  processedItems: number;
  sourceType: string;
  status: string;
  totalItems: number;
};

type SharePointCreateResult = {
  id: string;
  syncStarted: boolean;
};

type CrawlStartResult = {
  createdAt: string;
  id: string;
  status: string;
  target: string;
};

const CONNECT_PROVIDERS = [
  { id: "microsoft", label: "Microsoft 365", detail: "SharePoint, OneDrive, Teams, Outlook", sources: ["sharepoint", "onedrive", "teams", "outlook"] },
  { id: "google", label: "Google Workspace", detail: "Drive and docs", sources: ["google_drive", "documents"] },
  { id: "notion", label: "Notion", detail: "Pages and databases", sources: ["pages", "databases"] },
  { id: "github", label: "GitHub", detail: "Repos, README, issues", sources: ["repos", "readme", "issues"] },
  { id: "slack", label: "Slack", detail: "Channels and thread history", sources: ["channels"] },
] as const;

const graphToneClass: Record<LiveKnowledgeGraphNode["tone"], string> = {
  core: "fill-[#151513] stroke-[#151513] dark:fill-[#F5F5F1] dark:stroke-[#F5F5F1]",
  support: "fill-[#EAF1ED] stroke-[#6C8E7A] dark:fill-[#17221D] dark:stroke-[#74A887]",
  policy: "fill-[#F2EAE0] stroke-[#B4834D] dark:fill-[#2A2017] dark:stroke-[#D49A5A]",
  product: "fill-[#E8ECF8] stroke-[#7382C7] dark:fill-[#1A1D30] dark:stroke-[#8B9BF0]",
  risk: "fill-[#F3E7E6] stroke-[#B86155] dark:fill-[#2B1C1C] dark:stroke-[#DD786B]",
};

const folderImageClass: Record<LiveKnowledgeFolder["tone"], string> = {
  warm: "bg-[radial-gradient(circle_at_34%_18%,rgba(255,238,205,0.92),transparent_18%),radial-gradient(circle_at_68%_16%,rgba(255,255,255,0.72),transparent_12%),radial-gradient(circle_at_20%_54%,#D32110_0,#EE4D13_24%,transparent_48%),radial-gradient(circle_at_72%_54%,#FF7A00_0,#C61910_30%,transparent_58%),linear-gradient(120deg,#5B1412,#F04A10_42%,#111111)]",
  green: "bg-[radial-gradient(circle_at_30%_20%,rgba(255,236,204,0.92),transparent_18%),radial-gradient(circle_at_66%_18%,rgba(255,255,255,0.7),transparent_12%),radial-gradient(circle_at_20%_55%,#D33013_0,#F16416_24%,transparent_48%),radial-gradient(circle_at_75%_55%,#FF8A00_0,#B81510_30%,transparent_58%),linear-gradient(120deg,#40120F,#F04A10_44%,#111111)]",
  blue: "bg-[radial-gradient(circle_at_30%_18%,rgba(255,237,207,0.9),transparent_18%),radial-gradient(circle_at_66%_16%,rgba(255,255,255,0.76),transparent_12%),radial-gradient(circle_at_22%_56%,#B91C10_0,#F05414_24%,transparent_48%),radial-gradient(circle_at_76%_55%,#FF8B00_0,#941510_30%,transparent_58%),linear-gradient(120deg,#3D1110,#E94710_42%,#141414)]",
  gray: "bg-[radial-gradient(circle_at_32%_18%,rgba(255,238,210,0.9),transparent_18%),radial-gradient(circle_at_66%_16%,rgba(255,255,255,0.74),transparent_12%),radial-gradient(circle_at_20%_54%,#C51E10_0,#F35A13_24%,transparent_48%),radial-gradient(circle_at_72%_54%,#FF8300_0,#AE1710_30%,transparent_58%),linear-gradient(120deg,#4A1211,#EC4C10_42%,#111111)]",
};

const integrationStatusClass: Record<LiveKnowledgeIntegration["status"], string> = {
  Connected: "bg-[#EEF8F1] text-[#1E7A45] dark:bg-[#122119] dark:text-[#8BE0A7]",
  Syncing: "bg-[#F2EFFE] text-[#6A55B8] dark:bg-[#1C1930] dark:text-[#B8A8FF]",
  Review: "bg-[#FFF7E8] text-[#9A661A] dark:bg-[#241B10] dark:text-[#EAB762]",
};

function filterKnowledgePayload(
  liveKnowledge: LiveKnowledgePayload,
  collectionId: string,
  searchQuery: string,
): LiveKnowledgePayload {
  const normalizedQuery = searchQuery.trim().toLowerCase();
  const providerScope = collectionId.startsWith("provider:") ? collectionId.slice("provider:".length) : null;
  const webScope = collectionId === "web";

  const matchesQuery = (...values: Array<string | undefined>) =>
    normalizedQuery.length === 0 || values.some((value) => value?.toLowerCase().includes(normalizedQuery));
  const matchesProvider = (providerKey: string) =>
    collectionId === "all" ||
    (providerScope ? providerKey === providerScope : false) ||
    (webScope ? providerKey === "web" : false);

  const folders = liveKnowledge.folders.filter((folder) =>
    matchesProvider(folder.providerKey) &&
    matchesQuery(folder.title, folder.subtitle, ...folder.connections),
  );
  const integrations = liveKnowledge.integrations.filter((integration) =>
    (collectionId === "all" || (providerScope ? integration.providerKey === providerScope : false)) &&
    matchesQuery(integration.name, integration.detail, integration.providerKey, integration.documents),
  );
  const files = liveKnowledge.files.filter((file) =>
    matchesProvider(file.providerKey) &&
    matchesQuery(file.name, file.addedBy, file.source, file.updated),
  );
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
  );
  const webSources = liveKnowledge.webSources.filter((source) =>
    matchesProvider(source.providerKey) &&
    matchesQuery(source.name, source.url, source.kind, source.status),
  );

  const allowedSourceIds = new Set(sources.map((source) => source.id));
  const graphNodes = liveKnowledge.graph.nodes.filter((node) => {
    const providerMatch = collectionId === "all"
      ? true
      : node.sourceIds.some((sourceId) => allowedSourceIds.has(sourceId));
    return providerMatch && matchesQuery(node.label, node.group, ...node.sourceRefs);
  });
  const allowedNodeIds = new Set(graphNodes.map((node) => node.id));
  const graphLinks = liveKnowledge.graph.links.filter((link) =>
    allowedNodeIds.has(link.from) && allowedNodeIds.has(link.to) && matchesQuery(link.label, ...link.sourceRefs)
  );

  return {
    ...liveKnowledge,
    folders,
    integrations,
    files,
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
  };
}

export function VelionKnowledgePage() {
  const [activeView, setActiveView] = useState<KnowledgeView>("overview");
  const [selectedCollectionId, setSelectedCollectionId] = useState("all");
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [selectedGraphNodeId, setSelectedGraphNodeId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [liveKnowledge, setLiveKnowledge] = useState<LiveKnowledgePayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [addSourceOpen, setAddSourceOpen] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    void loadKnowledgeWorkspace(controller.signal);
    return () => controller.abort();
  }, []);

  useEffect(() => {
    if (!liveKnowledge) return;
    if (!liveKnowledge.collections.some((collection) => collection.id === selectedCollectionId)) {
      setSelectedCollectionId(liveKnowledge.collections[0]?.id ?? "all");
    }
  }, [liveKnowledge, selectedCollectionId]);

  const visibleKnowledge = liveKnowledge
    ? filterKnowledgePayload(liveKnowledge, selectedCollectionId, searchQuery)
    : null;

  useEffect(() => {
    if (!visibleKnowledge) return;
    if (!selectedSourceId || !visibleKnowledge.sources.some((source) => source.id === selectedSourceId)) {
      setSelectedSourceId(visibleKnowledge.sources[0]?.id ?? null);
    }
    if (!selectedGraphNodeId || !visibleKnowledge.graph.nodes.some((node) => node.id === selectedGraphNodeId)) {
      setSelectedGraphNodeId(visibleKnowledge.graph.nodes[0]?.id ?? null);
    }
  }, [visibleKnowledge, selectedGraphNodeId, selectedSourceId]);

  const selectedSource = visibleKnowledge?.sources.find((source) => source.id === selectedSourceId) ?? visibleKnowledge?.sources[0] ?? null;
  const selectedGraphNode = visibleKnowledge?.graph.nodes.find((node) => node.id === selectedGraphNodeId) ?? visibleKnowledge?.graph.nodes[0] ?? null;
  const relatedGraphSources = visibleKnowledge && selectedGraphNode
    ? visibleKnowledge.sources.filter((source) => selectedGraphNode.sourceIds.includes(source.id))
    : [];

  async function loadKnowledgeWorkspace(signal?: AbortSignal) {
    setLoading(true);
    try {
      const nextKnowledge = await apiGet<LiveKnowledgePayload>("/api/v1/knowledge/sources", { signal });
      setLiveKnowledge(nextKnowledge);
    } catch (error) {
      if (error instanceof DOMException && error.name === "AbortError") return;
      setLiveKnowledge(null);
      setNotice({
        tone: "warn",
        message: error instanceof Error ? error.message : "Knowledge workspace could not be loaded.",
      });
    } finally {
      setLoading(false);
    }
  }

  async function handleSync() {
    setBusyAction("sync");
    setNotice(null);
    try {
      const result = await apiSend<SyncResult>("/api/v1/knowledge/sync", {});
      const failures = [...result.integrationFailures, ...result.finspoFailures];
      setNotice({
        tone: failures.length > 0 ? "warn" : "good",
        message: failures.length > 0
          ? `Started ${result.integrationStarted + result.finspoStarted} syncs, but ${failures.length} sources still need review.`
          : `Started ${result.integrationStarted + result.finspoStarted} source syncs.`,
      });
      await loadKnowledgeWorkspace();
    } catch (error) {
      setNotice({
        tone: "warn",
        message: error instanceof Error ? error.message : "Knowledge sync could not be started.",
      });
    } finally {
      setBusyAction(null);
    }
  }

  async function handleUploadFiles(files: File[]) {
    setBusyAction("upload");
    setNotice(null);
    try {
      const result = await uploadKnowledgeFiles(files);
      setNotice({
        tone: "good",
        message: `Imports-core queued ${result.totalItems || files.length} file${files.length === 1 ? "" : "s"} for ingestion.`,
      });
      setAddSourceOpen(false);
      await loadKnowledgeWorkspace();
    } catch (error) {
      setNotice({
        tone: "warn",
        message: error instanceof Error ? error.message : "File upload could not be started.",
      });
    } finally {
      setBusyAction(null);
    }
  }

  async function handleRegisterSharePoint(input: {
    driveId: string;
    driveName: string;
    driveType: string;
    siteId: string;
    siteWebUrl: string;
    tenantId: string;
  }) {
    setBusyAction("sharepoint");
    setNotice(null);
    try {
      const result = await apiSend<SharePointCreateResult>("/api/v1/knowledge/sharepoint", input);
      setNotice({
        tone: "good",
        message: result.syncStarted
          ? "SharePoint drive registered and sync started in Finspo."
          : "SharePoint drive registered. Sync can be started from Knowledge.",
      });
      setAddSourceOpen(false);
      await loadKnowledgeWorkspace();
    } catch (error) {
      setNotice({
        tone: "warn",
        message: error instanceof Error ? error.message : "SharePoint source could not be registered.",
      });
    } finally {
      setBusyAction(null);
    }
  }

  async function handleConnectProvider(provider: {
    detail: string;
    id: string;
    label: string;
    sources: readonly string[];
  }) {
    setBusyAction("connect");
    setNotice(null);
    try {
      const session = await createConnectSession({
        provider: provider.id,
        sources: [...provider.sources],
      });
      const authWindow = window.open(session.connectUrl, "_blank", popupFeatures());
      if (!authWindow) {
        throw new Error("The authorization window was blocked by the browser.");
      }
      setNotice({
        tone: "good",
        message: `${provider.label} authorization opened in a new window. Return here after approval to refresh the workspace.`,
      });
      setAddSourceOpen(false);
      const closePoll = window.setInterval(() => {
        if (!authWindow.closed) return;
        window.clearInterval(closePoll);
        void loadKnowledgeWorkspace();
      }, 1_000);
    } catch (error) {
      setNotice({
        tone: "warn",
        message: error instanceof Error ? error.message : "Connection flow could not be started.",
      });
    } finally {
      setBusyAction(null);
    }
  }

  async function handleStartWebsiteCrawl(input: { maxPages?: number; url: string }) {
    setBusyAction("crawl");
    setNotice(null);
    try {
      const result = await apiSend<CrawlStartResult>("/api/v1/knowledge/crawl", input);
      setNotice({
        tone: "good",
        message: `Started a website crawl for ${result.target}. Track run ${result.id} in Ingestions while pages flow into Knowledge.`,
      });
      setAddSourceOpen(false);
      await loadKnowledgeWorkspace();
    } catch (error) {
      setNotice({
        tone: "warn",
        message: error instanceof Error ? error.message : "The website crawl could not be started.",
      });
    } finally {
      setBusyAction(null);
    }
  }

  return (
    <div className="velion-page-surface h-full min-h-0 overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[1560px] flex-col gap-5 p-4 sm:p-5 lg:p-7">
        <WorkspaceHeader
          activeView={activeView}
          collections={liveKnowledge?.collections ?? []}
          selectedCollectionId={selectedCollectionId}
          syncing={busyAction === "sync"}
          onActiveViewChange={setActiveView}
          onAddSource={() => setAddSourceOpen(true)}
          onCollectionChange={setSelectedCollectionId}
          onSync={() => void handleSync()}
        />

        {notice ? <NoticeBanner notice={notice} /> : null}

        {loading && !liveKnowledge ? (
          <section className="velion-panel p-5 text-[13px] text-[#666B64] dark:text-[#AEB4C0]">
            Loading knowledge workspace…
          </section>
        ) : null}

        {!loading && !liveKnowledge ? (
          <EmptyPanel
            title="Knowledge workspace unavailable"
            description="The page could not load live data from Data Plane v2 and the ingestion services."
          />
        ) : null}

        {visibleKnowledge ? (
          activeView === "overview" ? (
            <OverviewCanvas
              liveKnowledge={visibleKnowledge}
              searchQuery={searchQuery}
              onSearchChange={setSearchQuery}
            />
          ) : activeView === "graph" ? (
            <GraphCanvas
              graph={visibleKnowledge.graph}
              relatedSources={relatedGraphSources}
              selectedNode={selectedGraphNode}
              onSelectNode={setSelectedGraphNodeId}
            />
          ) : (
            <ChunksCanvas
              selectedSource={selectedSource}
              sources={visibleKnowledge.sources}
              onSelectSource={setSelectedSourceId}
            />
          )
        ) : null}
      </div>

      {addSourceOpen ? (
        <KnowledgeAddSourceModal
          busy={busyAction !== null}
          providers={CONNECT_PROVIDERS}
          onClose={() => setAddSourceOpen(false)}
          onConnectProvider={handleConnectProvider}
          onRegisterSharePoint={handleRegisterSharePoint}
          onStartWebsiteCrawl={handleStartWebsiteCrawl}
          onUploadFiles={handleUploadFiles}
        />
      ) : null}
    </div>
  );
}

function WorkspaceHeader({
  activeView,
  collections,
  selectedCollectionId,
  syncing,
  onActiveViewChange,
  onAddSource,
  onCollectionChange,
  onSync,
}: {
  activeView: KnowledgeView;
  collections: LiveKnowledgeCollection[];
  selectedCollectionId: string;
  syncing: boolean;
  onActiveViewChange: (view: KnowledgeView) => void;
  onAddSource: () => void;
  onCollectionChange: (collectionId: string) => void;
  onSync: () => void;
}) {
  return (
    <header className="flex flex-col gap-4 border-b border-[#DDDCD6] pb-5 dark:border-[#292B31] lg:flex-row lg:items-end lg:justify-between">
      <div className="min-w-0">
        <div className="relative inline-flex max-w-full items-center">
          <VelionSelect
            aria-label="Select knowledge collection"
            value={selectedCollectionId}
            onChange={(event) => onCollectionChange(event.target.value)}
            className="min-w-[240px] appearance-none border-0 bg-transparent py-0 pl-0 pr-10 text-[28px] font-semibold tracking-normal text-[#111111] shadow-none ring-0 focus:outline-none dark:text-white sm:text-[34px]"
          >
            {collections.map((collection) => (
              <option key={collection.id} value={collection.id}>
                {collection.label}
              </option>
            ))}
          </VelionSelect>
          <ChevronDown className="pointer-events-none absolute right-0 top-1/2 size-6 -translate-y-1/2 text-[#8A8A84]" strokeWidth={2} />
        </div>
        <p className="velion-page-body mt-3 max-w-2xl">
          Overview of folders, integrations, files, and retrieval health for this knowledge space.
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <SegmentedView activeView={activeView} onActiveViewChange={onActiveViewChange} />
        <Link href="/ingestions">
          <VelionButton radius="sm" className="px-3">
            <ArrowUpRight className="size-4" />
            Ingestions
          </VelionButton>
        </Link>
        <VelionButton radius="sm" className="px-3" onClick={onSync} disabled={syncing}>
          <RefreshCw className={cn("size-4", syncing && "animate-spin")} />
          Sync
        </VelionButton>
        <VelionButton variant="primary" radius="sm" className="px-3" onClick={onAddSource}>
          <FilePlus2 className="size-4" />
          Add source
        </VelionButton>
      </div>
    </header>
  );
}

function SegmentedView({
  activeView,
  onActiveViewChange,
}: {
  activeView: KnowledgeView;
  onActiveViewChange: (view: KnowledgeView) => void;
}) {
  const views: Array<{ id: KnowledgeView; label: string; icon: React.ReactNode }> = [
    { id: "overview", label: "Overview", icon: <Grid2X2 className="size-4" /> },
    { id: "graph", label: "Graph", icon: <Network className="size-4" /> },
    { id: "chunks", label: "Chunks", icon: <Table2 className="size-4" /> },
  ];

  return (
    <VelionSegmented>
      {views.map((view) => (
        <VelionSegmentedButton
          key={view.id}
          aria-pressed={activeView === view.id}
          onClick={() => onActiveViewChange(view.id)}
        >
          {view.icon}
          {view.label}
        </VelionSegmentedButton>
      ))}
    </VelionSegmented>
  );
}

function OverviewCanvas({
  liveKnowledge,
  onSearchChange,
  searchQuery,
}: {
  liveKnowledge: LiveKnowledgePayload;
  onSearchChange: (query: string) => void;
  searchQuery: string;
}) {
  return (
    <main className="flex min-w-0 flex-col gap-7">
      <section>
        <SectionHeader title="Folders" description="Browse the strongest source groups and where their files come from." />
        <div className="mt-4 grid grid-cols-1 gap-5 xl:grid-cols-2 2xl:grid-cols-3">
          {liveKnowledge.folders.length > 0 ? liveKnowledge.folders.map((folder) => (
            <FolderCard key={folder.id} folder={folder} />
          )) : (
            <EmptyPanel
              title="No source groups yet"
              description="Connect an integration or import files to start building grouped knowledge folders."
            />
          )}
        </div>
      </section>

      <section>
        <SectionHeader title="Integrations" description="Connected source systems feeding this knowledge space." />
        <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
          {liveKnowledge.integrations.length > 0 ? liveKnowledge.integrations.map((integration) => (
            <IntegrationCard key={integration.id} integration={integration} />
          )) : (
            <EmptyPanel
              title="No integrations connected"
              description="Start a workspace connection from Add source to pull in live knowledge."
            />
          )}
        </div>
      </section>

      <WebSourcesPanel webSources={liveKnowledge.webSources} />

      {liveKnowledge.sources.length > 0 ? (
        <LiveSourceInspector liveKnowledge={liveKnowledge} />
      ) : null}

      <section className="grid gap-4 2xl:grid-cols-[minmax(0,1fr)_420px]">
        <FilesTable files={liveKnowledge.files} searchQuery={searchQuery} onSearchChange={onSearchChange} />
        <MetricPanel metrics={liveKnowledge.metricCards} />
      </section>
    </main>
  );
}

function LiveSourceInspector({ liveKnowledge }: { liveKnowledge: LiveKnowledgePayload }) {
  return (
    <section className="velion-panel p-5">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-[22px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white">
            Source evidence
          </h2>
          <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
            Documents, chunks, graph links, and source-system sync state from the live knowledge stack.
          </p>
        </div>
        <span className="w-fit rounded-[10px] bg-black/[0.04] px-3 py-1.5 text-[12px] font-medium text-[#5E635B] dark:bg-white/10 dark:text-[#DDE3ED]">
          {liveKnowledge.graph.available
            ? `${liveKnowledge.graph.nodeCount} nodes · ${liveKnowledge.graph.edgeCount} edges`
            : `${liveKnowledge.dataPlane.documentCount} Data Plane documents`}
        </span>
      </div>
      <div className="mt-4 grid gap-3 lg:grid-cols-2">
        {liveKnowledge.sources.slice(0, 6).map((source) => (
          <article key={source.id} className="rounded-[14px] border border-[#E3E2DC] bg-white/58 p-4 dark:border-white/10 dark:bg-white/[0.04]">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h3 className="truncate text-[14px] font-semibold text-[#111111] dark:text-white">{source.title}</h3>
                <p className="mt-1 text-[12px] text-[#6D7169] dark:text-[#AEB4C0]">{source.provider}</p>
              </div>
              <span className="shrink-0 rounded-[9px] bg-[#F1F2ED] px-2.5 py-1 text-[11px] font-medium text-[#555B52] dark:bg-white/10 dark:text-[#DDE3ED]">
                {source.status}
              </span>
            </div>
            <p className="mt-3 text-[12px] leading-5 text-[#626760] dark:text-[#C3CAD4]">{source.description}</p>
            <div className="mt-3 flex flex-wrap gap-1.5">
              {source.tags.concat(source.related).slice(0, 4).map((tag) => (
                <span key={`${source.id}-${tag}`} className="rounded-full bg-black/[0.04] px-2 py-1 text-[11px] text-[#5E635B] dark:bg-white/10 dark:text-[#DDE3ED]">
                  {tag}
                </span>
              ))}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function SectionHeader({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex flex-col gap-2">
      <div>
        <h2 className="text-[26px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white sm:text-[34px]">{title}</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">{description}</p>
      </div>
    </div>
  );
}

function FolderCard({ folder }: { folder: LiveKnowledgeFolder }) {
  return (
    <button
      type="button"
      className="group relative min-h-[370px] w-full max-w-full overflow-hidden rounded-[34px] bg-[#F4F2E8] text-left shadow-[0_20px_46px_rgba(20,21,24,0.08)] ring-1 ring-[#E7E2D6] transition-all hover:-translate-y-0.5 hover:shadow-[0_28px_58px_rgba(20,21,24,0.12)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/15 dark:bg-[#EDEBE2] sm:aspect-[1.18/1]"
    >
      <div className={cn("absolute inset-x-0 top-0 h-[50%] overflow-hidden rounded-t-[34px]", folderImageClass[folder.tone])}>
        <div className="absolute inset-[-18px] backdrop-blur-[2px]" />
        <div className="absolute inset-0 bg-[linear-gradient(105deg,rgba(255,255,255,0.16),transparent_24%,rgba(255,255,255,0.1)_57%,transparent_72%)]" />
        <div className="absolute right-6 top-8 max-w-[160px] text-right text-[20px] font-semibold leading-[1.04] text-white drop-shadow-[0_2px_8px_rgba(0,0,0,0.18)] sm:right-8 sm:max-w-[230px] sm:text-[24px]">
          {folder.connections[0] ?? "Velion"}
          <br />
          Source Group
        </div>
      </div>

      <div className="absolute inset-x-0 bottom-0 h-[56%] rounded-b-[34px] bg-[#F4F2E8] dark:bg-[#EDEBE2]">
        <div className="absolute -top-[58px] left-0 h-[86px] w-[34%] rounded-tl-[34px] bg-[#F4F2E8] dark:bg-[#EDEBE2]" />
        <div className="absolute -top-[58px] left-[28%] h-[86px] w-[24%] origin-bottom-left skew-x-[34deg] rounded-tr-[18px] bg-[#F4F2E8] dark:bg-[#EDEBE2]" />
      </div>

      <div className="absolute left-8 right-8 top-[45%] z-10">
        <h3 className="text-[24px] font-semibold leading-tight text-[#060606] sm:text-[28px]">{folder.title}</h3>
        <p className="mt-2 text-[22px] font-normal leading-tight text-[#6B6B65] sm:text-[26px]">{folder.subtitle}</p>
      </div>

      <div className="absolute inset-x-8 bottom-8 z-10 flex items-end justify-between gap-4">
        <div className="flex items-end gap-2 whitespace-nowrap">
          <span className="text-[54px] font-semibold leading-none tracking-normal text-black sm:text-[64px]">{folder.primaryValue}</span>
          <span className="pb-2 text-[20px] leading-none text-[#6B6B65] sm:text-[24px]">{folder.primaryLabel}</span>
        </div>
        <div className="whitespace-nowrap pb-2 text-right text-[20px] font-semibold leading-none text-black sm:text-[24px]">
          {folder.secondaryValue} {folder.secondaryLabel}
        </div>
      </div>
    </button>
  );
}

function IntegrationCard({ integration }: { integration: LiveKnowledgeIntegration }) {
  return (
    <article className="velion-panel p-4">
      <div className="flex items-start justify-between gap-3">
        <span className="grid size-10 place-items-center rounded-[8px] bg-[#F1F2ED] text-[13px] font-bold text-[#272B25] dark:bg-[#101114] dark:text-white">
          {integration.name.slice(0, 1)}
        </span>
        <span className={cn("rounded-full px-2 py-1 text-[11px] font-medium", integrationStatusClass[integration.status])}>{integration.status}</span>
      </div>
      <h3 className="mt-4 text-[16px] font-semibold text-[#171A16] dark:text-white">{integration.name}</h3>
      <div className="mt-4 grid grid-cols-2 gap-2 text-[12px]">
        <div>
          <p className="text-[#858980] dark:text-[#8F96A3]">Coverage</p>
          <p className="mt-1 font-medium text-[#242821] dark:text-[#F4F6FA]">{integration.documents}</p>
        </div>
        <div>
          <p className="text-[#858980] dark:text-[#8F96A3]">Freshness</p>
          <p className="mt-1 font-medium text-[#242821] dark:text-[#F4F6FA]">{integration.freshness}</p>
        </div>
      </div>
      {integration.detail ? (
        <p className="mt-4 text-[12px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">{integration.detail}</p>
      ) : null}
    </article>
  );
}

function WebSourcesPanel({ webSources }: { webSources: LiveKnowledgeWebSource[] }) {
  return (
    <section>
      <SectionHeader title="Tracked web sources" description="Quarry-backed website targets that can refresh into the knowledge workspace." />
      <div className="mt-4 grid grid-cols-1 gap-3 xl:grid-cols-3">
        {webSources.length > 0 ? webSources.map((source) => (
          <article key={source.id} className="velion-panel p-4">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h3 className="truncate text-[16px] font-semibold text-[#171A16] dark:text-white">{source.name}</h3>
                <a
                  href={source.url}
                  target="_blank"
                  rel="noreferrer"
                  className="mt-1 block truncate text-[12px] text-[#6D7169] transition-colors hover:text-[#2F4E90] dark:text-[#AEB4C0] dark:hover:text-[#B8C6FF]"
                >
                  {source.url}
                </a>
              </div>
              <span className={cn(
                "rounded-full px-2 py-1 text-[11px] font-medium",
                source.status === "active"
                  ? "bg-[#EEF8F1] text-[#1E7A45] dark:bg-[#122119] dark:text-[#8BE0A7]"
                  : source.status === "running" || source.status === "queued"
                    ? "bg-[#F2EFFE] text-[#6A55B8] dark:bg-[#1C1930] dark:text-[#B8A8FF]"
                    : "bg-[#FFF7E8] text-[#9A661A] dark:bg-[#241B10] dark:text-[#EAB762]",
              )}>
                {formatGraphGroup(source.status)}
              </span>
            </div>
            <div className="mt-4 flex items-center justify-between text-[12px] text-[#6D7169] dark:text-[#AEB4C0]">
              <span className="capitalize">{source.kind}</span>
              <span>{source.updated}</span>
            </div>
          </article>
        )) : (
          <EmptyPanel
            title="No tracked websites yet"
            description="Start a Quarry crawl from Add source to move website content into the ingestion and knowledge stack."
          />
        )}
      </div>
    </section>
  );
}

function FilesTable({
  files,
  onSearchChange,
  searchQuery,
}: {
  files: LiveKnowledgeFile[];
  onSearchChange: (query: string) => void;
  searchQuery: string;
}) {
  return (
    <section className="velion-panel overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">
        <div>
          <h2 className="text-[22px] font-semibold text-[#111111] dark:text-white">Files</h2>
          <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">Latest files available to retrieval.</p>
        </div>
        <label className="hidden min-w-[280px] items-center rounded-[8px] border border-[#DAD8D1] bg-[#FAFAF8] px-3 py-2 dark:border-[#30333A] dark:bg-[#101114] sm:flex">
          <Search className="size-4 text-[#8B8E86]" />
          <VelionInput
            aria-label="Search files and sources"
            value={searchQuery}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder="Search files and sources…"
            className="ml-2 border-0 bg-transparent p-0 text-[13px] shadow-none focus:outline-none"
          />
        </label>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full min-w-[720px] border-separate border-spacing-0 text-left">
          <thead>
            <tr className="text-[13px] font-medium text-[#72766F] dark:text-[#9EA3AD]">
              <th className="border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">Name</th>
              <th className="border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">Added By</th>
              <th className="border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">Source</th>
              <th className="border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">Updated</th>
            </tr>
          </thead>
          <tbody>
            {files.length > 0 ? files.map((file) => {
              const Icon = sourceTypeIcon[file.type];
              return (
                <tr key={file.id} className="text-[14px] text-[#22251F] dark:text-[#F4F6FA]">
                  <td className="border-b border-[#ECEBE5] p-4 dark:border-[#292B31]">
                    <span className="inline-flex items-center gap-2 font-medium">
                      <Icon className="size-4.5 text-[#8B8E86]" />
                      {file.name}
                    </span>
                  </td>
                  <td className="border-b border-[#ECEBE5] p-4 dark:border-[#292B31]">{file.addedBy}</td>
                  <td className="border-b border-[#ECEBE5] p-4 dark:border-[#292B31]">{file.source}</td>
                  <td className="border-b border-[#ECEBE5] p-4 dark:border-[#292B31]">{file.updated}</td>
                </tr>
              );
            }) : (
              <tr>
                <td colSpan={4} className="p-4 text-[13px] text-[#74786F] dark:text-[#9EA3AD]">No retrieval files match the current filters.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function MetricPanel({ metrics }: { metrics: LiveKnowledgeMetric[] }) {
  return (
    <aside className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-1">
      {metrics.map((metric) => (
        <article key={metric.label} className="velion-panel p-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-[13px] font-medium text-[#555A52] dark:text-[#AEB4C0]">{metric.label}</h2>
              <p className="mt-3 text-[26px] font-semibold leading-none text-[#111111] dark:text-white">{metric.value}</p>
            </div>
            <div className="flex h-12 items-end gap-1 rounded-[6px] bg-[#F3F4F0] px-2 pb-2 dark:bg-[#101114]">
              {[0, 1, 2, 3].map((bar) => (
                <span
                  key={bar}
                  className={cn(
                    "block w-4 rounded-[3px]",
                    bar === 2 ? (metric.tone === "good" ? "h-8 bg-[#28A866]" : "h-4 bg-[#E05151]") : "h-6 bg-[#E1E2DC] dark:bg-[#2C2E34]",
                  )}
                />
              ))}
            </div>
          </div>
          <p className={cn("mt-3 text-[12px] font-medium", metric.tone === "good" ? "text-[#1E9A55]" : "text-[#D5534B]")}>{metric.delta}</p>
        </article>
      ))}
    </aside>
  );
}

function GraphCanvas({
  graph,
  relatedSources,
  selectedNode,
  onSelectNode,
}: {
  graph: LiveKnowledgePayload["graph"];
  relatedSources: LiveKnowledgeSource[];
  selectedNode: LiveKnowledgeGraphNode | null;
  onSelectNode: (nodeId: string) => void;
}) {
  return (
    <main className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
      <GraphPanel graph={graph} selectedNode={selectedNode} onSelectNode={onSelectNode} />
      <GraphInspectorPanel selectedNode={selectedNode} relatedSources={relatedSources} />
    </main>
  );
}

function ChunksCanvas({
  selectedSource,
  sources,
  onSelectSource,
}: {
  selectedSource: LiveKnowledgeSource | null;
  sources: LiveKnowledgeSource[];
  onSelectSource: (sourceId: string) => void;
}) {
  return (
    <main className="grid gap-4 xl:grid-cols-[300px_minmax(0,1fr)]">
      <section className="velion-panel p-3">
        <h2 className="px-1 text-[14px] font-semibold text-[#171A16] dark:text-white">Sources</h2>
        <div className="mt-3 space-y-1">
          {sources.length > 0 ? sources.map((source) => (
            <button
              key={source.id}
              type="button"
              aria-pressed={source.id === selectedSource?.id}
              onClick={() => onSelectSource(source.id)}
              className={cn(
                "flex w-full items-center justify-between rounded-[8px] px-3 py-2 text-left text-[13px] transition-colors",
                source.id === selectedSource?.id
                  ? "bg-[#161616] text-white dark:bg-white dark:text-[#111111]"
                  : "text-[#3B3F38] hover:bg-[#F0F1EC] dark:text-[#D9DEE7] dark:hover:bg-[#202127]",
              )}
            >
              <span>{source.title}</span>
              <span className="text-[12px] opacity-70">{source.chunks}</span>
            </button>
          )) : (
            <p className="px-3 py-2 text-[13px] text-[#74786F] dark:text-[#9EA3AD]">No chunk-backed documents yet.</p>
          )}
        </div>
      </section>
      <ChunksPanel source={selectedSource} />
    </main>
  );
}

function GraphPanel({
  graph,
  selectedNode,
  onSelectNode,
}: {
  graph: LiveKnowledgePayload["graph"];
  selectedNode: LiveKnowledgeGraphNode | null;
  onSelectNode: (nodeId: string) => void;
}) {
  const nodeById = new Map(graph.nodes.map((node) => [node.id, node]));

  return (
    <section className="velion-panel velion-panel-muted relative min-h-[620px] overflow-hidden" aria-label="RAGGraph relationship map">
      <div className="flex items-center justify-between gap-3 border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">
        <div>
          <h2 className="text-[18px] font-semibold text-[#171A16] dark:text-white">RAGGraph relationship map</h2>
          <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">Entity relationships grounded in source chunks from Data Plane v2.</p>
        </div>
        <GitBranch className="size-5 text-[#777B74] dark:text-[#AEB4C0]" />
      </div>

      <div className="relative h-[560px]">
        <div className="pointer-events-none absolute inset-0 opacity-80 [background-image:linear-gradient(to_right,rgba(17,17,17,0.045)_1px,transparent_1px),linear-gradient(to_bottom,rgba(17,17,17,0.045)_1px,transparent_1px)] [background-size:36px_36px] dark:opacity-30" aria-hidden="true" />
        <svg className="absolute inset-0 size-full" viewBox="0 0 640 420" role="img" aria-label="Knowledge source graph">
          {graph.links.map((link) => {
            const from = nodeById.get(link.from);
            const to = nodeById.get(link.to);
            if (!from || !to) return null;
            const selected = link.from === selectedNode?.id || link.to === selectedNode?.id;
            return (
              <line
                key={`${link.from}-${link.to}-${link.label}`}
                x1={from.x}
                y1={from.y}
                x2={to.x}
                y2={to.y}
                stroke={selected ? "#111111" : "#B8B9B1"}
                strokeWidth={selected ? link.strength : 1}
                strokeOpacity={selected ? 0.82 : 0.48}
                className="dark:stroke-[#D8DDE8]"
              />
            );
          })}
        </svg>

        <div className="absolute inset-0">
          {graph.nodes.map((node) => {
            const active = node.id === selectedNode?.id;
            return (
              <button
                key={node.id}
                type="button"
                aria-label={`Select ${node.label}`}
                onClick={() => onSelectNode(node.id)}
                className={cn(
                  "absolute rounded-full transition-transform focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/20 dark:focus-visible:ring-white/30 hover:scale-105",
                  active ? "scale-110" : "",
                )}
                style={{
                  left: `${(node.x / 640) * 100}%`,
                  top: `${(node.y / 420) * 100}%`,
                  width: node.radius * 2,
                  height: node.radius * 2,
                  transform: "translate(-50%, -50%)",
                }}
              >
                <span className={cn("block size-full rounded-full border shadow-[0_16px_30px_rgba(17,17,17,0.12)]", graphToneClass[node.tone])} />
                <span className="absolute left-1/2 top-[calc(100%+6px)] w-max max-w-[130px] -translate-x-1/2 rounded-[6px] bg-white px-2 py-1 text-[11px] font-medium text-[#22251F] shadow-[0_8px_20px_rgba(20,21,24,0.1)] dark:bg-[#202127] dark:text-white">
                  {node.label}
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function GraphInspectorPanel({
  relatedSources,
  selectedNode,
}: {
  relatedSources: LiveKnowledgeSource[];
  selectedNode: LiveKnowledgeGraphNode | null;
}) {
  if (!selectedNode) {
    return (
      <EmptyPanel
        title="No graph node selected"
        description="Choose a node in the graph to inspect related retrieval sources and chunk evidence."
      />
    );
  }

  const chunkEvidence = relatedSources
    .flatMap((source) => source.chunksPreview.map((chunk) => ({ ...chunk, sourceTitle: source.title })))
    .slice(0, 4);

  return (
    <section className="velion-panel p-4">
      <h2 className="text-[20px] font-semibold text-[#171A16] dark:text-white">{selectedNode.label}</h2>
      <p className="mt-2 text-[13px] leading-5 text-[#666B64] dark:text-[#AEB4C0]">
        {formatGraphGroup(selectedNode.group)} · {selectedNode.sourceRefs.length} linked chunk reference{selectedNode.sourceRefs.length === 1 ? "" : "s"}.
      </p>

      <div className="mt-4 flex flex-wrap gap-2">
        {relatedSources.length > 0 ? relatedSources.map((source) => (
          <span key={source.id} className="rounded-full bg-black/[0.04] px-2.5 py-1 text-[11px] text-[#5E635B] dark:bg-white/10 dark:text-[#DDE3ED]">
            {source.title}
          </span>
        )) : (
          <span className="text-[12px] text-[#858980] dark:text-[#8F96A3]">No document previews were resolved for this node yet.</span>
        )}
      </div>

      <div className="mt-5 space-y-3">
        {chunkEvidence.length > 0 ? chunkEvidence.map((chunk) => (
          <article key={`${chunk.sourceTitle}-${chunk.id}`} className="rounded-[8px] border border-[#E3E1DA] bg-[#FAFAF8] p-3 dark:border-[#30333A] dark:bg-[#101114]">
            <div className="flex items-center justify-between gap-3">
              <span className="inline-flex items-center gap-2 text-[12px] font-medium text-[#696E66] dark:text-[#AEB4C0]">
                <Blocks className="size-4" />
                {chunk.sourceTitle}
              </span>
              <span className="text-[12px] text-[#696E66] dark:text-[#AEB4C0]">{chunk.score}</span>
            </div>
            <h3 className="mt-3 text-[14px] font-semibold text-[#171A16] dark:text-white">{chunk.title}</h3>
            <p className="mt-2 text-[13px] leading-5 text-[#5F645C] dark:text-[#AEB4C0]">{chunk.text}</p>
          </article>
        )) : null}
      </div>
    </section>
  );
}

function ChunksPanel({ source }: { source: LiveKnowledgeSource | null }) {
  if (!source) {
    return (
      <EmptyPanel
        title="No chunk source selected"
        description="Choose a document to inspect the chunks currently available to retrieval."
      />
    );
  }

  return (
    <section className="velion-panel p-4">
      <h2 className="text-[20px] font-semibold text-[#171A16] dark:text-white">{source.title}</h2>
      <p className="mt-2 text-[13px] leading-5 text-[#666B64] dark:text-[#AEB4C0]">{source.description}</p>
      <div className="mt-4 flex flex-wrap gap-2">
        {source.tags.concat(source.related).slice(0, 4).map((tag) => (
          <span key={`${source.id}-${tag}`} className="rounded-full bg-black/[0.04] px-2 py-1 text-[11px] text-[#5E635B] dark:bg-white/10 dark:text-[#DDE3ED]">
            {tag}
          </span>
        ))}
      </div>
      <div className="mt-5 space-y-3">
        {source.chunksPreview.map((chunk) => (
          <article key={chunk.id} className="rounded-[8px] border border-[#E3E1DA] bg-[#FAFAF8] p-3 dark:border-[#30333A] dark:bg-[#101114]">
            <div className="flex items-center justify-between gap-3">
              <span className="inline-flex items-center gap-2 text-[12px] font-medium text-[#696E66] dark:text-[#AEB4C0]">
                <Blocks className="size-4" />
                {chunk.id}
              </span>
              <span className={cn(
                "text-[12px]",
                chunk.score.startsWith("#") ? "text-[#696E66] dark:text-[#AEB4C0]" : "font-mono text-[#15945D]",
              )}>
                {chunk.score}
              </span>
            </div>
            <h3 className="mt-3 text-[14px] font-semibold text-[#171A16] dark:text-white">{chunk.title}</h3>
            <p className="mt-2 text-[13px] leading-5 text-[#5F645C] dark:text-[#AEB4C0]">{chunk.text}</p>
          </article>
        ))}
      </div>
    </section>
  );
}

function NoticeBanner({ notice }: { notice: Notice }) {
  return (
    <section className={cn(
      "rounded-[16px] border p-4 text-[13px]",
      notice.tone === "good"
        ? "border-[#B8E1C7] bg-[#EEF8F1] text-[#1E7A45] dark:border-[#23472F] dark:bg-[#132118] dark:text-[#A3E0B8]"
        : "border-[#E7C98B] bg-[#FFF6E5] text-[#6E5220] dark:border-[#5A4520] dark:bg-[#2A2214] dark:text-[#E6C27A]",
    )}>
      {notice.message}
    </section>
  );
}

function EmptyPanel({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <section className="velion-panel p-5">
      <h2 className="text-[18px] font-semibold text-[#171A16] dark:text-white">{title}</h2>
      <p className="mt-2 text-[13px] leading-5 text-[#666B64] dark:text-[#AEB4C0]">{description}</p>
    </section>
  );
}

function popupFeatures() {
  return [
    "width=980",
    "height=760",
    "scrollbars=yes",
    "resizable=yes",
    "status=no",
    "toolbar=no",
    "location=no",
    "copyhistory=no",
    "menubar=no",
    "directories=no",
  ].join(",");
}

async function uploadKnowledgeFiles(files: File[]) {
  const body = new FormData();
  for (const file of files) {
    body.append("files", file, file.name);
  }
  const response = await fetch("/api/v1/knowledge/import/upload", {
    method: "POST",
    body,
    cache: "no-store",
    credentials: "include",
  });
  return readEnvelope<UploadResult>(response);
}

async function readEnvelope<T>(response: Response): Promise<T> {
  const payload = (await response.json().catch(() => null)) as
    | { data?: T; error?: { message?: string } }
    | null;
  if (!response.ok || !payload?.data) {
    throw new Error(payload?.error?.message || `Request failed: ${response.status}`);
  }
  return payload.data;
}

function formatGraphGroup(group: string) {
  return group.replace(/[_:-]+/g, " ").replace(/\b\w/g, (character) => character.toUpperCase());
}
