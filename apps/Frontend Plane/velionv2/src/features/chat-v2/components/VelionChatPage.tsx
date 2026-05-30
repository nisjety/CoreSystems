"use client";

import dynamic from "next/dynamic";
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  AlertCircle,
  Check,
  CheckCircle2,
  Clock3,
  Copy,
  GitBranch,
  MessageSquarePlus,
  RefreshCw,
  Sparkles,
  Square,
} from "lucide-react";
import type { ComposerSubmitPayload } from "@/features/chat-v2/components/VelionComposer";
import { VelionIconButton } from "@/components/ui/velion-ui";
import { EmptyChatPromptChips } from "@/features/chat-v2/components/EmptyChatPromptChips";
import {
  useVelionChatWorkspace,
  type AgentTaskStep,
  type ChatMessage,
  type ChatSession,
  type TaskStepStatus,
} from "@/features/chat-v2/lib/chat-workspace";
import { formatTime, toolLabels } from "@/features/chat-v2/lib/chat-format";
import { useChatDashboardComposer } from "@/features/chat-v2/hooks/use-chat-dashboard-composer";
import type { DashboardComposerProps } from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import { cn } from "@/lib/utils";

const DashboardComposer = dynamic<DashboardComposerProps>(
  () => import("@/features/composer-v2/components/DashboardComposer").then((mod) => mod.DashboardComposer),
  { ssr: false },
);

export function VelionChatPage() {
  const messageListRef = useRef<HTMLDivElement>(null);
  const chat = useVelionChatWorkspace();
  const { activeSession, activeSessionId, composerDraft, copiedMessageId } = chat;
  const hasActiveMessages = Boolean(activeSession?.messages.length);

  useEffect(() => {
    if (!hasActiveMessages) {
      return;
    }

    const list = messageListRef.current;
    if (!list) {
      return;
    }

    list.scrollTo({ top: list.scrollHeight, behavior: "smooth" });
  }, [activeSession?.messages.length, activeSessionId, hasActiveMessages]);

  return (
    <div className="flex h-full min-h-0 w-full overflow-hidden bg-transparent text-[#26282f] transition-colors dark:text-[#F7F8F8]">
      <section className="flex min-w-0 flex-1 flex-col" aria-label="Velion chat workspace">
        {hasActiveMessages ? (
          <ChatHeader
            activeSession={activeSession}
            onNewChat={chat.startNewChat}
            onRegenerate={chat.regenerate}
          />
        ) : null}
        {hasActiveMessages ? (
          <div ref={messageListRef} className="min-h-0 flex-1 overflow-y-auto px-4 py-6">
            <div className="mx-auto flex min-h-full w-full max-w-[880px] flex-col">
              <div className="space-y-7 pb-6">
                {activeSession?.messages.map((message) => (
                  <MessageBlock
                    key={message.id}
                    copied={copiedMessageId === message.id}
                    message={message}
                    onBranch={() => chat.branch(message.id)}
                    onCopy={() => {
                      void chat.copy(message);
                    }}
                  />
                ))}
                {activeSession?.taskSteps.length ? (
                  <TaskStreamCard steps={activeSession.taskSteps} onStopTask={chat.stopTask} />
                ) : null}
              </div>
            </div>
          </div>
        ) : (
          <EmptyChatState
            composerDraft={composerDraft}
            onSubmit={chat.submit}
          />
        )}
        {hasActiveMessages ? (
          <div className="shrink-0 bg-[#FCFCFD]/96 p-4 backdrop-blur dark:bg-[#101114]/94 md:px-6">
            <div className="mx-auto w-full max-w-[760px]">
              <ChatDashboardComposer
                key={`${activeSessionId ?? "new"}:${composerDraft}`}
                initialValue={composerDraft}
                onSubmit={chat.submit}
              />
            </div>
          </div>
        ) : null}
      </section>
    </div>
  );
}

function ChatDashboardComposer({
  initialValue,
  onSubmit,
}: {
  initialValue: string;
  onSubmit: (payload: ComposerSubmitPayload) => void;
}) {
  const composerProps = useChatDashboardComposer({ initialValue, onSubmit });

  return <DashboardComposer {...composerProps} />;
}

function ChatHeader({
  activeSession,
  onNewChat,
  onRegenerate,
}: {
  activeSession: ChatSession | null;
  onNewChat: () => void;
  onRegenerate: () => void;
}) {
  return (
    <header className="flex h-[58px] shrink-0 items-center justify-between bg-[var(--linear-main-bg)]/92 px-4 backdrop-blur dark:bg-[#101114]/90">
      <div className="min-w-0">
        <h1 className="truncate text-[15px] font-semibold text-[#26282f] dark:text-white">
          {activeSession?.title ?? "Velion Chat"}
        </h1>
        {activeSession ? (
          <p className="truncate text-[11px] font-medium text-[#8A8D96] dark:text-[#7A808B]">
            {activeSession.messages.length} messages · {activeSession.branchCount} regenerations
          </p>
        ) : null}
      </div>

      <div className="flex items-center gap-1.5">
        <TopLayerTooltip label="Regenerate latest response" placement="bottom">
          <VelionIconButton
            type="button"
            aria-label="Regenerate latest response"
            disabled={!activeSession?.messages.some((message) => message.role === "user")}
            onClick={onRegenerate}
            size="md"
            radius="sm"
            className="text-[#6F757E] disabled:opacity-40 dark:text-[#AEB4C0]"
          >
            <RefreshCw className="size-4" />
          </VelionIconButton>
        </TopLayerTooltip>
        <TopLayerTooltip label="New chat" placement="bottom">
          <VelionIconButton
            type="button"
            aria-label="New chat"
            onClick={onNewChat}
            size="md"
            radius="sm"
            className="bg-[#111111] text-white hover:bg-[#2A2A2A] dark:bg-white dark:text-[#111111]"
          >
            <MessageSquarePlus className="size-4" />
          </VelionIconButton>
        </TopLayerTooltip>
      </div>
    </header>
  );
}

function EmptyChatState({
  composerDraft,
  onSubmit,
}: {
  composerDraft: string;
  onSubmit: (payload: ComposerSubmitPayload) => void;
}) {
  const [selectedPrompt, setSelectedPrompt] = useState<string | null>(null);
  const composerValue = selectedPrompt ?? composerDraft;

  return (
    <div className="flex min-h-0 flex-1 items-center justify-center px-4 py-10">
      <div className="w-full max-w-[980px] -translate-y-[2vh]">
        <div className="mb-8 flex items-center justify-center gap-3 text-center">
          <Sparkles className="size-7 text-[#DD7A1F]" strokeWidth={1.75} />
          <h1 className="text-[38px] font-semibold leading-none tracking-normal text-[#202126] dark:text-white sm:text-[50px]">
            Hva kan jeg hjelpe med?
          </h1>
        </div>
        <div className="mx-auto w-full max-w-[760px]">
          <ChatDashboardComposer
            key={`empty:${composerValue}`}
            initialValue={composerValue}
            onSubmit={onSubmit}
          />
        </div>
        <EmptyChatPromptChips onSelectPrompt={setSelectedPrompt} />
      </div>
    </div>
  );
}

function MessageBlock({
  copied,
  message,
  onBranch,
  onCopy,
}: {
  copied: boolean;
  message: ChatMessage;
  onBranch: () => void;
  onCopy: () => void;
}) {
  const assistant = message.role === "assistant";

  return (
    <article className={cn("flex", assistant ? "justify-start" : "justify-end")}>
      <div className={cn("group max-w-[720px]", assistant ? "w-full" : "w-fit")}>
        {assistant ? (
          <div className="mb-2 flex items-center gap-2 text-[12px] font-semibold uppercase tracking-[0.12em] text-[#8A8D96] dark:text-[#7A808B]">
            <Sparkles className="size-3.5" />
            Velion
          </div>
        ) : null}
        <div
          className={cn(
            "text-[15px] leading-7",
            assistant
              ? "rounded-[18px] border border-[#ECECF1] bg-white px-4 py-3 text-[#26282f] shadow-[0_12px_32px_rgba(20,21,24,0.05)] dark:border-[#2A2C31] dark:bg-[#15161A] dark:text-[#F7F8F8]"
              : "rounded-[22px] bg-[#111111] px-4 py-3 text-white shadow-[0_12px_28px_rgba(17,17,17,0.12)] dark:bg-white dark:text-[#111111]",
          )}
        >
          {message.status === "waiting" ? (
            <span className="mr-2 inline-flex size-2 animate-pulse rounded-full bg-[#3578F6] align-middle" />
          ) : null}
          {message.content}
          {message.attachments.length > 0 ? (
            <div className="mt-3 flex flex-wrap gap-2">
              {message.attachments.map((attachment) => (
                <span key={attachment.id} className="rounded-full bg-white/12 px-2.5 py-1 text-[11px] font-medium text-current ring-1 ring-white/18">
                  {attachment.name}
                </span>
              ))}
            </div>
          ) : null}
          {message.tools.length > 0 ? (
            <div className="mt-3 flex flex-wrap gap-1.5">
              {message.tools.map((tool) => (
                <span key={tool} className={cn("rounded-full px-2 py-0.5 text-[10px] font-semibold", assistant ? "bg-[#F2F5FF] text-[#3578F6] dark:bg-[#172236] dark:text-[#8BB7FF]" : "bg-white/15 text-white dark:bg-[#111111]/10 dark:text-[#111111]")}>
                  {toolLabels[tool]}
                </span>
              ))}
            </div>
          ) : null}
        </div>
        <div className={cn("mt-2 flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100", assistant ? "justify-start" : "justify-end")}>
          <MessageAction label={copied ? "Copied" : "Copy"} onClick={onCopy}>
            {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          </MessageAction>
          <MessageAction label="Branch from here" onClick={onBranch}>
            <GitBranch className="size-3.5" />
          </MessageAction>
        </div>
      </div>
    </article>
  );
}

function MessageAction({
  children,
  label,
  onClick,
}: {
  children: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <TopLayerTooltip label={label} placement="top">
      <button
        type="button"
        aria-label={label}
        onClick={onClick}
        className="grid size-8 place-items-center rounded-full text-[#7A8089] transition hover:bg-[#F0F1F4] hover:text-[#26282f] dark:text-[#8A909B] dark:hover:bg-[#1C1E24] dark:hover:text-white"
      >
        {children}
      </button>
    </TopLayerTooltip>
  );
}

function TaskStreamCard({
  steps,
  onStopTask,
}: {
  steps: AgentTaskStep[];
  onStopTask: () => void;
}) {
  const activeTask = steps.some((step) => step.status === "active" || step.status === "waiting");

  return (
    <section className="velion-panel ml-0 max-w-[720px] p-4" aria-label="Agent activity">
      <div className="flex items-center justify-between gap-3">
        <div>
          <p className="text-[13px] font-semibold text-[#26282f] dark:text-white">Agent activity</p>
          <p className="text-[11px] font-medium text-[#8A8D96] dark:text-[#7A808B]">Live task status</p>
        </div>
        <button
          type="button"
          onClick={onStopTask}
          disabled={!activeTask}
          className="inline-flex h-8 items-center gap-1.5 rounded-full border border-[#E2E3E9] bg-white px-3 text-[11px] font-semibold text-[#6F757E] transition hover:text-[#26282f] disabled:opacity-45 dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#AEB4C0] dark:hover:text-white"
        >
          <Square className="size-3" />
          Stop
        </button>
      </div>

      <div className="mt-4 space-y-3">
        {steps.map((step, index) => (
          <TaskStep key={step.id} isLast={index === steps.length - 1} step={step} />
        ))}
      </div>
    </section>
  );
}

function TaskStep({ isLast, step }: { isLast: boolean; step: AgentTaskStep }) {
  const icon = getTaskStepIcon(step.status);

  return (
    <div className="relative flex gap-3">
      {!isLast ? <span className="absolute left-[10px] top-6 h-[calc(100%+4px)] w-px bg-[#E4E5EA] dark:bg-[#2A2C31]" /> : null}
      <span className={cn("relative z-10 mt-0.5 grid size-5 shrink-0 place-items-center rounded-full border bg-white dark:bg-[#111216]", icon.className)}>
        {icon.node}
      </span>
      <div className="min-w-0 pb-2">
        <div className="flex items-center gap-2">
          <p className="truncate text-[13px] font-semibold text-[#26282f] dark:text-white">{step.title}</p>
          <span className="shrink-0 text-[10px] font-medium text-[#A1A5AE] dark:text-[#6F7682]">{formatTime(step.createdAt)}</span>
        </div>
        <p className="mt-1 text-[12px] leading-5 text-[#6F757E] dark:text-[#AEB4C0]">{step.detail}</p>
      </div>
    </div>
  );
}

function getTaskStepIcon(status: TaskStepStatus) {
  if (status === "done") {
    return {
      className: "border-[#10B981] text-[#10B981]",
      node: <CheckCircle2 className="size-3.5" />,
    };
  }

  if (status === "active") {
    return {
      className: "border-[#3578F6] text-[#3578F6]",
      node: <Clock3 className="size-3.5 animate-pulse" />,
    };
  }

  if (status === "waiting") {
    return {
      className: "border-[#D69E2E] text-[#B7791F]",
      node: <AlertCircle className="size-3.5" />,
    };
  }

  return {
    className: "border-[#A1A5AE] text-[#8A8D96]",
    node: <Square className="size-2.5" />,
  };
}
