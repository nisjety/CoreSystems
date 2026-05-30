"use client";

import type { ReactNode } from "react";
import {
  Bot,
  CalendarClock,
  CircleDashed,
  Code2,
  Database,
  GitBranch,
  Globe2,
  MessageSquareText,
  Search,
  Send,
  ShoppingBag,
  Sparkles,
  Split,
  TicketCheck,
} from "lucide-react";
import { cn } from "@/lib/utils";

export type AgentVisualProps = {
  large?: boolean;
};

function VisualFrame({
  children,
  className,
  large = false,
}: {
  children: ReactNode;
  className?: string;
  large?: boolean;
}) {
  return (
    <div
      aria-hidden="true"
      className={cn(
        "relative overflow-hidden rounded-[6px] border border-white/70 bg-[#F0F1F3] shadow-inner dark:border-white/10 dark:bg-[#17181C]",
        large ? "min-h-[320px]" : "h-[138px]",
        className,
      )}
    >
      {children}
    </div>
  );
}

function DottedCanvas({
  children,
  className,
  large,
}: {
  children: ReactNode;
  className?: string;
  large?: boolean;
}) {
  return (
    <VisualFrame
      large={large}
      className={cn(
        "bg-[radial-gradient(circle_at_1px_1px,rgba(37,39,45,0.13)_1px,transparent_0)] [background-size:18px_18px] dark:bg-[radial-gradient(circle_at_1px_1px,rgba(255,255,255,0.13)_1px,transparent_0)]",
        className,
      )}
    >
      {children}
    </VisualFrame>
  );
}

export function ServiceVisual({ large = false }: AgentVisualProps) {
  return (
    <VisualFrame large={large} className="bg-[#D68B2F]">
      <div className="absolute -left-8 top-5 h-20 w-52 rounded-full border-[20px] border-[#F7C262]/70" />
      <div className="absolute -right-10 bottom-1 h-28 w-60 rounded-full border-[22px] border-[#8D4A12]/55" />
      <div className={cn("absolute rounded-[8px] bg-white p-2 shadow-[0_12px_28px_rgba(0,0,0,0.18)]", large ? "left-[18%] top-[18%] w-[58%]" : "left-5 top-5 w-[74%]")}>
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-center gap-1.5">
            <span className="grid size-5 place-items-center rounded-[5px] bg-[#111111] text-white">
              <Sparkles className="size-3" />
            </span>
            <span className="text-[10px] font-semibold text-[#24262D]">Velion Service</span>
          </div>
          <span className="size-1.5 rounded-full bg-[#12B76A]" />
        </div>
        <div className="ml-auto w-[72%] rounded-[8px] bg-[#111111] px-2 py-1.5 text-[9px] leading-3 text-white">Can you help with my order?</div>
        <div className="mt-1.5 w-[82%] rounded-[8px] bg-[#F1F2F4] px-2 py-1.5 text-[9px] leading-3 text-[#343842]">
          I found the order and can create a support case if needed.
        </div>
      </div>
      <div className="absolute bottom-3 right-3 grid size-8 place-items-center rounded-full bg-[#111111] text-white shadow-lg">
        <MessageSquareText className="size-4" />
      </div>
    </VisualFrame>
  );
}

export function SalesVisual({ large = false }: AgentVisualProps) {
  return (
    <VisualFrame large={large} className="bg-[linear-gradient(135deg,#D7E4FF_0%,#7C8DFF_54%,#163152_100%)]">
      <div className={cn("absolute rounded-[8px] bg-[#20283F] p-2 text-white shadow-[0_12px_30px_rgba(18,31,63,0.24)]", large ? "right-[18%] top-[13%] w-[48%]" : "right-4 top-4 w-[66%]")}>
        <div className="mb-2 flex items-center justify-between text-[8px] font-semibold uppercase text-[#C9D3F2]">
          <span>Calendar rules</span>
          <span>Availability</span>
        </div>
        <div className="grid grid-cols-7 gap-1">
          {Array.from({ length: 21 }).map((_, index) => (
            <span
              key={index}
              className={cn(
                "grid aspect-square place-items-center rounded-[4px] text-[7px]",
                index === 10 ? "bg-white text-[#20283F]" : "bg-white/10 text-white/70",
              )}
            >
              {index + 1}
            </span>
          ))}
        </div>
        <div className="mt-2 grid gap-1">
          {["Owner", "Team", "Fallback"].map((slot, index) => (
            <span key={slot} className={cn("rounded-[4px] px-2 py-1 text-center text-[8px]", index === 1 ? "bg-white text-[#20283F]" : "bg-white/12 text-white/80")}>
              {slot}
            </span>
          ))}
        </div>
      </div>
      <div className="absolute bottom-3 left-4 flex w-[70%] items-center gap-1 rounded-full bg-white px-2 py-1.5 text-[9px] text-[#6B7280] shadow-lg">
        <MessageSquareText className="size-3 text-[#2F73D9]" />
        Book a demo with sales
      </div>
    </VisualFrame>
  );
}

export function EcommerceVisual({ large = false }: AgentVisualProps) {
  return (
    <VisualFrame large={large} className="bg-[linear-gradient(135deg,#E9F7D7_0%,#A6DB7B_48%,#2E6C45_100%)]">
      <div className={cn("absolute rounded-[8px] bg-white p-2 shadow-[0_12px_28px_rgba(18,55,34,0.18)]", large ? "left-[21%] top-[13%] w-[58%]" : "left-5 top-5 w-[74%]")}>
        <div className="mb-2 rounded-full bg-[#F2F5F0] px-2 py-1 text-[8px] font-medium text-[#55605B]">Looking for running shoes?</div>
        <div className="grid grid-cols-3 gap-1.5">
          {["#ECEFF3", "#D9D0BE", "#1F2428"].map((color, index) => (
            <div key={color} className="rounded-[6px] border border-[#E8E9EC] bg-white p-1">
              <div className="grid aspect-[4/3] place-items-center rounded-[5px]" style={{ backgroundColor: color }}>
                <span className="h-3 w-8 rounded-full bg-white/70 shadow-sm" />
              </div>
              <div className="mt-1 h-1.5 w-8 rounded-full bg-[#D7DCE2]" />
              <div className="mt-1 h-1.5 w-5 rounded-full bg-[#A8B0BA]" />
              <div className="mt-1 text-[7px] font-semibold text-[#22252B]">{index === 0 ? "Data" : "Rule"}</div>
            </div>
          ))}
        </div>
      </div>
      <div className="absolute bottom-3 right-3 grid size-8 place-items-center rounded-full bg-[#111111] text-white shadow-lg">
        <ShoppingBag className="size-4" />
      </div>
    </VisualFrame>
  );
}

export function ChatbotVisual({ large = false }: AgentVisualProps) {
  return (
    <DottedCanvas large={large}>
      <div className={cn("absolute left-2 top-2 rounded-[8px] border border-[#E3E4E8] bg-white/88 p-2 shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C]/90", large ? "h-[88%] w-[31%]" : "h-[118px] w-[36%]")}>
        <div className="mb-2 flex items-center gap-1.5 text-[9px] font-semibold text-[#202126] dark:text-white">
          <Bot className="size-3" />
          Playground
        </div>
        <div className="mb-2 rounded-[6px] bg-[#F4F5F7] px-2 py-1.5 text-[8px] text-[#18864B] dark:bg-[#202228]">Source mapped</div>
        {["Model", "Actions", "Instructions"].map((item) => (
          <div key={item} className="mb-1.5 h-5 rounded-[5px] border border-[#ECECF0] bg-white px-2 py-1 text-[7px] text-[#555B65] dark:border-[#2B2D33] dark:bg-[#111216] dark:text-[#BBC1CB]">
            {item}
          </div>
        ))}
      </div>

      <div className={cn("absolute rounded-[8px] border border-[#E3E4E8] bg-white shadow-[0_16px_34px_rgba(20,21,24,0.12)] dark:border-[#2B2D33] dark:bg-[#111216]", large ? "right-[13%] top-[12%] h-[76%] w-[46%]" : "right-3 top-5 h-[102px] w-[52%]")}>
        <div className="flex items-center justify-between border-b border-[#EEEFF2] px-3 py-2 dark:border-[#2B2D33]">
          <div className="flex items-center gap-1.5">
            <span className="grid size-5 place-items-center rounded-full bg-[#111111] text-white">
              <Sparkles className="size-3" />
            </span>
            <span className="text-[9px] font-semibold text-[#24262D] dark:text-white">Velion Chatbot</span>
          </div>
          <CircleDashed className="size-3 text-[#9AA0AA]" />
        </div>
        <div className="p-3">
          <div className="w-[72%] rounded-[10px] bg-[#F2F2F3] px-2 py-1.5 text-[9px] text-[#333740] dark:bg-[#202228] dark:text-[#E8ECF2]">Hi. What can I help you with?</div>
        </div>
        <div className="absolute bottom-2 left-3 right-3 flex items-center gap-1 rounded-full border border-[#E6E7EA] bg-white px-2 py-1.5 dark:border-[#2B2D33] dark:bg-[#17181C]">
          <span className="flex-1 text-[8px] text-[#A1A5AE]">Message…</span>
          <Send className="size-3 text-[#A1A5AE]" />
        </div>
      </div>
    </DottedCanvas>
  );
}

export function WorkflowVisual({ large = false }: AgentVisualProps) {
  const nodeClass = large ? "size-12" : "size-10";

  return (
    <DottedCanvas large={large}>
      <div className={cn("absolute left-2 top-2 rounded-[8px] border border-[#E4E5E9] bg-white/86 p-2 dark:border-[#2B2D33] dark:bg-[#17181C]/90", large ? "bottom-2 w-[148px]" : "bottom-2 w-[82px]")}>
        <div className="mb-2 flex items-center gap-1 text-[8px] font-semibold text-[#333740] dark:text-white">
          <Search className="size-3" />
          Tools
        </div>
        <div className="grid grid-cols-2 gap-1.5">
          {[Bot, Database, Globe2, TicketCheck, Split, Code2].map((ToolIcon) => (
            <span key={ToolIcon.displayName ?? ToolIcon.name} className="grid aspect-square place-items-center rounded-[6px] bg-[#F1F2F4] text-[#555B65] dark:bg-[#22242A] dark:text-[#D0D6E0]">
              <ToolIcon className="size-3.5" />
            </span>
          ))}
        </div>
      </div>

      <div className={cn("absolute flex items-center", large ? "left-[28%] top-[42%] gap-10" : "left-[38%] top-[42%] gap-5")}>
        <WorkflowNode className={nodeClass} icon={<CalendarClock className="size-4" />} label={large ? "Trigger" : undefined} />
        <Connector />
        <WorkflowNode className={nodeClass} icon={<Bot className="size-4" />} label={large ? "AI step" : undefined} />
        <Connector />
        <WorkflowNode className={nodeClass} icon={<GitBranch className="size-4" />} label={large ? "Branch" : undefined} />
      </div>

      {large ? (
        <div className="absolute bottom-5 left-[39%] flex w-[34%] items-center gap-2 rounded-full border border-[#E5E6EA] bg-white px-3 py-2 text-[10px] text-[#9196A1] shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C]">
          Describe your workflow to Velion
          <span className="ml-auto grid size-5 place-items-center rounded-full bg-[#111111] text-white">
            <Send className="size-3" />
          </span>
        </div>
      ) : null}

      <div className={cn("absolute right-2 top-2 rounded-[8px] border border-[#E4E5E9] bg-white/88 p-2 dark:border-[#2B2D33] dark:bg-[#17181C]/90", large ? "bottom-2 w-[152px]" : "hidden")}>
        <div className="mb-2 flex items-center justify-between text-[9px] font-semibold text-[#333740] dark:text-white">
          Generate caption
          <CircleDashed className="size-3 text-[#9AA0AA]" />
        </div>
        {["Provider", "Model", "Prompt"].map((item, index) => (
          <div key={item} className={cn("mb-2 rounded-[6px] border border-[#ECECF0] px-2 py-1.5 text-[8px] text-[#69707B] dark:border-[#2B2D33] dark:text-[#C0C6D0]", index === 2 ? "h-16" : "h-6")}>
            {item}
          </div>
        ))}
      </div>
    </DottedCanvas>
  );
}

function WorkflowNode({
  className,
  icon,
  label,
}: {
  className: string;
  icon: ReactNode;
  label?: string;
}) {
  return (
    <div className="relative flex flex-col items-center">
      <span className={cn("grid place-items-center rounded-[8px] border border-[#E0E1E6] bg-white text-[#262A33] shadow-[0_10px_24px_rgba(20,21,24,0.08)] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-white", className)}>
        {icon}
      </span>
      {label ? <span className="mt-2 text-[10px] font-medium text-[#545A64] dark:text-[#B6BDCA]">{label}</span> : null}
    </div>
  );
}

function Connector() {
  return (
    <span className="relative h-px w-9 bg-[#C9CDD4] dark:bg-[#3A3E48]">
      <span className="absolute -right-1 -top-1 size-2 rotate-45 border-r border-t border-[#C9CDD4] dark:border-[#3A3E48]" />
    </span>
  );
}
