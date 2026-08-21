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
} from '@/shared/icons'
import { Button } from '@/shared/ui/Button'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'
import { cn } from '@/shared/lib/cn'
import type { ChatbotAddOnId } from '@/features/agents/lib/agent-roles'
import type { SupportIntegrationStatus } from '@/features/agents/lib/use-chatbot-support-status'
import { ToggleSwitch } from '@/features/agents/components/ChatbotStudioCards'
import { DesignPreviewBadge } from '@/features/agents/components/DesignPreviewBadge'
import {
  PlaygroundAccordion,
  SettingInput,
  SettingTextarea,
  SupportIntegrationBanner,
} from '@/features/agents/components/ChatbotStudioPrimitives'
import { chatbotDisplayName } from '@/features/agents/lib/verevon-chatbot-studio-data'
import { useI18n } from '@/shared/i18n'

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
    <div class="verevon-page-surface h-full min-h-0 overflow-y-auto xl:overflow-hidden">
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
  const i18n = useI18n()
  const actionEnabled = () => props.visibleAddOns.has('subscription-action')

  return (
    <section class="verevon-sidebar-type verevon-panel flex h-[660px] min-h-0 flex-col overflow-hidden text-[#1D1D1F] xl:h-full dark:text-[#F7F8F8]">
      <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-5 pt-5">
        <div class="flex flex-wrap items-center gap-3">
          <h1 class="text-[24px] font-semibold leading-8 tracking-normal text-[#0F1011] dark:text-white">
            {i18n.tr('Playground', 'Playground')}
          </h1>
          <DesignPreviewBadge />
        </div>
        <SupportIntegrationBanner status={props.supportStatus} />

        <div class="mt-6 space-y-3">
          <PlaygroundAccordion defaultOpen Icon={Box} title={i18n.tr('AI-innstillinger', 'AI Settings')}>
            <div class="rounded-[9px] bg-[#FAFAFA] px-4 py-3 dark:bg-[#111216]">
              <div class="flex items-center gap-2 text-[14px] font-semibold text-[#12944B]">
                <span class="size-2 rounded-full bg-[#0BA95B]" />
                {i18n.tr('Kjøretid klar', 'Runtime ready')}
              </div>
              <p class="mt-2 text-[13px] font-medium text-[#767676] dark:text-[#AEB4C0]">
                {i18n.tr('Treningsstatus oppdateres når ekte kilder eller finjusteringsdatasett kobles til.', 'Training state updates after real sources or fine-tuning datasets are connected.')}
              </p>
            </div>
            <div class="mt-3 flex h-11 items-center justify-between rounded-[9px] border border-[#E8E8EA] bg-white px-3 dark:border-[#2A2C31] dark:bg-[#15161A]">
              <span class="text-[13px] font-medium text-[#67686D] dark:text-[#D7DCE4]">{i18n.tr('Sammenlign AI-modeller', 'Compare AI models')}</span>
              {/* Phase 4 honesty sweep: model comparison has no backend yet. */}
              <Button size="xs" shape="rounded" disabled>
                {i18n.tr('Sammenlign', 'Compare')}
              </Button>
            </div>
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Sparkles} title={i18n.tr('Modellvelger', 'Model Selector')}>
            <label for="chatbot-model" class="block text-[13px] font-medium text-[#5F6067] dark:text-[#C6CCD6]">
              {i18n.tr('Modell', 'Model')}
            </label>
            <div class="relative mt-2">
              <VerevonSelect
                id="chatbot-model"
                value="gpt-5"
                class="appearance-none pl-10 pr-9 text-[14px] font-semibold text-[#202126] shadow-sm dark:text-white"
              >
                <option value="gpt-5">GPT-5</option>
                <option value="gpt-5-mini">GPT-5 mini</option>
                <option value="gpt-4.1">GPT-4.1</option>
              </VerevonSelect>
              <Sparkles class="pointer-events-none absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-[#1D1D1F] dark:text-white" />
              <ChevronDown class="pointer-events-none absolute right-4 top-1/2 size-3.5 -translate-y-1/2 text-[#8E949E]" />
            </div>
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Wrench} title={i18n.tr('AI-verktøy', 'AI Tools')}>
            <div class="flex items-center justify-between gap-3">
              <h2 class="text-[13px] font-medium text-[#5F6067] dark:text-[#C6CCD6]">{i18n.tr('Verktøysett', 'Tool pool')}</h2>
              <button type="button" onClick={() => props.onClearCanvas()} class="text-[12px] font-semibold text-[#8B8F98] transition-colors hover:text-[#1D1D1F] dark:hover:text-white">
                {i18n.tr('Tøm', 'Clear')}
              </button>
            </div>
            <button
              type="button"
              aria-label={i18n.tr('Velg tillegg for abonnementsoppdatering', 'Select update subscription add-on')}
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
                {actionEnabled() ? i18n.tr('1 verktøy aktivert', '1 Tool Enabled') : i18n.tr('Verktøy klart til å aktiveres', 'Tool ready to enable')}
              </span>
              <ChevronDown class="-rotate-90 text-[#111111] dark:text-white" />
            </button>
            {actionEnabled()
              ? (
                <button type="button" onClick={() => props.onRemoveSelected()} class="mt-2 text-[12px] font-semibold text-[#9A4A32] transition-colors hover:text-[#6E2E1C] dark:text-[#F0A08A]">
                  {i18n.tr('Fjern valgt verktøy', 'Remove selected tool')}
                </button>
              )
              : null}
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={UserRound} title={i18n.tr('Lead-innsamling', 'Lead Collections')}>
            <SettingInput label={i18n.tr('Tittel på lead-skjema', 'Lead form title')} value={i18n.tr('Snakk med salg', 'Talk to sales')} />
            <SettingInput label={i18n.tr('Påkrevde felt', 'Required fields')} value={i18n.tr('Navn, e-post, firma', 'Name, email, company')} />
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Table2} title={i18n.tr('Meldinger', 'Messages')}>
            <SettingInput label={i18n.tr('Inndataplassholder', 'Input placeholder')} value={i18n.tr('Skriv meldingen din…', 'Type your message…')} />
            <SettingTextarea
              label={i18n.tr('Foreslåtte spørsmål', 'Suggested queries')}
              value={i18n.tr('Hva kan du hjelpe med?\nHvordan fungerer dette?', 'What can you help with?\nHow does this work?')}
            />
            <SettingTextarea
              label={i18n.tr('Innledende melding', 'Initial Message')}
              value={i18n.tr('Hei! Jeg er en AI-assistent.\nHvordan kan jeg hjelpe deg i dag?', 'Hi! I am an AI Assistant.\nHow can I help you today?')}
            />
            <div class="mt-4 flex items-center justify-between gap-3">
              <span class="text-[14px] font-medium text-[#202126] dark:text-white">{i18n.tr('Forhåndsvisning av innledende melding', 'Tease Initial Messages')}</span>
              <ToggleSwitch enabled label={i18n.tr('Forhåndsvisning av innledende melding', 'Tease initial messages')} />
            </div>
            <SettingInput compact label={i18n.tr('Forsinkelse (sekunder)', 'Delay (seconds)')} value="3" />
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={FileText} title={i18n.tr('Instruksjoner', 'Instructions')}>
            <div class="flex gap-2">
              {/* Phase 4 honesty sweep: instruction-set switcher is preview-only. */}
              <Button shape="rounded" size="md" disabled class="min-w-0 flex-1 justify-between">
                {i18n.tr('Grunninstruksjoner', 'Base Instructions')}
                <ChevronDown class="size-3.5 text-[#9EA3AA]" />
              </Button>
              <VerevonIconButton size="lg" shape="rounded" onClick={() => props.onResetCanvas()} aria-label={i18n.tr('Tilbakestill playground-instruksjoner', 'Reset playground instructions')} class="shrink-0">
                <RotateCcw class="size-4" />
              </VerevonIconButton>
            </div>
            <VerevonTextarea
              aria-label={i18n.tr('Systemprompt for instruksjoner', 'Instructions system prompt')}
              value={i18n.tr(
                `Rolle: Du er Verevon Design Concierge, en ekspert på UI/UX-mønstre, kundeautomatisering og produktstrategi. Oppdraget ditt er å hjelpe team med å finne akkurat det svaret, den arbeidsflyten eller kilden de trenger.

Stemme og tone:
- Kuratert og sofistikert: Bruk klart produktspråk og praktiske anbefalinger.
- Konsis først: Start med svaret, og legg til detaljer ved behov.
- Verktøybevisst: Bruk aktiverte verktøy kun etter eksplisitt bekreftelse.`,
                `Role: You are the Verevon Design Concierge, an expert in UI/UX patterns, customer automation, and product strategy. Your mission is to help teams find the exact answer, workflow, or source they need.

Voice & Tone:
- Curated & sophisticated: Use clear product language and practical recommendations.
- Concise first: Start with the answer, then add details when needed.
- Tool aware: Use enabled tools only after explicit confirmation.`,
              )}
              class="mt-4 min-h-[240px]"
            />
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={MessageSquare} title={i18n.tr('Chattevindu', 'Chat Window')}>
            <SettingInput label={i18n.tr('Vindutittel', 'Window title')} value={chatbotDisplayName} />
            <SettingInput label={i18n.tr('Merkevarefarge', 'Brand color')} value="#111111" />
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={MessageCircle} title={i18n.tr('Chatteboble', 'Chat Bubble')}>
            <SettingInput label={i18n.tr('Bobleposisjon', 'Bubble position')} value={i18n.tr('Nederst til høyre', 'Bottom right')} />
            <SettingInput label={i18n.tr('Bobletekst', 'Bubble label')} value={i18n.tr('Spør AI', 'Ask AI')} />
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={Globe2} title={i18n.tr('Kontekster', 'Contexts')}>
            <SettingInput label={i18n.tr('Standard språk', 'Default locale')} value={i18n.tr('Engelsk', 'English')} />
            <SettingInput
              label={i18n.tr('Tilkoblet kontekst', 'Connected context')}
              value={i18n.tr('Kundeprofil, abonnement, siste sak', 'Customer profile, subscription, last ticket')}
            />
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
  const i18n = useI18n()

  return (
    <section class="verevon-panel verevon-panel-strong relative flex min-h-[600px] min-w-0 overflow-hidden text-[#111111] xl:min-h-0 dark:text-white">
      <div
        aria-hidden="true"
        class="absolute inset-0 bg-[radial-gradient(circle_at_2px_2px,rgba(38,42,48,0.11)_1.6px,transparent_0)] [background-size:28px_28px] dark:bg-[radial-gradient(circle_at_2px_2px,rgba(255,255,255,0.11)_1.6px,transparent_0)]"
      />
      <div class="relative z-10 flex min-h-full w-full items-center justify-center px-6 py-8">
        <ChatbotDevice selectedAddOn={props.selectedAddOn} supportStatus={props.supportStatus} />
      </div>
      {/* Phase 4 honesty sweep: preview-only widget launcher (no live widget). */}
      <button
        type="button"
        disabled
        aria-label={i18n.tr('Åpne chatbot-widget', 'Open chatbot widget')}
        class="absolute bottom-5 right-5 z-20 grid size-[52px] place-items-center rounded-full bg-[#111111] text-white shadow-[0_12px_34px_rgba(0,0,0,0.22)] transition-transform hover:scale-[1.03] disabled:cursor-not-allowed"
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
  const i18n = useI18n()
  const placeholder = () => props.selectedAddOn === 'subscription-action' ? i18n.tr('Melding…', 'Message…') : i18n.tr('Still et spørsmål…', 'Ask a question…')
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
          disabled
          aria-label={i18n.tr('Oppdater chatbot-forhåndsvisning', 'Refresh chatbot preview')}
          class="grid size-9 shrink-0 place-items-center rounded-[9px] text-[#4F5661] transition-colors hover:bg-[#F4F5F7] disabled:cursor-not-allowed dark:text-[#D7DCE4] dark:hover:bg-[#202229]"
        >
          <RefreshCw class="size-4" strokeWidth={1.9} />
        </button>
      </div>

      <div class="flex min-h-0 flex-1 flex-col px-5 pb-4 pt-6">
        <div class="w-max max-w-[78%] rounded-[18px] bg-[#F4F4F5] px-4 py-2.5 text-[14px] leading-5 text-[#35383F] dark:bg-[#202229] dark:text-[#E8ECF2]">
          {i18n.tr(
            'Hei. Jeg kan svare basert på godkjente kilder og overlevere til supportteamet ditt når saken trenger et menneske.',
            'Hi. I can answer from approved sources and hand off to your support team when the case needs a person.',
          )}
        </div>
        <div class="mt-4 w-max max-w-[86%] rounded-[14px] border border-[#E5E6EA] bg-white px-4 py-2 text-[12px] font-semibold leading-5 text-[#5F6673] dark:border-[#303238] dark:bg-[#15161A] dark:text-[#C6CCD6]">
          {connected()
            ? i18n.tr(
                `Live handlinger tilgjengelig: ${props.supportStatus.agents} agenter, ${props.supportStatus.groups} grupper, ${props.supportStatus.macros} makroer.`,
                `Live actions available: ${props.supportStatus.agents} agents, ${props.supportStatus.groups} groups, ${props.supportStatus.macros} macros.`,
              )
            : i18n.tr('Koble til support-core/Zammad før du aktiverer live sakshandlinger.', 'Connect support-core/Zammad before enabling live ticket actions.')}
        </div>
        <div class="mt-auto pb-3 text-center text-[12px] font-medium text-[#B1B3B9]">
          <span class="mr-1 inline-grid size-4 place-items-center rounded-[4px] bg-[#AEB0B7] text-[10px] font-bold text-white">V</span>
          {i18n.tr('Drevet av Verevon', 'Powered by Verevon')}
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
