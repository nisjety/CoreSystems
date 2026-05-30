"use client";

import type { CSSProperties } from "react";
import {
  Bot,
  Check,
  Loader2,
  Maximize2,
  Mic,
  Play,
  Send,
  Sparkles,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { VelionButton, VelionIconButton } from "@/components/ui/velion-ui";
import type { WorkflowBuilderToolId } from "@/features/agents-v2/lib/agent-roles";
import {
  type CanvasNodeId,
  type WorkflowNode,
  workflowNodes,
} from "@/features/agents-v2/lib/velion-workflow-builder-data";
import { BrandMark } from "@/features/agents-v2/components/VelionWorkflowBrandMark";

export function WorkflowTopBar() {
  return (
    <header className="absolute left-4 right-4 top-4 z-20 flex items-center justify-center lg:right-[354px]">
      <div className="flex min-h-12 w-full max-w-[900px] items-center justify-between gap-3 rounded-full border border-white/78 bg-white/72 px-2.5 py-1.5 shadow-[0_18px_54px_rgba(42,44,50,0.12)] backdrop-blur-xl dark:border-white/10 dark:bg-[#17181C]/76">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="grid h-8 w-12 shrink-0 place-items-center rounded-full border border-white/80 bg-white text-[#26282F] shadow-inner dark:border-white/10 dark:bg-[#101114] dark:text-white">
            <Sparkles className="size-[18px]" strokeWidth={1.8} />
          </span>
          <div className="min-w-0">
            <h1 className="truncate text-[14px] font-semibold leading-5 text-[#282A30] dark:text-white">
              Generate Social Media Post
            </h1>
            <p className="flex items-center gap-1 text-[11px] font-medium text-[#7B808A] dark:text-[#AEB4C0]">
              <Bot className="size-3" strokeWidth={2} />
              Team project
            </p>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <VelionButton variant="secondary" size="xs" radius="pill" className="px-3 font-semibold">
            <Play className="size-3.5 fill-current" strokeWidth={1.8} />
            <span className="hidden sm:inline">Test Run</span>
          </VelionButton>
          <VelionButton variant="primary" size="xs" radius="pill" className="px-4 font-semibold">
            Publish
          </VelionButton>
        </div>
      </div>
    </header>
  );
}

export function WorkflowCanvas({
  onNodeSelect,
  selectedNodeId,
}: {
  onNodeSelect: (tool: WorkflowBuilderToolId) => void;
  selectedNodeId: CanvasNodeId;
}) {
  return (
    <div className="absolute inset-0">
      <div className="absolute left-1/2 top-1/2 h-[560px] w-[1040px] origin-center -translate-x-1/2 -translate-y-1/2 scale-[0.62] xl:scale-[0.7] 2xl:scale-[0.88] min-[1800px]:scale-100">
        <svg
          aria-hidden="true"
          viewBox="0 0 1040 560"
          className="absolute inset-0 size-full overflow-visible [&_.workflow-edge]:fill-none [&_.workflow-edge]:stroke-[#C8CCD2] [&_.workflow-edge]:stroke-[2] dark:[&_.workflow-edge]:stroke-[#424751]"
        >
          <defs>
            <marker id="workflow-arrow" markerHeight="8" markerWidth="8" orient="auto" refX="6" refY="4">
              <path d="M0,0 L7,4 L0,8" fill="none" stroke="#C8CCD2" strokeWidth="1.8" />
            </marker>
          </defs>
          <path d="M112 304 H202" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M282 304 H372" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M452 304 H528 Q548 304 548 264 V214 Q548 178 590 178" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M452 304 H552" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M452 304 H528 Q548 304 548 344 V394 Q548 430 590 430" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M632 178 H682 Q720 178 720 238 V304 H738" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M632 304 H738" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M632 430 H682 Q720 430 720 370 V304 H738" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
          <path d="M782 304 H892" className="workflow-edge" markerEnd="url(#workflow-arrow)" />
        </svg>

        {workflowNodes.map((node) => (
          <WorkflowCanvasNode
            key={node.id}
            node={node}
            active={selectedNodeId === node.id}
            onSelect={() => onNodeSelect(node.id)}
          />
        ))}
      </div>
    </div>
  );
}

function WorkflowCanvasNode({
  active,
  node,
  onSelect,
}: {
  active: boolean;
  node: WorkflowNode;
  onSelect: () => void;
}) {
  const Icon = node.Icon;
  const style = { left: node.x, top: node.y } satisfies CSSProperties;
  const label = node.title.split("\n");

  return (
    <button
      type="button"
      aria-label={`Select ${label.join(" ")} workflow node`}
      onClick={onSelect}
      className={cn(
        "absolute flex -translate-x-1/2 -translate-y-1/2 flex-col items-center text-center transition duration-150 hover:scale-[1.02] focus:outline-none",
        active ? "z-10" : "z-0",
      )}
      style={style}
    >
      <span
        className={cn(
          "grid place-items-center border border-white/74 bg-white/82 text-[#30333A] shadow-[inset_0_1px_1px_rgba(255,255,255,0.8),0_16px_38px_rgba(40,42,48,0.11)] backdrop-blur dark:border-white/10 dark:bg-[#17181C]/90 dark:text-white",
          node.tall ? "h-[84px] w-12 rounded-[14px]" : "size-[64px] rounded-[16px]",
          active ? "ring-2 ring-[#111111]/70 ring-offset-3 ring-offset-transparent dark:ring-white/80" : "",
        )}
      >
        {node.brand ? (
          <BrandMark brand={node.brand} size="medium" />
        ) : Icon ? (
          <Icon className="size-6" strokeWidth={1.9} />
        ) : null}
      </span>
      <span className="mt-2.5 max-w-[108px] whitespace-pre-line text-[13px] font-medium leading-4 text-[#50545D] dark:text-[#D4D8E0]">
        {node.title}
      </span>
    </button>
  );
}

export function WorkflowGenerationStatus() {
  return (
    <div className="absolute bottom-[80px] left-1/2 z-20 hidden w-[300px] -translate-x-1/2 text-[11px] font-medium text-[#8B909A] sm:block lg:left-[calc(50%-169px)]">
      <div className="flex items-center gap-2">
        <Check className="size-3.5" strokeWidth={2} />
        Searching nodes
      </div>
      <div className="mt-2 flex items-center gap-2">
        <Check className="size-3.5" strokeWidth={2} />
        Adding nodes
      </div>
      <div className="mt-2 flex items-center gap-2 text-[#555963] dark:text-[#D7DCE4]">
        <Loader2 className="size-3.5 animate-spin" strokeWidth={2} />
        Validating workflow
      </div>
    </div>
  );
}

export function WorkflowPromptComposer() {
  return (
    <form className="absolute bottom-6 left-4 right-4 z-20 mx-auto flex h-12 max-w-[660px] items-center gap-2 rounded-full border border-white/82 bg-white/82 px-3.5 shadow-[inset_0_1px_1px_rgba(255,255,255,0.9),0_18px_48px_rgba(42,44,50,0.13)] backdrop-blur-xl dark:border-white/10 dark:bg-[#17181C]/86">
      <input
        aria-label="Workflow prompt"
        placeholder="Describe your workflow to Aira"
        defaultValue=""
        className="min-w-0 flex-1 bg-transparent text-[13px] font-medium text-[#2E3138] outline-none placeholder:text-[#A7ABB3] dark:text-white dark:placeholder:text-[#797F8A]"
      />
      <VelionIconButton type="button" radius="pill" aria-label="Expand composer" className="shrink-0">
        <Maximize2 className="size-3.5" strokeWidth={2} />
      </VelionIconButton>
      <VelionIconButton type="button" radius="pill" aria-label="Dictate workflow prompt" className="shrink-0">
        <Mic className="size-4" strokeWidth={2} />
      </VelionIconButton>
      <VelionIconButton
        type="submit"
        size="md"
        radius="pill"
        aria-label="Generate workflow"
        className="shrink-0 bg-[#252528] text-white shadow-[0_10px_26px_rgba(0,0,0,0.22)] hover:bg-[#050505] hover:text-white dark:bg-white dark:text-[#111111]"
      >
        <Send className="size-3.5" strokeWidth={2.1} />
      </VelionIconButton>
    </form>
  );
}
