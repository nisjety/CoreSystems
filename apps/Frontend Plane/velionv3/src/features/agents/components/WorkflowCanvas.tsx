import {
  Bot,
  Check,
  Loader2,
  Maximize2,
  Mic,
  Send,
  Sparkles,
} from 'lucide-solid'
import { For } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { cn } from '@/shared/lib/cn'
import type { WorkflowBuilderToolId } from '@/features/agents/lib/agent-roles'
import { DesignPreviewBadge } from '@/features/agents/components/DesignPreviewBadge'
import { WorkflowBrandMark } from '@/features/agents/components/WorkflowBrandMark'
import {
  type CanvasNodeId,
  type WorkflowNode,
  workflowNodes,
} from '@/features/agents/lib/velion-workflow-builder-data'
import { useI18n } from '@/shared/i18n'

export function WorkflowTopBar() {
  const i18n = useI18n()
  return (
    <header class="absolute left-4 right-4 top-4 z-20 flex items-center justify-center lg:right-[354px]">
      <div class="flex min-h-12 w-full max-w-[900px] items-center justify-between gap-3 rounded-full border border-white/78 bg-white/72 px-2.5 py-1.5 shadow-[0_18px_54px_rgba(42,44,50,0.12)] backdrop-blur-xl dark:border-white/10 dark:bg-[#17181C]/76">
        <div class="flex min-w-0 items-center gap-2.5">
          <span class="grid h-8 w-12 shrink-0 place-items-center rounded-full border border-white/80 bg-white text-[#26282F] shadow-inner dark:border-white/10 dark:bg-[#101114] dark:text-white">
            <Sparkles class="size-[18px]" strokeWidth={1.8} />
          </span>
          <div class="min-w-0">
            <h1 class="truncate text-[14px] font-semibold leading-5 text-[#282A30] dark:text-white">
              {i18n.tr('Generer innlegg til sosiale medier', 'Generate Social Media Post')}
            </h1>
            <p class="flex items-center gap-1 text-[11px] font-medium text-[#7B808A] dark:text-[#AEB4C0]">
              <Bot class="size-3" strokeWidth={2} />
              {i18n.tr('Teamprosjekt', 'Team project')}
            </p>
          </div>
        </div>

        {/*
          Phase 3 PR-1 (honesty): the WorkflowBuilder is a static design preview
          with no execution/publish substrate (the model plane is an LLM agent
          loop, not an n8n DAG runner). The dead "Test Run" / "Publish" controls
          are removed and replaced with a non-interactive Design preview badge so
          no control implies a backend that does not exist.
        */}
        <div class="flex shrink-0 items-center gap-2">
          <DesignPreviewBadge />
        </div>
      </div>
    </header>
  )
}

export function WorkflowCanvas(props: {
  onNodeSelect: (tool: WorkflowBuilderToolId) => void
  selectedNodeId: CanvasNodeId
}) {
  const i18n = useI18n()
  return (
    <div class="absolute inset-0">
      <div class="absolute left-1/2 top-1/2 h-[560px] w-[1040px] origin-center -translate-x-1/2 -translate-y-1/2 scale-[0.62] xl:scale-[0.7] 2xl:scale-[0.88] min-[1800px]:scale-100">
        <svg
          aria-hidden="true"
          viewBox="0 0 1040 560"
          class="absolute inset-0 size-full overflow-visible [&_.workflow-edge]:fill-none [&_.workflow-edge]:stroke-[#C8CCD2] [&_.workflow-edge]:stroke-[2] dark:[&_.workflow-edge]:stroke-[#424751]"
        >
          <defs>
            <marker id="workflow-arrow" markerHeight="8" markerWidth="8" orient="auto" refX="6" refY="4">
              <path d="M0,0 L7,4 L0,8" fill="none" stroke="#C8CCD2" stroke-width="1.8" />
            </marker>
          </defs>
          <path d="M112 304 H202" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M282 304 H372" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M452 304 H528 Q548 304 548 264 V214 Q548 178 590 178" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M452 304 H552" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M452 304 H528 Q548 304 548 344 V394 Q548 430 590 430" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M632 178 H682 Q720 178 720 238 V304 H738" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M632 304 H738" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M632 430 H682 Q720 430 720 370 V304 H738" class="workflow-edge" marker-end="url(#workflow-arrow)" />
          <path d="M782 304 H892" class="workflow-edge" marker-end="url(#workflow-arrow)" />
        </svg>

        <For each={workflowNodes}>
          {(node) => (
            <WorkflowCanvasNode
              active={props.selectedNodeId === node.id}
              node={node}
              onSelect={() => props.onNodeSelect(node.id)}
              i18n={i18n}
            />
          )}
        </For>
      </div>
    </div>
  )
}

function WorkflowCanvasNode(props: {
  active: boolean
  node: WorkflowNode
  onSelect: () => void
  i18n: ReturnType<typeof useI18n>
}) {
  const label = () => props.node.title.split('\n').join(' ')

  return (
    <button
      type="button"
      aria-label={props.i18n.tr(`Velg arbeidsflytnoden ${label()}`, `Select ${label()} workflow node`)}
      onClick={() => props.onSelect()}
      class={cn(
        'absolute flex -translate-x-1/2 -translate-y-1/2 flex-col items-center text-center transition duration-150 hover:scale-[1.02] focus:outline-none',
        props.active ? 'z-10' : 'z-0',
      )}
      style={{ left: `${props.node.x}px`, top: `${props.node.y}px` }}
    >
      <span
        class={cn(
          'grid place-items-center border border-white/74 bg-white/82 text-[#30333A] shadow-[inset_0_1px_1px_rgba(255,255,255,0.8),0_16px_38px_rgba(40,42,48,0.11)] backdrop-blur dark:border-white/10 dark:bg-[#17181C]/90 dark:text-white',
          props.node.tall ? 'h-[84px] w-12 rounded-[14px]' : 'size-[64px] rounded-[16px]',
          props.active ? 'ring-2 ring-[#111111]/70 ring-offset-3 ring-offset-transparent dark:ring-white/80' : '',
        )}
      >
        {props.node.brand
          ? <WorkflowBrandMark brand={props.node.brand} size="medium" />
          : props.node.Icon
            ? <Dynamic component={props.node.Icon} class="size-6" strokeWidth={1.9} />
            : null}
      </span>
      <span class="mt-2.5 max-w-[108px] whitespace-pre-line text-[13px] font-medium leading-4 text-[#50545D] dark:text-[#D4D8E0]">
        {props.node.title}
      </span>
    </button>
  )
}

export function WorkflowGenerationStatus() {
  const i18n = useI18n()
  return (
    <div class="absolute bottom-[80px] left-1/2 z-20 hidden w-[300px] -translate-x-1/2 text-[11px] font-medium text-[#8B909A] sm:block lg:left-[calc(50%-169px)]">
      <div class="flex items-center gap-2">
        <Check class="size-3.5" strokeWidth={2} />
        {i18n.tr('Søker etter noder', 'Searching nodes')}
      </div>
      <div class="mt-2 flex items-center gap-2">
        <Check class="size-3.5" strokeWidth={2} />
        {i18n.tr('Legger til noder', 'Adding nodes')}
      </div>
      <div class="mt-2 flex items-center gap-2 text-[#555963] dark:text-[#D7DCE4]">
        <Loader2 class="size-3.5 animate-spin" strokeWidth={2} />
        {i18n.tr('Validerer arbeidsflyt', 'Validating workflow')}
      </div>
    </div>
  )
}

export function WorkflowPromptComposer() {
  const i18n = useI18n()
  return (
    <form
      class="absolute bottom-6 left-4 right-4 z-20 mx-auto flex h-12 max-w-[660px] items-center gap-2 rounded-full border border-white/82 bg-white/82 px-3.5 shadow-[inset_0_1px_1px_rgba(255,255,255,0.9),0_18px_48px_rgba(42,44,50,0.13)] backdrop-blur-xl dark:border-white/10 dark:bg-[#17181C]/86"
      onSubmit={(event) => event.preventDefault()}
    >
      {/* Phase 3 PR-1: composer is inert in the design preview — workflow
          generation has no backend, so the input + action buttons are disabled
          rather than presenting a "Generate" affordance that does nothing. */}
      <input
        aria-label={i18n.tr('Arbeidsflyt-prompt', 'Workflow prompt')}
        disabled
        placeholder={i18n.tr('Arbeidsflytgenerering er en designforhåndsvisning', 'Workflow generation is a design preview')}
        class="min-w-0 flex-1 bg-transparent text-[13px] font-medium text-[#2E3138] outline-none placeholder:text-[#A7ABB3] disabled:cursor-not-allowed dark:text-white dark:placeholder:text-[#797F8A]"
      />
      <VelionIconButton type="button" size="sm" shape="circle" aria-label={i18n.tr('Utvid komponisten', 'Expand composer')} disabled class="shrink-0">
        <Maximize2 class="size-3.5" strokeWidth={2} />
      </VelionIconButton>
      <VelionIconButton type="button" size="sm" shape="circle" aria-label={i18n.tr('Dikter arbeidsflyt-prompt', 'Dictate workflow prompt')} disabled class="shrink-0">
        <Mic class="size-4" strokeWidth={2} />
      </VelionIconButton>
      <VelionIconButton
        type="submit"
        size="md"
        tone="primary"
        shape="circle"
        aria-label={i18n.tr('Generer arbeidsflyt', 'Generate workflow')}
        disabled
        class="shrink-0"
      >
        <Send class="size-3.5" strokeWidth={2.1} />
      </VelionIconButton>
    </form>
  )
}
