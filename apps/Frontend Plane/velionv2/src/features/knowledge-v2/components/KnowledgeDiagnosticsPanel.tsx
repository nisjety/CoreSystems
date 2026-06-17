"use client";

import type {
  LiveKnowledgeDiagnosticItem,
  LiveKnowledgeDiagnosticTone,
  LiveKnowledgeDiagnostics,
  LiveKnowledgePayload,
} from "@/features/knowledge-v2/lib/knowledge-live";
import { cn } from "@/lib/utils";

const toneBadgeClass: Record<LiveKnowledgeDiagnosticTone, string> = {
  bad: "bg-[#FBE8E6] text-[#A24539] dark:bg-[#2A1918] dark:text-[#F0A397]",
  good: "bg-[#EEF8F1] text-[#1E7A45] dark:bg-[#122119] dark:text-[#8BE0A7]",
  neutral: "bg-[#F1F2ED] text-[#555B52] dark:bg-white/10 dark:text-[#DDE3ED]",
  warn: "bg-[#FFF7E8] text-[#9A661A] dark:bg-[#241B10] dark:text-[#EAB762]",
};

const toneBorderClass: Record<LiveKnowledgeDiagnosticTone, string> = {
  bad: "border-[#F1D3CF] dark:border-[#4A2B29]",
  good: "border-[#D7ECDC] dark:border-[#24412E]",
  neutral: "border-[#E3E2DC] dark:border-white/10",
  warn: "border-[#F2DFBD] dark:border-[#4D3B1B]",
};

export function KnowledgeDiagnosticsPanel({
  dataPlane,
  diagnostics,
}: {
  dataPlane: LiveKnowledgePayload["dataPlane"];
  diagnostics?: LiveKnowledgeDiagnostics | null;
}) {
  const safeDiagnostics = diagnostics ?? {
    available: false,
    sparseBackend: null,
    vectorCollections: [],
    quickwitIndexes: [],
    services: [],
    storage: [],
    capabilities: [],
  };

  return (
    <section>
      <div className="flex flex-col gap-2">
        <div>
          <h2 className="text-[26px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white sm:text-[34px]">
            Data Plane status
          </h2>
          <p className="mt-1 text-[13px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
            Live storage, retrieval, embedding, graph, and wiki runtime state from Data Plane v2.
          </p>
        </div>
      </div>

      <div className="velion-panel mt-4 p-5">
        <div className="flex flex-wrap items-center gap-2">
          <SummaryChip label="Documents" value={`${formatCount(dataPlane.documentCount)} live`} />
          <SummaryChip label="Indexed" value={`${formatCount(dataPlane.indexedCount)} ready`} />
          <SummaryChip label="Sparse backend" value={safeDiagnostics.sparseBackend || "unknown"} />
          <SummaryChip label="Vectors" value={`${formatCount(safeDiagnostics.vectorCollections.length)} collections`} />
          <SummaryChip label="Quickwit" value={`${formatCount(safeDiagnostics.quickwitIndexes.length)} indexes`} />
        </div>

        <div className="mt-5 grid gap-4 xl:grid-cols-2">
          <DiagnosticsGroup title="Runtime services" items={safeDiagnostics.services} />
          <DiagnosticsGroup title="Storage + retrieval backends" items={safeDiagnostics.storage} />
        </div>

        <div className="mt-5">
          <h3 className="text-[18px] font-semibold text-[#171A16] dark:text-white">Capabilities</h3>
          <p className="mt-1 text-[12px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
            This distinguishes what is live now from what is only defined in code or still absent in Data Plane v2.
          </p>
          <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2 2xl:grid-cols-3">
            {safeDiagnostics.capabilities.map((item) => (
              <DiagnosticCard key={item.id} item={item} compact />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

function DiagnosticsGroup({
  items,
  title,
}: {
  items: LiveKnowledgeDiagnosticItem[];
  title: string;
}) {
  return (
    <div className="rounded-[18px] border border-[#E7E3D8] bg-[#FBFAF5] p-4 dark:border-white/10 dark:bg-white/[0.03]">
      <h3 className="text-[18px] font-semibold text-[#171A16] dark:text-white">{title}</h3>
      {items.length > 0 ? (
        <div className="mt-3 grid gap-3">
          {items.map((item) => (
            <DiagnosticCard key={item.id} item={item} />
          ))}
        </div>
      ) : (
        <p className="mt-3 text-[12px] leading-5 text-[#6D7169] dark:text-[#AEB4C0]">
          No live diagnostics were returned for this group yet.
        </p>
      )}
    </div>
  );
}

function DiagnosticCard({
  compact = false,
  item,
}: {
  compact?: boolean;
  item: LiveKnowledgeDiagnosticItem;
}) {
  return (
    <article
      className={cn(
        "rounded-[16px] border bg-white/70 p-4 dark:bg-white/[0.04]",
        toneBorderClass[item.tone],
        compact && "h-full",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h4 className="text-[14px] font-semibold text-[#171A16] dark:text-white">{item.label}</h4>
          {item.meta ? (
            <p className="mt-1 text-[11px] uppercase tracking-[0.08em] text-[#7A7E76] dark:text-[#9EA3AD]">
              {item.meta}
            </p>
          ) : null}
        </div>
        <span className={cn("shrink-0 rounded-full px-2.5 py-1 text-[11px] font-medium", toneBadgeClass[item.tone])}>
          {item.status}
        </span>
      </div>
      <p className="mt-3 text-[12px] leading-5 text-[#626760] dark:text-[#C3CAD4]">{item.detail}</p>
    </article>
  );
}

function SummaryChip({ label, value }: { label: string; value: string }) {
  return (
    <span className="rounded-full bg-black/[0.04] px-3 py-1.5 text-[12px] font-medium text-[#555B52] dark:bg-white/10 dark:text-[#DDE3ED]">
      {label}: {value}
    </span>
  );
}

function formatCount(value: number): string {
  return new Intl.NumberFormat("en-US").format(Math.max(0, Math.trunc(value)));
}
