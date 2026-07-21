import {
  BarChart3,
  Blocks,
  CalendarDays,
  ChevronDown,
  Download,
  FileText,
  Globe2,
  Info,
  Mail,
  MessageCircle,
  MessageSquare,
  MessagesSquare,
  MoreHorizontal,
  Plug,
  Plus,
  RefreshCw,
  Search,
  Send,
  Settings2,
  Sparkles,
  TestTubeDiagonal,
  ThumbsDown,
  ThumbsUp,
  Upload,
  UserRound,
  Webhook,
  Wrench,
} from 'lucide-solid'
import { useNavigate } from '@solidjs/router'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { VelionSegmented, VelionSegmentedButton } from '@/shared/ui/velion/VelionSegmented'
import { cn } from '@/shared/lib/cn'
import type { ChatbotAddOnId, ChatbotBuilderSectionId } from '@/features/agents/lib/agent-roles'
import {
  createChatbotSupportStatusQuery,
  resolveChatbotSupportStatus,
  type SupportIntegrationStatus,
} from '@/features/agents/lib/use-chatbot-support-status'
import {
  allAddOnIds,
  chatbotDisplayName,
  defaultVisibleAddOns,
  studioSections,
} from '@/features/agents/lib/velion-chatbot-studio-data'
import { useAgentSelection, useChatbotAddOn, useChatbotBuilderSection } from '@/features/agents/lib/use-agent-selection'
import { ChatbotPlaygroundSurface } from '@/features/agents/components/ChatbotPlayground'
import { DesignPreviewBadge } from '@/features/agents/components/DesignPreviewBadge'
import {
  ActionCard,
  ChannelCard,
  ChannelHeroCard,
  IntegrationCard,
  SquareIconButton,
} from '@/features/agents/components/ChatbotStudioCards'
import {
  EmptyStateCard,
  EmptyStateInline,
  MetricCard,
  SectionHeader,
  type StudioIcon,
} from '@/features/agents/components/ChatbotStudioPrimitives'
import { useI18n } from '@/shared/i18n'

type AnalyticsTabId = 'chat-count' | 'topics' | 'sentiment'

type AddOnCanvasState = {
  hidden: ReadonlySet<ChatbotAddOnId>
  visible: ReadonlySet<ChatbotAddOnId>
}

type AddOnCanvasAction =
  | { type: 'add'; addOn: ChatbotAddOnId }
  | { type: 'clear' }
  | { type: 'remove'; addOn: ChatbotAddOnId }
  | { type: 'reset' }
  | { type: 'select'; addOn: ChatbotAddOnId }

const analyticsTabs: Array<{ id: AnalyticsTabId; labelNo: string; labelEn: string }> = [
  { id: 'chat-count', labelNo: 'Antall samtaler', labelEn: 'Chat count' },
  { id: 'topics', labelNo: 'Emner', labelEn: 'Topics' },
  { id: 'sentiment', labelNo: 'Sentiment', labelEn: 'Sentiment' },
]

// Phase 4 honesty sweep: these preview surfaces have no measurement backend yet,
// so they carry NO hardcoded values. MetricCard / SentimentPanel render a neutral
// placeholder; a real aggregate is wired in only once a source produces one.
const insightMetrics: Array<{ labelNo: string; labelEn: string; Icon: StudioIcon }> = [
  { labelNo: 'Totalt antall samtaler', labelEn: 'Total conversations', Icon: MessagesSquare },
  { labelNo: 'Totalt antall meldinger', labelEn: 'Total messages', Icon: MessageSquare },
  { labelNo: 'Meldinger med tommel opp', labelEn: 'Thumbs up messages', Icon: ThumbsUp },
  { labelNo: 'Meldinger med tommel ned', labelEn: 'Thumbs down messages', Icon: ThumbsDown },
]

const sentimentCards = [
  { labelNo: 'Positiv', labelEn: 'Positive', className: 'bg-[#E9F8EF] text-[#16834A]' },
  { labelNo: 'Nøytral', labelEn: 'Neutral', className: 'bg-[#F4F5F7] text-[#555B65]' },
  { labelNo: 'Negativ', labelEn: 'Negative', className: 'bg-[#FFF0EC] text-[#B6482C]' },
]

function getInitialAddOnCanvasState(): AddOnCanvasState {
  return {
    visible: new Set(defaultVisibleAddOns),
    hidden: new Set(),
  }
}

function addOnCanvasReducer(state: AddOnCanvasState, action: AddOnCanvasAction): AddOnCanvasState {
  switch (action.type) {
    case 'add': {
      const hidden = new Set(state.hidden)
      hidden.delete(action.addOn)
      return {
        visible: new Set([...state.visible, action.addOn]),
        hidden,
      }
    }
    case 'clear':
      return {
        visible: new Set(),
        hidden: new Set(allAddOnIds),
      }
    case 'remove':
      return {
        visible: new Set([...state.visible].filter((addOn) => addOn !== action.addOn)),
        hidden: new Set([...state.hidden, action.addOn]),
      }
    case 'reset':
      return getInitialAddOnCanvasState()
    case 'select': {
      const hidden = new Set(state.hidden)
      hidden.delete(action.addOn)
      return {
        visible: new Set([...state.visible, action.addOn]),
        hidden,
      }
    }
  }
}

export function ChatbotStudio() {
  const [agentSelection, setAgentSelection] = useAgentSelection()
  const [section] = useChatbotBuilderSection()
  const [selectedAddOn, setSelectedAddOn] = useChatbotAddOn()
  const supportQuery = createChatbotSupportStatusQuery()
  const supportStatus = () => resolveChatbotSupportStatus(supportQuery)
  const [addOnCanvas, setAddOnCanvas] = createSignal<AddOnCanvasState>(getInitialAddOnCanvasState())
  const displayedAddOns = createMemo<ReadonlySet<ChatbotAddOnId>>(() => {
    const canvas = addOnCanvas()
    const nextAddOns = new Set(canvas.visible)
    if (!canvas.hidden.has(selectedAddOn())) {
      nextAddOns.add(selectedAddOn())
    }
    return nextAddOns
  })
  const dispatchAddOnCanvas = (action: AddOnCanvasAction) => {
    setAddOnCanvas((state) => addOnCanvasReducer(state, action))
  }

  createEffect(() => {
    if (agentSelection() !== 'chatbot') {
      setAgentSelection('chatbot')
    }
  })

  const addSelectedAddOn = () => {
    dispatchAddOnCanvas({ type: 'add', addOn: selectedAddOn() })
  }
  const removeSelectedAddOn = () => {
    dispatchAddOnCanvas({ type: 'remove', addOn: selectedAddOn() })
  }
  const resetToEssentials = () => {
    dispatchAddOnCanvas({ type: 'reset' })
    setSelectedAddOn('subscription-action')
  }
  const clearCanvas = () => {
    dispatchAddOnCanvas({ type: 'clear' })
  }
  const handleAddOnSelect = (addOn: ChatbotAddOnId) => {
    setSelectedAddOn(addOn)
    dispatchAddOnCanvas({ type: 'select', addOn })
  }

  return (
    <Show
      when={section() !== 'playground'}
      fallback={(
        <ChatbotPlaygroundSurface
          onAddOnSelect={handleAddOnSelect}
          onAddSelected={addSelectedAddOn}
          onClearCanvas={clearCanvas}
          onRemoveSelected={removeSelectedAddOn}
          onResetCanvas={resetToEssentials}
          selectedAddOn={selectedAddOn()}
          supportStatus={supportStatus()}
          visibleAddOns={displayedAddOns()}
        />
      )}
    >
      <ChatbotSectionSurface section={section()} supportStatus={supportStatus()} />
    </Show>
  )
}

function ChatbotSectionSurface(props: {
  section: ChatbotBuilderSectionId
  supportStatus: SupportIntegrationStatus
}) {
  return (
    <div class="h-full min-h-0 overflow-y-auto bg-white text-[#111111] dark:bg-[#101114] dark:text-[#F7F8F8]">
      <Show when={props.section === 'analytics'}><AnalyticsPage /></Show>
      <Show when={props.section === 'data-sources'}><FineTuningPage /></Show>
      <Show when={props.section === 'integrations'}><IntegrationsPage supportStatus={props.supportStatus} /></Show>
      <Show when={props.section === 'actions'}><ToolsPage supportStatus={props.supportStatus} /></Show>
      <Show when={props.section === 'install'}><InstallPage /></Show>
      <Show when={props.section === 'chat-logs'}><ChatLogsPage /></Show>
      <Show when={props.section === 'leads'}><LeadsPage /></Show>
      <Show when={props.section === 'insights'}><InsightsPage /></Show>
      <Show when={props.section === 'settings'}><SettingsPage /></Show>
    </div>
  )
}

function AnalyticsPage() {
  const i18n = useI18n()
  const [activeTab, setActiveTab] = createSignal<AnalyticsTabId>('chat-count')

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title={i18n.tr('Analyse', 'Analytics')}
        description={i18n.tr('Mål volum, temafordeling og sentimentsignaler for chatboten.', 'Measure chatbot volume, topic distribution, and sentiment signals.')}
        preview
        action={(
          <Button shape="rounded" size="md" disabled>
            <CalendarDays class="size-4" />
            {i18n.tr('Live hendelsesvindu', 'Live event window')}
          </Button>
        )}
      />
      <VelionSegmented class="mt-8">
        <For each={analyticsTabs}>
          {(tab) => (
            <VelionSegmentedButton
              selected={activeTab() === tab.id}
              onClick={() => setActiveTab(tab.id)}
            >
              {i18n.tr(tab.labelNo, tab.labelEn)}
            </VelionSegmentedButton>
          )}
        </For>
      </VelionSegmented>

      <Show when={activeTab() === 'chat-count'}>
        <div class="mt-7 grid grid-cols-1 gap-4 md:grid-cols-3">
          <MetricCard Icon={MessagesSquare} label={i18n.tr('Samtaler', 'Chats')} />
          <MetricCard Icon={MessageSquare} label={i18n.tr('Meldinger', 'Messages')} />
          <MetricCard Icon={ThumbsUp} label={i18n.tr('Positive tilbakemeldinger', 'Positive feedback')} />
        </div>
        <EmptyStateCard
          Icon={BarChart3}
          title={i18n.tr('Ingen live chatbot-analyse ennå', 'No live chatbot analytics yet')}
          description={i18n.tr(
            'Analyse fylles ut fra ekte chatbot-samtaler når widgeten eller hjelpesiden er installert.',
            'Analytics populate from real chatbot conversations once the widget or help page is installed.',
          )}
        />
      </Show>
      <Show when={activeTab() === 'topics'}><TopicsPanel /></Show>
      <Show when={activeTab() === 'sentiment'}><SentimentPanel /></Show>
    </section>
  )
}

function InsightsPage() {
  const i18n = useI18n()

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title={i18n.tr('Innsikt', 'Insights')}
        description={i18n.tr('Gå gjennom signalene som bør forme forbedringer av chatboten.', 'Review the signals that should shape chatbot improvements.')}
        preview
        action={(
          <Button shape="rounded" size="md" disabled>
            <CalendarDays class="size-4" />
            {i18n.tr('Live hendelsesvindu', 'Live event window')}
          </Button>
        )}
      />

      <div class="mt-12 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
        <For each={insightMetrics}>
          {(metric) => <MetricCard Icon={metric.Icon} label={i18n.tr(metric.labelNo, metric.labelEn)} />}
        </For>
      </div>

      <EmptyStateCard
        Icon={Sparkles}
        title={i18n.tr('Ingen forbedringssignaler ennå', 'No improvement signals yet')}
        description={i18n.tr(
          'Velion vil rangere ubesvarte spørsmål, manglende kilder og mislykkede handlinger når det kommer inn ekte samtaler.',
          'Velion will rank unanswered questions, missing sources, and action failures after live conversations arrive.',
        )}
      />

      <div class="mt-6 grid gap-6 xl:grid-cols-[minmax(0,1fr)_520px]">
        <CountryCard />
        <LeadsCard />
      </div>
    </section>
  )
}

function TopicsPanel() {
  const i18n = useI18n()

  return (
    <div class="velion-panel mt-7 p-6">
      <h2 class="text-[20px] font-semibold">{i18n.tr('Emner', 'Topics')}</h2>
      <p class="mt-2 text-[14px] text-[#6F747D] dark:text-[#AEB4C0]">{i18n.tr('De vanligste temaene oppdaget på tvers av chatbot-samtaler.', 'Most common subjects detected across chatbot conversations.')}</p>
      <EmptyStateInline
        Icon={Search}
        title={i18n.tr('Ingen temaklynger ennå', 'No topic clusters yet')}
        description={i18n.tr('Temagrupper genereres fra ekte samtaler.', 'Topic groups are generated from real conversations.')}
      />
    </div>
  )
}

function SentimentPanel() {
  const i18n = useI18n()

  return (
    <div class="mt-7 grid gap-4 lg:grid-cols-3">
      <For each={sentimentCards}>
        {(item) => (
          <div class="velion-panel p-6">
            <div class={cn('inline-flex rounded-full px-3 py-1 text-[12px] font-semibold', item.className)}>{i18n.tr(item.labelNo, item.labelEn)}</div>
            <div class="mt-5 text-[34px] font-semibold text-[#8A909B] dark:text-[#AEB4C0]">—</div>
            <p class="mt-3 text-[14px] leading-6 text-[#6F747D] dark:text-[#AEB4C0]">
              {i18n.tr(
                'Målt fra klassifiserte kunde- og assistentturer når live samtaler er tilgjengelige.',
                'Measured from classified customer and assistant turns once live conversations are available.',
              )}
            </p>
          </div>
        )}
      </For>
    </div>
  )
}

function FineTuningPage() {
  const navigate = useNavigate()
  const i18n = useI18n()
  return (
    <section class="grid min-h-full gap-8 px-7 py-8 xl:grid-cols-[minmax(0,1fr)_420px]">
      <div class="min-w-0">
        <SectionHeader
          title={i18n.tr('Finjustering', 'Fine-tuning')}
          description={i18n.tr(
            'Last opp veiledede eksempler og datasett for ekte finjustering av modellen. Bruk dette for modellvekter/adaptere, ikke prompt-engineering.',
            'Upload supervised examples and datasets for real model fine-tuning. Use this for model weights/adapters, not prompt engineering.',
          )}
          action={(
            <Button shape="rounded" size="md" onClick={() => navigate('/settings/finetune')}>
              <Info class="size-5" />
              {i18n.tr('Åpne finjusteringsjobber', 'Open fine-tune jobs')}
            </Button>
          )}
        />

        <div class="velion-panel mt-12 p-7">
          {/* Phase 4 honesty sweep: dataset upload has no backend yet (the real
              wired action on this surface is the "Open fine-tune jobs" link). */}
          <div class="flex items-center justify-between gap-3">
            <div class="flex flex-wrap items-center gap-3">
              <h2 class="text-[23px] font-semibold">{i18n.tr('Legg til filer', 'Add files')}</h2>
              <DesignPreviewBadge />
            </div>
            <ChevronDown class="size-5 rotate-180 text-[#7C828C]" />
          </div>
          <div class="mt-7 flex min-h-11 items-center gap-3 rounded-[8px] border border-[#F1DCA6] bg-[#FFF9DF] px-4 text-[14px] font-semibold text-[#BA5A16]">
            <Info class="size-4 shrink-0" />
            {i18n.tr(
              'Finjusteringsdata bør bruke rene eksempler med input, forventet output og evalueringsetiketter.',
              'Fine-tuning data should use clean examples with input, expected output, and evaluation labels.',
            )}
          </div>
          <button
            type="button"
            disabled
            class="mt-6 grid min-h-[250px] w-full place-items-center rounded-[10px] border border-dashed border-[#D6D8DD] bg-[#FCFCFD] text-center transition-colors hover:bg-[#FAFAFB] disabled:cursor-not-allowed dark:border-[#303238] dark:bg-[#111216] dark:hover:bg-[#17181C]"
          >
            <span>
              <Upload class="mx-auto size-8 text-[#767C86]" />
              <span class="mt-6 block text-[17px] font-medium text-[#343842] dark:text-white">{i18n.tr('Dra og slipp finjusteringsdatasett her', 'Drag and drop fine-tuning datasets here')}</span>
              <span class="mt-2 block text-[14px] text-[#6F747D] dark:text-[#AEB4C0]">{i18n.tr('Støttede filtyper: jsonl, csv, parquet, txt', 'Supported file types: jsonl, csv, parquet, txt')}</span>
            </span>
          </button>
        </div>

        <div class="mt-12">
          <div class="flex flex-wrap items-center justify-between gap-4">
            <h2 class="text-[23px] font-semibold">{i18n.tr('Treningsdatasett', 'Training datasets')}</h2>
            <div class="relative w-full sm:w-[360px]">
              <Search class="pointer-events-none absolute left-4 top-1/2 size-5 -translate-y-1/2 text-[#A0A5AE]" />
              <VelionInput
                aria-label={i18n.tr('Søk i treningsdatasett', 'Search training datasets')}
                placeholder={i18n.tr('Søk …', 'Search…')}
                class="pl-12 pr-4 text-[13px]"
              />
            </div>
          </div>
          <div class="mt-7 flex items-center justify-between border-b border-[#E8E9EC] pb-6">
            <label class="inline-flex items-center gap-4 text-[16px] font-semibold">
              <input type="checkbox" class="size-5 rounded border-[#D8DADE]" />
              {i18n.tr('Velg alle', 'Select all')}
            </label>
            <button type="button" disabled class="inline-flex items-center gap-2 text-[16px] font-semibold text-[#5D626C] disabled:cursor-not-allowed disabled:opacity-70">
              {i18n.tr('Sorter etter:', 'Sort by:')} <span class="text-[#111111] dark:text-white">{i18n.tr('Standard', 'Default')}</span>
              <ChevronDown class="size-4" />
            </button>
          </div>
          <EmptyStateInline
            Icon={FileText}
            title={i18n.tr('Ingen treningsdatasett lastet opp', 'No training datasets uploaded')}
            description={i18n.tr('Last opp validerte filer før du starter en finjusteringsjobb.', 'Upload validated files before starting a fine-tune job.')}
          />
        </div>
      </div>

      <aside class="border-l border-[#E3E4E8] bg-[#F7F7F8] p-8 dark:border-[#2A2C31] dark:bg-[#111216] xl:-my-8 xl:-mr-7">
        <h2 class="text-[23px] font-semibold">{i18n.tr('Finjustering', 'Fine-tuning')}</h2>
        <div class="velion-panel mt-8 p-5">
          <div class="flex items-center justify-between text-[17px] font-semibold">
            <span class="inline-flex items-center gap-3"><TestTubeDiagonal class="size-5" />{i18n.tr('0 datasett', '0 datasets')}</span>
            <span>0 KB</span>
          </div>
        </div>
        <div class="velion-panel mt-6 p-5">
          <div class="flex items-center justify-between text-[16px]">
            <span class="text-[#6F747D] dark:text-[#AEB4C0]">{i18n.tr('Treningsstørrelse', 'Training size')}</span>
            <span class="font-semibold">0 KB / 20 MB</span>
          </div>
          <Button disabled shape="rounded" size="md" fullWidth class="mt-6">
            {i18n.tr('Start finjustering', 'Start fine-tune')}
          </Button>
        </div>
        <div class="mt-6 flex min-h-[54px] items-center gap-3 rounded-[9px] border border-[#F0DCA6] bg-[#FFF8DC] px-4 text-[15px] font-semibold text-[#B85E16]">
          <RefreshCw class="size-5" />
          {i18n.tr('Finjustering oppretter en ny modelladapter når valideringen er bestått', 'Fine-tuning creates a new model adapter after validation passes')}
        </div>
      </aside>
    </section>
  )
}

function ToolsPage(props: { supportStatus: SupportIntegrationStatus }) {
  const i18n = useI18n()
  const supportConnected = () => props.supportStatus.status === 'connected'
  const actions = () => [
    {
      title: i18n.tr('Samle inn leads', 'Collect leads'),
      subtitle: i18n.tr('Ferdighet: fang opp kvalifiserte kontaktfelt', 'Skill: capture qualified contact fields'),
      color: 'text-[#E53688] bg-[#FFF0F7] border-[#F7B8D5]',
      Icon: UserRound,
      enabled: true,
    },
    {
      title: i18n.tr('Opprett supportsak', 'Create support ticket'),
      subtitle: supportConnected()
        ? i18n.tr('Verktøy: Zammad-sakopprettelse klar', 'Tool: Zammad ticket creation ready')
        : i18n.tr('Koble til support-integrasjon for å aktivere', 'Connect support integration to enable'),
      color: 'text-[#EE7A50] bg-[#FFF3EC] border-[#F5C7B4]',
      Icon: Wrench,
      enabled: supportConnected(),
    },
    {
      title: i18n.tr('Rut til supportgruppe', 'Route to support group'),
      subtitle: supportConnected()
        ? i18n.tr(`${props.supportStatus.groups} live grupper tilgjengelig`, `${props.supportStatus.groups} live groups available`)
        : i18n.tr('Venter på supportgrupper', 'Waiting for support groups'),
      color: 'text-[#4A9C9C] bg-[#F2FBFB] border-[#D8E7EA]',
      Icon: MessagesSquare,
      enabled: supportConnected(),
    },
  ]

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title={i18n.tr('Verktøy', 'Tools')}
        description={i18n.tr(
          'Konfigurer verktøyene chatboten kan kalle, og ferdighetene den kan utføre. Integrasjoner gir datatilgang; verktøy avgjør hva boten har lov til å gjøre med den.',
          'Configure the tools the chatbot can call and the skills it can perform. Integrations provide data access; tools decide what the bot may do with it.',
        )}
        preview
        action={(
          <div class="flex min-w-0 flex-1 justify-end gap-3">
            <div class="relative w-full max-w-[470px]">
              <Search class="pointer-events-none absolute left-4 top-1/2 size-5 -translate-y-1/2 text-[#6F747D]" />
              <VelionInput
                aria-label={i18n.tr('Søk i verktøy', 'Search tools')}
                placeholder={i18n.tr('Søk', 'Search')}
                class="pl-12 pr-4 text-[13px]"
              />
            </div>
            <Button variant="primary" shape="rounded" size="md" disabled class="shrink-0">
              <Plus class="size-5" />
              {i18n.tr('Opprett verktøy', 'Create tool')}
            </Button>
          </div>
        )}
      />
      <div class="mt-12 grid grid-cols-1 gap-5 lg:grid-cols-3">
        <For each={actions()}>
          {(action) => <ActionCard {...action} />}
        </For>
      </div>
    </section>
  )
}

function InstallPage() {
  const i18n = useI18n()
  const channels: Array<{ title: string; badge?: string; description: string; Icon: StudioIcon; action: string }> = [
    {
      title: i18n.tr('E-post', 'Email'),
      badge: i18n.tr('Beta', 'Beta'),
      description: i18n.tr('Koble agenten din til en e-postadresse og la den svare på meldinger fra kundene dine.', 'Connect your agent to an email address and let it respond to messages from your customers.'),
      Icon: Mail,
      action: i18n.tr('Abonner for å aktivere', 'Subscribe to enable'),
    },
    {
      title: 'Zapier',
      description: i18n.tr('Koble agenten din til tusenvis av apper med Zapier.', 'Connect your agent with thousands of apps using Zapier.'),
      Icon: Plug,
      action: i18n.tr('Abonner for å aktivere', 'Subscribe to enable'),
    },
    {
      title: 'Slack',
      description: i18n.tr('Koble agenten din til Slack, nevn den, og la den svare på enhver melding.', 'Connect your agent to Slack, mention it, and have it reply to any message.'),
      Icon: MessagesSquare,
      action: i18n.tr('Abonner for å aktivere', 'Subscribe to enable'),
    },
    {
      title: 'WordPress',
      description: i18n.tr('Installer Velion-widgetskriptet gjennom en WordPress-embed eller plugin-wrapper.', 'Install the Velion widget script through a WordPress embed or plugin wrapper.'),
      Icon: Globe2,
      action: i18n.tr('Oppsett', 'Setup'),
    },
    {
      title: 'WhatsApp',
      description: i18n.tr('Koble agenten din til et WhatsApp-nummer og svar i samme tråd.', 'Connect your agent to a WhatsApp number and respond in the same thread.'),
      Icon: MessageCircle,
      action: i18n.tr('Abonner for å aktivere', 'Subscribe to enable'),
    },
    {
      title: 'Messenger',
      description: i18n.tr('Koble agenten din til en Facebook-side og la den svare kundene.', 'Connect your agent to a Facebook page and let it reply to customers.'),
      Icon: Send,
      action: i18n.tr('Abonner for å aktivere', 'Subscribe to enable'),
    },
  ]

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <div class="flex flex-wrap items-center gap-3">
        <h1 class="text-[30px] font-semibold tracking-normal">{i18n.tr('Alle kanaler', 'All channels')}</h1>
        <DesignPreviewBadge />
      </div>
      <div class="mt-12 grid gap-5 xl:grid-cols-2">
        <ChannelHeroCard displayName={chatbotDisplayName} type="widget" />
        <ChannelHeroCard displayName={chatbotDisplayName} type="help" />
      </div>
      <div class="mt-5 grid grid-cols-1 gap-5 lg:grid-cols-3">
        <For each={channels}>
          {(channel) => <ChannelCard {...channel} />}
        </For>
      </div>
    </section>
  )
}

function IntegrationsPage(props: { supportStatus: SupportIntegrationStatus }) {
  const i18n = useI18n()
  const supportConnected = () => props.supportStatus.status === 'connected'
  const integrations = () => [
    {
      title: i18n.tr('Supportagenter', 'Support agents'),
      description: i18n.tr('Les tilgjengelige menneskelige agenter for overlevering og eierskap.', 'Read available human agents for handoff and ownership.'),
      Icon: UserRound,
      status: supportConnected() ? i18n.tr(`${props.supportStatus.agents} tilkoblet`, `${props.supportStatus.agents} connected`) : i18n.tr('Ikke tilkoblet', 'Not connected'),
    },
    {
      title: i18n.tr('Supportgrupper', 'Support groups'),
      description: i18n.tr('Rut samtaler til ekte supportteam og køer.', 'Route conversations to real support teams and queues.'),
      Icon: MessagesSquare,
      status: supportConnected() ? i18n.tr(`${props.supportStatus.groups} tilkoblet`, `${props.supportStatus.groups} connected`) : i18n.tr('Ikke tilkoblet', 'Not connected'),
    },
    {
      title: i18n.tr('Support-makroer', 'Support macros'),
      description: i18n.tr('Eksponer godkjente svar- og arbeidsflytmakroer som beskyttede handlinger.', 'Expose approved response and workflow macros as guarded actions.'),
      Icon: Wrench,
      status: supportConnected() ? i18n.tr(`${props.supportStatus.macros} tilkoblet`, `${props.supportStatus.macros} connected`) : i18n.tr('Ikke tilkoblet', 'Not connected'),
    },
    {
      title: i18n.tr('Nettside-crawler', 'Website crawler'),
      description: i18n.tr('Hent offentlige sider, dokumenter og produkttekst for gjenfinning.', 'Fetch public pages, docs, and product copy for retrieval.'),
      Icon: Globe2,
      status: i18n.tr('Konfigurer', 'Configure'),
    },
    {
      title: 'Webhook API',
      description: i18n.tr('Kall interne systemer gjennom signerte forespørsel-endepunkter.', 'Call internal systems through signed request endpoints.'),
      Icon: Webhook,
      status: i18n.tr('Konfigurer', 'Configure'),
    },
    {
      title: i18n.tr('Vektorlager', 'Vector store'),
      description: i18n.tr('Synkroniser embeddings og gjenfinningsindekser som brukes av chatbot-verktøy.', 'Sync embeddings and retrieval indexes used by chatbot tools.'),
      Icon: Blocks,
      status: i18n.tr('Konfigurer', 'Configure'),
    },
  ]

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title={i18n.tr('Integrasjoner', 'Integrations')}
        description={i18n.tr(
          'Koble til systemene chatboten kan hente data fra. Verktøy definerer deretter hvilke handlinger som er tillatt over disse integrasjonene.',
          'Connect the systems the chatbot can fetch data from. Tools then define the allowed option pool over those integrations.',
        )}
        preview
        action={(
          <Button variant="primary" shape="rounded" size="md" disabled>
            <Plus class="size-4" />
            {i18n.tr('Legg til integrasjon', 'Add integration')}
          </Button>
        )}
      />
      <div class="mt-8 grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        <For each={integrations()}>
          {(integration) => <IntegrationCard {...integration} />}
        </For>
      </div>
    </section>
  )
}

function LeadsPage() {
  const i18n = useI18n()

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title={i18n.tr('Leads', 'Leads')}
        description={i18n.tr('Gå gjennom lead-innsendinger samlet inn av chatbot-ferdigheter, og eksporter dem for oppfølging.', 'Review lead submissions collected by chatbot skills and export them for follow-up.')}
        preview
        action={(
          <Button variant="primary" shape="rounded" size="md" disabled>
            {i18n.tr('Eksporter', 'Export')}
            <Download class="size-4" />
          </Button>
        )}
      />
      <div class="mt-8">
        <LeadsCard hideHeaderAction />
      </div>
    </section>
  )
}

function ChatLogsPage() {
  const i18n = useI18n()

  return (
    <section class="grid min-h-full lg:grid-cols-[480px_minmax(0,1fr)]">
      <aside class="border-r border-[#E3E4E8] bg-white px-6 py-8 dark:border-[#2A2C31] dark:bg-[#101114]">
        <div class="flex items-center justify-between gap-4">
          <div class="flex flex-wrap items-center gap-3">
            <h1 class="text-[30px] font-semibold tracking-normal">{i18n.tr('Chattelogger', 'Chat logs')}</h1>
            <DesignPreviewBadge />
          </div>
          {/* Phase 4 honesty sweep: chat-log filter/refresh/download have no backend yet. */}
          <div class="flex items-center gap-2">
            <SquareIconButton disabled label={i18n.tr('Filtrer chattelogger', 'Filter chat logs')} Icon={Settings2} />
            <SquareIconButton disabled label={i18n.tr('Oppdater chattelogger', 'Refresh chat logs')} Icon={RefreshCw} />
            <VelionIconButton size="lg" shape="rounded" tone="primary" disabled aria-label={i18n.tr('Last ned chattelogger', 'Download chat logs')}>
              <Download class="size-5" />
            </VelionIconButton>
          </div>
        </div>
        <div class="mt-12 space-y-3">
          <EmptyStateInline
            Icon={MessagesSquare}
            title={i18n.tr('Ingen chatbot-samtaler ennå', 'No chatbot conversations yet')}
            description={i18n.tr('Live chattelogger vises her når widgeten eller hjelpesiden mottar trafikk.', 'Live chat logs appear here after the widget or help page receives traffic.')}
          />
        </div>
      </aside>

      <main class="min-w-0 bg-white dark:bg-[#101114]">
        <div class="flex h-[118px] items-start justify-between border-b border-[#E3E4E8] px-8 py-7 dark:border-[#2A2C31]">
          <div>
            <h2 class="text-[21px] font-semibold">{i18n.tr('Playground', 'Playground')}</h2>
            <div class="mt-6 flex gap-8 text-[16px] font-medium">
              <span class="border-b-2 border-black pb-4 text-black dark:border-white dark:text-white">{i18n.tr('Chat', 'Chat')}</span>
              <span class="pb-4 text-[#6F747D]">{i18n.tr('Detaljer', 'Details')}</span>
            </div>
          </div>
          <SquareIconButton disabled label={i18n.tr('Åpne chattelogg-meny', 'Open chat log menu')} Icon={MoreHorizontal} />
        </div>
        <div class="mx-auto max-w-[820px] p-8">
          <EmptyStateCard
            Icon={MessageSquare}
            title={i18n.tr('Velg en live samtale', 'Select a live conversation')}
            description={i18n.tr('Samtaletranskripsjoner, kilder og verktøyspor vises her når det finnes ekte chattelogger.', 'Conversation transcripts, sources, and tool traces will render here when real chat logs exist.')}
          />
        </div>
      </main>
    </section>
  )
}

function SettingsPage() {
  const i18n = useI18n()
  const section = studioSections.settings
  const settings = [
    [i18n.tr('Agentnavn', 'Agent name'), chatbotDisplayName],
    [i18n.tr('Tone', 'Tone'), i18n.tr('Klar, konsis og produktbevisst', 'Clear, concise, and product-aware')],
    [i18n.tr('Fallback-oppførsel', 'Fallback behavior'), i18n.tr('Be om avklaring før overlevering', 'Ask for clarification before handing off')],
  ] as const

  return (
    <section class="mx-auto max-w-[980px] px-7 py-8">
      <SectionHeader title={section.title} description={section.description} />
      <div class="mt-10 grid gap-5">
        <For each={settings}>
          {(setting) => (
            <label class="velion-panel grid gap-3 p-5">
              <span class="text-[14px] font-semibold text-[#6F747D] dark:text-[#AEB4C0]">{setting[0]}</span>
              <VelionInput value={setting[1]} class="px-4 text-[13px]" />
            </label>
          )}
        </For>
      </div>
    </section>
  )
}

function CountryCard() {
  const i18n = useI18n()

  return (
    <div class="velion-panel p-7">
      <h2 class="text-[23px] font-semibold">{i18n.tr('Samtaler etter land', 'Chats by country')}</h2>
      <div class="mt-8 grid gap-8 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div class="relative min-h-[260px] overflow-hidden rounded-[10px] bg-[#FAFAFB] dark:bg-[#111216]">
          <div class="absolute left-14 top-16 h-24 w-36 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div class="absolute left-[42%] top-10 h-28 w-44 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div class="absolute bottom-10 right-16 h-32 w-52 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div class="absolute bottom-16 left-[18%] h-16 w-24 rounded-[48%] bg-[#FDE4D2]" />
        </div>
        <div>
          <div class="grid grid-cols-[1fr_80px] border-b border-dashed border-[#D8DADE] pb-3 text-[16px] text-[#7B808A]">
            <span>{i18n.tr('Land', 'Country')}</span><span class="text-right">{i18n.tr('Samtaler', 'Chats')}</span>
          </div>
          <EmptyStateInline
            Icon={Globe2}
            title={i18n.tr('Ingen stedsdata ennå', 'No location data yet')}
            description={i18n.tr('Land vises når ekte samtaler inneholder stedsmetadata.', 'Countries appear after real conversations include location metadata.')}
          />
        </div>
      </div>
    </div>
  )
}

function LeadsCard(props: { hideHeaderAction?: boolean }) {
  const i18n = useI18n()

  return (
    <div class="velion-panel p-7">
      <div class="flex items-center justify-between gap-4">
        <div>
          <h2 class="text-[23px] font-semibold">{i18n.tr('Leads', 'Leads')}</h2>
          <p class="mt-2 text-[15px] text-[#6F747D] dark:text-[#AEB4C0]">{i18n.tr('Sendt inn fra lead-innsamlingsferdigheter.', 'Submitted from lead collection skills.')}</p>
        </div>
        <Show when={!props.hideHeaderAction}>
          <Button variant="primary" shape="rounded" size="md" disabled>
            {i18n.tr('Eksporter', 'Export')}
            <Download class="size-4" />
          </Button>
        </Show>
      </div>
      <div class="mt-6 overflow-hidden rounded-[10px] border border-[#E8E9EC]">
        <div class="grid grid-cols-[0.8fr_1.4fr_1fr_1.2fr] border-b border-[#E8E9EC] bg-[#FAFAFB] px-4 py-3 text-[13px] font-semibold">
          <span>{i18n.tr('Navn', 'Name')}</span><span>{i18n.tr('E-post', 'Email')}</span><span>{i18n.tr('Telefon', 'Phone')}</span><span>{i18n.tr('Sendt inn', 'Submitted at')}</span>
        </div>
        <div class="p-4">
          <EmptyStateInline
            Icon={UserRound}
            title={i18n.tr('Ingen fangede leads ennå', 'No captured leads yet')}
            description={i18n.tr('Lead-rader opprettes kun fra ekte chatbot-lead-innsendinger.', 'Lead rows are created only from live chatbot lead-collection submissions.')}
          />
        </div>
      </div>
    </div>
  )
}
