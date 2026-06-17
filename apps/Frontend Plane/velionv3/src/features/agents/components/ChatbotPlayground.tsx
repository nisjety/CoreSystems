import {
  Box,
  ChevronDown,
  FileText,
  Globe2,
  MessageCircle,
  MessageSquare,
  Mic,
  RefreshCw,
  RotateCcw,
  Send,
  Sparkles,
  Table2,
  UserRound,
  Wrench,
  Zap,
} from 'lucide-solid'
import { Button } from '@/shared/ui/Button'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { VelionSelect } from '@/shared/ui/velion/VelionSelect'
import { VelionTextarea } from '@/shared/ui/velion/VelionTextarea'
import { cn } from '@/shared/lib/cn'
import type { ChatbotAddOnId } from '@/features/agents/lib/agent-roles'
import type { SupportIntegrationStatus } from '@/features/agents/lib/use-chatbot-support-status'
import { ToggleSwitch } from '@/features/agents/components/ChatbotStudioCards'
import {
  PlaygroundAccordion,
  SettingInput,
  SettingTextarea,
  SupportIntegrationBanner,
} from '@/features/agents/components/ChatbotStudioPrimitives'
import { chatbotDisplayName } from '@/features/agents/lib/velion-chatbot-studio-data'

export function ChatbotPlaygroundSurface(props: {
  onAddOnSelect: (addOn: ChatbotAddOnId) => void
  onAddSelected: () => void
  onClearCanvas: () => void
  onRemoveSelected: () => void
  onResetCanvas: () => void
  selectedAddOn: ChatbotAddOnId
  supportStatus: SupportIntegrationStatus
  visibleAddOns: ReadonlySet<ChatbotAddOnId>
}) {
  return (
    <div class="velion-page-surface h-full min-h-0 overflow-y-auto xl:overflow-hidden">
      <div class="grid min-h-full grid-cols-1 gap-2 p-2 xl:h-full xl:min-h-0 xl:grid-cols-[360px_minmax(0,1fr)]">
        <PlaygroundSettingsPanel
          onAddOnSelect={props.onAddOnSelect}
          onAddSelected={props.onAddSelected}
          onClearCanvas={props.onClearCanvas}
          onRemoveSelected={props.onRemoveSelected}
          onResetCanvas={props.onResetCanvas}
          selectedAddOn={props.selectedAddOn}
          supportStatus={props.supportStatus}
          visibleAddOns={props.visibleAddOns}
        />
        <PlaygroundBotPanel selectedAddOn={props.selectedAddOn} supportStatus={props.supportStatus} />
      </div>
    </div>
  )
}

function PlaygroundSettingsPanel(props: {
  onAddOnSelect: (addOn: ChatbotAddOnId) => void
  onAddSelected: () => void
  onClearCanvas: () => void
  onRemoveSelected: () => void
  onResetCanvas: () => void
  selectedAddOn: ChatbotAddOnId
  supportStatus: SupportIntegrationStatus
  visibleAddOns: ReadonlySet<ChatbotAddOnId>
}) {
  const actionEnabled = () => props.visibleAddOns.has('subscription-action')

  return (
    <section class="velion-sidebar-type velion-panel flex h-[660px] min-h-0 flex-col overflow-hidden text-[#1D1D1F] xl:h-full dark:text-[#F7F8F8]">
      <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-5 pt-5">
        <h1 class="text-[24px] font-semibold leading-8 tracking-normal text-[#0F1011] dark:text-white">
          Playground
        </h1>
        <SupportIntegrationBanner status={props.supportStatus} />

        <div class="mt-6 space-y-3">
          <PlaygroundAccordion defaultOpen Icon={Box} title="AI Settings">
            <div class="rounded-[9px] bg-[#FAFAFA] px-4 py-3 dark:bg-[#111216]">
              <div class="flex items-center gap-2 text-[14px] font-semibold text-[#12944B]">
                <span class="size-2 rounded-full bg-[#0BA95B]" />
                Runtime ready
              </div>
              <p class="mt-2 text-[13px] font-medium text-[#767676] dark:text-[#AEB4C0]">
                Training state updates after real sources or fine-tuning datasets are connected.
              </p>
            </div>
            <div class="mt-3 flex h-11 items-center justify-between rounded-[9px] border border-[#E8E8EA] bg-white px-3 dark:border-[#2A2C31] dark:bg-[#15161A]">
              <span class="text-[13px] font-medium text-[#67686D] dark:text-[#D7DCE4]">Compare AI models</span>
              <Button size="xs" shape="rounded">
                Compare
              </Button>
            </div>
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Sparkles} title="Model Selector">
            <label for="chatbot-model" class="block text-[13px] font-medium text-[#5F6067] dark:text-[#C6CCD6]">
              Model
            </label>
            <div class="relative mt-2">
              <VelionSelect
                id="chatbot-model"
                value="gpt-5"
                class="appearance-none pl-10 pr-9 text-[14px] font-semibold text-[#202126] shadow-sm dark:text-white"
              >
                <option value="gpt-5">GPT-5</option>
                <option value="gpt-5-mini">GPT-5 mini</option>
                <option value="gpt-4.1">GPT-4.1</option>
              </VelionSelect>
              <Sparkles class="pointer-events-none absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-[#1D1D1F] dark:text-white" />
              <ChevronDown class="pointer-events-none absolute right-4 top-1/2 size-3.5 -translate-y-1/2 text-[#8E949E]" />
            </div>
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Wrench} title="AI Tools">
            <div class="flex items-center justify-between gap-3">
              <h2 class="text-[13px] font-medium text-[#5F6067] dark:text-[#C6CCD6]">Tool pool</h2>
              <button type="button" onClick={() => props.onClearCanvas()} class="text-[12px] font-semibold text-[#8B8F98] transition-colors hover:text-[#1D1D1F] dark:hover:text-white">
                Clear
              </button>
            </div>
            <button
              type="button"
              aria-label="Select update subscription add-on"
              onClick={() => props.onAddOnSelect('subscription-action')}
              class={cn(
                'mt-3 flex h-12 w-full items-center gap-3 rounded-[9px] border px-3 text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#111111]',
                props.selectedAddOn === 'subscription-action'
                  ? 'border-[#17181C] bg-white shadow-sm dark:border-white dark:bg-[#202229]'
                  : 'border-[#E1E2E6] bg-white hover:bg-[#FAFAFB] dark:border-[#303238] dark:bg-[#111216] dark:hover:bg-[#202229]',
              )}
            >
              <span class="grid size-8 shrink-0 place-items-center rounded-full border border-[#E8E8EA] bg-white text-[#111111] dark:border-[#303238] dark:bg-[#15161A]">
                <Zap class="size-3.5" />
              </span>
              <span class="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#202126] dark:text-white">
                {actionEnabled() ? '1 Tool Enabled' : 'Tool ready to enable'}
              </span>
              <ChevronDown class="-rotate-90 text-[#111111] dark:text-white" />
            </button>
            {actionEnabled()
              ? (
                <button type="button" onClick={() => props.onRemoveSelected()} class="mt-2 text-[12px] font-semibold text-[#9A4A32] transition-colors hover:text-[#6E2E1C] dark:text-[#F0A08A]">
                  Remove selected tool
                </button>
              )
              : null}
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={UserRound} title="Lead Collections">
            <SettingInput label="Lead form title" value="Talk to sales" />
            <SettingInput label="Required fields" value="Name, email, company" />
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Table2} title="Messages">
            <SettingInput label="Input placeholder" value="Type your message…" />
            <SettingTextarea label="Suggested queries" value={'What can you help with?\nHow does this work?'} />
            <SettingTextarea label="Initial Message" value={'Hi! I am an AI Assistant.\nHow can I help you today?'} />
            <div class="mt-4 flex items-center justify-between gap-3">
              <span class="text-[14px] font-medium text-[#202126] dark:text-white">Tease Initial Messages</span>
              <ToggleSwitch enabled label="Tease initial messages" />
            </div>
            <SettingInput compact label="Delay (seconds)" value="3" />
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={FileText} title="Instructions">
            <div class="flex gap-2">
              <Button shape="rounded" size="md" class="min-w-0 flex-1 justify-between">
                Base Instructions
                <ChevronDown class="size-3.5 text-[#9EA3AA]" />
              </Button>
              <VelionIconButton size="lg" shape="rounded" onClick={() => props.onResetCanvas()} aria-label="Reset playground instructions" class="shrink-0">
                <RotateCcw class="size-4" />
              </VelionIconButton>
            </div>
            <VelionTextarea
              aria-label="Instructions system prompt"
              value={`Role: You are the Velion Design Concierge, an expert in UI/UX patterns, customer automation, and product strategy. Your mission is to help teams find the exact answer, workflow, or source they need.

Voice & Tone:
- Curated & sophisticated: Use clear product language and practical recommendations.
- Concise first: Start with the answer, then add details when needed.
- Tool aware: Use enabled tools only after explicit confirmation.`}
              class="mt-4 min-h-[240px]"
            />
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={MessageSquare} title="Chat Window">
            <SettingInput label="Window title" value={chatbotDisplayName} />
            <SettingInput label="Brand color" value="#111111" />
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={MessageCircle} title="Chat Bubble">
            <SettingInput label="Bubble position" value="Bottom right" />
            <SettingInput label="Bubble label" value="Ask AI" />
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={Globe2} title="Contexts">
            <SettingInput label="Default locale" value="English" />
            <SettingInput label="Connected context" value="Customer profile, subscription, last ticket" />
          </PlaygroundAccordion>
        </div>
      </div>
    </section>
  )
}

function PlaygroundBotPanel(props: {
  selectedAddOn: ChatbotAddOnId
  supportStatus: SupportIntegrationStatus
}) {
  return (
    <section class="velion-panel velion-panel-strong relative flex min-h-[600px] min-w-0 overflow-hidden text-[#111111] xl:min-h-0 dark:text-white">
      <div
        aria-hidden="true"
        class="absolute inset-0 bg-[radial-gradient(circle_at_2px_2px,rgba(38,42,48,0.11)_1.6px,transparent_0)] [background-size:28px_28px] dark:bg-[radial-gradient(circle_at_2px_2px,rgba(255,255,255,0.11)_1.6px,transparent_0)]"
      />
      <div class="relative z-10 flex min-h-full w-full items-center justify-center px-6 py-8">
        <ChatbotDevice selectedAddOn={props.selectedAddOn} supportStatus={props.supportStatus} />
      </div>
      <button
        type="button"
        aria-label="Open chatbot widget"
        class="absolute bottom-5 right-5 z-20 grid size-[52px] place-items-center rounded-full bg-[#111111] text-white shadow-[0_12px_34px_rgba(0,0,0,0.22)] transition-transform hover:scale-[1.03]"
      >
        <Sparkles class="size-5" />
      </button>
    </section>
  )
}

function ChatbotDevice(props: {
  selectedAddOn: ChatbotAddOnId
  supportStatus: SupportIntegrationStatus
}) {
  const placeholder = () => props.selectedAddOn === 'subscription-action' ? 'Message…' : 'Ask a question…'
  const connected = () => props.supportStatus.status === 'connected'

  return (
    <div class="flex h-[min(68vh,640px)] min-h-[500px] w-full max-w-[440px] flex-col overflow-hidden rounded-[20px] border border-[#E6E7EB] bg-white shadow-[0_18px_56px_rgba(31,35,42,0.10)] dark:border-[#2A2C31] dark:bg-[#111216]">
      <div class="flex h-16 shrink-0 items-center justify-between border-b border-[#EFF0F2] px-5 dark:border-[#292B31]">
        <div class="flex min-w-0 items-center gap-3">
          <span class="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white">
            <span class="size-0 border-b-[11px] border-l-[7px] border-r-[7px] border-b-white border-l-transparent border-r-transparent" />
          </span>
          <h2 class="truncate text-[14px] font-semibold text-[#17181C] dark:text-white">
            {chatbotDisplayName}
          </h2>
        </div>
        <button
          type="button"
          aria-label="Refresh chatbot preview"
          class="grid size-9 shrink-0 place-items-center rounded-[9px] text-[#4F5661] transition-colors hover:bg-[#F4F5F7] dark:text-[#D7DCE4] dark:hover:bg-[#202229]"
        >
          <RefreshCw class="size-4" strokeWidth={1.9} />
        </button>
      </div>

      <div class="flex min-h-0 flex-1 flex-col px-5 pb-4 pt-6">
        <div class="w-max max-w-[78%] rounded-[18px] bg-[#F4F4F5] px-4 py-2.5 text-[14px] leading-5 text-[#35383F] dark:bg-[#202229] dark:text-[#E8ECF2]">
          Hi. I can answer from approved sources and hand off to your support team when the case needs a person.
        </div>
        <div class="mt-4 w-max max-w-[86%] rounded-[14px] border border-[#E5E6EA] bg-white px-4 py-2 text-[12px] font-semibold leading-5 text-[#5F6673] dark:border-[#303238] dark:bg-[#15161A] dark:text-[#C6CCD6]">
          {connected()
            ? `Live actions available: ${props.supportStatus.agents} agents, ${props.supportStatus.groups} groups, ${props.supportStatus.macros} macros.`
            : 'Connect support-core/Zammad before enabling live ticket actions.'}
        </div>
        <div class="mt-auto pb-3 text-center text-[12px] font-medium text-[#B1B3B9]">
          <span class="mr-1 inline-grid size-4 place-items-center rounded-[4px] bg-[#AEB0B7] text-[10px] font-bold text-white">V</span>
          Powered by Velion
        </div>
        <div class="flex h-12 items-center gap-2 rounded-full border border-[#E2E3E8] bg-white px-4 text-[14px] text-[#A1A5AE] shadow-[0_8px_24px_rgba(31,35,42,0.08)] dark:border-[#303238] dark:bg-[#17181C]">
          {placeholder()}
          <Mic class="ml-auto size-4 shrink-0 text-[#7D828C]" />
          <span class="grid size-8 shrink-0 place-items-center rounded-full bg-[#F4F5F7] text-[#D0D3D9] dark:bg-[#202229]">
            <Send class="size-3.5" />
          </span>
        </div>
      </div>
    </div>
  )
}
