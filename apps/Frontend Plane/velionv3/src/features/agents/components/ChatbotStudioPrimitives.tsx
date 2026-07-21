import { ChevronDown, type LucideProps } from 'lucide-solid'
import { createSignal, Show, type Component, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { cn } from '@/shared/lib/cn'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { VelionTextarea } from '@/shared/ui/velion/VelionTextarea'
import { DesignPreviewBadge } from '@/features/agents/components/DesignPreviewBadge'
import type { SupportIntegrationStatus } from '@/features/agents/lib/use-chatbot-support-status'
import { useI18n } from '@/shared/i18n'

export type StudioIcon = Component<LucideProps>

export function PlaygroundAccordion(props: {
  children: JSX.Element
  defaultOpen?: boolean
  Icon: StudioIcon
  title: string
}) {
  const [open, setOpen] = createSignal(Boolean(props.defaultOpen))

  return (
    <details
      class="group rounded-[10px] border border-transparent"
      open={open()}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary class="flex cursor-pointer list-none items-center gap-3 rounded-[10px] px-0 py-2 text-[16px] font-semibold text-[#141519] transition-colors hover:bg-[#FAFAFB] dark:text-white dark:hover:bg-[#202229]">
        <span class="grid size-9 shrink-0 place-items-center rounded-[9px] bg-[#F4F4F5] text-[#1D1D1F] dark:bg-[#202229] dark:text-white">
          <Dynamic component={props.Icon} class="size-4" strokeWidth={1.8} />
        </span>
        <span class="min-w-0 flex-1 truncate">{props.title}</span>
        <ChevronDown class="size-5 text-[#5F6673] transition-transform group-open:rotate-180 dark:text-[#AEB4C0]" />
      </summary>
      <div class="pb-4 pt-2">{props.children}</div>
    </details>
  )
}

export function SupportIntegrationBanner(props: { status: SupportIntegrationStatus }) {
  const i18n = useI18n()
  const connected = () => props.status.status === 'connected'
  const checking = () => props.status.status === 'loading'

  return (
    <div
      class={cn(
        'mt-4 rounded-[10px] border p-3 text-[12px] leading-5',
        connected()
          ? 'border-[#CFE8D7] bg-[#F1FBF4] text-[#216A39] dark:border-[#254832] dark:bg-[#101A13] dark:text-[#BCE8C8]'
          : 'border-[#E3DFD7] bg-[#FAF7F1] text-[#6D6257] dark:border-[#34302B] dark:bg-[#161310] dark:text-[#D8C9BA]',
      )}
    >
      <div class="flex items-center gap-2 font-semibold">
        <span class={cn('size-2 rounded-full', checking() ? 'bg-[#D59F45]' : connected() ? 'bg-[#10B35A]' : 'bg-[#EE7A50]')} />
        {checking()
          ? i18n.tr('Sjekker live supporthandlinger', 'Checking live support actions')
          : connected()
            ? i18n.tr('Live supporthandlinger tilkoblet', 'Live support actions connected')
            : i18n.tr('Supporthandlinger ikke tilkoblet', 'Support actions not connected')}
      </div>
      <p class="mt-1">{props.status.message}</p>
      <div class="mt-2 flex flex-wrap gap-1.5">
        <SupportIntegrationPill label={i18n.tr('Agenter', 'Agents')} value={props.status.agents} />
        <SupportIntegrationPill label={i18n.tr('Grupper', 'Groups')} value={props.status.groups} />
        <SupportIntegrationPill label={i18n.tr('Makroer', 'Macros')} value={props.status.macros} />
      </div>
    </div>
  )
}

function SupportIntegrationPill(props: { label: string; value: number }) {
  return (
    <span class="rounded-full border border-black/5 bg-white/70 px-2 py-0.5 text-[10px] font-semibold uppercase text-current dark:border-white/10 dark:bg-white/5">
      {props.label}: {props.value}
    </span>
  )
}

export function SettingInput(props: {
  compact?: boolean
  label: string
  value: string
}) {
  return (
    <label class={cn('mt-4 block', props.compact ? 'max-w-[124px]' : '')}>
      <span class="block text-[14px] font-medium text-[#202126] dark:text-white">{props.label}</span>
      <VelionInput
        value={props.value}
        class="mt-2"
      />
    </label>
  )
}

export function SettingTextarea(props: { label: string; value: string }) {
  return (
    <label class="mt-4 block">
      <span class="block text-[14px] font-medium text-[#202126] dark:text-white">{props.label}</span>
      <VelionTextarea
        value={props.value}
        class="mt-2"
      />
    </label>
  )
}

export function SectionHeader(props: {
  action?: JSX.Element
  description?: string
  /**
   * Phase 4 agents-studio honesty sweep: marks the surface as a not-yet-wired
   * design preview. When set, every action-implying control on the surface
   * must also be disabled (the badge never ships without neutralized controls).
   */
  preview?: boolean
  title: string
}) {
  return (
    <div class="flex flex-wrap items-start justify-between gap-5">
      <div class="min-w-0">
        <h1 class="text-[26px] font-semibold leading-tight tracking-normal">{props.title}</h1>
        {props.description ? <p class="mt-2 text-[14px] leading-6 text-[#555B65] dark:text-[#AEB4C0]">{props.description}</p> : null}
        <Show when={props.preview}>
          <DesignPreviewBadge class="mt-4" />
        </Show>
      </div>
      {props.action ? <div class="min-w-0">{props.action}</div> : null}
    </div>
  )
}

export function EmptyStateCard(props: {
  Icon: StudioIcon
  description: string
  title: string
}) {
  return (
    <div class="mt-5 rounded-[12px] border border-dashed border-[#DADDE4] bg-[#FCFCFD] p-8 text-center dark:border-[#303238] dark:bg-[#111216]">
      <span class="mx-auto grid size-11 place-items-center rounded-[10px] bg-white text-[#8A909B] shadow-sm dark:bg-[#17181C] dark:text-[#C6CCD6]">
        <Dynamic component={props.Icon} class="size-5" />
      </span>
      <h2 class="mt-4 text-[18px] font-semibold text-[#202126] dark:text-white">{props.title}</h2>
      <p class="mx-auto mt-2 max-w-[520px] text-[14px] leading-6 text-[#6F747D] dark:text-[#AEB4C0]">{props.description}</p>
    </div>
  )
}

export function EmptyStateInline(props: {
  Icon: StudioIcon
  description: string
  title: string
}) {
  return (
    <div class="mt-5 rounded-[10px] border border-dashed border-[#DADDE4] bg-[#FCFCFD] px-4 py-5 text-center dark:border-[#303238] dark:bg-[#111216]">
      <Dynamic component={props.Icon} class="mx-auto size-5 text-[#8A909B]" />
      <p class="mt-3 text-[14px] font-semibold text-[#30343B] dark:text-white">{props.title}</p>
      <p class="mx-auto mt-1 max-w-[420px] text-[13px] leading-5 text-[#6F747D] dark:text-[#AEB4C0]">{props.description}</p>
    </div>
  )
}

/**
 * Phase 4 honesty sweep: when no measured value exists yet, MetricCard renders a
 * neutral em-dash placeholder rather than a fabricated `0`. A real aggregate is
 * passed only once a backend produces one — there is no measurement source on
 * these preview surfaces today.
 */
export function MetricCard(props: { Icon: StudioIcon; label: string; value?: string }) {
  return (
    <div class="velion-panel p-5">
      <div class="flex items-center gap-2 text-[14px] font-semibold text-[#555B65] dark:text-[#D7DCE4]">
        <Dynamic component={props.Icon} class="size-5" />
        {props.label}
      </div>
      <div class="mt-5 text-[30px] font-medium leading-none text-[#8A909B] dark:text-[#AEB4C0]">{props.value ?? '—'}</div>
    </div>
  )
}

export function FieldPill(props: { label: string; value: string }) {
  return (
    <div class="flex items-center justify-between gap-3 rounded-full bg-[#F3F1ED] px-2.5 py-1.5 text-[10px] font-semibold uppercase text-[#6E737D] dark:bg-[#202228] dark:text-[#C0C6D0]">
      <span>{props.label}</span>
      <span class="rounded-full bg-white px-2 py-0.5 text-[#383D47] dark:bg-[#111216] dark:text-white">{props.value}</span>
    </div>
  )
}
