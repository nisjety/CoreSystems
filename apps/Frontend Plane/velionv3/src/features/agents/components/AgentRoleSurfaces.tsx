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
import { useI18n } from '@/shared/i18n'

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
  const i18n = useI18n()
  return (
    <Switch
      fallback={
        <section class="grid gap-3" aria-label={i18n.tr('Arbeidsområde for saksløsning', 'Service resolution workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={TicketCheck}
            eyebrow={i18n.tr('Zendesk/Ada-stil', 'Zendesk / Ada style')}
            title={i18n.tr('Løsningskø', 'Resolution queue')}
            description={i18n.tr('Rut hver supporthenvendelse gjennom svar, handling, QA og overlevering til menneske.', 'Route every support issue through answer, action, QA, and human handoff states.')}
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
                  <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>{i18n.tr('Zendesk-stil verifisering', 'Zendesk-style verification')}</p>
                  <p class="mt-1 text-[12px] font-semibold text-[#202126] dark:text-white">{i18n.tr('Telles kun når saken faktisk er løst', 'Only counted when the issue is actually resolved')}</p>
                </div>
                <span class={cn('rounded-full px-2 py-1 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>{i18n.tr('QA-utvalg', 'QA sampled')}</span>
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
            eyebrow={i18n.tr('Forethought-stil', 'Forethought style')}
            title={i18n.tr('Kvalitetstilsyn', 'Quality supervisor')}
            description={i18n.tr('Hvert svar poengsettes for kildedekning, policy-samsvar og eskaleringsrisiko.', 'Every reply is scored for source coverage, policy fit, and escalation risk.')}
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
            eyebrow={i18n.tr('Intercom Fin-stil', 'Intercom Fin style')}
            title={i18n.tr('Utrulling på alle kanaler', 'Omnichannel rollout')}
            description={i18n.tr('Lanser i kanalene med riktig konfidens- og eskaleringsinnstillinger.', 'Launch in the channels with the right confidence and escalation settings.')}
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
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(280px,0.8fr)]" aria-label={i18n.tr('Arbeidsområde for kunnskap', 'Service knowledge workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={Database}
            eyebrow={i18n.tr('Intercom Fin-stil', 'Intercom Fin style')}
            title={i18n.tr('Svardekningskart', 'Answer coverage map')}
            description={i18n.tr('Organiser pålitelig supportinnhold etter hva kundene faktisk spør om, før agenten svarer i stor skala.', 'Organize trusted support content by what customers actually ask before the agent answers at scale.')}
          >
            <div class="grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.knowledge}>
                {(source) => <FeatureRow feature={source} role={props.role} />}
              </For>
            </div>
            <div class="mt-3 grid gap-1.5 sm:grid-cols-4" aria-label={i18n.tr('Fin-stil veiledningskontroller', 'Fin-style guidance controls')}>
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
                <span class={roleEyebrowClass(props.role)}>{i18n.tr('Dekningshelse', 'Coverage health')}</span>
                <span class="text-[#202126] dark:text-white">{i18n.tr('Kilderevisjon', 'Source audit')}</span>
              </div>
              <p class="mt-2 text-[11px] leading-4 text-[#68707B] dark:text-[#AEB4C0]">
                {i18n.tr('Live dekningsmetrikker vises etter at godkjente kilder og samtalehendelser er koblet til.', 'Live coverage metrics appear after approved sources and conversation events are connected.')}
              </p>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Search}
            eyebrow={i18n.tr('Kø for hull', 'Gap queue')}
            title={i18n.tr('Ubesvarte emner', 'Unanswered topics')}
            description={i18n.tr('Gjør svar med lav konfidens om til kildeforespørsler i stedet for skjulte feil.', 'Turn low-confidence answers into source requests instead of hidden failures.')}
          >
            <For each={serviceUnansweredTopics}>
              {(item, index) => (
                <div class={cn('mt-2 flex items-center justify-between rounded-[8px] border px-3 py-2 first:mt-0', roleInsetClass(props.role))}>
                  <span class="text-[12px] font-semibold text-[#202126] dark:text-white">{item}</span>
                  <span class={cn('rounded-full px-2 py-0.5 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>{index() === 0 ? i18n.tr('Kilde', 'Source') : i18n.tr('Vurder', 'Review')}</span>
                </div>
              )}
            </For>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'service-actions'}>
        <section class="grid gap-3 lg:grid-cols-3" aria-label={i18n.tr('Tillatelser for servicehandlinger', 'Service action permissions')}>
          <For each={props.operatingModel.actions}>
            {(action) => (
              <CounterpartPanel
                role={props.role}
                icon={action.icon}
                eyebrow={i18n.tr('Kontrollert verktøy', 'Controlled tool')}
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
        <section class="grid gap-3" aria-label={i18n.tr('Utrulling av servicekanaler', 'Service channel rollout')}>
          <CounterpartPanel
            role={props.role}
            icon={Globe2}
            eyebrow={i18n.tr('Intercom Fin-stil', 'Intercom Fin style')}
            title={i18n.tr('Utrulling på alle kanaler', 'Omnichannel rollout')}
            description={i18n.tr('Lanser i kanalene med riktig konfidens, tone og eskaleringsinnstillinger.', 'Launch in the channels with the right confidence, tone, and escalation settings.')}
          >
            <div class="grid gap-2 md:grid-cols-3">
              <For each={props.operatingModel.channels}>
                {(channel) => <FeatureRow feature={channel} role={props.role} />}
              </For>
            </div>
            <div class="mt-3 flex flex-wrap gap-1.5" aria-label={i18n.tr('Støttede servicekanaler', 'Supported service channels')}>
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
        <section class="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" aria-label={i18n.tr('Servicekvalitet og innsikt', 'Service quality and insights')}>
          <CounterpartPanel
            role={props.role}
            icon={CheckCircle2}
            eyebrow={props.feature === 'service-quality' ? i18n.tr('Forethought-stil', 'Forethought style') : i18n.tr('Innsiktsløkke', 'Insight loop')}
            title={props.feature === 'service-quality' ? i18n.tr('Kvalitetstilsyn', 'Quality supervisor') : i18n.tr('Samtaleintelligens', 'Conversation intelligence')}
            description={props.feature === 'service-quality' ? i18n.tr('Hvert svar poengsettes for kildedekning, policy-samsvar og eskaleringsrisiko.', 'Every reply is scored for source coverage, policy fit, and escalation risk.') : i18n.tr('Grupperer uløste spørsmål, overleveringsårsaker og manglende innhold til de beste neste forbedringene.', 'Cluster unresolved questions, handoff reasons, and missing content into next best improvements.')}
          >
            <For each={props.feature === 'service-quality' ? serviceVerifiedQaRows : serviceInsightQaRows}>
              {(row) => <StatusRow label={row.label} value={row.value} role={props.role} />}
            </For>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Sparkles}
            eyebrow={i18n.tr('Anbefalt arbeid', 'Recommended work')}
            title={i18n.tr('Neste forbedringer', 'Next improvements')}
            description={i18n.tr('Hold agenten i utvikling uten at support-ledere må lete gjennom rå transkripsjoner.', 'Keep the agent improving without making support leads hunt through raw transcripts.')}
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
  const i18n = useI18n()
  return (
    <Switch
      fallback={
        <section class="grid gap-3" aria-label={i18n.tr('Arbeidsområde for AI SDR-salg', 'Sales AI SDR workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={Zap}
            eyebrow={i18n.tr('Qualified Piper-stil', 'Qualified Piper style')}
            title={i18n.tr('AI SDR-reise', 'AI SDR journey')}
            description={i18n.tr('Oppdag hensikt, kvalifiser i chat, book riktig eier, og skriv CRM-overleveringen.', 'Detect intent, qualify in chat, book the right owner, and write the CRM handoff.')}
          >
            <div class="grid gap-3 lg:grid-cols-[190px_minmax(0,1fr)]">
              <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
                <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>{i18n.tr('Hensikts-strøm', 'Intent stream')}</p>
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
                <div class="mt-3 grid grid-cols-2 gap-1.5" aria-label={i18n.tr('Piper engasjementsmodus', 'Piper engagement modes')}>
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
                    <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>{i18n.tr('Kvalifisering', 'Qualification')}</p>
                    <h3 class="mt-1 text-[18px] font-semibold text-[#202126] dark:text-white">{i18n.tr('Regler for salgsklare leads', 'Sales-ready rules')}</h3>
                  </div>
                  <span class={cn('rounded-full px-2 py-1 text-[10px] font-semibold', props.role.ringClass, props.role.iconClass)}>{i18n.tr('Rutet med sperre', 'Route gated')}</span>
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
                  {i18n.tr('Agenten stiller to kartleggingsspørsmål før den viser ledige tider, og sender deretter CRM-overleveringen med sidehistorikk og innvendinger.', 'The agent asks two discovery questions before showing calendar slots, then sends the CRM handoff with page history and objections.')}
                </p>
              </div>
            </div>
          </CounterpartPanel>

          <CounterpartPanel
            role={props.role}
            icon={CalendarClock}
            eyebrow={i18n.tr('HubSpot/Salesforce-stil', 'HubSpot / Salesforce style')}
            title={i18n.tr('Møte + CRM', 'Meeting + CRM')}
            description={i18n.tr('Booking og overlevering behandles som kontrollerte salgshandlinger.', 'Booking and handoff are treated as controlled sales actions.')}
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
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label={i18n.tr('Arbeidsområde for salgskvalifisering', 'Sales qualification workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={props.feature === 'sales-qualification' ? BriefcaseBusiness : ShieldCheck}
            eyebrow={props.feature === 'sales-qualification' ? i18n.tr('Qualified Piper-stil', 'Qualified Piper style') : i18n.tr('Godkjent spillbok', 'Approved playbook')}
            title={props.feature === 'sales-qualification' ? i18n.tr('Kvalifiseringskort', 'Qualification scorecard') : i18n.tr('Håndtering av innvendinger', 'Objection handling')}
            description={props.feature === 'sales-qualification' ? i18n.tr('Samle fit, hastegrad og kjøpskontekst før et møte tilbys.', 'Collect fit, urgency, and buying context before a meeting is offered.') : i18n.tr('Svar på pris-, sikkerhets- og migreringsbekymringer fra godkjent salgsveiledning.', 'Answer pricing, security, and migration concerns from approved sales guidance.')}
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
              <div class="mt-3 grid gap-1.5 sm:grid-cols-4" aria-label={i18n.tr('Signaler om besøkende', 'Visitor intelligence signals')}>
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
            eyebrow={i18n.tr('Samtale-blueprint', 'Conversation blueprint')}
            title={i18n.tr('Kjøperkontekst', 'Buyer context')}
            description={i18n.tr('Agenten holder samtalen fokusert og kompakt.', 'The agent keeps the conversation focused and compact.')}
          >
            <RoleConversationPreview operatingModel={props.operatingModel} role={props.role} />
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'sales-booking' || props.feature === 'sales-crm'}>
        <section class="grid gap-3 lg:grid-cols-2" aria-label={i18n.tr('Arbeidsområde for booking og CRM', 'Sales booking and CRM workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={props.feature === 'sales-booking' ? CalendarClock : Database}
            eyebrow={i18n.tr('HubSpot/Salesforce-stil', 'HubSpot / Salesforce style')}
            title={props.feature === 'sales-booking' ? i18n.tr('Møteruter', 'Meeting router') : i18n.tr('CRM-overleveringsbygger', 'CRM handoff builder')}
            description={props.feature === 'sales-booking' ? i18n.tr('Vis kun ledige tider etter at leaden når salgsklar terskel.', 'Show the right slots only after the lead reaches a sales-ready threshold.') : i18n.tr('Lag strukturerte overleveringer med kildesider, innvendinger, kvalifisering og neste steg.', 'Create structured handoffs with source pages, objections, qualification, and next steps.')}
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
                  <div class="grid grid-cols-2 gap-1.5 pt-1" aria-label={i18n.tr('Breeze-stil researchkontekst', 'Breeze-style research context')}>
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
                  {i18n.tr('Bruker eierens kalender først; sender en møtelenke hvis ingen ledig tid finnes.', 'Uses the record owner calendar first; sends a meeting link if no live slot is available.')}
                </div>
              </div>
            </Show>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Split}
            eyebrow={i18n.tr('Rutingregler', 'Routing rules')}
            title={i18n.tr('Eiertildeling', 'Owner assignment')}
            description={i18n.tr('Rut etter segment, region, kontoeier og hastegrad.', 'Route by segment, region, account owner, and urgency.')}
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
        <section class="grid gap-3 md:grid-cols-2 xl:grid-cols-4" aria-label={i18n.tr('Arbeidsområde for salgsinnsikt', 'Sales insights workspace')}>
          <For each={salesInsightMetrics}>
            {(metric) => (
              <CounterpartPanel role={props.role} icon={BarChart3} eyebrow={i18n.tr('Pipeline-innsikt', 'Pipeline insight')} title={metric.label} description={metric.detail}>
                <p class="text-[34px] font-semibold leading-none text-[#202126] dark:text-white">{metric.value}</p>
                <StatusRow label={i18n.tr('Signal', 'Signal')} value={i18n.tr('Sperret i kjøretid', 'Runtime gated')} role={props.role} />
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
  const i18n = useI18n()
  return (
    <Switch
      fallback={
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_330px]" aria-label={i18n.tr('Arbeidsområde for handleassistent', 'Ecommerce shopping assistant workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={ShoppingBag}
            eyebrow={i18n.tr('Gorgias-stil', 'Gorgias style')}
            title={i18n.tr('Handleassistent + supportagent', 'Shopping Assistant + Support Agent')}
            description={i18n.tr('Produktoppdagelse før kjøp og ordrearbeid etter kjøp lever i én handelsagent.', 'Pre-purchase product discovery and post-purchase order work live in one commerce agent.')}
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
            eyebrow={i18n.tr('AI-FAQ', 'AI FAQ')}
            title={i18n.tr('Spørsmål på produktsiden', 'Product-page questions')}
            description={i18n.tr('Svar proaktivt på spørsmål om størrelse, materiale, frakt og retur der kundene nøler.', 'Proactively answer sizing, material, shipping, and return questions where shoppers hesitate.')}
          >
            <For each={ecommerceProductQuestions}>
              {(question) => (
                <div class={cn('mt-2 rounded-[8px] border px-3 py-2 text-[12px] font-medium text-[#4B515C] first:mt-0 dark:text-[#D8DEE8]', roleInsetClass(props.role))}>
                  {question}
                </div>
              )}
            </For>
            <div class="mt-3 flex flex-wrap gap-1.5" aria-label={i18n.tr('Proaktive hurtigsvar', 'Proactive quick replies')}>
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
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label={i18n.tr('Arbeidsområde for support og ordrer', 'Ecommerce support and orders workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={TicketCheck}
            eyebrow={i18n.tr('Gorgias Support Agent-stil', 'Gorgias Support Agent style')}
            title={i18n.tr('Ordrer, returer og abonnementer', 'Orders, returns, and subscriptions')}
            description={i18n.tr('Løs arbeid etter kjøp med sanntids ordrestatus og avgrensede supporthandlinger.', 'Resolve post-purchase work with live order state and scoped support actions.')}
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
            eyebrow={i18n.tr('Krav til butikkstatus', 'Store state requirements')}
            title={i18n.tr('Ordreoppslag', 'Order lookup')}
            description={i18n.tr('Agenten sjekker status før den lover refusjoner, leveringsdatoer eller abonnementsendringer.', 'The agent checks status before promising refunds, delivery dates, or subscription changes.')}
          >
            <StatusRow label={i18n.tr('Ordredata', 'Order data')} value={i18n.tr('Koble til', 'Connect')} role={props.role} />
            <StatusRow label={i18n.tr('Returpolicy', 'Return policy')} value={i18n.tr('Vurder', 'Review')} role={props.role} />
            <StatusRow label={i18n.tr('Abonnementshandling', 'Subscription action')} value={i18n.tr('Avgrens', 'Scope')} role={props.role} />
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-product-finder'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" aria-label={i18n.tr('Arbeidsområde for produktfinner', 'Ecommerce product finder workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={Search}
            eyebrow={i18n.tr('Rep AI Guided Search-stil', 'Rep AI Guided Search style')}
            title={i18n.tr('Veiledet produktfinner', 'Guided product finder')}
            description={i18n.tr('Still ett nyttig spørsmål, filtrer med katalogattributter, og forklar de beste treffene.', 'Ask one useful question, filter with catalog attributes, and explain the best matches.')}
          >
            <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
              <p class="text-[12px] font-semibold text-[#202126] dark:text-white">{i18n.tr('Hva betyr mest for vinterløping?', 'What matters most for winter runs?')}</p>
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
            eyebrow={i18n.tr('Kjøperintelligens', 'Shopper intelligence')}
            title={i18n.tr('Hensiktsprofil', 'Intent profile')}
            description={i18n.tr('Bruk nettleseratferd, budsjett og handlekurvkontekst for å holde anbefalingene relevante.', 'Use browsing behavior, budget, and cart context to keep recommendations relevant.')}
          >
            <FeatureRow feature={props.operatingModel.knowledge[1]!} role={props.role} />
            <div class="mt-2">
              <FeatureRow feature={props.operatingModel.guardrails[0]!} role={props.role} />
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-cart'}>
        <section class="grid gap-3 lg:grid-cols-3" aria-label={i18n.tr('Arbeidsområde for handlekurvgjenoppretting', 'Ecommerce cart recovery workspace')}>
          <For each={ecommerceCartRecoveryCards}>
            {(item) => (
              <CounterpartPanel role={props.role} icon={Rocket} eyebrow={i18n.tr('Gjenoppretting av handlekurv', 'Cart recovery')} title={item.title} description={item.detail}>
                <span class={cn('inline-flex rounded-full px-3 py-1 text-[11px] font-semibold', props.role.ringClass, props.role.iconClass)}>{item.value}</span>
              </CounterpartPanel>
            )}
          </For>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-brand'}>
        <section class="grid gap-3 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]" aria-label={i18n.tr('Arbeidsområde for merkevarestemme', 'Ecommerce brand voice workspace')}>
          <CounterpartPanel
            role={props.role}
            icon={MessageSquareText}
            eyebrow={i18n.tr('Siena AI Personas-stil', 'Siena AI Personas style')}
            title={i18n.tr('AI-persona + sosiale medier', 'AI Persona + social')}
            description={i18n.tr('Hold selvstendige handelssvar merkevaretro på tvers av chat, e-post, WhatsApp, Instagram og sosiale kommentarer.', 'Keep autonomous commerce replies on-brand across chat, email, WhatsApp, Instagram, and social comments.')}
          >
            <StatusRow label={i18n.tr('Samsvar med merkevarestemme', 'Brand voice match')} value={i18n.tr('Vurder', 'Review')} role={props.role} />
            <StatusRow label={i18n.tr('Kvalitet på sosiale svar', 'Social response quality')} value={i18n.tr('Policy', 'Policy')} role={props.role} />
            <StatusRow label={i18n.tr('Dekning av kundens stemme', 'Voice of customer coverage')} value={i18n.tr('Venter', 'Waiting')} role={props.role} />
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={Globe2}
            eyebrow={i18n.tr('Kanalregler', 'Channel rules')}
            title={i18n.tr('Selvstendige CX-kanaler', 'Autonomous CX channels')}
            description={i18n.tr('Ulike kanaler kan ha ulik tone, lengde, eskalering og svartillatelser.', 'Different channels can have different tone, length, escalation, and response permissions.')}
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
        <section class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label={i18n.tr('Kommandosentral for butikk', 'Ecommerce store command center')}>
          <CounterpartPanel
            role={props.role}
            icon={Settings2}
            eyebrow={i18n.tr('Shopify Sidekick-stil', 'Shopify Sidekick style')}
            title={i18n.tr('Kommandosentral for butikk', 'Store command center')}
            description={i18n.tr('Spør i vanlig språk, se over genererte butikkhandlinger, og bruk dem etter godkjenning.', 'Ask in plain language, review generated store actions, then apply with approval.')}
            preview
          >
            <div class={cn('rounded-[8px] border p-3', roleInsetClass(props.role))}>
              <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(props.role))}>{i18n.tr('Kommando', 'Command')}</p>
              <div class="mt-2 rounded-[8px] border border-[#E1E3E8] bg-white px-3 py-2 text-[12px] text-[#343A43] shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#E7EBF1]">
                {i18n.tr('Lag en produktfinner for vinterløping og tagg varer med lav lagerbeholdning.', 'Create a winter-running product finder and tag low-stock items.')}
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
                <div class="grid grid-cols-3 gap-1.5 pt-1" aria-label={i18n.tr('Sidekick-oppgavetyper', 'Sidekick task types')}>
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
                <Button variant="primary" size="md" shape="pill" disabled class={cn('min-h-8 flex-1 px-3 text-[12px] font-semibold', controlFocusClass)}>{i18n.tr('Vurder', 'Review')}</Button>
                <Button variant="secondary" size="md" shape="pill" disabled class={cn('min-h-8 flex-1 px-3 text-[12px] font-semibold', controlFocusClass)}>{i18n.tr('Bruk', 'Apply')}</Button>
              </div>
            </div>
          </CounterpartPanel>
          <CounterpartPanel
            role={props.role}
            icon={CheckCircle2}
            eyebrow={i18n.tr('Godkjenningsspor', 'Approval rail')}
            title={i18n.tr('Handlingssikkerhet', 'Action safety')}
            description={i18n.tr('Butikkendringer forblir vurderbare og avgrensede før noe skrives til Shopify.', 'Store changes stay reviewable and scoped before anything writes to Shopify.')}
          >
            <FeatureRow feature={props.operatingModel.guardrails[0]!} role={props.role} />
            <div class="mt-2">
              <FeatureRow feature={props.operatingModel.guardrails[1]!} role={props.role} />
            </div>
          </CounterpartPanel>
        </section>
      </Match>

      <Match when={props.feature === 'commerce-insights'}>
        <section class="grid gap-3 lg:grid-cols-3" aria-label={i18n.tr('Arbeidsområde for kjøperinnsikt', 'Ecommerce shopper insights workspace')}>
          <For each={ecommerceInsightCards}>
            {(metric) => (
              <CounterpartPanel role={props.role} icon={BarChart3} eyebrow={i18n.tr('Kjøperinnsikt', 'Shopper insight')} title={metric.title} description={metric.detail}>
                <p class="text-[32px] font-semibold leading-none text-[#202126] dark:text-white">{metric.value}</p>
              </CounterpartPanel>
            )}
          </For>
        </section>
      </Match>
    </Switch>
  )
}
