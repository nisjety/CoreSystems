import {
  BarChart3,
  Bot,
  Database,
  Mail,
  MessageCircle,
  MessagesSquare,
  Plug,
  Settings2,
  Sparkles,
  UserRound,
  Webhook,
  Zap,
  type LucideIcon,
} from "lucide-react";
import type { ReactNode } from "react";
import type {
  ChatbotAddOnId,
  ChatbotBuilderSectionId,
} from "@/features/agents-v2/lib/agent-roles";
import { FieldPill } from "@/features/agents-v2/components/VelionChatbotStudioPrimitives";

type StudioSection = {
  id: ChatbotBuilderSectionId;
  title: string;
  description: string;
  status: string;
};

type AddOnModal = {
  id: ChatbotAddOnId;
  eyebrow: string;
  title: string;
  className: string;
  Icon: LucideIcon;
  accentClass: string;
  body: ReactNode;
};

export const chatbotDisplayName = "Velion Support Agent";

export const studioSections: Record<ChatbotBuilderSectionId, StudioSection> = {
  playground: {
    id: "playground",
    title: "Playground",
    description: "Tune model, instructions, tools, message behavior, and the live chatbot preview.",
    status: "Runtime checked live",
  },
  "data-sources": {
    id: "data-sources",
    title: "Fine-tuning",
    description: "Prepare training files, run real model fine-tuning jobs, and manage adapters.",
    status: "No datasets uploaded",
  },
  integrations: {
    id: "integrations",
    title: "Integrations",
    description: "Connect the systems the chatbot can fetch data from before tools use them.",
    status: "Live status checked",
  },
  actions: {
    id: "actions",
    title: "Tools",
    description: "Configure the tools, skill pool, and executable capabilities the chatbot can use.",
    status: "Guarded actions",
  },
  "chat-logs": {
    id: "chat-logs",
    title: "Chat logs",
    description: "Review conversations and inspect the live playground transcript.",
    status: "No conversations yet",
  },
  analytics: {
    id: "analytics",
    title: "Analytics",
    description: "Track chat count, topic trends, and sentiment across conversations.",
    status: "Waiting for live events",
  },
  leads: {
    id: "leads",
    title: "Leads",
    description: "Review lead submissions collected by the chatbot.",
    status: "No leads captured",
  },
  insights: {
    id: "insights",
    title: "Insights",
    description: "Summarize performance, countries, feedback, and improvement opportunities.",
    status: "Waiting for signals",
  },
  install: {
    id: "install",
    title: "Install",
    description: "Install the chatbot across web widgets, help pages, email, and messaging channels.",
    status: "Configure channels",
  },
  settings: {
    id: "settings",
    title: "Settings",
    description: "Control chatbot identity, tone, safety, and defaults.",
    status: "Draft changes",
  },
};

const addOnModals: AddOnModal[] = [
  {
    id: "subscription-action",
    eyebrow: "Action connected",
    title: "Update subscription",
    className: "",
    Icon: Zap,
    accentClass: "bg-[#635BFF]",
    body: (
      <p className="text-[11px] font-semibold uppercase leading-4 text-[#4C515D] dark:text-[#D7DCE4]">
        Use this when a customer wants to cancel, pause, change plan, or confirm a billing update.
      </p>
    ),
  },
  {
    id: "instructions",
    eyebrow: "System prompt",
    title: "Reading instructions",
    className: "",
    Icon: Database,
    accentClass: "bg-[#111111]",
    body: (
      <ol className="space-y-2 text-[11px] leading-4 text-[#555B65] dark:text-[#D7DCE4]">
        <li>1. Use get plan to fetch the customer subscription.</li>
        <li>2. Offer a pause option before cancellation.</li>
        <li>3. Update the record after explicit confirmation.</li>
      </ol>
    ),
  },
  {
    id: "channels",
    eyebrow: "Channel ID",
    title: "Connected channels",
    className: "",
    Icon: Plug,
    accentClass: "bg-[#EA6B22]",
    body: (
      <div className="grid grid-cols-7 gap-1.5">
        {[MessageCircle, Mail, Webhook, Sparkles, Plug, MessagesSquare, Bot].map((Icon) => (
          <span key={Icon.displayName ?? Icon.name} className="grid size-7 place-items-center rounded-[7px] border border-[#E5E1DA] bg-[#FAF9F7] text-[#EA6B22] dark:border-[#2B2D33] dark:bg-[#111216]">
            <Icon className="size-3.5" />
          </span>
        ))}
      </div>
    ),
  },
  {
    id: "user-info",
    eyebrow: "Context",
    title: "User information",
    className: "",
    Icon: UserRound,
    accentClass: "bg-[#22252B]",
    body: (
      <div className="rounded-[9px] border border-dashed border-[#D8DCE3] bg-white/70 p-3 text-[11px] font-semibold leading-5 text-[#6F747D] dark:border-[#303238] dark:bg-[#111216] dark:text-[#C0C6D0]">
        Customer context is attached from the selected inbox conversation before the agent can read profile, order, or subscription fields.
      </div>
    ),
  },
  {
    id: "guidance",
    eyebrow: "Applying guidance",
    title: "Tone and policy",
    className: "",
    Icon: Settings2,
    accentClass: "bg-[#8A8F98]",
    body: (
      <div className="space-y-2">
        <FieldPill label="Tone of voice" value="Professional" />
        <FieldPill label="Answer length" value="Standard" />
        <FieldPill label="Naming conventions" value="Enabled" />
      </div>
    ),
  },
  {
    id: "billing-analytics",
    eyebrow: "Analytics",
    title: "Billing topic trend",
    className: "",
    Icon: BarChart3,
    accentClass: "bg-[#6D4C9F]",
    body: null,
  },
  {
    id: "automation-rate",
    eyebrow: "Analytics",
    title: "Automation rate",
    className: "",
    Icon: Sparkles,
    accentClass: "bg-[#6FD6A6]",
    body: null,
  },
  {
    id: "performance",
    eyebrow: "Analytics",
    title: "Performance over time",
    className: "",
    Icon: BarChart3,
    accentClass: "bg-[#FE4C02]",
    body: null,
  },
];

export const defaultVisibleAddOns: ChatbotAddOnId[] = ["subscription-action", "instructions", "guidance"];
export const allAddOnIds = addOnModals.map((addOn) => addOn.id);
