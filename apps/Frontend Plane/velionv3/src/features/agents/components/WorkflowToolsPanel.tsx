import { Bot, MoreHorizontal, Search, X } from 'lucide-solid'
import { createMemo, createSignal, For } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { cn } from '@/shared/lib/cn'
import {
  workflowToolOptions,
  workflowToolTabOptions,
  type WorkflowBuilderToolId,
  type WorkflowToolTabId,
} from '@/features/agents/lib/agent-roles'
import { WorkflowBrandMark } from '@/features/agents/components/WorkflowBrandMark'
import {
  toolToCanvasNode,
  workflowToolBrandMap,
  workflowToolIconMap,
} from '@/features/agents/lib/velion-workflow-builder-data'
import { useWorkflowBuilderTool } from '@/features/agents/lib/use-agent-selection'
import { useI18n } from '@/shared/i18n'

export function WorkflowToolsPanel(props: {
  activeTool?: WorkflowBuilderToolId
  class?: string
  onCollapse?: () => void
  onToolChange?: (tool: WorkflowBuilderToolId) => void
  withFrame?: boolean
}) {
  const i18n = useI18n()
  const [selectedToolFromContext, setSelectedToolFromContext] = useWorkflowBuilderTool()
  const [activeTab, setActiveTab] = createSignal<WorkflowToolTabId>('ai-apps')
  const [searchQuery, setSearchQuery] = createSignal('')
  const selectedTool = () => props.activeTool ?? selectedToolFromContext()
  const setSelectedTool = (tool: WorkflowBuilderToolId) => {
    if (props.onToolChange) {
      props.onToolChange(tool)
      return
    }
    setSelectedToolFromContext(tool)
  }
  const normalizedQuery = createMemo(() => searchQuery().trim().toLowerCase())
  const visibleTools = createMemo(() => workflowToolOptions.filter((tool) => {
    const matchesTab = activeTab() === 'all' || tool.tabId === activeTab()
    const matchesSearch = !normalizedQuery() || tool.label.toLowerCase().includes(normalizedQuery())
    return matchesTab && matchesSearch
  }))

  return (
    <aside
      aria-label={i18n.tr('Arbeidsflytverktøy', 'Workflow tools')}
      class={cn(
        'velion-sidebar-type flex h-full min-w-0 flex-col bg-[#F7F7F8] font-sans text-[#25272D] dark:bg-[#101114] dark:text-white',
        props.class,
      )}
    >
      <div class="flex h-11 shrink-0 items-center justify-between gap-3 px-1">
        <div class="flex items-center gap-2.5">
          <span class="grid size-5 grid-cols-2 gap-0.5">
            <For each={[0, 1, 2, 3]}>
              {() => <span class="rounded-full border-[1.7px] border-[#17181C] dark:border-white" />}
            </For>
          </span>
          <h2 class="velion-sidebar-group-title">{i18n.tr('Verktøy', 'Tools')}</h2>
        </div>
        <div class="flex items-center gap-1">
          <VelionIconButton type="button" size="xs" shape="rounded" aria-label={i18n.tr('Flere alternativer for arbeidsflytverktøy', 'More workflow tools options')}>
            <MoreHorizontal class="size-3.5" strokeWidth={2} />
          </VelionIconButton>
          {props.onCollapse
            ? (
              <VelionIconButton
                type="button"
                size="xs"
                shape="rounded"
                onClick={props.onCollapse}
                aria-label={i18n.tr('Skjul arbeidsflytverktøy', 'Collapse workflow tools')}
              >
                <X class="size-3.5" strokeWidth={2} />
              </VelionIconButton>
            )
            : null}
        </div>
      </div>

      <div class="px-1 pb-3">
        <label class="relative block">
          <Search class="pointer-events-none absolute left-3 top-1/2 size-[15px] -translate-y-1/2 text-[#A3A7AF]" strokeWidth={2} />
          <input
            aria-label={i18n.tr('Søk i arbeidsflytverktøy', 'Workflow tool search')}
            type="search"
            value={searchQuery()}
            onInput={(event) => setSearchQuery(event.currentTarget.value)}
            placeholder={i18n.tr('Søk …', 'Search…')}
            class="velion-field-compact h-9 pl-9 pr-3 placeholder:text-[#B2B6BE] dark:placeholder:text-[#777E8B]"
          />
        </label>
      </div>

      <div class="velion-sidebar-row-strong grid grid-cols-4 border-b border-[#E1E4E8] px-1 dark:border-[#292C33]">
        <For each={workflowToolTabOptions}>
          {(tab) => (
            <button
              type="button"
              onClick={() => setActiveTab(tab.id)}
              class={cn(
                'min-w-0 pb-2.5 text-center transition-colors',
                activeTab() === tab.id
                  ? 'border-b-2 border-[#202126] text-[#202126] dark:border-white dark:text-white'
                  : 'text-[#A0A4AD] hover:text-[#50545D] dark:hover:text-[#D7DCE4]',
              )}
            >
              <span class="block truncate">{tab.label}</span>
            </button>
          )}
        </For>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-1 py-3">
        <div class="grid grid-cols-2 gap-2.5">
          <For each={visibleTools()}>
            {(tool) => (
              <WorkflowToolCard
                active={selectedTool() === tool.id || toolToCanvasNode[selectedTool()] === tool.id}
                label={tool.label}
                onSelect={setSelectedTool}
                toolId={tool.id}
              />
            )}
          </For>
        </div>
      </div>
    </aside>
  )
}

function WorkflowToolCard(props: {
  active: boolean
  label: string
  onSelect: (tool: WorkflowBuilderToolId) => void
  toolId: WorkflowBuilderToolId
}) {
  const i18n = useI18n()
  return (
    <button
      type="button"
      draggable
      aria-label={i18n.tr(`Velg verktøyet ${props.label}`, `Select ${props.label} tool`)}
      onClick={() => props.onSelect(props.toolId)}
      onDragStart={(event) => {
        event.dataTransfer?.setData('text/plain', props.toolId)
      }}
      class={cn(
        'flex min-h-[104px] flex-col items-center justify-center rounded-[8px] border p-2.5 text-center transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/15 dark:focus-visible:ring-white/30',
        props.active
          ? 'border-[#202126] bg-white text-[#202126] shadow-[0_12px_28px_rgba(42,44,50,0.09)] dark:border-white dark:bg-[#202228] dark:text-white'
          : 'border-[#ECEEF2] bg-[#ECEDEF] text-[#3D414A] hover:border-[#D8DCE2] hover:bg-white dark:border-[#202228] dark:bg-[#17181C] dark:text-[#D7DCE4] dark:hover:bg-[#202228]',
      )}
    >
      <span class="grid size-12 place-items-center rounded-[8px] border border-white/72 bg-white/78 shadow-inner dark:border-white/10 dark:bg-[#101114]">
        {workflowToolBrandMap[props.toolId]
          ? <WorkflowBrandMark brand={workflowToolBrandMap[props.toolId]!} size="medium" />
          : workflowToolIconMap[props.toolId]
            ? <Dynamic component={workflowToolIconMap[props.toolId]!} class="size-5" strokeWidth={1.8} />
            : <Bot class="size-5" strokeWidth={1.8} />}
      </span>
      <span class="velion-sidebar-row-strong mt-2.5 max-w-full">{props.label}</span>
    </button>
  )
}
