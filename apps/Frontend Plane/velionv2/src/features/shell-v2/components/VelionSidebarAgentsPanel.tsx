"use client";

import { useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  BarChart3,
  BookOpen,
  Bot,
  BriefcaseBusiness,
  CalendarClock,
  CheckCircle2,
  ChartNoAxesColumnIncreasing,
  ChevronDown,
  Database,
  Globe2,
  LayoutGrid,
  MessagesSquare,
  Plug,
  Rocket,
  Search,
  Settings,
  ShieldCheck,
  ShoppingBag,
  Sparkles,
  TicketCheck,
  UsersRound,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import {
  agentFeatureOptionsByRole,
  agentRoleOptions,
  chatbotBuilderSectionOptions,
  isCoreAgentRoleId,
  type AgentFeatureId,
  type ChatbotBuilderSectionId,
  type CoreAgentRoleId,
} from "@/features/agents-v2/lib/agent-roles";
import {
  useAgentFeature,
  useAgentSelection,
  useChatbotBuilderSection,
} from "@/features/agents-v2/lib/use-agent-selection";
import { WorkflowToolsPanel } from "@/features/agents-v2/components/VerevonWorkflowToolsPanel";
import { SidebarPanelTitle } from "@/features/shell-v2/components/VerevonSidebarPrimitives";
import { sidebarFocusClass, sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

const agentFeatureIcons: Record<AgentFeatureId, LucideIcon> = {
  "service-resolution": TicketCheck,
  "service-knowledge": BookOpen,
  "service-actions": Wrench,
  "service-channels": Globe2,
  "service-quality": CheckCircle2,
  "service-insights": BarChart3,
  "sales-lead-capture": Sparkles,
  "sales-qualification": BriefcaseBusiness,
  "sales-objections": ShieldCheck,
  "sales-booking": CalendarClock,
  "sales-crm": Database,
  "sales-insights": ChartNoAxesColumnIncreasing,
  "commerce-shopping": ShoppingBag,
  "commerce-support": TicketCheck,
  "commerce-product-finder": Search,
  "commerce-cart": Rocket,
  "commerce-brand": MessagesSquare,
  "commerce-store": Settings,
  "commerce-insights": BarChart3,
};

const chatbotSidebarTabIcons: Record<ChatbotBuilderSectionId, LucideIcon> = {
  playground: LayoutGrid,
  "chat-logs": MessagesSquare,
  "data-sources": Database,
  integrations: Plug,
  actions: Wrench,
  analytics: BarChart3,
  leads: UsersRound,
  insights: ChartNoAxesColumnIncreasing,
  install: Rocket,
  settings: Settings,
};

const chatbotSidebarTabs: Array<{ id: ChatbotBuilderSectionId; label: string; icon: LucideIcon }> = [
  ...chatbotBuilderSectionOptions.map((option) => ({
    ...option,
    icon: chatbotSidebarTabIcons[option.id],
  })),
];

export function AgentsExpandedSidebarPanel({ onCollapse }: { onCollapse: () => void }) {
  const [agentSelection, , updateAgentSelectionFromValue] = useAgentSelection();
  const [agentFeature, setAgentFeature] = useAgentFeature(agentSelection);
  const [chatbotSection, setChatbotSection] = useChatbotBuilderSection();
  const selectedCoreAgent = isCoreAgentRoleId(agentSelection) ? agentSelection : null;
  const isChatbotSelected = agentSelection === "chatbot";
  const isWorkflowSelected = agentSelection === "workflow";

  if (isChatbotSelected) {
    return (
      <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-3 pb-4 pt-5 dark:bg-[#101114]">
        <AgentsSidebarHeader onCollapse={onCollapse} />
        <AgentSelector
          value={agentSelection}
          onChange={updateAgentSelectionFromValue}
        />
        <ChatbotBuilderSidebarNav
          activeSection={chatbotSection}
          onSectionChange={setChatbotSection}
        />
        <ChatbotCreditCard />
      </div>
    );
  }

  if (isWorkflowSelected) {
    return (
      <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-3 pb-4 pt-5 dark:bg-[#101114]">
        <AgentsSidebarHeader onCollapse={onCollapse} />
        <AgentSelector
          value={agentSelection}
          onChange={updateAgentSelectionFromValue}
        />
        <div className="min-h-0 flex-1 overflow-hidden">
          <WorkflowToolsPanel />
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-5 pb-5 pt-6 dark:bg-[#101114]">
      <AgentsSidebarHeader onCollapse={onCollapse} />
      <AgentSelector
        value={agentSelection}
        onChange={updateAgentSelectionFromValue}
      />
      <AgentFeatureSidebarNav
        activeFeature={agentFeature}
        disabled={!selectedCoreAgent}
        onFeatureChange={setAgentFeature}
        role={selectedCoreAgent}
      />
    </div>
  );
}

function AgentsSidebarHeader({ onCollapse }: { onCollapse: () => void }) {
  return (
    <SidebarPanelTitle spacing="mb-4" onCollapse={onCollapse}>
      Agents
    </SidebarPanelTitle>
  );
}

function AgentSelector({
  onChange,
  value,
}: {
  onChange: (value: string) => void;
  value: string;
}) {
  const [open, setOpen] = useState(false);
  const listboxId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const selectedIndex = (() => {
    const optionIndex = agentRoleOptions.findIndex((option) => option.id === value);
    return optionIndex >= 0 ? optionIndex : 0;
  })();
  const [activeIndex, setActiveIndex] = useState(selectedIndex);
  const selectedOption = agentRoleOptions[selectedIndex] ?? agentRoleOptions[0];

  const selectOptionAtIndex = (nextIndex: number) => {
    const option = agentRoleOptions[nextIndex];
    if (!option) {
      return;
    }

    onChange(option.id);
    setOpen(false);
  };

  const moveActiveOption = (direction: 1 | -1) => {
    setActiveIndex((currentIndex) => {
      const baseIndex = open ? currentIndex : selectedIndex;
      return (baseIndex + direction + agentRoleOptions.length) % agentRoleOptions.length;
    });
    setOpen(true);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActiveOption(1);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActiveOption(-1);
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      setActiveIndex(0);
      setOpen(true);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      setActiveIndex(agentRoleOptions.length - 1);
      setOpen(true);
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (open) {
        selectOptionAtIndex(activeIndex);
        return;
      }

      setOpen(true);
      return;
    }

    if (event.key === "Escape") {
      setOpen(false);
    }
  };

  useEffect(() => {
    if (!open) {
      return;
    }

    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && rootRef.current?.contains(target)) {
        return;
      }

      setOpen(false);
    };

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };

    document.addEventListener("pointerdown", closeOnOutsidePointer, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative mb-3">
      <span className="pointer-events-none absolute left-2.5 top-1/2 z-10 grid size-[22px] -translate-y-1/2 place-items-center rounded-[7px] bg-[#1D1D1F] text-white dark:bg-white dark:text-[#111111]">
        <Bot className="size-3" strokeWidth={2.1} />
      </span>
      <button
        type="button"
        aria-label="Select agent type"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? listboxId : undefined}
        onClick={() => {
          setActiveIndex(selectedIndex);
          setOpen((current) => !current);
        }}
        onKeyDown={handleKeyDown}
        className={cn(
          "flex h-9 w-full items-center rounded-[9px] border border-[#E3E5EA] bg-white pl-10 pr-8 text-left text-[#1D1D1F] shadow-[0_1px_2px_rgba(16,24,40,0.04)] outline-none transition-colors hover:bg-[#FAFAFB] focus:ring-2 focus:ring-[#DD7A1F]/20 dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-white dark:hover:bg-[#202228]",
          sidebarType.rowStrong,
          sidebarFocusClass,
        )}
      >
        <span className="min-w-0 flex-1 truncate">{selectedOption.label}</span>
      </button>
      <ChevronDown className={cn("pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[#1D1D1F] transition-transform dark:text-white", open ? "rotate-180" : "")} strokeWidth={2.2} />
      {open ? (
        <menu id={listboxId} className="verevon-popover absolute left-0 right-0 top-[calc(100%+8px)] z-[90] m-0 list-none p-1" aria-label="Agent type options">
          {agentRoleOptions.map((option, index) => {
            const selected = option.id === value;
            const active = activeIndex === index;
            return (
              <li key={option.id} role="presentation">
                <button
                  id={`${listboxId}-${option.id}`}
                  type="button"
                  role="menuitemradio"
                  aria-checked={selected}
                  onMouseEnter={() => setActiveIndex(index)}
                  onClick={() => {
                    selectOptionAtIndex(index);
                  }}
                  className={cn(
                    "flex h-9 w-full items-center rounded-[10px] px-3 text-left transition-colors",
                    sidebarType.row,
                    selected
                      ? "bg-[#F2F2F2] text-[#111111] dark:bg-[#23252A] dark:text-white"
                      : active
                      ? "bg-[#FAFAFA] text-[#111111] dark:bg-[#191A1F] dark:text-white"
                      : "text-[#555555] hover:bg-[#FAFAFA] hover:text-[#111111] dark:text-[#D0D6E0] dark:hover:bg-[#191A1F] dark:hover:text-white",
                    sidebarFocusClass,
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{option.label}</span>
                  {selected ? <CheckCircle2 className="size-3.5 shrink-0 text-[#12B76A]" strokeWidth={2} /> : null}
                </button>
              </li>
            );
          })}
        </menu>
      ) : null}
    </div>
  );
}

function AgentFeatureSidebarNav({
  activeFeature,
  disabled,
  onFeatureChange,
  role,
}: {
  activeFeature: AgentFeatureId;
  disabled: boolean;
  onFeatureChange: (feature: AgentFeatureId) => void;
  role: CoreAgentRoleId | null;
}) {
  const tabs = role ? agentFeatureOptionsByRole[role] : [];

  return (
    <nav className="space-y-1.5" aria-label="Agent feature tabs">
      {tabs.length === 0 ? (
        <div className={cn("rounded-[12px] border border-dashed border-[#DFE0E6] bg-white/60 px-3 py-4 text-[#8A8D96] dark:border-[#2A2C31] dark:bg-[#17181C]/60 dark:text-[#8A909B]", sidebarType.secondary)}>
          Select Service, Sales, or Ecommerce to configure its features.
        </div>
      ) : null}
      {tabs.map((tab) => {
        const Icon = agentFeatureIcons[tab.id];
        const active = !disabled && activeFeature === tab.id;

        return (
          <button
            key={tab.id}
            type="button"
            aria-pressed={active}
            disabled={disabled}
            onClick={() => onFeatureChange(tab.id)}
            className={cn(
              "group flex min-h-9 w-full items-start gap-2.5 rounded-[9px] p-2 text-left transition-colors",
              sidebarType.row,
              disabled
                ? "cursor-not-allowed text-[#A7A7A7] opacity-70 dark:text-[#6F7682]"
                : active
                ? "bg-[#EFEFF2] text-[#1D1D1F] dark:bg-[#202228] dark:text-white"
                : "text-[#9A9A9A] hover:bg-[#EFEFF2] hover:text-[#333333] dark:text-[#8C929D] dark:hover:bg-[#1A1B20] dark:hover:text-white",
              sidebarFocusClass,
            )}
          >
            <Icon className={cn("mt-0.5 shrink-0", sidebarType.icon)} strokeWidth={2} />
            <span className="min-w-0">
              <span className="block truncate">{tab.label}</span>
              <span className="mt-0.5 block line-clamp-2 text-[10px] font-normal leading-4 text-[#7A808B] group-hover:text-[#636873] dark:text-[#777E8B] dark:group-hover:text-[#AEB4C0]">
                {tab.description}
              </span>
            </span>
          </button>
        );
      })}
    </nav>
  );
}

function ChatbotBuilderSidebarNav({
  activeSection,
  onSectionChange,
}: {
  activeSection: ChatbotBuilderSectionId;
  onSectionChange: (section: ChatbotBuilderSectionId) => void;
}) {
  return (
    <nav className="space-y-1" aria-label="Chatbot builder navigation">
      {chatbotSidebarTabs.map((tab) => {
        const Icon = tab.icon;
        const active = activeSection === tab.id;

        return (
          <button
            key={tab.id}
            type="button"
            aria-pressed={active}
            onClick={() => onSectionChange(tab.id)}
            className={cn(
              "flex h-9 w-full items-center gap-2.5 rounded-[8px] px-2.5 text-left transition-colors",
              sidebarType.row,
              active
                ? "bg-white text-[#1D1D1F] shadow-[0_1px_2px_rgba(16,24,40,0.06)] dark:bg-[#202228] dark:text-white"
                : "text-[#6F747D] hover:bg-[#EFEFF2] hover:text-[#333333] dark:text-[#8C929D] dark:hover:bg-[#1A1B20] dark:hover:text-white",
              sidebarFocusClass,
            )}
          >
            <Icon className={cn("shrink-0", sidebarType.icon)} strokeWidth={2} />
            <span>{tab.label}</span>
          </button>
        );
      })}
    </nav>
  );
}

function ChatbotCreditCard() {
  return (
    <div className="mt-auto pt-4">
      <div className="rounded-[9px] bg-[#17181C] p-2.5 text-white shadow-[0_12px_28px_rgba(17,18,22,0.16)]">
        <div className="flex items-start gap-2 text-[11px] font-semibold">
          <span className="mt-0.5 size-2 shrink-0 rounded-full bg-[#F6AF6E]" />
          <div className="min-w-0">
            <span className="block">Runtime checks live</span>
            <span className="mt-1 block text-[10px] font-medium leading-4 text-white/62">
              Support actions, datasets, logs, and analytics show only connected data.
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
