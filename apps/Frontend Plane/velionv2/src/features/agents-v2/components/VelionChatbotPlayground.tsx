"use client";

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
} from "lucide-react";
import {
  VerevonButton,
  VerevonIconButton,
  VerevonSelect,
  VerevonTextarea,
} from "@/components/ui/verevon-ui";
import type { ChatbotAddOnId } from "@/features/agents-v2/lib/agent-roles";
import type { SupportIntegrationStatus } from "@/features/agents-v2/lib/use-chatbot-support-status";
import { ToggleSwitch } from "@/features/agents-v2/components/VerevonChatbotStudioCards";
import {
  PlaygroundAccordion,
  SettingInput,
  SettingTextarea,
  SupportIntegrationBanner,
} from "@/features/agents-v2/components/VerevonChatbotStudioPrimitives";
import {
  chatbotDisplayName,
} from "@/features/agents-v2/lib/verevon-chatbot-studio-data";
import { cn } from "@/lib/utils";

export function ChatbotPlaygroundSurface({
  onAddOnSelect,
  onClearCanvas,
  onRemoveSelected,
  onResetCanvas,
  selectedAddOn,
  supportStatus,
  visibleAddOns,
}: {
  onAddOnSelect: (addOn: ChatbotAddOnId) => void;
  onAddSelected: () => void;
  onClearCanvas: () => void;
  onRemoveSelected: () => void;
  onResetCanvas: () => void;
  selectedAddOn: ChatbotAddOnId;
  supportStatus: SupportIntegrationStatus;
  visibleAddOns: Set<ChatbotAddOnId>;
}) {
  return (
    <div className="verevon-page-surface h-full min-h-0 overflow-y-auto xl:overflow-hidden">
      <div className="grid min-h-full grid-cols-1 gap-2 p-2 xl:h-full xl:min-h-0 xl:grid-cols-[360px_minmax(0,1fr)]">
        <PlaygroundSettingsPanel
          onAddOnSelect={onAddOnSelect}
          onClearCanvas={onClearCanvas}
          onRemoveSelected={onRemoveSelected}
          onResetCanvas={onResetCanvas}
          selectedAddOn={selectedAddOn}
          supportStatus={supportStatus}
          visibleAddOns={visibleAddOns}
        />
        <PlaygroundBotPanel selectedAddOn={selectedAddOn} supportStatus={supportStatus} />
      </div>
    </div>
  );
}

function PlaygroundSettingsPanel({
  onAddOnSelect,
  onClearCanvas,
  onRemoveSelected,
  onResetCanvas,
  selectedAddOn,
  supportStatus,
  visibleAddOns,
}: {
  onAddOnSelect: (addOn: ChatbotAddOnId) => void;
  onClearCanvas: () => void;
  onRemoveSelected: () => void;
  onResetCanvas: () => void;
  selectedAddOn: ChatbotAddOnId;
  supportStatus: SupportIntegrationStatus;
  visibleAddOns: Set<ChatbotAddOnId>;
}) {
  const actionEnabled = visibleAddOns.has("subscription-action");

  return (
    <section className="verevon-sidebar-type verevon-panel flex h-[660px] min-h-0 flex-col overflow-hidden text-[#1D1D1F] xl:h-full dark:text-[#F7F8F8]">
      <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-5 pt-5">
        <h1 className="text-[24px] font-semibold leading-8 tracking-normal text-[#0F1011] dark:text-white">
          Playground
        </h1>
        <SupportIntegrationBanner status={supportStatus} />

        <div className="mt-6 space-y-3">
          <PlaygroundAccordion defaultOpen Icon={Box} title="AI Settings">
            <div className="rounded-[9px] bg-[#FAFAFA] px-4 py-3 dark:bg-[#111216]">
              <div className="flex items-center gap-2 text-[14px] font-semibold text-[#12944B]">
                <span className="size-2 rounded-full bg-[#0BA95B]" />
                Runtime ready
              </div>
              <p className="mt-2 text-[13px] font-medium text-[#767676] dark:text-[#AEB4C0]">
                Training state updates after real sources or fine-tuning datasets are connected.
              </p>
            </div>
            <div className="mt-3 flex h-11 items-center justify-between rounded-[9px] border border-[#E8E8EA] bg-white px-3 dark:border-[#2A2C31] dark:bg-[#15161A]">
              <span className="text-[13px] font-medium text-[#67686D] dark:text-[#D7DCE4]">Compare AI models</span>
              <VerevonButton size="xs" radius="sm" className="px-3 text-[12px] font-semibold">
                Compare
              </VerevonButton>
            </div>
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Sparkles} title="Model Selector">
            <label htmlFor="chatbot-model" className="block text-[13px] font-medium text-[#5F6067] dark:text-[#C6CCD6]">
              Model
            </label>
            <div className="relative mt-2">
              <VerevonSelect
                id="chatbot-model"
                defaultValue="gpt-5"
                variant="compact"
                className="appearance-none pl-10 pr-9 text-[14px] font-semibold text-[#202126] shadow-sm dark:text-white"
              >
                <option value="gpt-5">GPT-5</option>
                <option value="gpt-5-mini">GPT-5 mini</option>
                <option value="gpt-4.1">GPT-4.1</option>
              </VerevonSelect>
              <Sparkles className="pointer-events-none absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-[#1D1D1F] dark:text-white" />
              <ChevronDown className="pointer-events-none absolute right-4 top-1/2 size-3.5 -translate-y-1/2 text-[#8E949E]" />
            </div>
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Wrench} title="AI Tools">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-[13px] font-medium text-[#5F6067] dark:text-[#C6CCD6]">Tool pool</h2>
              <button type="button" onClick={onClearCanvas} className="text-[12px] font-semibold text-[#8B8F98] transition-colors hover:text-[#1D1D1F] dark:hover:text-white">
                Clear
              </button>
            </div>
            <button
              type="button"
              aria-label="Select update subscription add-on"
              onClick={() => onAddOnSelect("subscription-action")}
              className={cn(
                "mt-3 flex h-12 w-full items-center gap-3 rounded-[9px] border px-3 text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]",
                selectedAddOn === "subscription-action"
                  ? "border-[#17181C] bg-white shadow-sm dark:border-white dark:bg-[#202229]"
                  : "border-[#E1E2E6] bg-white hover:bg-[#FAFAFB] dark:border-[#303238] dark:bg-[#111216] dark:hover:bg-[#202229]",
              )}
            >
              <span className="grid size-8 shrink-0 place-items-center rounded-full border border-[#E8E8EA] bg-white text-[#635BFF] dark:border-[#303238] dark:bg-[#15161A]">
                <Zap className="size-3.5" />
              </span>
              <span className="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#202126] dark:text-white">
                {actionEnabled ? "1 Tool Enabled" : "Tool ready to enable"}
              </span>
              <ChevronDown className="-rotate-90 text-[#111111] dark:text-white" />
            </button>
            {actionEnabled ? (
              <button type="button" onClick={onRemoveSelected} className="mt-2 text-[12px] font-semibold text-[#9A4A32] transition-colors hover:text-[#6E2E1C] dark:text-[#F0A08A]">
                Remove selected tool
              </button>
            ) : null}
          </PlaygroundAccordion>

          <PlaygroundAccordion Icon={UserRound} title="Lead Collections">
            <SettingInput label="Lead form title" value="Talk to sales" />
            <SettingInput label="Required fields" value="Name, email, company" />
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={Table2} title="Messages">
            <SettingInput label="Input placeholder" value="Type your message…" />
            <SettingTextarea label="Suggested queries" value={"What can you help with?\nHow does this work?"} />
            <SettingTextarea label="Initial Message" value={"Hi! I am an AI Assistant.\nHow can I help you today?"} />
            <div className="mt-4 flex items-center justify-between gap-3">
              <span className="text-[14px] font-medium text-[#202126] dark:text-white">Tease Initial Messages</span>
              <ToggleSwitch enabled />
            </div>
            <SettingInput compact label="Delay (seconds)" value="3" />
          </PlaygroundAccordion>

          <PlaygroundAccordion defaultOpen Icon={FileText} title="Instructions">
            <div className="flex gap-2">
              <VerevonButton radius="sm" className="min-w-0 flex-1 justify-between px-3 text-[13px] font-medium text-[#202126] dark:text-white">
                Base Instructions
                <ChevronDown className="size-3.5 text-[#9EA3AA]" />
              </VerevonButton>
              <VerevonIconButton size="lg" radius="sm" onClick={onResetCanvas} aria-label="Reset playground instructions" className="shrink-0 border border-[#E1E2E6] bg-white dark:border-[#303238] dark:bg-[#111216]">
                <RotateCcw className="size-4" />
              </VerevonIconButton>
            </div>
            <VerevonTextarea
              aria-label="Instructions system prompt"
              defaultValue={`Role: You are the Verevon Design Concierge, an expert in UI/UX patterns, customer automation, and product strategy. Your mission is to help teams find the exact answer, workflow, or source they need.

Voice & Tone:
- Curated & sophisticated: Use clear product language and practical recommendations.
- Concise first: Start with the answer, then add details when needed.
- Tool aware: Use enabled tools only after explicit confirmation.`}
              variant="compact"
              className="verevon-textarea-large mt-4"
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
  );
}

function PlaygroundBotPanel({
  selectedAddOn,
  supportStatus,
}: {
  selectedAddOn: ChatbotAddOnId;
  supportStatus: SupportIntegrationStatus;
}) {
  return (
    <section className="verevon-panel verevon-panel-strong relative flex min-h-[600px] min-w-0 overflow-hidden text-[#111111] xl:min-h-0 dark:text-white">
      <div
        aria-hidden="true"
        className="absolute inset-0 bg-[radial-gradient(circle_at_2px_2px,rgba(38,42,48,0.11)_1.6px,transparent_0)] [background-size:28px_28px] dark:bg-[radial-gradient(circle_at_2px_2px,rgba(255,255,255,0.11)_1.6px,transparent_0)]"
      />
      <div className="relative z-10 flex min-h-full w-full items-center justify-center px-6 py-8">
        <ChatbotDevice selectedAddOn={selectedAddOn} supportStatus={supportStatus} />
      </div>
      <button
        type="button"
        aria-label="Open chatbot widget"
        className="absolute bottom-5 right-5 z-20 grid size-[52px] place-items-center rounded-full bg-[#111111] text-white shadow-[0_12px_34px_rgba(0,0,0,0.22)] transition-transform hover:scale-[1.03]"
      >
        <Sparkles className="size-5" />
      </button>
    </section>
  );
}

function ChatbotDevice({
  selectedAddOn,
  supportStatus,
}: {
  selectedAddOn: ChatbotAddOnId;
  supportStatus: SupportIntegrationStatus;
}) {
  const placeholder = selectedAddOn === "subscription-action" ? "Message…" : "Ask a question…";
  const connected = supportStatus.status === "connected";

  return (
    <div className="flex h-[min(68vh,640px)] min-h-[500px] w-full max-w-[440px] flex-col overflow-hidden rounded-[20px] border border-[#E6E7EB] bg-white shadow-[0_18px_56px_rgba(31,35,42,0.10)] dark:border-[#2A2C31] dark:bg-[#111216]">
      <div className="flex h-16 shrink-0 items-center justify-between border-b border-[#EFF0F2] px-5 dark:border-[#292B31]">
        <div className="flex min-w-0 items-center gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white">
            <span className="size-0 border-b-[11px] border-l-[7px] border-r-[7px] border-b-white border-l-transparent border-r-transparent" />
          </span>
          <h2 className="truncate text-[14px] font-semibold text-[#17181C] dark:text-white">
            {chatbotDisplayName}
          </h2>
        </div>
        <button
          type="button"
          aria-label="Refresh chatbot preview"
          className="grid size-9 shrink-0 place-items-center rounded-[9px] text-[#4F5661] transition-colors hover:bg-[#F4F5F7] dark:text-[#D7DCE4] dark:hover:bg-[#202229]"
        >
          <RefreshCw className="size-4" strokeWidth={1.9} />
        </button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col px-5 pb-4 pt-6">
        <div className="w-max max-w-[78%] rounded-[18px] bg-[#F4F4F5] px-4 py-2.5 text-[14px] leading-5 text-[#35383F] dark:bg-[#202229] dark:text-[#E8ECF2]">
          Hi. I can answer from approved sources and hand off to your support team when the case needs a person.
        </div>
        <div className="mt-4 w-max max-w-[86%] rounded-[14px] border border-[#E5E6EA] bg-white px-4 py-2 text-[12px] font-semibold leading-5 text-[#5F6673] dark:border-[#303238] dark:bg-[#15161A] dark:text-[#C6CCD6]">
          {connected
            ? `Live actions available: ${supportStatus.agents} agents, ${supportStatus.groups} groups, ${supportStatus.macros} macros.`
            : "Connect support-core/Zammad before enabling live ticket actions."}
        </div>
        <div className="mt-auto pb-3 text-center text-[12px] font-medium text-[#B1B3B9]">
          <span className="mr-1 inline-grid size-4 place-items-center rounded-[4px] bg-[#AEB0B7] text-[10px] font-bold text-white">V</span>
          Powered by Verevon
        </div>
        <div className="flex h-12 items-center gap-2 rounded-full border border-[#E2E3E8] bg-white px-4 text-[14px] text-[#A1A5AE] shadow-[0_8px_24px_rgba(31,35,42,0.08)] dark:border-[#303238] dark:bg-[#17181C]">
          {placeholder}
          <Mic className="ml-auto size-4 shrink-0 text-[#7D828C]" />
          <span className="grid size-8 shrink-0 place-items-center rounded-full bg-[#F4F5F7] text-[#D0D3D9] dark:bg-[#202229]">
            <Send className="size-3.5" />
          </span>
        </div>
      </div>
    </div>
  );
}
