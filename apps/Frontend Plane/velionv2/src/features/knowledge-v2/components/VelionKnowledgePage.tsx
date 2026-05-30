"use client";

import { useState } from "react";
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
  VelionSegmented,
  VelionSegmentedButton,
} from "@/components/ui/velion-ui";
import {
  graphLinks,
  graphNodes,
  knowledgeFiles,
  knowledgeFolderCards,
  knowledgeIntegrations,
  knowledgeMetrics,
  knowledgeSources,
  sourceTypeIcon,
  type GraphNode,
  type KnowledgeFolder,
  type KnowledgeIntegration,
  type KnowledgeSource,
} from "@/features/knowledge-v2/lib/knowledge-data";
import { cn } from "@/lib/utils";

type KnowledgeView = "overview" | "graph" | "chunks";

const graphToneClass: Record<GraphNode["tone"], string> = {
  core: "fill-[#151513] stroke-[#151513] dark:fill-[#F5F5F1] dark:stroke-[#F5F5F1]",
  support: "fill-[#EAF1ED] stroke-[#6C8E7A] dark:fill-[#17221D] dark:stroke-[#74A887]",
  policy: "fill-[#F2EAE0] stroke-[#B4834D] dark:fill-[#2A2017] dark:stroke-[#D49A5A]",
  product: "fill-[#E8ECF8] stroke-[#7382C7] dark:fill-[#1A1D30] dark:stroke-[#8B9BF0]",
  risk: "fill-[#F3E7E6] stroke-[#B86155] dark:fill-[#2B1C1C] dark:stroke-[#DD786B]",
};

const folderImageClass: Record<KnowledgeFolder["tone"], string> = {
  warm: "bg-[radial-gradient(circle_at_34%_18%,rgba(255,238,205,0.92),transparent_18%),radial-gradient(circle_at_68%_16%,rgba(255,255,255,0.72),transparent_12%),radial-gradient(circle_at_20%_54%,#D32110_0,#EE4D13_24%,transparent_48%),radial-gradient(circle_at_72%_54%,#FF7A00_0,#C61910_30%,transparent_58%),linear-gradient(120deg,#5B1412,#F04A10_42%,#111111)]",
  green: "bg-[radial-gradient(circle_at_30%_20%,rgba(255,236,204,0.92),transparent_18%),radial-gradient(circle_at_66%_18%,rgba(255,255,255,0.7),transparent_12%),radial-gradient(circle_at_20%_55%,#D33013_0,#F16416_24%,transparent_48%),radial-gradient(circle_at_75%_55%,#FF8A00_0,#B81510_30%,transparent_58%),linear-gradient(120deg,#40120F,#F04A10_44%,#111111)]",
  blue: "bg-[radial-gradient(circle_at_30%_18%,rgba(255,237,207,0.9),transparent_18%),radial-gradient(circle_at_66%_16%,rgba(255,255,255,0.76),transparent_12%),radial-gradient(circle_at_22%_56%,#B91C10_0,#F05414_24%,transparent_48%),radial-gradient(circle_at_76%_55%,#FF8B00_0,#941510_30%,transparent_58%),linear-gradient(120deg,#3D1110,#E94710_42%,#141414)]",
  gray: "bg-[radial-gradient(circle_at_32%_18%,rgba(255,238,210,0.9),transparent_18%),radial-gradient(circle_at_66%_16%,rgba(255,255,255,0.74),transparent_12%),radial-gradient(circle_at_20%_54%,#C51E10_0,#F35A13_24%,transparent_48%),radial-gradient(circle_at_72%_54%,#FF8300_0,#AE1710_30%,transparent_58%),linear-gradient(120deg,#4A1211,#EC4C10_42%,#111111)]",
};

const integrationStatusClass: Record<KnowledgeIntegration["status"], string> = {
  Connected: "bg-[#EEF8F1] text-[#1E7A45] dark:bg-[#122119] dark:text-[#8BE0A7]",
  Syncing: "bg-[#F2EFFE] text-[#6A55B8] dark:bg-[#1C1930] dark:text-[#B8A8FF]",
  Review: "bg-[#FFF7E8] text-[#9A661A] dark:bg-[#241B10] dark:text-[#EAB762]",
};

export function VelionKnowledgePage() {
  const [activeView, setActiveView] = useState<KnowledgeView>("overview");
  const [selectedSourceId, setSelectedSourceId] = useState(knowledgeSources[0].id);
  const selectedSource = knowledgeSources.find((source) => source.id === selectedSourceId) ?? knowledgeSources[0];

  return (
    <div className="velion-page-surface h-full min-h-0 overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[1560px] flex-col gap-5 p-4 sm:p-5 lg:p-7">
        <WorkspaceHeader activeView={activeView} onActiveViewChange={setActiveView} />

        {activeView === "overview" ? (
          <OverviewCanvas />
        ) : activeView === "graph" ? (
          <GraphCanvas selectedSource={selectedSource} onSelectSource={setSelectedSourceId} />
        ) : (
          <ChunksCanvas selectedSource={selectedSource} onSelectSource={setSelectedSourceId} />
        )}
      </div>
    </div>
  );
}

function WorkspaceHeader({
  activeView,
  onActiveViewChange,
}: {
  activeView: KnowledgeView;
  onActiveViewChange: (view: KnowledgeView) => void;
}) {
  return (
    <header className="flex flex-col gap-4 border-b border-[#DDDCD6] pb-5 dark:border-[#292B31] lg:flex-row lg:items-end lg:justify-between">
      <div className="min-w-0">
        <button
          type="button"
          className="velion-page-title inline-flex items-center gap-2 transition-colors hover:text-[#333333] dark:hover:text-[#DDE3ED] sm:text-[34px]"
          aria-label="Select knowledge collection"
        >
          General Knowledge
          <ChevronDown className="size-6 text-[#8A8A84]" strokeWidth={2} />
        </button>
        <p className="velion-page-body mt-3 max-w-2xl">
          Overview of folders, integrations, files, and retrieval health for this knowledge space.
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <SegmentedView activeView={activeView} onActiveViewChange={onActiveViewChange} />
        <VelionButton radius="sm" className="px-3">
          <RefreshCw className="size-4" />
          Sync
        </VelionButton>
        <VelionButton variant="primary" radius="sm" className="px-3">
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

function OverviewCanvas() {
  return (
    <main className="flex min-w-0 flex-col gap-7">
      <section>
        <SectionHeader title="Folders" description="Browse the strongest source groups and where their files come from." />
        <div className="mt-4 grid grid-cols-1 gap-5 xl:grid-cols-2 2xl:grid-cols-3">
          {knowledgeFolderCards.map((folder) => (
            <FolderCard key={folder.id} folder={folder} />
          ))}
        </div>
      </section>

      <section>
        <SectionHeader title="Integrations" description="Connected source systems feeding this knowledge space." />
        <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
          {knowledgeIntegrations.map((integration) => (
            <IntegrationCard key={integration.id} integration={integration} />
          ))}
        </div>
      </section>

      <section className="grid gap-4 2xl:grid-cols-[minmax(0,1fr)_420px]">
        <FilesTable />
        <MetricPanel />
      </section>
    </main>
  );
}

function SectionHeader({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
      <div>
        <h2 className="text-[26px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white sm:text-[34px]">{title}</h2>
        <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">{description}</p>
      </div>
      <VelionButton size="sm" radius="sm" className="w-fit px-3 text-[12px]">
        View all
        <ArrowUpRight className="size-3.5" />
      </VelionButton>
    </div>
  );
}

function FolderCard({ folder }: { folder: KnowledgeFolder }) {
  const fileCount = String(folder.fileCount).padStart(2, "0");

  return (
    <button
      type="button"
      className="group relative min-h-[370px] w-full max-w-full overflow-hidden rounded-[34px] bg-[#F4F2E8] text-left shadow-[0_20px_46px_rgba(20,21,24,0.08)] ring-1 ring-[#E7E2D6] transition-all hover:-translate-y-0.5 hover:shadow-[0_28px_58px_rgba(20,21,24,0.12)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/15 dark:bg-[#EDEBE2] sm:aspect-[1.18/1]"
    >
      <div className={cn("absolute inset-x-0 top-0 h-[50%] overflow-hidden rounded-t-[34px]", folderImageClass[folder.tone])}>
        <div className="absolute inset-[-18px] backdrop-blur-[2px]" />
        <div className="absolute inset-0 bg-[linear-gradient(105deg,rgba(255,255,255,0.16),transparent_24%,rgba(255,255,255,0.1)_57%,transparent_72%)]" />
        <div className="absolute right-6 top-8 max-w-[150px] text-right text-[22px] font-semibold leading-[1.04] text-white drop-shadow-[0_2px_8px_rgba(0,0,0,0.18)] sm:right-8 sm:max-w-[240px] sm:text-[25px]">
          Velion Knowledge
          <br />
          Base
        </div>
      </div>

      <div className="absolute inset-x-0 bottom-0 h-[56%] rounded-b-[34px] bg-[#F4F2E8] dark:bg-[#EDEBE2]">
        <div className="absolute -top-[58px] left-0 h-[86px] w-[34%] rounded-tl-[34px] bg-[#F4F2E8] dark:bg-[#EDEBE2]" />
        <div className="absolute -top-[58px] left-[28%] h-[86px] w-[24%] origin-bottom-left skew-x-[34deg] rounded-tr-[18px] bg-[#F4F2E8] dark:bg-[#EDEBE2]" />
      </div>

      <div className="absolute left-8 right-8 top-[45%] z-10">
        <h3 className="text-[24px] font-semibold leading-tight text-[#060606] sm:text-[28px]">{folder.title}</h3>
        <p className="mt-2 text-[24px] font-normal leading-tight text-[#6B6B65] sm:text-[28px]">{folder.subtitle}</p>
      </div>

      <div className="absolute inset-x-8 bottom-8 z-10 flex items-end justify-between gap-4">
        <div className="flex items-end gap-2 whitespace-nowrap">
          <span className="text-[54px] font-semibold leading-none tracking-normal text-black sm:text-[64px]">{fileCount}</span>
          <span className="pb-2 text-[22px] leading-none text-[#6B6B65] sm:text-[27px]">Doc</span>
        </div>
        <div className="whitespace-nowrap pb-2 text-right text-[22px] font-semibold leading-none text-black sm:text-[27px]">
          {folder.noteCount} Notes
        </div>
      </div>
    </button>
  );
}

function IntegrationCard({ integration }: { integration: KnowledgeIntegration }) {
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
          <p className="text-[#858980] dark:text-[#8F96A3]">Documents</p>
          <p className="mt-1 font-medium text-[#242821] dark:text-[#F4F6FA]">{integration.documents}</p>
        </div>
        <div>
          <p className="text-[#858980] dark:text-[#8F96A3]">Freshness</p>
          <p className="mt-1 font-medium text-[#242821] dark:text-[#F4F6FA]">{integration.freshness}</p>
        </div>
      </div>
    </article>
  );
}

function FilesTable() {
  return (
    <section className="velion-panel overflow-hidden">
      <div className="flex items-center justify-between gap-3 border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">
        <div>
          <h2 className="text-[22px] font-semibold text-[#111111] dark:text-white">Files</h2>
          <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">Latest files available to retrieval.</p>
        </div>
        <div className="hidden min-w-[240px] items-center rounded-[8px] border border-[#DAD8D1] bg-[#FAFAF8] px-3 py-2 dark:border-[#30333A] dark:bg-[#101114] sm:flex">
          <Search className="size-4 text-[#8B8E86]" />
          <span className="ml-2 text-[13px] text-[#858980] dark:text-[#8F96A3]">Search files…</span>
        </div>
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
            {knowledgeFiles.map((file) => {
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
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function MetricPanel() {
  return (
    <aside className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-1">
      {knowledgeMetrics.map((metric) => (
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
  selectedSource,
  onSelectSource,
}: {
  selectedSource: KnowledgeSource;
  onSelectSource: (sourceId: string) => void;
}) {
  return (
    <main className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
      <GraphPanel selectedSource={selectedSource} onSelectSource={onSelectSource} />
      <ChunksPanel source={selectedSource} />
    </main>
  );
}

function ChunksCanvas({
  selectedSource,
  onSelectSource,
}: {
  selectedSource: KnowledgeSource;
  onSelectSource: (sourceId: string) => void;
}) {
  return (
    <main className="grid gap-4 xl:grid-cols-[300px_minmax(0,1fr)]">
      <section className="velion-panel p-3">
        <h2 className="px-1 text-[14px] font-semibold text-[#171A16] dark:text-white">Sources</h2>
        <div className="mt-3 space-y-1">
          {knowledgeSources.map((source) => (
            <button
              key={source.id}
              type="button"
              aria-pressed={source.id === selectedSource.id}
              onClick={() => onSelectSource(source.id)}
              className={cn(
                "flex w-full items-center justify-between rounded-[8px] px-3 py-2 text-left text-[13px] transition-colors",
                source.id === selectedSource.id
                  ? "bg-[#161616] text-white dark:bg-white dark:text-[#111111]"
                  : "text-[#3B3F38] hover:bg-[#F0F1EC] dark:text-[#D9DEE7] dark:hover:bg-[#202127]",
              )}
            >
              <span>{source.title}</span>
              <span className="text-[12px] opacity-70">{source.chunks}</span>
            </button>
          ))}
        </div>
      </section>
      <ChunksPanel source={selectedSource} />
    </main>
  );
}

function GraphPanel({
  selectedSource,
  onSelectSource,
}: {
  selectedSource: KnowledgeSource;
  onSelectSource: (sourceId: string) => void;
}) {
  const nodeById = new Map(graphNodes.map((node) => [node.id, node]));

  return (
    <section className="velion-panel velion-panel-muted relative min-h-[620px] overflow-hidden" aria-label="RAGGraph relationship map">
      <div className="flex items-center justify-between gap-3 border-b border-[#E4E3DD] px-4 py-3 dark:border-[#292B31]">
        <div>
          <h2 className="text-[18px] font-semibold text-[#171A16] dark:text-white">RAGGraph relationship map</h2>
          <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">Source links, backlinks, and retrieval neighborhoods.</p>
        </div>
        <GitBranch className="size-5 text-[#777B74] dark:text-[#AEB4C0]" />
      </div>

      <div className="relative h-[560px]">
        <div className="pointer-events-none absolute inset-0 opacity-80 [background-image:linear-gradient(to_right,rgba(17,17,17,0.045)_1px,transparent_1px),linear-gradient(to_bottom,rgba(17,17,17,0.045)_1px,transparent_1px)] [background-size:36px_36px] dark:opacity-30" aria-hidden="true" />
        <svg className="absolute inset-0 size-full" viewBox="0 0 640 420" role="img" aria-label="Knowledge source graph">
          {graphLinks.map((link) => {
            const from = nodeById.get(link.from);
            const to = nodeById.get(link.to);
            if (!from || !to) return null;

            const selected = link.from === selectedSource.id || link.to === selectedSource.id;
            return (
              <line
                key={`${link.from}-${link.to}`}
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
          {graphNodes.map((node) => {
            const active = node.id === selectedSource.id;
            const source = knowledgeSources.find((item) => item.id === node.id);
            return (
              <button
                key={node.id}
                type="button"
                disabled={!source}
                aria-label={source ? `Select ${source.title}` : node.label}
                onClick={() => {
                  if (source) onSelectSource(source.id);
                }}
                className={cn("absolute rounded-full transition-transform focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/20 dark:focus-visible:ring-white/30", source ? "hover:scale-105" : "cursor-default", active ? "scale-110" : "")}
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

function ChunksPanel({ source }: { source: KnowledgeSource }) {
  return (
    <section className="velion-panel p-4">
      <h2 className="text-[20px] font-semibold text-[#171A16] dark:text-white">{source.title}</h2>
      <p className="mt-2 text-[13px] leading-5 text-[#666B64] dark:text-[#AEB4C0]">{source.description}</p>
      <div className="mt-5 space-y-3">
        {source.chunksPreview.map((chunk) => (
          <article key={chunk.id} className="rounded-[8px] border border-[#E3E1DA] bg-[#FAFAF8] p-3 dark:border-[#30333A] dark:bg-[#101114]">
            <div className="flex items-center justify-between gap-3">
              <span className="inline-flex items-center gap-2 text-[12px] font-medium text-[#696E66] dark:text-[#AEB4C0]">
                <Blocks className="size-4" />
                {chunk.id}
              </span>
              <span className="font-mono text-[12px] text-[#15945D]">{chunk.score}</span>
            </div>
            <h3 className="mt-3 text-[14px] font-semibold text-[#171A16] dark:text-white">{chunk.title}</h3>
            <p className="mt-2 text-[13px] leading-5 text-[#5F645C] dark:text-[#AEB4C0]">{chunk.text}</p>
          </article>
        ))}
      </div>
    </section>
  );
}
