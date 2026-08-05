"use client";

import { useState } from "react";
import {
  Bot,
  MoreHorizontal,
  Search,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { VerevonIconButton } from "@/components/ui/verevon-ui";
import {
  workflowToolOptions,
  workflowToolTabOptions,
  type WorkflowBuilderToolId,
  type WorkflowToolTabId,
} from "@/features/agents-v2/lib/agent-roles";
import { useWorkflowBuilderTool } from "@/features/agents-v2/lib/use-agent-selection";
import {
  toolToCanvasNode,
  workflowToolBrandMap,
  workflowToolIconMap,
} from "@/features/agents-v2/lib/verevon-workflow-builder-data";
import { BrandMark } from "@/features/agents-v2/components/VerevonWorkflowBrandMark";

export function WorkflowToolsPanel({ onCollapse }: { onCollapse?: () => void }) {
  const [selectedTool, setSelectedTool] = useWorkflowBuilderTool();
  const [activeTab, setActiveTab] = useState<WorkflowToolTabId>("ai-apps");
  const [searchQuery, setSearchQuery] = useState("");
  const normalizedQuery = searchQuery.trim().toLowerCase();

  const visibleTools = workflowToolOptions.filter((tool) => {
    const matchesTab = activeTab === "all" || tool.tabId === activeTab;
    const matchesSearch = !normalizedQuery || tool.label.toLowerCase().includes(normalizedQuery);
    return matchesTab && matchesSearch;
  });

  return (
    <aside
      aria-label="Workflow tools"
      className="verevon-sidebar-type flex h-full min-w-0 flex-col bg-[#F7F7F8] font-sans text-[#25272D] dark:bg-[#101114] dark:text-white"
    >
      <div className="flex h-11 shrink-0 items-center justify-between gap-3 px-1">
        <div className="flex items-center gap-2.5">
          <span className="grid size-5 grid-cols-2 gap-0.5">
            {Array.from({ length: 4 }).map((_, index) => (
              <span key={index} className="rounded-full border-[1.7px] border-[#17181C] dark:border-white" />
            ))}
          </span>
          <h2 className="verevon-sidebar-group-title">Tools</h2>
        </div>
        <div className="flex items-center gap-1">
          <VerevonIconButton type="button" size="xs" radius="sm" aria-label="More workflow tools options">
            <MoreHorizontal className="size-3.5" strokeWidth={2} />
          </VerevonIconButton>
          {onCollapse ? (
            <VerevonIconButton
              type="button"
              size="xs"
              radius="sm"
              onClick={onCollapse}
              aria-label="Collapse workflow tools"
            >
              <X className="size-3.5" strokeWidth={2} />
            </VerevonIconButton>
          ) : null}
        </div>
      </div>

      <div className="px-1 pb-3">
        <label className="relative block">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-[15px] -translate-y-1/2 text-[#A3A7AF]" strokeWidth={2} />
          <input
            aria-label="Workflow tool search"
            type="search"
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder="Search…"
            className="verevon-field-compact h-9 pl-9 pr-3 placeholder:text-[#B2B6BE] dark:placeholder:text-[#777E8B]"
          />
        </label>
      </div>

      <div className="verevon-sidebar-row-strong grid grid-cols-4 border-b border-[#E1E4E8] px-1 dark:border-[#292C33]">
        {workflowToolTabOptions.map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => setActiveTab(tab.id)}
            className={cn(
              "min-w-0 pb-2.5 text-center transition-colors",
              activeTab === tab.id
                ? "border-b-2 border-[#202126] text-[#202126] dark:border-white dark:text-white"
                : "text-[#A0A4AD] hover:text-[#50545D] dark:hover:text-[#D7DCE4]",
            )}
          >
            <span className="block truncate">{tab.label}</span>
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-1 py-3">
        <div className="grid grid-cols-2 gap-2.5">
          {visibleTools.map((tool) => (
            <WorkflowToolCard
              key={tool.id}
              active={selectedTool === tool.id || toolToCanvasNode[selectedTool] === tool.id}
              label={tool.label}
              toolId={tool.id}
              onSelect={setSelectedTool}
            />
          ))}
        </div>
      </div>
    </aside>
  );
}

function WorkflowToolCard({
  active,
  label,
  onSelect,
  toolId,
}: {
  active: boolean;
  label: string;
  onSelect: (tool: WorkflowBuilderToolId) => void;
  toolId: WorkflowBuilderToolId;
}) {
  const Icon = workflowToolIconMap[toolId];
  const brand = workflowToolBrandMap[toolId];

  return (
    <button
      type="button"
      draggable
      aria-label={`Select ${label} tool`}
      onClick={() => onSelect(toolId)}
      onDragStart={(event) => event.dataTransfer.setData("text/plain", toolId)}
      className={cn(
        "flex min-h-[104px] flex-col items-center justify-center rounded-[8px] border p-2.5 text-center transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/15 dark:focus-visible:ring-white/30",
        active
          ? "border-[#202126] bg-white text-[#202126] shadow-[0_12px_28px_rgba(42,44,50,0.09)] dark:border-white dark:bg-[#202228] dark:text-white"
          : "border-[#ECEEF2] bg-[#ECEDEF] text-[#3D414A] hover:border-[#D8DCE2] hover:bg-white dark:border-[#202228] dark:bg-[#17181C] dark:text-[#D7DCE4] dark:hover:bg-[#202228]",
      )}
    >
      <span className="grid size-12 place-items-center rounded-[8px] border border-white/72 bg-white/78 shadow-inner dark:border-white/10 dark:bg-[#101114]">
        {brand ? (
          <BrandMark brand={brand} size="medium" />
        ) : Icon ? (
          <Icon className="size-5" strokeWidth={1.8} />
        ) : (
          <Bot className="size-5" strokeWidth={1.8} />
        )}
      </span>
      <span className="verevon-sidebar-row-strong mt-2.5 max-w-full">{label}</span>
    </button>
  );
}
