import {
  ChevronDown,
  GripVertical,
  Info,
  MoreHorizontal,
  Pencil,
  Plus,
  X,
} from 'lucide-solid'
import { For } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { Button } from '@/shared/ui/Button'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { WorkflowBrandMark } from '@/features/agents/components/WorkflowBrandMark'
import type { InspectorField, ToolInspector } from '@/features/agents/lib/velion-workflow-builder-data'

export function WorkflowInspector(props: { inspector: ToolInspector }) {
  return (
    <aside class="absolute bottom-4 right-4 top-4 z-30 hidden w-[318px] flex-col overflow-hidden rounded-[18px] border border-white/76 bg-white/82 shadow-[0_24px_70px_rgba(43,45,52,0.16)] backdrop-blur-xl dark:border-white/10 dark:bg-[#17181C]/90 lg:flex">
      <div class="flex h-12 shrink-0 items-center gap-2 border-b border-[#E8E9EC] px-4 dark:border-[#2A2C31]">
        <span class="grid size-6 place-items-center rounded-full bg-[#F2F3F5] text-[#343842] dark:bg-[#202228] dark:text-white">
          {props.inspector.brand
            ? <WorkflowBrandMark brand={props.inspector.brand} size="small" />
            : props.inspector.Icon
              ? <Dynamic component={props.inspector.Icon} class="size-3.5" strokeWidth={2} />
              : null}
        </span>
        <h2 class="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#282B31] dark:text-white">
          {props.inspector.title}
        </h2>
        {/* Phase 3 PR-1: inspector is part of the WorkflowBuilder design
            preview — its config actions have no backend, so they are disabled. */}
        <VelionIconButton type="button" size="xs" shape="rounded" aria-label="More tool options" disabled>
          <MoreHorizontal class="size-3.5" strokeWidth={2} />
        </VelionIconButton>
        <VelionIconButton type="button" size="xs" shape="rounded" aria-label="Close tool settings" disabled>
          <X class="size-3.5" strokeWidth={2} />
        </VelionIconButton>
      </div>

      <div class="grid grid-cols-[1fr_auto_1fr_auto_1fr] items-center border-b border-[#E8E9EC] px-4 py-3 text-[11px] font-semibold dark:border-[#2A2C31]">
        <span class="text-[#606672]">Setup</span>
        <ChevronDown class="-rotate-90 text-[#A2A7B0]" size={15} />
        <span class="border-b-2 border-[#343842] pb-3 text-center text-[#343842] dark:border-white dark:text-white">Configure</span>
        <ChevronDown class="-rotate-90 text-[#A2A7B0]" size={15} />
        <span class="text-right text-[#B2B6BE]">Test</span>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-4">
        <div class="space-y-4">
          <For each={props.inspector.fields}>
            {(field) => <InspectorSelect field={field} />}
          </For>

          <div>
            <InspectorLabel label="Prompt" />
            <div class="mt-2 rounded-[8px] border border-[#DDE0E5] bg-white/70 p-3 text-[11px] leading-5 text-[#4C515B] dark:border-[#30333B] dark:bg-[#111216]/70 dark:text-[#D8DDE6]">
              <For each={props.inspector.prompt.split('\n')}>
                {(line) => (
                  <p class={line.startsWith('-') ? 'pl-3' : undefined}>
                    {line}
                  </p>
                )}
              </For>
            </div>
          </div>

          <Button variant="secondary" size="xs" shape="pill" disabled>
            <Pencil class="size-3.5" strokeWidth={2} />
            Improve prompt
          </Button>

          <div>
            <InspectorLabel label="Output" />
            <div class="mt-2 rounded-[8px] border border-[#DDE0E5] bg-white/70 p-2 dark:border-[#30333B] dark:bg-[#111216]/70">
              <For each={props.inspector.outputs}>
                {(output) => (
                  <div class="flex h-8 items-center gap-2 rounded-[7px] px-1.5 text-[11px] font-medium text-[#5B616C] dark:text-[#D6DBE4]">
                    <GripVertical class="size-4 text-[#B1B6BE]" strokeWidth={1.8} />
                    <span class="min-w-0 flex-1 truncate">{output}</span>
                    <Pencil class="size-3.5 text-[#A5AAB3]" strokeWidth={2} />
                  </div>
                )}
              </For>
            </div>
            <Button variant="secondary" size="xs" shape="pill" class="mt-2" disabled>
              <Plus class="size-3.5" strokeWidth={2} />
              Add output
            </Button>
          </div>
        </div>
      </div>

      <div class="shrink-0 px-4 pb-4 pt-2">
        <Button variant="secondary" size="xs" shape="pill" fullWidth disabled>
          {props.inspector.nextLabel}
        </Button>
      </div>
    </aside>
  )
}

function InspectorLabel(props: { label: string }) {
  return (
    <div class="flex items-center gap-1.5 text-[11px] font-medium text-[#5B616C] dark:text-[#C8CDD6]">
      {props.label}
      <Info class="size-3.5 text-[#A6ABB4]" strokeWidth={2} />
    </div>
  )
}

function InspectorSelect(props: { field: InspectorField }) {
  return (
    <label class="block">
      <InspectorLabel label={props.field.label} />
      <span class="mt-2 flex h-9 items-center rounded-[8px] border border-[#DDE0E5] bg-white/70 px-3 text-[11px] font-medium text-[#555B65] dark:border-[#30333B] dark:bg-[#111216]/70 dark:text-[#D8DDE6]">
        <span class="min-w-0 flex-1 truncate">{props.field.value}</span>
        <ChevronDown class="size-4 text-[#9EA4AD]" strokeWidth={2} />
      </span>
    </label>
  )
}
