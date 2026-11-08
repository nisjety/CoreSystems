import { createMemo, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { CalendarClock, ChevronRight, Gauge, Puzzle, SlidersHorizontal } from '@/shared/icons'
import { controlFocusClass } from '@/features/agents/lib/verevon-agent-page-styles'
import {
  automationStateOf,
  countAutomations,
  type AutomationRow,
} from '@/features/automations/lib/automation-state'
import { ApiError } from '@/shared/api/api-error'
import type { CronSchedule } from '@/shared/api/cron-client'
import { listMySchedules } from '@/shared/api/schedule-from-chat'
import { getAgentInstallations, type AgentDefinitionInstallations } from '@/shared/api/spaces-client'
import { isZdrCode, zdrRefusalPresentation } from '@/shared/api/zdr-refusals'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { createResource, type ResourceAccessor } from '@/shared/lib/create-resource-compat'
import { getSession } from '@/shared/session/session-store'

type Tr = ReturnType<typeof useI18n>['tr']

// The landing is a summary: a card names this many agents and says how many it leaves out.
const AGENTS_LISTED = 3

/**
 * The top of the agents landing (EP-6-S5): what the person already has, over the reads the
 * hubs make. Agents come from the org's installations, automations from the person's own
 * schedules; runs and customisation are links. A failed read is said to have failed, never shown
 * as an empty list. Reads only: nothing here offers to act.
 */
export function AgentsHubSummary() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id?.trim() ?? '')
  const myUserId = createMemo(() => session.user?.id ?? '')

  const [agents] = createResource(getAgentInstallations)
  // Schedules are the person's own, so the read waits for both the organisation and the person.
  const [schedules] = createResource(
    () => {
      const org = orgId()
      const user = myUserId()
      return org && user ? { org, user } : undefined
    },
    ({ org, user }) => listMySchedules(org, user),
  )

  return (
    <section aria-labelledby="agents-hub-summary-heading" class="agents-overview-section">
      <h2 id="agents-hub-summary-heading" class="text-[16px] font-semibold text-[var(--verevon-dark-surface)] dark:text-[var(--panel)]">
        {i18n.tr('Dine agenter og automatiseringer', 'Your agents and automations')}
      </h2>
      <div class="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
        <AgentsCard agents={agents} />
        <AutomationsCard schedules={schedules} ready={() => Boolean(orgId() && myUserId())} />
        <SummaryCard
          eyebrow={i18n.tr('Kjøringer', 'Runs')}
          href="/agents/runs"
          icon={<Gauge class="size-3.5" strokeWidth={2.1} />}
          linkLabel={i18n.tr('Åpne kjøringskonsollen', 'Open the run console')}
        >
          <p>
            {i18n.tr(
              'Kjøringene du starter, og godkjenningene de trenger, finner du i kjøringskonsollen.',
              'Runs you start, and the approvals they need, are in the run console.',
            )}
          </p>
        </SummaryCard>
        <SummaryCard
          eyebrow={i18n.tr('Tilpass', 'Customize')}
          href="/agents/customize"
          icon={<SlidersHorizontal class="size-3.5" strokeWidth={2.1} />}
          linkLabel={i18n.tr('Åpne Tilpass', 'Open Customize')}
        >
          <p>
            {i18n.tr(
              'Hva agentene kan gjøre, hvilke verktøy og instrukser de bruker, og hvilke regler som gjelder før de handler.',
              'What agents may do, which tools and instructions they use, and the rules that apply before they act.',
            )}
          </p>
        </SummaryCard>
      </div>
    </section>
  )
}

function AgentsCard(props: { agents: ResourceAccessor<readonly AgentDefinitionInstallations[]> }) {
  const i18n = useI18n()
  const definitions = () => props.agents() ?? []
  const countLabel = () => {
    const total = definitions().length
    return i18n.tr(`${total} ${total === 1 ? 'agent' : 'agenter'}`, `${total} ${total === 1 ? 'agent' : 'agents'}`)
  }

  return (
    <SummaryCard
      eyebrow={i18n.tr('Agenter', 'Agents')}
      href="/agents/installations"
      icon={<Puzzle class="size-3.5" strokeWidth={2.1} />}
      linkLabel={i18n.tr('Åpne installasjoner', 'Open installations')}
    >
      <Show
        when={!props.agents.loading}
        fallback={<p role="status">{i18n.tr('Henter agentene …', 'Loading agents…')}</p>}
      >
        <Show
          when={!props.agents.error}
          fallback={(
            <p role="alert">
              {i18n.tr('Agentene kunne ikke hentes akkurat nå.', 'Agents could not be loaded right now.')}
            </p>
          )}
        >
          <Show
            when={definitions().length > 0}
            fallback={(
              <p>
                <strong class="font-semibold text-[var(--verevon-dark-surface)] dark:text-[var(--panel)]">{i18n.tr('Ingen agenter ennå.', 'No agents yet.')}</strong>{' '}
                {i18n.tr('Legg til en agent fra et roms Agent-fane, så vises den her.', 'Add one from a room\'s Agent tab and it will show here.')}
              </p>
            )}
          >
            <p class="text-[15px] font-semibold text-[var(--verevon-dark-surface)] dark:text-[var(--panel)]">{countLabel()}</p>
            <ul class="flex flex-col gap-1">
              <For each={definitions().slice(0, AGENTS_LISTED)}>
                {(definition) => (
                  <li class="flex min-w-0 items-baseline justify-between gap-2">
                    <span class="truncate font-medium text-[var(--verevon-dark-surface)] dark:text-[var(--panel)]">
                      {definition.name?.trim() || i18n.tr('Agent uten navn', 'Agent with no name')}
                    </span>
                    <span class="shrink-0 text-[12px] text-[var(--verevon-text-muted)] dark:text-[var(--verevon-text-subtle)]">
                      {roomsLabel(definition.installations.length, i18n)}
                    </span>
                  </li>
                )}
              </For>
              <Show when={definitions().length > AGENTS_LISTED}>
                <li class="text-[12px] text-[var(--verevon-text-muted)] dark:text-[var(--verevon-text-subtle)]">
                  {i18n.tr(
                    `og ${definitions().length - AGENTS_LISTED} til`,
                    `and ${definitions().length - AGENTS_LISTED} more`,
                  )}
                </li>
              </Show>
            </ul>
          </Show>
        </Show>
      </Show>
    </SummaryCard>
  )
}

function AutomationsCard(props: { schedules: ResourceAccessor<CronSchedule[]>; ready: () => boolean }) {
  const i18n = useI18n()
  const localeTag = () => (i18n.locale() === 'no' ? 'nb-NO' : 'en-US')
  const rows = createMemo((): AutomationRow[] => {
    const now = Date.now()
    return (props.schedules() ?? []).map((schedule) => ({ schedule, state: automationStateOf(schedule, now) }))
  })
  const counts = createMemo(() => countAutomations(rows()))
  const nextRun = createMemo(() => nextRunOf(rows()))
  const countLabel = () => {
    const { active, paused, expired } = counts()
    const parts = [i18n.tr(`${active} ${active === 1 ? 'aktiv' : 'aktive'}`, `${active} active`)]
    if (paused > 0) parts.push(i18n.tr(`${paused} pauset`, `${paused} paused`))
    if (expired > 0) parts.push(i18n.tr(`${expired} utløpt`, `${expired} expired`))
    return parts.join(' · ')
  }

  return (
    <SummaryCard
      eyebrow={i18n.tr('Automatiseringer', 'Automations')}
      href="/agents/automations"
      icon={<CalendarClock class="size-3.5" strokeWidth={2.1} />}
      linkLabel={i18n.tr('Åpne automatiseringer', 'Open automations')}
    >
      <Show
        when={props.ready() && !props.schedules.loading}
        fallback={<p role="status">{i18n.tr('Henter automatiseringene …', 'Loading automations…')}</p>}
      >
        <Show
          when={!props.schedules.error}
          fallback={(
            <p role="alert">
              {refusalText(props.schedules.error, i18n.tr) ?? i18n.tr(
                'Automatiseringene kunne ikke hentes akkurat nå.',
                'Automations could not be loaded right now.',
              )}
            </p>
          )}
        >
          <Show
            when={rows().length > 0}
            fallback={(
              <p>
                <strong class="font-semibold text-[var(--verevon-dark-surface)] dark:text-[var(--panel)]">{i18n.tr('Ingen automatiseringer ennå.', 'No automations yet.')}</strong>{' '}
                <a href="/chat" link class={cn('underline-offset-4 hover:underline', controlFocusClass)}>
                  {i18n.tr('Planlegg en forespørsel i chatten', 'Schedule a request from chat')}
                </a>
              </p>
            )}
          >
            <p class="text-[15px] font-semibold text-[var(--verevon-dark-surface)] dark:text-[var(--panel)]">{countLabel()}</p>
            <Show when={nextRun()}>
              {(iso) => (
                <p>
                  {i18n.tr('Neste kjøring: ', 'Next run: ')}
                  <time datetime={iso()}>{formatWhen(iso(), localeTag())}</time>
                </p>
              )}
            </Show>
          </Show>
        </Show>
      </Show>
    </SummaryCard>
  )
}

function SummaryCard(props: {
  children: JSX.Element
  eyebrow: string
  href: string
  icon: JSX.Element
  linkLabel: string
}) {
  return (
    <div class="flex min-w-0 flex-col rounded-[8px] border border-[var(--verevon-border)] bg-[var(--verevon-surface)] p-4 shadow-sm dark:border-[var(--verevon-dark-border)] dark:bg-[var(--foreground)]">
      <h3 class="flex items-center gap-2 text-[11px] font-semibold uppercase text-[var(--verevon-text-muted)] dark:text-[var(--verevon-text-subtle)]">
        {props.icon}
        {props.eyebrow}
      </h3>
      <div class="mt-3 flex flex-1 flex-col gap-2 text-[13px] leading-5 text-[var(--verevon-text-soft)] dark:text-[var(--border)]">
        {props.children}
      </div>
      <a
        href={props.href}
        link
        class={cn(
          'mt-4 inline-flex items-center gap-1 self-start rounded-[6px] text-[12px] font-semibold text-[var(--verevon-dark-surface)] hover:underline dark:text-[var(--panel)]',
          controlFocusClass,
        )}
      >
        {props.linkLabel}
        <ChevronRight class="size-3.5" strokeWidth={2.2} />
      </a>
    </div>
  )
}

/** The soonest next fire among the schedules that are active now, as an ISO instant. */
function nextRunOf(rows: readonly AutomationRow[]): string | null {
  let soonest: number | null = null
  for (const row of rows) {
    if (row.state !== 'active' || !row.schedule.next_fire_at) continue
    const time = Date.parse(row.schedule.next_fire_at)
    if (Number.isNaN(time)) continue
    if (soonest === null || time < soonest) soonest = time
  }
  return soonest === null ? null : new Date(soonest).toISOString()
}

function formatWhen(iso: string, localeTag: string): string {
  return new Date(iso).toLocaleString(localeTag, { dateStyle: 'short', timeStyle: 'short' })
}

function roomsLabel(count: number, i18n: ReturnType<typeof useI18n>): string {
  if (count === 0) return i18n.tr('ikke i et rom ennå', 'not in a room yet')
  return i18n.tr(`i ${count} rom`, `in ${count} room${count === 1 ? '' : 's'}`)
}

/**
 * A refused automations read says what was refused, in the words the chat gives for the same
 * refusal. Any other failure keeps the plain sentence.
 */
function refusalText(error: unknown, tr: Tr): string | null {
  if (error instanceof ApiError && error.code !== null && isZdrCode(error.code)) {
    const shown = zdrRefusalPresentation(error.code, 'scheduled_step', tr)
    return shown.next === null ? shown.message : `${shown.message} ${shown.next}`
  }
  return null
}
