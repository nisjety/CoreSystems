import {
  MoreHorizontal,
  Smartphone,
  Sparkles,
  Zap,
} from '@/shared/icons'
import { Dynamic } from '@solidjs/web'
import { Button } from '@/shared/ui/Button'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'
import { VerevonSwitch } from '@/shared/ui/verevon/VerevonSwitch'
import { cn } from '@/shared/lib/cn'
import type { StudioIcon } from '@/features/agents/components/ChatbotStudioPrimitives'
import { useI18n } from '@/shared/i18n'

export function IntegrationCard(props: {
  Icon: StudioIcon
  description: string
  status: string
  title: string
}) {
  const i18n = useI18n()
  const connected = () => props.status.includes('connected')

  return (
    <article class="flex min-h-[210px] flex-col rounded-[12px] border border-[#E4E5E8] bg-white p-6 shadow-sm dark:border-[#303238] dark:bg-[#15161A]">
      <div class="flex items-start justify-between gap-4">
        <span class="grid size-12 place-items-center rounded-[10px] bg-[#F4F5F7] text-[#111111] dark:bg-[#202229] dark:text-white">
          <Dynamic component={props.Icon} class="size-5" />
        </span>
        <span
          class={cn(
            'rounded-full px-3 py-1 text-[12px] font-semibold',
            connected()
              ? 'bg-[#E9F8EF] text-[#16834A]'
              : 'bg-[#F4F1EB] text-[#7A7168]',
          )}
        >
          {props.status}
        </span>
      </div>
      <h2 class="mt-5 text-[18px] font-semibold">{props.title}</h2>
      <p class="mt-3 text-[14px] leading-6 text-[#5F6673] dark:text-[#AEB4C0]">{props.description}</p>
      {/* Phase 4 honesty sweep: no integration-config backend yet. */}
      <Button size="sm" shape="rounded" disabled class="mt-auto">
        {i18n.tr('Konfigurer', 'Configure')}
      </Button>
    </article>
  )
}

export function ActionCard(props: {
  Icon: StudioIcon
  color: string
  enabled: boolean
  subtitle: string
  title: string
}) {
  const i18n = useI18n()

  return (
    <article class="flex min-h-[260px] flex-col rounded-[12px] border border-[#E4E5E8] bg-white p-8 shadow-[0_1px_2px_rgba(16,24,40,0.04)] dark:border-[#303238] dark:bg-[#15161A]">
      <div class="flex items-start justify-between gap-4">
        <span class={cn('grid size-16 place-items-center rounded-[9px] border', props.color)}>
          <Dynamic component={props.Icon} class="size-7" />
        </span>
        <ToggleSwitch enabled={props.enabled} label={i18n.tr(`${props.title} aktivert`, `${props.title} enabled`)} />
      </div>
      <h2 class="mt-7 text-[21px] font-semibold">{props.title}</h2>
      <p class="mt-2 flex items-center gap-2 text-[17px] font-medium text-[#6F747D]">
        <Zap class="size-4" />
        {props.subtitle}
      </p>
      {/* Phase 4 honesty sweep: tool menu/customize have no backend yet. */}
      <div class="mt-auto flex justify-end gap-3">
        <VerevonIconButton size="lg" shape="rounded" disabled aria-label={i18n.tr(`Åpne verktøymeny for ${props.title}`, `Open ${props.title} tool menu`)}>
          <MoreHorizontal class="size-5" />
        </VerevonIconButton>
        <Button shape="rounded" size="md" disabled>
          {i18n.tr('Tilpass', 'Customize')}
        </Button>
      </div>
    </article>
  )
}

export function ToggleSwitch(props: { enabled: boolean; label: string }) {
  return (
    <VerevonSwitch
      checked={props.enabled}
      label={props.label}
    />
  )
}

export function ChannelHeroCard(props: {
  displayName: string
  type: 'widget' | 'help'
}) {
  const i18n = useI18n()
  const widget = () => props.type === 'widget'

  return (
    <article class="overflow-hidden rounded-[12px] border border-[#E4E5E8] bg-white shadow-[0_1px_2px_rgba(16,24,40,0.04)] dark:border-[#303238] dark:bg-[#15161A]">
      <div class={cn('relative h-[290px] overflow-hidden', widget() ? 'bg-[linear-gradient(135deg,#FFD9A8,#EE7A50)]' : 'bg-[linear-gradient(135deg,#FFAA13,#FFF356)]')}>
        {widget()
          ? (
            <div class="absolute bottom-0 left-1/2 h-[250px] w-[330px] -translate-x-1/2 rounded-t-[18px] border border-[#E1E2E6] bg-white shadow-[0_14px_34px_rgba(31,35,42,0.12)]">
              <div class="flex h-12 items-center gap-3 px-5 text-[12px] font-semibold">
                <span class="grid size-8 place-items-center rounded-full bg-[#111111] text-white"><Sparkles class="size-4" /></span>
                {props.displayName}
              </div>
              <div class="ml-5 mt-3 w-max rounded-full bg-[#F4F4F5] px-4 py-2 text-[12px]">{i18n.tr('Hei! Hva kan jeg hjelpe deg med?', 'Hi! What can I help you with?')}</div>
            </div>
          )
          : (
            <div class="absolute left-1/2 top-12 h-[246px] w-[620px] -translate-x-1/2 rounded-t-[18px] bg-white px-10 pt-16 shadow-[0_14px_34px_rgba(31,35,42,0.12)]">
              <div class="absolute left-6 top-5 flex gap-2">
                <span class="size-3 rounded-full bg-[#FF3B30]" />
                <span class="size-3 rounded-full bg-[#FFCC00]" />
                <span class="size-3 rounded-full bg-[#34C759]" />
              </div>
              <h3 class="text-center text-[24px] font-semibold">{i18n.tr('Hvordan kan vi hjelpe deg i dag?', 'How can we help you today?')}</h3>
              <div class="mt-8 flex h-20 items-center rounded-[16px] border border-[#E1E2E6] px-7 text-[17px] text-[#B1B3B9]">{i18n.tr('Still et spørsmål…', 'Ask a question…')}</div>
            </div>
          )}
      </div>
      <div class="p-7">
        <div class="flex items-start justify-between gap-4">
          <div>
            <h2 class="text-[21px] font-semibold">{widget() ? i18n.tr('Chat-widget', 'Chat widget') : i18n.tr('Hjelpeside', 'Help page')}</h2>
            <p class="mt-3 max-w-[650px] text-[17px] leading-7 text-[#555B65] dark:text-[#AEB4C0]">
              {widget()
                ? i18n.tr('Legg til et flytende chattevindu på nettstedet ditt.', 'Add a floating chat window to your site.')
                : i18n.tr('Hjelpeside i ChatGPT-stil, distribuert alene eller under en sti på nettstedet ditt (/help).', 'ChatGPT-style help page, deployed standalone or under a path on your site (/help).')}
            </p>
          </div>
          {widget() ? <ToggleSwitch enabled label={i18n.tr('Chat-widget aktivert', 'Chat widget enabled')} /> : null}
        </div>
        {/* Phase 4 honesty sweep: channel manage/setup has no backend yet. */}
        <div class="mt-10 flex justify-end gap-3">
          <VerevonIconButton
            size="lg"
            shape="rounded"
            disabled
            aria-label={i18n.tr(
              `${widget() ? 'Chat-widget' : 'Hjelpeside'} forhåndsvisningsenhet`,
              `${widget() ? 'Chat widget' : 'Help page'} preview device`,
            )}
          >
            <Smartphone class="size-5" />
          </VerevonIconButton>
          <Button shape="rounded" size="md" disabled>
            {widget() ? i18n.tr('Administrer', 'Manage') : i18n.tr('Oppsett', 'Setup')}
          </Button>
        </div>
      </div>
    </article>
  )
}

export function ChannelCard(props: {
  Icon: StudioIcon
  action: string
  badge?: string
  description: string
  title: string
}) {
  const i18n = useI18n()

  return (
    <article class="flex min-h-[260px] flex-col rounded-[12px] border border-[#E4E5E8] bg-white p-8 dark:border-[#303238] dark:bg-[#15161A]">
      <span class="grid size-16 place-items-center rounded-[10px] bg-[#F4F5F7] text-[#EE7A50] dark:bg-[#111216]">
        <Dynamic component={props.Icon} class="size-8" />
      </span>
      <h2 class="mt-7 flex items-center gap-3 text-[22px] font-semibold">
        {props.title}
        {props.badge ? <span class="rounded-full bg-[#111111] px-3 py-1 text-[12px] font-semibold text-white">{props.badge}</span> : null}
      </h2>
      <p class="mt-3 text-[17px] leading-7 text-[#555B65] dark:text-[#AEB4C0]">{props.description}</p>
      {/* Phase 4 honesty sweep: channel actions have no backend yet. */}
      <div class="mt-auto flex justify-end gap-3">
        <VerevonIconButton size="lg" shape="rounded" disabled aria-label={i18n.tr(`${props.title} enhetsforhåndsvisning`, `${props.title} device preview`)}>
          <Smartphone class="size-5" />
        </VerevonIconButton>
        <Button shape="rounded" size="md" disabled>{props.action}</Button>
      </div>
    </article>
  )
}

export function SquareIconButton(props: { disabled?: boolean; Icon: StudioIcon; label: string }) {
  return (
    <VerevonIconButton size="lg" shape="rounded" disabled={props.disabled} aria-label={props.label}>
      <Dynamic component={props.Icon} class="size-5" />
    </VerevonIconButton>
  )
}
