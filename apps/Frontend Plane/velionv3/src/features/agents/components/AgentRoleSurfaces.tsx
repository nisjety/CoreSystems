import {
  BarChart3,
  BriefcaseBusiness,
  CalendarClock,
  CheckCircle2,
  Database,
  Globe2,
  MessageSquareText,
  Rocket,
  Search,
  Settings2,
  ShieldCheck,
  ShoppingBag,
  Sparkles,
  Split,
  TicketCheck,
  Zap,
} from 'lucide-solid'
import { For, Match, Show, Switch } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import { cn } from '@/shared/lib/cn'
import {
  AgentFeatureBoard,
  CounterpartPanel,
  FeatureRow,
  RoleConversationPreview,
  StatusRow,
} from '@/features/agents/components/AgentsWorkspacePrimitives'
import type { AgentFeatureId } from '@/features/agents/lib/agent-roles'
import type {
  AgentBlueprint,
  RoleOperatingModel,
} from '@/features/agents/lib/velion-agent-page-types'
import {
  ecommerceAssistantModules,
  ecommerceCartRecoveryCards,
  ecommerceFinderChips,
  ecommerceInsightCards,
  ecommerceProductQuestions,
  ecommerceProductRecommendations,
  ecommerceQuickReplies,
  ecommerceSidekickTasks,
  ecommerceStoreActionPlan,
  ecommerceSupportRequests,
  salesBreezeResearch,
  salesCrmHandoffItems,
  salesEngagementModes,
  salesInsightMetrics,
  salesIntentSignals,
  salesLeadTags,
  salesMeetingSlots,
  salesObjectionRows,
  salesQualificationScores,
  salesVisitorIntelligenceSignals,
  serviceActionChecks,
  serviceChannelRollout,
  serviceGuidanceControls,
  serviceInsightQaRows,
  serviceNextImprovements,
  serviceResolutionQueue,
  serviceUnansweredTopics,
  serviceVerifiedQaRows,
} from '@/features/agents/lib/velion-agent-surface-data'
import {
  controlFocusClass,
  roleEyebrowClass,
  roleInsetClass,
  rolePanelClass,
} from '@/features/agents/lib/velion-agent-page-styles'

export function RoleCounterpartSurface(props: {
  feature: AgentFeatureId
  operatingModel: RoleOperatingModel
  role: AgentBlueprint
}) {
  return (
    <Switch fallback={<AgentFeatureBoard operatingModel={props.operatingModel} role={props.role} />}>
      <Match when={props.role.id === 'service'}>
        <ServiceResolutionSurface feature={props.feature} operatingModel={props.operatingModel} role={props.role} />
      </Match>
      <Match when={props.role.id === 'sales'}>
        <SalesSdrSurface feature={props.feature} operatingModel={props.operatingModel} role={props.role} />
      </Match>
      <Match when={props.role.id === 'ecommerce'}>
        <EcommerceCommerceSurface feature={props.feature} operatingModel={props.operatingModel} role={props.role} />
      </Match>
    </Switch>
  )
}

function ServiceResolutionSurface(props: {
  feature: AgentFeatureId
  operatingModel: RoleOperatingModel
  role: AgentBlueprint
}) {
  return (
    <Switch
      fallback={
        <section class="grid gap-3" aria-label="Service resolution workspace">
          <CounterpartPanel
            role={props.role}
            icon={TicketCheck}
            eyebrow="Zendesk / Ada style"
            title="Resolution queue"
            description="Route every support issue through answer, action, QA, and human handoff states."
          >
            <div class="grid gap-2 md:grid-cols-3">
              <For each={serviceResolutionQueue}>
                {(item) => (
                  <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                    <div class="flex items-center justify-between gap-2">
                      <h3 class="text-[12px] font-semibold text-[#202126] dark:text-white">{item.title}</h3>
                      <span class={cn('size-2 rounded-full', props.role.accentClass)} />
                    </div>
                    <p class="mt-2 text-[11px] font-semibold text-[#4A505A] dark:text-[#DCE2EC]">{item.status}</p>
                    <p class="mt-1 text-[11px] leading-4 text-[#747B87] dark:text-[#AEB4C0]">{item.detail}</p>
                  </div>
                )}
              </For>
            </div>
            <div class={cn('mt-3 rounded-[8px] border p-3', roleInsetClass(props.role))}>
              <div class="flex items-center justify-between gap-3">
                <div>
                  <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>Zendesk-style verification</p>
                  <p class="mt-1 text-[12px] font-semibold text-[#202126] dark:text-white">Only counted when the issue is actually resolved</p>
                </div>
                <span class={cn('rounded-full px-2 py-1 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>QA sampled</span>
              </div>
            </div>
            <div class="mt-3 grid gap-2 sm:grid-cols-2">
              <FeatureRow feature={props.operatingModel.knowledge[0]!} role={props.role} />
              <FeatureRow feature={props.operatingModel.actions[0]!} role={props.role} />
            </div>
          </CounterpartPanel>

          <CounterpartPanel
            role={props.role}
            icon={CheckCircle2}
            eyebrow="Forethought style"
            title="Quality supervisor"
            description="Every reply is scored for source coverage, policy fit, and escalation risk."
          >
            <For each={serviceVerifiedQaRows}>
              {(row) => <StatusRow label={row.label} value={row.value} role={props.role} />}
            </For>
            <div class="mt-3 space-y-2">
              <For each={props.operatingModel.guardrails}>
                {(guardrail) => <FeatureRow feature={guardrail} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>

          <CounterpartPanel
            role={props.role}
            icon={Globe2}
            eyebrow="Intercom Fin style"
            title="Omnichannel rollout"
            description="Launch in the channels with the right confidence and escalation settings."
          >
            <div class="grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.channels}>
                {(channel) => <FeatureRow feature={channel} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      }
    >
      <Match when={props.feature === 'service-knowledge'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(280px,0.8fr)]" aria-label="Service knowledge workspace">
          <CounterpartPanel
            role={props.role}
            icon={Database}
            eyebrow="Intercom Fin style"
            title="Answer coverage map"
            description="Organize trusted support content by what customers actually ask before the agent answers at scale."
          >
            <div class="grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.knowledge}>
                {(source) => <FeatureRow feature={source} role={props.role} />}
              </For>
            </div>
            <div class="mt-3 grid gap-1.5 sm:grid-cols-4" aria-label="Fin-style guidance controls">
              <For each={serviceGuidanceControls}>
                {(item) => (
                  <span class={cn('rounded-full border px-2 py-1 text-center text-[10px] font-semibold', roleInsetClass(props.role), roleEyebrowClass(props.role))}>
                    {item}
                  </span>
                )}
              </For>
            </div>
            <div class={cn('mt-3 rounded-[8px] border p-3', roleInsetClass(props.role))}>
              <div class="flex items-center justify-between text-[11px] font-semibold">
                <span class={roleEyebrowClass(props.role)}>Coverage health</span>
                <span class="text-[#202126] dark:text-white">Source audit</span>
              </div>
              <p class="mt-2 text-[11px] leading-4 text-[#68707B] dark:text-[#AEB4C0]">
                Live coverage metrics appear after approved sources and conversation events are connected.
              </p>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Search}
            eyebrow="Gap queue"
            title="Unanswered topics"
            description="Turn low-confidence answers into source requests instead of hidden failures."
          >
            <For each={serviceUnansweredTopics}>
              {(item, index) => (
                <div class={cn('mt-2 flex items-center justify-between rounded-[8px] border px-3 py-2 first:mt-0', roleInsetClass(props.role))}>
                  <span class="text-[12px] font-semibold text-[#202126] dark:text-white">{item}</span>
                  <span class={cn('rounded-full px-2 py-0.5 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>{index() === 0 ? 'Source' : 'Review'}</span>
                </div>
              )}
            </For>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'service-actions'}>
        <section class="grid gap-3 lg:grid-cols-3" aria-label="Service action permissions">
          <For each={props.operatingModel.actions}>
            {(action) => (
              <CounterpartPanel
                role={props.role}
                icon={action.icon}
                eyebrow="Controlled tool"
                title={action.title}
                description={action.description}
              >
                <div class="space-y-2">
                  <For each={serviceActionChecks}>
                    {(item) => (
                      <div class={cn('flex items-center gap-2 rounded-[8px] border px-3 py-2', roleInsetClass(props.role))}>
                        <CheckCircle2 class={cn('size-4', props.role.iconClass)} />
                        <span class="text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]">{item}</span>
                      </div>
                    )}
                  </For>
                </div>
              </CounterpartPanel>
            )}
          </For>
        </section>
      </Match>

      <Match when={props.feature === 'service-channels'}>
        <section class="grid gap-3" aria-label="Service channel rollout">
          <CounterpartPanel
            role={props.role}
            icon={Globe2}
            eyebrow="Intercom Fin style"
            title="Omnichannel rollout"
            description="Launch in the channels with the right confidence, tone, and escalation settings."
          >
            <div class="grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.channels}>
                {(channel) => <FeatureRow feature={channel} role={props.role} />}
              </For>
            </div>
            <div class="mt-3 flex flex-wrap gap-1.5" aria-label="Supported service channels">
              <For each={serviceChannelRollout}>
                {(channel) => (
                  <span class={cn('rounded-full px-2.5 py-1 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>
                    {channel}
                  </span>
                )}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'service-quality' || props.feature === 'service-insights'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" aria-label="Service quality and insights">
          <CounterpartPanel
            role={props.role}
            icon={CheckCircle2}
            eyebrow={props.feature === 'service-quality' ? 'Forethought style' : 'Insight loop'}
            title={props.feature === 'service-quality' ? 'Quality supervisor' : 'Conversation intelligence'}
            description={props.feature === 'service-quality' ? 'Every reply is scored for source coverage, policy fit, and escalation risk.' : 'Cluster unresolved questions, handoff reasons, and missing content into next best improvements.'}
          >
            <For each={props.feature === 'service-quality' ? serviceVerifiedQaRows : serviceInsightQaRows}>
              {(row) => <StatusRow label={row.label} value={row.value} role={props.role} />}
            </For>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Sparkles}
            eyebrow="Recommended work"
            title="Next improvements"
            description="Keep the agent improving without making support leads hunt through raw transcripts."
          >
            <div class="grid gap-2 sm:grid-cols-3">
              <For each={serviceNextImprovements}>
                {(item) => (
                  <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                    <CheckCircle2 class={cn('size-4', props.role.iconClass)} />
                    <p class="mt-2 text-[12px] font-semibold text-[#202126] dark:text-white">{item}</p>
                  </div>
                )}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      </Match>
    </Switch>
  )
}

function SalesSdrSurface(props: {
  feature: AgentFeatureId
  operatingModel: RoleOperatingModel
  role: AgentBlueprint
}) {
  return (
    <Switch
      fallback={
        <section class="grid gap-3" aria-label="Sales AI SDR workspace">
          <CounterpartPanel
            role={props.role}
            icon={Zap}
            eyebrow="Qualified Piper style"
            title="AI SDR journey"
            description="Detect intent, qualify in chat, book the right owner, and write the CRM handoff."
          >
            <div class="grid gap-3 lg:grid-cols-[190px_minmax(0,1fr)]">
              <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>Intent stream</p>
                <div class="mt-3 space-y-2">
                  <For each={salesIntentSignals}>
                    {(signal) => (
                      <div class="flex items-center gap-2 text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]">
                        <span class={cn('size-1.5 rounded-full', props.role.accentClass)} />
                        {signal}
                      </div>
                    )}
                  </For>
                </div>
                <div class="mt-3 grid grid-cols-2 gap-1.5" aria-label="Piper engagement modes">
                  <For each={salesEngagementModes}>
                    {(mode) => (
                      <span class={cn('rounded-full px-2 py-1 text-center text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>
                        {mode}
                      </span>
                    )}
                  </For>
                </div>
              </div>
              <div class={cn('rounded-[8px] border p-3 shadow-sm', rolePanelClass(props.role))}>
                <div class="flex items-start justify-between gap-3">
                  <div>
                    <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>Qualification</p>
                    <h3 class="mt-1 text-[18px] font-semibold text-[#202126] dark:text-white">Sales-ready rules</h3>
                  </div>
                  <span class={cn('rounded-full px-2 py-1 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>Route gated</span>
                </div>
                <div class="mt-3 grid gap-2 sm:grid-cols-3">
                  <For each={salesLeadTags}>
                    {(item) => (
                      <span class="rounded-full border border-[#E3E4E8] px-2.5 py-1 text-[11px] font-medium text-[#4B515C] dark:border-[#2B2D33] dark:text-[#D8DEE8]">
                        {item}
                      </span>
                    )}
                  </For>
                </div>
                <p class="mt-3 text-[12px] leading-5 text-[#68707B] dark:text-[#AEB4C0]">
                  The agent asks two discovery questions before showing calendar slots, then sends the CRM handoff with page history and objections.
                </p>
              </div>
            </div>
          </CounterpartPanel>

          <CounterpartPanel
            role={props.role}
            icon={CalendarClock}
            eyebrow="HubSpot / Salesforce style"
            title="Meeting + CRM"
            description="Booking and handoff are treated as controlled sales actions."
            preview
          >
            {/* Phase 4 honesty sweep: preview-only slot picker — no booking backend. */}
            <div class="grid grid-cols-2 gap-2">
              <For each={salesMeetingSlots}>
                {(slot, index) => (
                  <Button
                    variant={index() === 1 ? 'primary' : 'secondary'}
                    size="md"
                    shape="pill"
                    disabled
                    aria-pressed={index() === 1}
                    class={cn('min-h-8 px-3 text-[12px] font-semibold', controlFocusClass)}
                  >
                    {slot}
                  </Button>
                )}
              </For>
            </div>
            <div class="mt-3 space-y-2">
              <For each={props.operatingModel.actions}>
                {(action) => <FeatureRow feature={action} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      }
    >
      <Match when={props.feature === 'sales-qualification' || props.feature === 'sales-objections'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label="Sales qualification workspace">
          <CounterpartPanel
            role={props.role}
            icon={props.feature === 'sales-qualification' ? BriefcaseBusiness : ShieldCheck}
            eyebrow={props.feature === 'sales-qualification' ? 'Qualified Piper style' : 'Approved playbook'}
            title={props.feature === 'sales-qualification' ? 'Qualification scorecard' : 'Objection handling'}
            description={props.feature === 'sales-qualification' ? 'Collect fit, urgency, and buying context before a meeting is offered.' : 'Answer pricing, security, and migration concerns from approved sales guidance.'}
          >
            <div class="grid gap-2 sm:grid-cols-3">
              <For each={salesQualificationScores}>
                {(item) => (
                  <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                    <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>{item.label}</p>
                    <p class="mt-2 text-[22px] font-semibold text-[#202126] dark:text-white">{item.value}</p>
                  </div>
                )}
              </For>
            </div>
            <div class="mt-3 space-y-2">
              <For each={props.feature === 'sales-qualification' ? props.operatingModel.actions : salesObjectionRows}>
                {(item) => <FeatureRow feature={item} role={props.role} />}
              </For>
            </div>
            <Show when={props.feature === 'sales-qualification'}>
              <div class="mt-3 grid gap-1.5 sm:grid-cols-4" aria-label="Visitor intelligence signals">
                <For each={salesVisitorIntelligenceSignals}>
                  {(item) => (
                    <span class={cn('rounded-full border px-2 py-1 text-center text-[10px] font-semibold', roleInsetClass(props.role), roleEyebrowClass(props.role))}>
                      {item}
                    </span>
                  )}
                </For>
              </div>
            </Show>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={MessageSquareText}
            eyebrow="Conversation blueprint"
            title="Buyer context"
            description="The agent keeps the conversation focused and compact."
          >
            <RoleConversationPreview operatingModel={props.operatingModel} role={props.role} />
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'sales-booking' || props.feature === 'sales-crm'}>
        <section class="grid gap-3 lg:grid-cols-2" aria-label="Sales booking and CRM workspace">
          <CounterpartPanel
            role={props.role}
            icon={props.feature === 'sales-booking' ? CalendarClock : Database}
            eyebrow="HubSpot / Salesforce style"
            title={props.feature === 'sales-booking' ? 'Meeting router' : 'CRM handoff builder'}
            description={props.feature === 'sales-booking' ? 'Show the right slots only after the lead reaches a sales-ready threshold.' : 'Create structured handoffs with source pages, objections, qualification, and next steps.'}
            preview={props.feature === 'sales-booking'}
          >
            <Show
              when={props.feature === 'sales-booking'}
              fallback={
                <div class="space-y-2">
                  <For each={salesCrmHandoffItems}>
                    {(item) => (
                      <div class={cn('rounded-[8px] border px-3 py-2 text-[12px] font-medium text-[#4B515C] dark:text-[#D8DEE8]', roleInsetClass(props.role))}>{item}</div>
                    )}
                  </For>
                  <div class="grid grid-cols-2 gap-1.5 pt-1" aria-label="Breeze-style research context">
                    <For each={salesBreezeResearch}>
                      {(item) => (
                        <span class={cn('rounded-full px-2 py-1 text-center text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>
                          {item}
                        </span>
                      )}
                    </For>
                  </div>
                </div>
              }
            >
              {/* Phase 4 honesty sweep: preview-only slot picker — no booking backend. */}
              <div class="grid grid-cols-2 gap-2">
                <For each={salesMeetingSlots}>
                  {(slot, index) => (
                    <Button
                      variant={index() === 1 ? 'primary' : 'secondary'}
                      shape="pill"
                      size="md"
                      disabled
                      aria-pressed={index() === 1}
                      class={cn('min-h-8 px-3 text-[12px] font-semibold', controlFocusClass)}
                    >
                      {slot}
                    </Button>
                  )}
                </For>
                <div class={cn('col-span-2 rounded-[8px] border px-3 py-2 text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]', roleInsetClass(props.role))}>
                  Uses the record owner calendar first; sends a meeting link if no live slot is available.
                </div>
              </div>
            </Show>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Split}
            eyebrow="Routing rules"
            title="Owner assignment"
            description="Route by segment, region, account owner, and urgency."
          >
            <div class="space-y-2">
              <For each={props.operatingModel.channels}>
                {(channel) => <FeatureRow feature={channel} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'sales-insights'}>
        <section class="grid gap-3 md:grid-cols-2 xl:grid-cols-4" aria-label="Sales insights workspace">
          <For each={salesInsightMetrics}>
            {(metric) => (
              <CounterpartPanel role={props.role} icon={BarChart3} eyebrow="Pipeline insight" title={metric.label} description={metric.detail}>
                <p class="text-[34px] font-semibold leading-none text-[#202126] dark:text-white">{metric.value}</p>
                <StatusRow label="Signal" value="Runtime gated" role={props.role} />
              </CounterpartPanel>
            )}
          </For>
        </section>
      </Match>
    </Switch>
  )
}

function EcommerceCommerceSurface(props: {
  feature: AgentFeatureId
  operatingModel: RoleOperatingModel
  role: AgentBlueprint
}) {
  return (
    <Switch
      fallback={
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_330px]" aria-label="Ecommerce shopping assistant workspace">
          <CounterpartPanel
            role={props.role}
            icon={ShoppingBag}
            eyebrow="Gorgias style"
            title="Shopping Assistant + Support Agent"
            description="Pre-purchase product discovery and post-purchase order work live in one commerce agent."
          >
            <div class="grid gap-2 md:grid-cols-2">
              <For each={ecommerceAssistantModules}>
                {(skill) => (
                  <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                    <div class="flex items-center gap-2">
                      <span class={cn('grid size-8 place-items-center rounded-[7px] text-white', props.role.accentClass)}>
                        <skill.icon class="size-4" />
                      </span>
                      <h3 class="text-[13px] font-semibold text-[#202126] dark:text-white">{skill.title}</h3>
                    </div>
                    <p class="mt-2 text-[11px] leading-4 text-[#68707B] dark:text-[#AEB4C0]">{skill.detail}</p>
                  </div>
                )}
              </For>
            </div>
            <div class="mt-3 grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.actions}>
                {(action) => <FeatureRow feature={action} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={MessageSquareText}
            eyebrow="AI FAQ"
            title="Product-page questions"
            description="Proactively answer sizing, material, shipping, and return questions where shoppers hesitate."
          >
            <For each={ecommerceProductQuestions}>
              {(question) => (
                <div class={cn('mt-2 rounded-[8px] border px-3 py-2 text-[12px] font-medium text-[#4B515C] first:mt-0 dark:text-[#D8DEE8]', roleInsetClass(props.role))}>
                  {question}
                </div>
              )}
            </For>
            <div class="mt-3 flex flex-wrap gap-1.5" aria-label="Proactive quick replies">
              <For each={ecommerceQuickReplies}>
                {(reply) => (
                  <span class={cn('rounded-full px-2.5 py-1 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>
                    {reply}
                  </span>
                )}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      }
    >
      <Match when={props.feature === 'commerce-support'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label="Ecommerce support and orders workspace">
          <CounterpartPanel
            role={props.role}
            icon={TicketCheck}
            eyebrow="Gorgias Support Agent style"
            title="Orders, returns, and subscriptions"
            description="Resolve post-purchase work with live order state and scoped support actions."
          >
            <div class="grid gap-2 sm:grid-cols-3">
              <For each={ecommerceSupportRequests}>
                {(item) => (
                  <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                    <p class="text-[12px] font-semibold text-[#202126] dark:text-white">{item.title}</p>
                    <p class="mt-2 text-[11px] text-[#68707B] dark:text-[#AEB4C0]">{item.detail}</p>
                  </div>
                )}
              </For>
            </div>
            <div class="mt-3 grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.actions.slice(1)}>
                {(action) => <FeatureRow feature={action} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Search}
            eyebrow="Store state requirements"
            title="Order lookup"
            description="The agent checks status before promising refunds, delivery dates, or subscription changes."
          >
            <StatusRow label="Order data" value="Connect" role={props.role} />
            <StatusRow label="Return policy" value="Review" role={props.role} />
            <StatusRow label="Subscription action" value="Scope" role={props.role} />
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-product-finder'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" aria-label="Ecommerce product finder workspace">
          <CounterpartPanel
            role={props.role}
            icon={Search}
            eyebrow="Rep AI Guided Search style"
            title="Guided product finder"
            description="Ask one useful question, filter with catalog attributes, and explain the best matches."
          >
            <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
              <p class="text-[12px] font-semibold text-[#202126] dark:text-white">What matters most for winter runs?</p>
              <div class="mt-2 flex flex-wrap gap-1.5">
                <For each={ecommerceFinderChips}>
                  {(chip) => (
                    <span class="rounded-full border border-[#DDE0E5] bg-white px-2.5 py-1 text-[10px] font-semibold text-[#4B515C] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#DCE2EC]">
                      {chip}
                    </span>
                  )}
                </For>
              </div>
            </div>
            <div class="mt-3 grid gap-2 md:grid-cols-3">
              <For each={ecommerceProductRecommendations}>
                {(product) => (
                  <div class={cn('rounded-[8px] border p-2 shadow-sm', rolePanelClass(props.role))}>
                    <div class="grid aspect-[4/3] place-items-center rounded-[7px] bg-[linear-gradient(135deg,#EAF9DF,#D7E9C8)] dark:bg-[linear-gradient(135deg,#1F3326,#253B2D)]">
                      <span class="h-4 w-14 rounded-full bg-white/80 shadow-sm dark:bg-white/20" />
                    </div>
                    <h3 class="mt-2 text-[11px] font-semibold text-[#202126] dark:text-white">{product.name}</h3>
                    <p class="mt-0.5 text-[10px] text-[#68707B] dark:text-[#AEB4C0]">{product.fit}</p>
                    <p class="mt-1 text-[12px] font-semibold text-[#202126] dark:text-white">{product.price}</p>
                  </div>
                )}
              </For>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={BarChart3}
            eyebrow="Shopper intelligence"
            title="Intent profile"
            description="Use browsing behavior, budget, and cart context to keep recommendations relevant."
          >
            <FeatureRow feature={props.operatingModel.knowledge[1]!} role={props.role} />
            <div class="mt-2">
              <FeatureRow feature={props.operatingModel.guardrails[0]!} role={props.role} />
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-cart'}>
        <section class="grid gap-3 lg:grid-cols-3" aria-label="Ecommerce cart recovery workspace">
          <For each={ecommerceCartRecoveryCards}>
            {(item) => (
              <CounterpartPanel role={props.role} icon={Rocket} eyebrow="Cart recovery" title={item.title} description={item.detail}>
                <span class={cn('inline-flex rounded-full px-3 py-1 text-[11px] font-semibold', props.role.ringClass, props.role.iconClass)}>{item.value}</span>
              </CounterpartPanel>
            )}
          </For>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-brand'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]" aria-label="Ecommerce brand voice workspace">
          <CounterpartPanel
            role={props.role}
            icon={MessageSquareText}
            eyebrow="Siena AI Personas style"
            title="AI Persona + social"
            description="Keep autonomous commerce replies on-brand across chat, email, WhatsApp, Instagram, and social comments."
          >
            <StatusRow label="Brand voice match" value="Review" role={props.role} />
            <StatusRow label="Social response quality" value="Policy" role={props.role} />
            <StatusRow label="Voice of customer coverage" value="Waiting" role={props.role} />
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Globe2}
            eyebrow="Channel rules"
            title="Autonomous CX channels"
            description="Different channels can have different tone, length, escalation, and response permissions."
          >
            <div class="grid gap-2 sm:grid-cols-3">
              <For each={props.operatingModel.channels}>
                {(channel) => <FeatureRow feature={channel} role={props.role} />}
              </For>
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-store'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label="Ecommerce store command center">
          <CounterpartPanel
            role={props.role}
            icon={Settings2}
            eyebrow="Shopify Sidekick style"
            title="Store command center"
            description="Ask in plain language, review generated store actions, then apply with approval."
            preview
          >
            <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
              <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>Command</p>
              <div class="mt-2 rounded-[8px] border border-[#E1E3E8] bg-white px-3 py-2 text-[12px] text-[#343A43] shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#E7EBF1]">
                Create a winter-running product finder and tag low-stock items.
              </div>
              <div class="mt-3 space-y-2">
                <For each={ecommerceStoreActionPlan}>
                  {(item) => (
                    <div class="flex items-center gap-2 text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]">
                      <CheckCircle2 class={cn('size-3.5', props.role.iconClass)} />
                      {item}
                    </div>
                  )}
                </For>
                <div class="grid grid-cols-3 gap-1.5 pt-1" aria-label="Sidekick task types">
                  <For each={ecommerceSidekickTasks}>
                    {(item) => (
                      <span class="rounded-full border border-[#DDE0E5] bg-white px-2 py-1 text-center text-[9px] font-semibold text-[#4B515C] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#DCE2EC]">
                        {item}
                      </span>
                    )}
                  </For>
                </div>
              </div>
              {/* Phase 4 honesty sweep: store actions have no apply backend yet. */}
              <div class="mt-3 flex gap-2">
                <Button variant="primary" size="md" shape="pill" disabled class={cn('min-h-8 flex-1 px-3 text-[12px] font-semibold', controlFocusClass)}>Review</Button>
                <Button variant="secondary" size="md" shape="pill" disabled class={cn('min-h-8 flex-1 px-3 text-[12px] font-semibold', controlFocusClass)}>Apply</Button>
              </div>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={CheckCircle2}
            eyebrow="Approval rail"
            title="Action safety"
            description="Store changes stay reviewable and scoped before anything writes to Shopify."
          >
            <FeatureRow feature={props.operatingModel.guardrails[0]!} role={props.role} />
            <div class="mt-2">
              <FeatureRow feature={props.operatingModel.guardrails[1]!} role={props.role} />
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-insights'}>
        <section class="grid gap-3 lg:grid-cols-3" aria-label="Ecommerce shopper insights workspace">
          <For each={ecommerceInsightCards}>
            {(metric) => (
              <CounterpartPanel role={props.role} icon={BarChart3} eyebrow="Shopper insight" title={metric.title} description={metric.detail}>
                <p class="text-[32px] font-semibold leading-none text-[#202126] dark:text-white">{metric.value}</p>
              </CounterpartPanel>
            )}
          </For>
        </section>
      </Match>
    </Switch>
  )
}
