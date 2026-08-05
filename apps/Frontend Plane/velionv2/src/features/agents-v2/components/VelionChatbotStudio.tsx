"use client";

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
} from "lucide-react";
import { useEffect, useReducer, useState } from "react";
import {
  VerevonButton,
  VerevonIconButton,
  VerevonInput,
  VerevonSegmented,
  VerevonSegmentedButton,
} from "@/components/ui/verevon-ui";
import { cn } from "@/lib/utils";
import {
  type ChatbotAddOnId,
  type ChatbotBuilderSectionId,
} from "@/features/agents-v2/lib/agent-roles";
import {
  useAgentSelection,
  useChatbotAddOn,
  useChatbotBuilderSection,
} from "@/features/agents-v2/lib/use-agent-selection";
import {
  useChatbotSupportStatus,
  type SupportIntegrationStatus,
} from "@/features/agents-v2/lib/use-chatbot-support-status";
import {
  ActionCard,
  ChannelCard,
  ChannelHeroCard,
  IntegrationCard,
  SquareIconButton,
} from "@/features/agents-v2/components/VerevonChatbotStudioCards";
import {
  EmptyStateCard,
  EmptyStateInline,
  MetricCard,
  SectionHeader,
} from "@/features/agents-v2/components/VerevonChatbotStudioPrimitives";
import { ChatbotPlaygroundSurface } from "@/features/agents-v2/components/VerevonChatbotPlayground";
import {
  allAddOnIds,
  chatbotDisplayName,
  defaultVisibleAddOns,
  studioSections,
} from "@/features/agents-v2/lib/verevon-chatbot-studio-data";

type AddOnCanvasState = {
  visible: Set<ChatbotAddOnId>;
  hidden: Set<ChatbotAddOnId>;
};

type AddOnCanvasAction =
  | { type: "add"; addOn: ChatbotAddOnId }
  | { type: "clear" }
  | { type: "remove"; addOn: ChatbotAddOnId }
  | { type: "reset" }
  | { type: "select"; addOn: ChatbotAddOnId };

function getInitialAddOnCanvasState(): AddOnCanvasState {
  return {
    visible: new Set(defaultVisibleAddOns),
    hidden: new Set(),
  };
}

function addOnCanvasReducer(
  state: AddOnCanvasState,
  action: AddOnCanvasAction,
): AddOnCanvasState {
  switch (action.type) {
    case "add": {
      const hidden = new Set(state.hidden);
      hidden.delete(action.addOn);
      return {
        visible: new Set([...state.visible, action.addOn]),
        hidden,
      };
    }
    case "clear":
      return {
        visible: new Set(),
        hidden: new Set(allAddOnIds),
      };
    case "remove":
      return {
        visible: new Set([...state.visible].filter((addOn) => addOn !== action.addOn)),
        hidden: new Set([...state.hidden, action.addOn]),
      };
    case "reset":
      return getInitialAddOnCanvasState();
    case "select": {
      const hidden = new Set(state.hidden);
      hidden.delete(action.addOn);
      return {
        visible: new Set([...state.visible, action.addOn]),
        hidden,
      };
    }
  }
}

export function VerevonChatbotStudio() {
  const [agentSelection, setAgentSelection] = useAgentSelection();
  const [section] = useChatbotBuilderSection();
  const [selectedAddOn, setSelectedAddOn] = useChatbotAddOn();
  const supportStatus = useChatbotSupportStatus();
  const [addOnCanvas, dispatchAddOnCanvas] = useReducer(addOnCanvasReducer, undefined, getInitialAddOnCanvasState);
  const displayedAddOns = new Set(addOnCanvas.visible);

  if (!addOnCanvas.hidden.has(selectedAddOn)) {
    displayedAddOns.add(selectedAddOn);
  }

  useEffect(() => {
    if (agentSelection !== "chatbot") {
      setAgentSelection("chatbot");
    }
  }, [agentSelection, setAgentSelection]);

  const addSelectedAddOn = () => {
    dispatchAddOnCanvas({ type: "add", addOn: selectedAddOn });
  };

  const removeSelectedAddOn = () => {
    dispatchAddOnCanvas({ type: "remove", addOn: selectedAddOn });
  };

  const resetToEssentials = () => {
    dispatchAddOnCanvas({ type: "reset" });
    setSelectedAddOn("subscription-action");
  };

  const clearCanvas = () => {
    dispatchAddOnCanvas({ type: "clear" });
  };

  const handleAddOnSelect = (addOn: ChatbotAddOnId) => {
    setSelectedAddOn(addOn);
    dispatchAddOnCanvas({ type: "select", addOn });
  };

  if (section === "playground") {
    return (
      <ChatbotPlaygroundSurface
        onAddOnSelect={handleAddOnSelect}
        onAddSelected={addSelectedAddOn}
        onClearCanvas={clearCanvas}
        onRemoveSelected={removeSelectedAddOn}
        onResetCanvas={resetToEssentials}
        selectedAddOn={selectedAddOn}
        supportStatus={supportStatus}
        visibleAddOns={displayedAddOns}
      />
    );
  }

  return <ChatbotSectionSurface section={section} supportStatus={supportStatus} />;
}

function ChatbotSectionSurface({
  section,
  supportStatus,
}: {
  section: ChatbotBuilderSectionId;
  supportStatus: SupportIntegrationStatus;
}) {
  return (
    <div className="h-full min-h-0 overflow-y-auto bg-white text-[#111111] dark:bg-[#101114] dark:text-[#F7F8F8]">
      {section === "analytics" ? <AnalyticsPage /> : null}
      {section === "data-sources" ? <FineTuningPage /> : null}
      {section === "integrations" ? <IntegrationsPage supportStatus={supportStatus} /> : null}
      {section === "actions" ? <ToolsPage supportStatus={supportStatus} /> : null}
      {section === "install" ? <InstallPage /> : null}
      {section === "chat-logs" ? <ChatLogsPage /> : null}
      {section === "leads" ? <LeadsPage /> : null}
      {section === "insights" ? <InsightsPage /> : null}
      {section === "settings" ? <SettingsPage /> : null}
    </div>
  );
}

function AnalyticsPage() {
  const [activeTab, setActiveTab] = useState<"chat-count" | "topics" | "sentiment">("chat-count");
  const tabs: Array<{ id: "chat-count" | "topics" | "sentiment"; label: string }> = [
    { id: "chat-count", label: "Chat count" },
    { id: "topics", label: "Topics" },
    { id: "sentiment", label: "Sentiment" },
  ];

  return (
    <section className="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Analytics"
        description="Measure chatbot volume, topic distribution, and sentiment signals."
        action={(
          <VerevonButton radius="sm" className="px-4 text-[13px] font-semibold">
            <CalendarDays className="size-4" />
            Live event window
          </VerevonButton>
        )}
      />
      <VerevonSegmented className="mt-8">
        {tabs.map((tab) => (
          <VerevonSegmentedButton
            key={tab.id}
            aria-pressed={activeTab === tab.id}
            onClick={() => setActiveTab(tab.id)}
            className="px-4 font-semibold"
          >
            {tab.label}
          </VerevonSegmentedButton>
        ))}
      </VerevonSegmented>
      {activeTab === "chat-count" ? (
        <>
          <div className="mt-7 grid grid-cols-1 gap-4 md:grid-cols-3">
            <MetricCard Icon={MessagesSquare} label="Chats" value="0" />
            <MetricCard Icon={MessageSquare} label="Messages" value="0" />
            <MetricCard Icon={ThumbsUp} label="Positive feedback" value="0" />
          </div>
          <EmptyStateCard
            Icon={BarChart3}
            title="No live chatbot analytics yet"
            description="Analytics populate from real chatbot conversations once the widget or help page is installed."
          />
        </>
      ) : null}
      {activeTab === "topics" ? <TopicsPanel /> : null}
      {activeTab === "sentiment" ? <SentimentPanel /> : null}
    </section>
  );
}

function InsightsPage() {
  const metrics = [
    { label: "Total conversations", value: "0", Icon: MessagesSquare },
    { label: "Total messages", value: "0", Icon: MessageSquare },
    { label: "Thumbs up messages", value: "0", Icon: ThumbsUp },
    { label: "Thumbs down messages", value: "0", Icon: ThumbsDown },
  ];

  return (
    <section className="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Insights"
        description="Review the signals that should shape chatbot improvements."
        action={(
          <VerevonButton radius="sm" className="px-4 text-[13px] font-semibold">
            <CalendarDays className="size-4" />
            Live event window
          </VerevonButton>
        )}
      />

      <div className="mt-12 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
        {metrics.map((metric) => (
          <MetricCard key={metric.label} {...metric} />
        ))}
      </div>

      <EmptyStateCard
        Icon={Sparkles}
        title="No improvement signals yet"
        description="Verevon will rank unanswered questions, missing sources, and action failures after live conversations arrive."
      />

      <div className="mt-6 grid gap-6 xl:grid-cols-[minmax(0,1fr)_520px]">
        <CountryCard />
        <LeadsCard />
      </div>
    </section>
  );
}

function TopicsPanel() {
  return (
    <div className="verevon-panel mt-7 p-6">
      <h2 className="text-[20px] font-semibold">Topics</h2>
      <p className="mt-2 text-[14px] text-[#6F747D] dark:text-[#AEB4C0]">Most common subjects detected across chatbot conversations.</p>
      <EmptyStateInline Icon={Search} title="No topic clusters yet" description="Topic groups are generated from real conversations." />
    </div>
  );
}

function SentimentPanel() {
  return (
    <div className="mt-7 grid gap-4 lg:grid-cols-3">
      {[
        ["Positive", "0", "bg-[#E9F8EF] text-[#16834A]"],
        ["Neutral", "0", "bg-[#F4F5F7] text-[#555B65]"],
        ["Negative", "0", "bg-[#FFF0EC] text-[#B6482C]"],
      ].map(([label, value, className]) => (
        <div key={label} className="verevon-panel p-6">
          <div className={cn("inline-flex rounded-full px-3 py-1 text-[12px] font-semibold", className)}>{label}</div>
          <div className="mt-5 text-[34px] font-semibold">{value}</div>
          <p className="mt-3 text-[14px] leading-6 text-[#6F747D] dark:text-[#AEB4C0]">Measured from classified customer and assistant turns once live conversations are available.</p>
        </div>
      ))}
    </div>
  );
}

function FineTuningPage() {
  return (
    <section className="grid min-h-full gap-8 px-7 py-8 xl:grid-cols-[minmax(0,1fr)_420px]">
      <div className="min-w-0">
        <SectionHeader
          title="Fine-tuning"
          description="Upload supervised examples and datasets for real model fine-tuning. Use this for model weights/adapters, not prompt engineering."
          action={(
            <VerevonButton radius="sm" className="px-5 text-[13px] font-semibold">
              <Info className="size-5" />
              Learn more
            </VerevonButton>
          )}
        />

        <div className="verevon-panel mt-12 p-7">
          <div className="flex items-center justify-between">
            <h2 className="text-[23px] font-semibold">Add files</h2>
            <ChevronDown className="size-5 rotate-180 text-[#7C828C]" />
          </div>
          <div className="mt-7 flex min-h-11 items-center gap-3 rounded-[8px] border border-[#F1DCA6] bg-[#FFF9DF] px-4 text-[14px] font-semibold text-[#BA5A16]">
            <Info className="size-4 shrink-0" />
            Fine-tuning data should use clean examples with input, expected output, and evaluation labels.
          </div>
          <button
            type="button"
            className="mt-6 grid min-h-[250px] w-full place-items-center rounded-[10px] border border-dashed border-[#D6D8DD] bg-[#FCFCFD] text-center transition-colors hover:bg-[#FAFAFB] dark:border-[#303238] dark:bg-[#111216] dark:hover:bg-[#17181C]"
          >
            <span>
              <Upload className="mx-auto size-8 text-[#767C86]" />
              <span className="mt-6 block text-[17px] font-medium text-[#343842] dark:text-white">Drag & drop fine-tuning datasets here</span>
              <span className="mt-2 block text-[14px] text-[#6F747D] dark:text-[#AEB4C0]">Supported file types: jsonl, csv, parquet, txt</span>
            </span>
          </button>
        </div>

        <div className="mt-12">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <h2 className="text-[23px] font-semibold">Training datasets</h2>
            <div className="relative w-full sm:w-[360px]">
              <Search className="pointer-events-none absolute left-4 top-1/2 size-5 -translate-y-1/2 text-[#A0A5AE]" />
              <VerevonInput
                aria-label="Search training datasets"
                placeholder="Search…"
                variant="compact"
                className="pl-12 pr-4 text-[13px]"
              />
            </div>
          </div>
          <div className="mt-7 flex items-center justify-between border-b border-[#E8E9EC] pb-6">
            <label className="inline-flex items-center gap-4 text-[16px] font-semibold">
              <input type="checkbox" className="size-5 rounded border-[#D8DADE]" />
              Select all
            </label>
            <button type="button" className="inline-flex items-center gap-2 text-[16px] font-semibold text-[#5D626C]">
              Sort by: <span className="text-[#111111] dark:text-white">Default</span>
              <ChevronDown className="size-4" />
            </button>
          </div>
          <div>
            <EmptyStateInline
              Icon={FileText}
              title="No training datasets uploaded"
              description="Upload validated files before starting a fine-tune job."
            />
          </div>
        </div>
      </div>

      <aside className="border-l border-[#E3E4E8] bg-[#F7F7F8] p-8 dark:border-[#2A2C31] dark:bg-[#111216] xl:-my-8 xl:-mr-7">
        <h2 className="text-[23px] font-semibold">Fine-tuning</h2>
        <div className="verevon-panel mt-8 p-5">
          <div className="flex items-center justify-between text-[17px] font-semibold">
            <span className="inline-flex items-center gap-3"><TestTubeDiagonal className="size-5" />0 datasets</span>
            <span>0 KB</span>
          </div>
        </div>
        <div className="verevon-panel mt-6 p-5">
          <div className="flex items-center justify-between text-[16px]">
            <span className="text-[#6F747D] dark:text-[#AEB4C0]">Training size</span>
            <span className="font-semibold">0 KB / 20 MB</span>
          </div>
          <VerevonButton disabled radius="sm" className="mt-6 w-full bg-[#D8DADE] text-[13px] font-semibold text-white transition-colors dark:bg-[#303238]">
            Start fine-tune
          </VerevonButton>
        </div>
        <div className="mt-6 flex min-h-[54px] items-center gap-3 rounded-[9px] border border-[#F0DCA6] bg-[#FFF8DC] px-4 text-[15px] font-semibold text-[#B85E16]">
          <RefreshCw className="size-5" />
          Fine-tuning creates a new model adapter after validation passes
        </div>
      </aside>
    </section>
  );
}

function ToolsPage({ supportStatus }: { supportStatus: SupportIntegrationStatus }) {
  const supportConnected = supportStatus.status === "connected";
  const actions = [
    { title: "Collect leads", subtitle: "Skill: capture qualified contact fields", color: "text-[#E53688] bg-[#FFF0F7] border-[#F7B8D5]", Icon: UserRound, enabled: true },
    { title: "Create support ticket", subtitle: supportConnected ? "Tool: Zammad ticket creation ready" : "Connect support integration to enable", color: "text-[#EE7A50] bg-[#FFF3EC] border-[#F5C7B4]", Icon: Wrench, enabled: supportConnected },
    { title: "Route to support group", subtitle: supportConnected ? `${supportStatus.groups} live groups available` : "Waiting for support groups", color: "text-[#4A9C9C] bg-[#F2FBFB] border-[#D8E7EA]", Icon: MessagesSquare, enabled: supportConnected },
  ];

  return (
    <section className="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Tools"
        description="Configure the tools the chatbot can call and the skills it can perform. Integrations provide data access; tools decide what the bot may do with it."
        action={(
          <div className="flex min-w-0 flex-1 justify-end gap-3">
            <div className="relative w-full max-w-[470px]">
              <Search className="pointer-events-none absolute left-4 top-1/2 size-5 -translate-y-1/2 text-[#6F747D]" />
              <VerevonInput
                aria-label="Search tools"
                placeholder="Search"
                variant="compact"
                className="pl-12 pr-4 text-[13px]"
              />
            </div>
            <VerevonButton variant="primary" radius="sm" className="shrink-0 px-6 text-[13px] font-semibold">
              <Plus className="size-5" />
              Create tool
            </VerevonButton>
          </div>
        )}
      />
      <div className="mt-12 grid grid-cols-1 gap-5 lg:grid-cols-3">
        {actions.map((action) => (
          <ActionCard key={action.title} {...action} />
        ))}
      </div>
    </section>
  );
}

function InstallPage() {
  const channels = [
    { title: "Email", badge: "Beta", description: "Connect your agent to an email address and let it respond to messages from your customers.", Icon: Mail, action: "Subscribe to enable" },
    { title: "Zapier", description: "Connect your agent with thousands of apps using Zapier.", Icon: Plug, action: "Subscribe to enable" },
    { title: "Slack", description: "Connect your agent to Slack, mention it, and have it reply to any message.", Icon: MessagesSquare, action: "Subscribe to enable" },
    { title: "WordPress", description: "Install the Verevon widget script through a WordPress embed or plugin wrapper.", Icon: Globe2, action: "Setup" },
    { title: "WhatsApp", description: "Connect your agent to a WhatsApp number and respond in the same thread.", Icon: MessageCircle, action: "Subscribe to enable" },
    { title: "Messenger", description: "Connect your agent to a Facebook page and let it reply to customers.", Icon: Send, action: "Subscribe to enable" },
  ];

  return (
    <section className="mx-auto max-w-[1280px] px-6 py-7">
      <h1 className="text-[30px] font-semibold tracking-normal">All channels</h1>
      <div className="mt-12 grid gap-5 xl:grid-cols-2">
        <ChannelHeroCard displayName={chatbotDisplayName} type="widget" />
        <ChannelHeroCard displayName={chatbotDisplayName} type="help" />
      </div>
      <div className="mt-5 grid grid-cols-1 gap-5 lg:grid-cols-3">
        {channels.map((channel) => (
          <ChannelCard key={channel.title} {...channel} />
        ))}
      </div>
    </section>
  );
}

function IntegrationsPage({ supportStatus }: { supportStatus: SupportIntegrationStatus }) {
  const supportConnected = supportStatus.status === "connected";
  const integrations = [
    { title: "Support agents", description: "Read available human agents for handoff and ownership.", Icon: UserRound, status: supportConnected ? `${supportStatus.agents} connected` : "Not connected" },
    { title: "Support groups", description: "Route conversations to real support teams and queues.", Icon: MessagesSquare, status: supportConnected ? `${supportStatus.groups} connected` : "Not connected" },
    { title: "Support macros", description: "Expose approved response and workflow macros as guarded actions.", Icon: Wrench, status: supportConnected ? `${supportStatus.macros} connected` : "Not connected" },
    { title: "Website crawler", description: "Fetch public pages, docs, and product copy for retrieval.", Icon: Globe2, status: "Configure" },
    { title: "Webhook API", description: "Call internal systems through signed request endpoints.", Icon: Webhook, status: "Configure" },
    { title: "Vector store", description: "Sync embeddings and retrieval indexes used by chatbot tools.", Icon: Blocks, status: "Configure" },
  ];

  return (
    <section className="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Integrations"
        description="Connect the systems the chatbot can fetch data from. Tools then define the allowed option pool over those integrations."
        action={(
          <VerevonButton variant="primary" radius="sm" className="px-4 text-[13px] font-semibold">
            <Plus className="size-4" />
            Add integration
          </VerevonButton>
        )}
      />
      <div className="mt-8 grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        {integrations.map((integration) => (
          <IntegrationCard key={integration.title} {...integration} />
        ))}
      </div>
    </section>
  );
}

function LeadsPage() {
  return (
    <section className="mx-auto max-w-[1280px] px-6 py-7">
      <SectionHeader
        title="Leads"
        description="Review lead submissions collected by chatbot skills and export them for follow-up."
        action={(
          <VerevonButton variant="primary" radius="sm" className="px-4 text-[13px] font-semibold">
            Export
            <Download className="size-4" />
          </VerevonButton>
        )}
      />
      <div className="mt-8">
        <LeadsCard hideHeaderAction />
      </div>
    </section>
  );
}

function ChatLogsPage() {
  return (
    <section className="grid min-h-full lg:grid-cols-[480px_minmax(0,1fr)]">
      <aside className="border-r border-[#E3E4E8] bg-white px-6 py-8 dark:border-[#2A2C31] dark:bg-[#101114]">
        <div className="flex items-center justify-between gap-4">
          <h1 className="text-[30px] font-semibold tracking-normal">Chat logs</h1>
          <div className="flex items-center gap-2">
            <SquareIconButton label="Filter chat logs" Icon={Settings2} />
            <SquareIconButton label="Refresh chat logs" Icon={RefreshCw} />
            <VerevonIconButton size="lg" radius="sm" aria-label="Download chat logs" className="bg-[#111111] text-white hover:bg-[#2A2A2A] hover:text-white">
              <Download className="size-5" />
            </VerevonIconButton>
          </div>
        </div>
        <div className="mt-12 space-y-3">
          <EmptyStateInline
            Icon={MessagesSquare}
            title="No chatbot conversations yet"
            description="Live chat logs appear here after the widget or help page receives traffic."
          />
        </div>
      </aside>

      <main className="min-w-0 bg-white dark:bg-[#101114]">
        <div className="flex h-[118px] items-start justify-between border-b border-[#E3E4E8] px-8 py-7 dark:border-[#2A2C31]">
          <div>
            <h2 className="text-[21px] font-semibold">Playground</h2>
            <div className="mt-6 flex gap-8 text-[16px] font-medium">
              <span className="border-b-2 border-black pb-4 text-black dark:border-white dark:text-white">Chat</span>
              <span className="pb-4 text-[#6F747D]">Details</span>
            </div>
          </div>
          <SquareIconButton label="Open chat log menu" Icon={MoreHorizontal} />
        </div>
        <div className="mx-auto max-w-[820px] p-8">
          <EmptyStateCard
            Icon={MessageSquare}
            title="Select a live conversation"
            description="Conversation transcripts, sources, and tool traces will render here when real chat logs exist."
          />
        </div>
      </main>
    </section>
  );
}

function SettingsPage() {
  const section = studioSections.settings;

  return (
    <section className="mx-auto max-w-[980px] px-7 py-8">
      <SectionHeader title={section.title} description={section.description} />
      <div className="mt-10 grid gap-5">
        {[
          ["Agent name", chatbotDisplayName],
          ["Tone", "Clear, concise, and product-aware"],
          ["Fallback behavior", "Ask for clarification before handing off"],
        ].map(([label, value]) => (
          <label key={label} className="verevon-panel grid gap-3 p-5">
            <span className="text-[14px] font-semibold text-[#6F747D] dark:text-[#AEB4C0]">{label}</span>
            <VerevonInput defaultValue={value} variant="compact" className="px-4 text-[13px]" />
          </label>
        ))}
      </div>
    </section>
  );
}

function CountryCard() {
  return (
    <div className="verevon-panel p-7">
      <h2 className="text-[23px] font-semibold">Chats by country</h2>
      <div className="mt-8 grid gap-8 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div className="relative min-h-[260px] overflow-hidden rounded-[10px] bg-[#FAFAFB] dark:bg-[#111216]">
          <div className="absolute left-14 top-16 h-24 w-36 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div className="absolute left-[42%] top-10 h-28 w-44 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div className="absolute bottom-10 right-16 h-32 w-52 rounded-[48%] border border-[#D9DDE5] bg-[#F1F3F6]" />
          <div className="absolute bottom-16 left-[18%] h-16 w-24 rounded-[48%] bg-[#D7ECFF]" />
        </div>
        <div>
          <div className="grid grid-cols-[1fr_80px] border-b border-dashed border-[#D8DADE] pb-3 text-[16px] text-[#7B808A]">
            <span>Country</span><span className="text-right">Chats</span>
          </div>
          <EmptyStateInline Icon={Globe2} title="No location data yet" description="Countries appear after real conversations include location metadata." />
        </div>
      </div>
    </div>
  );
}

function LeadsCard({ hideHeaderAction = false }: { hideHeaderAction?: boolean }) {
  return (
    <div className="verevon-panel p-7">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 className="text-[23px] font-semibold">Leads</h2>
          <p className="mt-2 text-[15px] text-[#6F747D] dark:text-[#AEB4C0]">Submitted from lead collection skills.</p>
        </div>
        {hideHeaderAction ? null : (
          <VerevonButton variant="primary" radius="sm" className="px-4 text-[13px] font-semibold">
            Export
            <Download className="size-4" />
          </VerevonButton>
        )}
      </div>
      <div className="mt-6 overflow-hidden rounded-[10px] border border-[#E8E9EC]">
        <div className="grid grid-cols-[0.8fr_1.4fr_1fr_1.2fr] border-b border-[#E8E9EC] bg-[#FAFAFB] px-4 py-3 text-[13px] font-semibold">
          <span>Name</span><span>Email</span><span>Phone</span><span>Submitted at</span>
        </div>
        <div className="p-4">
          <EmptyStateInline Icon={UserRound} title="No captured leads yet" description="Lead rows are created only from live chatbot lead-collection submissions." />
        </div>
      </div>
    </div>
  );
}
