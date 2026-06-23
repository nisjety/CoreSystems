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

const analyticsTabs: Array<{ id: AnalyticsTabId; label: string }> = [
  { id: 'chat-count', label: 'Chat count' },
  { id: 'topics', label: 'Topics' },
  { id: 'sentiment', label: 'Sentiment' },
]

// Phase 4 honesty sweep: these preview surfaces have no measurement backend yet,
// so they carry NO hardcoded values. MetricCard / SentimentPanel render a neutral
// placeholder; a real aggregate is wired in only once a source produces one.
const insightMetrics: Array<{ label: string; Icon: StudioIcon }> = [
  { label: 'Total conversations', Icon: MessagesSquare },
  { label: 'Total messages', Icon: MessageSquare },
  { label: 'Thumbs up messages', Icon: ThumbsUp },
  { label: 'Thumbs down messages', Icon: ThumbsDown },
]

const sentimentCards = [
  { label: 'Positive', className: 'bg-[#E9F8EF] text-[#16834A]' },
  { label: 'Neutral', className: 'bg-[#F4F5F7] text-[#555B65]' },
  { label: 'Negative', className: 'bg-[#FFF0EC] text-[#B6482C]' },
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
  const [activeTab, setActiveTab] = createSignal<AnalyticsTabId>('chat-count')

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Analytics"
        description="Measure chatbot volume, topic distribution, and sentiment signals."
        preview
        action={(
          <Button shape="rounded" size="md" disabled>
            <CalendarDays class="size-4" />
            Live event window
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
              {tab.label}
            </VelionSegmentedButton>
          )}
        </For>
      </VelionSegmented>

      <Show when={activeTab() === 'chat-count'}>
        <div class="mt-7 grid grid-cols-1 gap-4 md:grid-cols-3">
          <MetricCard Icon={MessagesSquare} label="Chats" />
          <MetricCard Icon={MessageSquare} label="Messages" />
          <MetricCard Icon={ThumbsUp} label="Positive feedback" />
        </div>
        <EmptyStateCard
          Icon={BarChart3}
          title="No live chatbot analytics yet"
          description="Analytics populate from real chatbot conversations once the widget or help page is installed."
        />
      </Show>
      <Show when={activeTab() === 'topics'}><TopicsPanel /></Show>
      <Show when={activeTab() === 'sentiment'}><SentimentPanel /></Show>
    </section>
  )
}

function InsightsPage() {
  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Insights"
        description="Review the signals that should shape chatbot improvements."
        preview
        action={(
          <Button shape="rounded" size="md" disabled>
            <CalendarDays class="size-4" />
            Live event window
          </Button>
        )}
      />

      <div class="mt-12 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
        <For each={insightMetrics}>
          {(metric) => <MetricCard Icon={metric.Icon} label={metric.label} />}
        </For>
      </div>

      <EmptyStateCard
        Icon={Sparkles}
        title="No improvement signals yet"
        description="Velion will rank unanswered questions, missing sources, and action failures after live conversations arrive."
      />

      <div class="mt-6 grid gap-6 xl:grid-cols-[minmax(0,1fr)_520px]">
        <CountryCard />
        <LeadsCard />
      </div>
    </section>
  )
}

function TopicsPanel() {
  return (
    <div class="velion-panel mt-7 p-6">
      <h2 class="text-[20px] font-semibold">Topics</h2>
      <p class="mt-2 text-[14px] text-[#6F747D] dark:text-[#AEB4C0]">Most common subjects detected across chatbot conversations.</p>
      <EmptyStateInline Icon={Search} title="No topic clusters yet" description="Topic groups are generated from real conversations." />
    </div>
  )
}

function SentimentPanel() {
  return (
    <div class="mt-7 grid gap-4 lg:grid-cols-3">
      <For each={sentimentCards}>
        {(item) => (
          <div class="velion-panel p-6">
            <div class={cn('inline-flex rounded-full px-3 py-1 text-[12px] font-semibold', item.className)}>{item.label}</div>
            <div class="mt-5 text-[34px] font-semibold text-[#8A909B] dark:text-[#AEB4C0]">—</div>
            <p class="mt-3 text-[14px] leading-6 text-[#6F747D] dark:text-[#AEB4C0]">Measured from classified customer and assistant turns once live conversations are available.</p>
          </div>
        )}
      </For>
    </div>
  )
}

function FineTuningPage() {
  const navigate = useNavigate()
  return (
    <section class="grid min-h-full gap-8 px-7 py-8 xl:grid-cols-[minmax(0,1fr)_420px]">
      <div class="min-w-0">
        <SectionHeader
          title="Fine-tuning"
          description="Upload supervised examples and datasets for real model fine-tuning. Use this for model weights/adapters, not prompt engineering."
          action={(
            <Button shape="rounded" size="md" onClick={() => navigate('/settings/finetune')}>
              <Info class="size-5" />
              Open fine-tune jobs
            </Button>
          )}
        />

        <div class="velion-panel mt-12 p-7">
          {/* Phase 4 honesty sweep: dataset upload has no backend yet (the real
              wired action on this surface is the "Open fine-tune jobs" link). */}
          <div class="flex items-center justify-between gap-3">
            <div class="flex flex-wrap items-center gap-3">
              <h2 class="text-[23px] font-semibold">Add files</h2>
              <DesignPreviewBadge />
            </div>
            <ChevronDown class="size-5 rotate-180 text-[#7C828C]" />
          </div>
          <div class="mt-7 flex min-h-11 items-center gap-3 rounded-[8px] border border-[#F1DCA6] bg-[#FFF9DF] px-4 text-[14px] font-semibold text-[#BA5A16]">
            <Info class="size-4 shrink-0" />
            Fine-tuning data should use clean examples with input, expected output, and evaluation labels.
          </div>
          <button
            type="button"
            disabled
            class="mt-6 grid min-h-[250px] w-full place-items-center rounded-[10px] border border-dashed border-[#D6D8DD] bg-[#FCFCFD] text-center transition-colors hover:bg-[#FAFAFB] disabled:cursor-not-allowed dark:border-[#303238] dark:bg-[#111216] dark:hover:bg-[#17181C]"
          >
            <span>
              <Upload class="mx-auto size-8 text-[#767C86]" />
              <span class="mt-6 block text-[17px] font-medium text-[#343842] dark:text-white">Drag and drop fine-tuning datasets here</span>
              <span class="mt-2 block text-[14px] text-[#6F747D] dark:text-[#AEB4C0]">Supported file types: jsonl, csv, parquet, txt</span>
            </span>
          </button>
        </div>

        <div class="mt-12">
          <div class="flex flex-wrap items-center justify-between gap-4">
            <h2 class="text-[23px] font-semibold">Training datasets</h2>
            <div class="relative w-full sm:w-[360px]">
              <Search class="pointer-events-none absolute left-4 top-1/2 size-5 -translate-y-1/2 text-[#A0A5AE]" />
              <VelionInput
                aria-label="Search training datasets"
                placeholder="Search…"
                class="pl-12 pr-4 text-[13px]"
              />
            </div>
          </div>
          <div class="mt-7 flex items-center justify-between border-b border-[#E8E9EC] pb-6">
            <label class="inline-flex items-center gap-4 text-[16px] font-semibold">
              <input type="checkbox" class="size-5 rounded border-[#D8DADE]" />
              Select all
            </label>
            <button type="button" disabled class="inline-flex items-center gap-2 text-[16px] font-semibold text-[#5D626C] disabled:cursor-not-allowed disabled:opacity-70">
              Sort by: <span class="text-[#111111] dark:text-white">Default</span>
              <ChevronDown class="size-4" />
            </button>
          </div>
          <EmptyStateInline
            Icon={FileText}
            title="No training datasets uploaded"
            description="Upload validated files before starting a fine-tune job."
          />
        </div>
      </div>

      <aside class="border-l border-[#E3E4E8] bg-[#F7F7F8] p-8 dark:border-[#2A2C31] dark:bg-[#111216] xl:-my-8 xl:-mr-7">
        <h2 class="text-[23px] font-semibold">Fine-tuning</h2>
        <div class="velion-panel mt-8 p-5">
          <div class="flex items-center justify-between text-[17px] font-semibold">
            <span class="inline-flex items-center gap-3"><TestTubeDiagonal class="size-5" />0 datasets</span>
            <span>0 KB</span>
          </div>
        </div>
        <div class="velion-panel mt-6 p-5">
          <div class="flex items-center justify-between text-[16px]">
            <span class="text-[#6F747D] dark:text-[#AEB4C0]">Training size</span>
            <span class="font-semibold">0 KB / 20 MB</span>
          </div>
          <Button disabled shape="rounded" size="md" fullWidth class="mt-6">
            Start fine-tune
          </Button>
        </div>
        <div class="mt-6 flex min-h-[54px] items-center gap-3 rounded-[9px] border border-[#F0DCA6] bg-[#FFF8DC] px-4 text-[15px] font-semibold text-[#B85E16]">
          <RefreshCw class="size-5" />
          Fine-tuning creates a new model adapter after validation passes
        </div>
      </aside>
    </section>
  )
}

function ToolsPage(props: { supportStatus: SupportIntegrationStatus }) {
  const supportConnected = () => props.supportStatus.status === 'connected'
  const actions = () => [
    { title: 'Collect leads', subtitle: 'Skill: capture qualified contact fields', color: 'text-[#E53688] bg-[#FFF0F7] border-[#F7B8D5]', Icon: UserRound, enabled: true },
    { title: 'Create support ticket', subtitle: supportConnected() ? 'Tool: Zammad ticket creation ready' : 'Connect support integration to enable', color: 'text-[#EE7A50] bg-[#FFF3EC] border-[#F5C7B4]', Icon: Wrench, enabled: supportConnected() },
    { title: 'Route to support group', subtitle: supportConnected() ? `${props.supportStatus.groups} live groups available` : 'Waiting for support groups', color: 'text-[#4A9C9C] bg-[#F2FBFB] border-[#D8E7EA]', Icon: MessagesSquare, enabled: supportConnected() },
  ]

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Tools"
        description="Configure the tools the chatbot can call and the skills it can perform. Integrations provide data access; tools decide what the bot may do with it."
        preview
        action={(
          <div class="flex min-w-0 flex-1 justify-end gap-3">
            <div class="relative w-full max-w-[470px]">
              <Search class="pointer-events-none absolute left-4 top-1/2 size-5 -translate-y-1/2 text-[#6F747D]" />
              <VelionInput
                aria-label="Search tools"
                placeholder="Search"
                class="pl-12 pr-4 text-[13px]"
              />
            </div>
            <Button variant="primary" shape="rounded" size="md" disabled class="shrink-0">
              <Plus class="size-5" />
              Create tool
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
  const channels: Array<{ title: string; badge?: string; description: string; Icon: StudioIcon; action: string }> = [
    { title: 'Email', badge: 'Beta', description: 'Connect your agent to an email address and let it respond to messages from your customers.', Icon: Mail, action: 'Subscribe to enable' },
    { title: 'Zapier', description: 'Connect your agent with thousands of apps using Zapier.', Icon: Plug, action: 'Subscribe to enable' },
    { title: 'Slack', description: 'Connect your agent to Slack, mention it, and have it reply to any message.', Icon: MessagesSquare, action: 'Subscribe to enable' },
    { title: 'WordPress', description: 'Install the Velion widget script through a WordPress embed or plugin wrapper.', Icon: Globe2, action: 'Setup' },
    { title: 'WhatsApp', description: 'Connect your agent to a WhatsApp number and respond in the same thread.', Icon: MessageCircle, action: 'Subscribe to enable' },
    { title: 'Messenger', description: 'Connect your agent to a Facebook page and let it reply to customers.', Icon: Send, action: 'Subscribe to enable' },
  ]

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <div class="flex flex-wrap items-center gap-3">
        <h1 class="text-[30px] font-semibold tracking-normal">All channels</h1>
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
  const supportConnected = () => props.supportStatus.status === 'connected'
  const integrations = () => [
    { title: 'Support agents', description: 'Read available human agents for handoff and ownership.', Icon: UserRound, status: supportConnected() ? `${props.supportStatus.agents} connected` : 'Not connected' },
    { title: 'Support groups', description: 'Route conversations to real support teams and queues.', Icon: MessagesSquare, status: supportConnected() ? `${props.supportStatus.groups} connected` : 'Not connected' },
    { title: 'Support macros', description: 'Expose approved response and workflow macros as guarded actions.', Icon: Wrench, status: supportConnected() ? `${props.supportStatus.macros} connected` : 'Not connected' },
    { title: 'Website crawler', description: 'Fetch public pages, docs, and product copy for retrieval.', Icon: Globe2, status: 'Configure' },
    { title: 'Webhook API', description: 'Call internal systems through signed request endpoints.', Icon: Webhook, status: 'Configure' },
    { title: 'Vector store', description: 'Sync embeddings and retrieval indexes used by chatbot tools.', Icon: Blocks, status: 'Configure' },
  ]

  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Integrations"
        description="Connect the systems the chatbot can fetch data from. Tools then define the allowed option pool over those integrations."
        preview
        action={(
          <Button variant="primary" shape="rounded" size="md" disabled>
            <Plus class="size-4" />
            Add integration
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
  return (
    <section class="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Leads"
        description="Review lead submissions collected by chatbot skills and export them for follow-up."
        preview
        action={(
          <Button variant="primary" shape="rounded" size="md" disabled>
            Export
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
  return (
    <section class="grid min-h-full lg:grid-cols-[480px_minmax(0,1fr)]">
      <aside class="border-r border-[#E3E4E8] bg-white px-6 py-8 dark:border-[#2A2C31] dark:bg-[#101114]">
        <div class="flex items-center justify-between gap-4">
          <div class="flex flex-wrap items-center gap-3">
            <h1 class="text-[30px] font-semibold tracking-normal">Chat logs</h1>
            <DesignPreviewBadge />
          </div>
          {/* Phase 4 honesty sweep: chat-log filter/refresh/download have no backend yet. */}
          <div class="flex items-center gap-2">
            <SquareIconButton disabled label="Filter chat logs" Icon={Settings2} />
            <SquareIconButton disabled label="Refresh chat logs" Icon={RefreshCw} />
            <VelionIconButton size="lg" shape="rounded" tone="primary" disabled aria-label="Download chat logs">
              <Download class="size-5" />
            </VelionIconButton>
          </div>
        </div>
        <div class="mt-12 space-y-3">
          <EmptyStateInline
            Icon={MessagesSquare}
            title="No chatbot conversations yet"
            description="Live chat logs appear here after the widget or help page receives traffic."
          />
        </div>
      </aside>

      <main class="min-w-0 bg-white dark:bg-[#101114]">
        <div class="flex h-[118px] items-start justify-between border-b border-[#E3E4E8] px-8 py-7 dark:border-[#2A2C31]">
          <div>
            <h2 class="text-[21px] font-semibold">Playground</h2>
            <div class="mt-6 flex gap-8 text-[16px] font-medium">
              <span class="border-b-2 border-black pb-4 text-black dark:border-white dark:text-white">Chat</span>
              <span class="pb-4 text-[#6F747D]">Details</span>
            </div>
          </div>
          <SquareIconButton disabled label="Open chat log menu" Icon={MoreHorizontal} />
        </div>
        <div class="mx-auto max-w-[820px] p-8">
          <EmptyStateCard
            Icon={MessageSquare}
            title="Select a live conversation"
            description="Conversation transcripts, sources, and tool traces will render here when real chat logs exist."
          />
        </div>
      </main>
    </section>
  )
}

function SettingsPage() {
  const section = studioSections.settings
  const settings = [
    ['Agent name', chatbotDisplayName],
    ['Tone', 'Clear, concise, and product-aware'],
    ['Fallback behavior', 'Ask for clarification before handing off'],
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
  return (
    <div class="velion-panel p-7">
      <h2 class="text-[23px] font-semibold">Chats by country</h2>
      <div class="mt-8 grid gap-8 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div class="relative min-h-[260px] overflow-hidden rounded-[10px] bg-[#FAFAFB] dark:bg-[#111216]">
          <div class="absolute left-14 top-16 h-24 w-36 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div class="absolute left-[42%] top-10 h-28 w-44 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div class="absolute bottom-10 right-16 h-32 w-52 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div class="absolute bottom-16 left-[18%] h-16 w-24 rounded-[48%] bg-[#FDE4D2]" />
        </div>
        <div>
          <div class="grid grid-cols-[1fr_80px] border-b border-dashed border-[#D8DADE] pb-3 text-[16px] text-[#7B808A]">
            <span>Country</span><span class="text-right">Chats</span>
          </div>
          <EmptyStateInline Icon={Globe2} title="No location data yet" description="Countries appear after real conversations include location metadata." />
        </div>
      </div>
    </div>
  )
}

function LeadsCard(props: { hideHeaderAction?: boolean }) {
  return (
    <div class="velion-panel p-7">
      <div class="flex items-center justify-between gap-4">
        <div>
          <h2 class="text-[23px] font-semibold">Leads</h2>
          <p class="mt-2 text-[15px] text-[#6F747D] dark:text-[#AEB4C0]">Submitted from lead collection skills.</p>
        </div>
        <Show when={!props.hideHeaderAction}>
          <Button variant="primary" shape="rounded" size="md" disabled>
            Export
            <Download class="size-4" />
          </Button>
        </Show>
      </div>
      <div class="mt-6 overflow-hidden rounded-[10px] border border-[#E8E9EC]">
        <div class="grid grid-cols-[0.8fr_1.4fr_1fr_1.2fr] border-b border-[#E8E9EC] bg-[#FAFAFB] px-4 py-3 text-[13px] font-semibold">
          <span>Name</span><span>Email</span><span>Phone</span><span>Submitted at</span>
        </div>
        <div class="p-4">
          <EmptyStateInline Icon={UserRound} title="No captured leads yet" description="Lead rows are created only from live chatbot lead-collection submissions." />
        </div>
      </div>
    </div>
  )
}
