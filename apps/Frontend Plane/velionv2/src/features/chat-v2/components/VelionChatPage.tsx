"use client";

import dynamic from "next/dynamic";
import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  AlertCircle,
  ArrowDown,
  Brain,
  Check,
  CheckCircle2,
  ChevronRight,
  Clock3,
  Copy,
  FileCode2,
  Link2,
  ListChecks,
  MessageSquare,
  MessageSquarePlus,
  MoreHorizontal,
  Paperclip,
  Pencil,
  RefreshCw,
  Sparkles,
  Square,
  ThumbsDown,
  ThumbsUp,
  Volume2,
  Wrench,
  X,
} from "lucide-react";
import type { ComposerSubmitPayload } from "@/features/chat-v2/components/VerevonComposer";
import { VerevonIconButton } from "@/components/ui/verevon-ui";
import { ChatMarkdown } from "@/features/chat-v2/components/ChatMarkdown";
import { EmptyChatPromptChips } from "@/features/chat-v2/components/EmptyChatPromptChips";
import type {
  ChatGroundingGraph,
  ChatGroundingSource,
  ChatKnowledgeGrounding,
} from "@/features/chat-v2/lib/chat-grounding";
import {
  consumeChatLaunchMotion,
  isAssistantPlaceholder,
  useVerevonChatWorkspace,
  type AgentTaskStep,
  type ChatArtifact,
  type ChatMessage,
  type ChatSession,
  type ChatToolCall,
  type Citation,
  type GeneratedFile,
  type TaskStepStatus,
} from "@/features/chat-v2/lib/chat-workspace";
import { formatRelative, formatTime, toolLabels } from "@/features/chat-v2/lib/chat-format";
import { imageArtifactSrc, selectLatestImageArtifact } from "@/features/chat-v2/lib/chat-artifacts";
import { useChatDashboardComposer } from "@/features/chat-v2/hooks/use-chat-dashboard-composer";
import type { DashboardComposerProps } from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import { cn } from "@/lib/utils";

const DashboardComposer = dynamic<DashboardComposerProps>(
  () => import("@/features/composer-v2/components/DashboardComposer").then((mod) => mod.DashboardComposer),
  { ssr: false },
);

export function VerevonChatPage() {
  const messageListRef = useRef<HTMLDivElement>(null);
  const chat = useVerevonChatWorkspace();
  const { activeSession, activeSessionId, composerDraft, copiedMessageId } = chat;
  const hasActiveMessages = Boolean(activeSession?.messages.length);
  const [launchMotion, setLaunchMotion] = useState(false);
  const [showScrollDown, setShowScrollDown] = useState(false);
  const [tabState, setTabState] = useState<{ sessionId: string | null; value: ChatTab }>({
    sessionId: activeSessionId ?? null,
    value: "chat",
  });
  const autoFollowRef = useRef(true);

  const messages = activeSession?.messages ?? [];
  const taskSteps = activeSession?.taskSteps ?? [];
  const evidenceSources = collectEvidenceSources(messages);
  const latestGrounding = collectLatestGrounding(messages);
  const artifacts = collectArtifacts(messages);
  const agentScreen = selectLatestImageArtifact(messages);
  const tab = tabState.sessionId === (activeSessionId ?? null) ? tabState.value : "chat";

  const isStreaming = messages.some((message) => message.status === "waiting");
  // Grows as deltas append → drives the streaming auto-follow effect.
  const streamSignal = messages.length > 0 ? messages[messages.length - 1].content.length : 0;

  const scrollToBottom = useCallback((behavior: ScrollBehavior = "smooth") => {
    const list = messageListRef.current;
    if (!list) {
      return;
    }
    list.scrollTo({ top: list.scrollHeight, behavior });
    autoFollowRef.current = true;
    // Functional updater returns the same value when already false → React
    // bails, so this can never feed an update loop.
    setShowScrollDown((value) => (value ? false : value));
  }, []);

  const handleScroll = () => {
    const list = messageListRef.current;
    if (!list) {
      return;
    }
    const distanceFromBottom = list.scrollHeight - list.scrollTop - list.clientHeight;
    autoFollowRef.current = distanceFromBottom < 80;
    setShowScrollDown(distanceFromBottom > 160);
  };

  useEffect(() => {
    if (!hasActiveMessages) {
      return;
    }

    const list = messageListRef.current;
    if (!list) {
      return;
    }

    list.scrollTo({ top: list.scrollHeight, behavior: "smooth" });
  }, [messages.length, activeSessionId, hasActiveMessages]);

  // Follow streamed tokens to the bottom only while the user is already there.
  useEffect(() => {
    if (autoFollowRef.current) {
      scrollToBottom(isStreaming ? "auto" : "smooth");
    }
  }, [streamSignal, isStreaming, scrollToBottom]);

  useEffect(() => {
    if (!activeSessionId || !hasActiveMessages || !consumeChatLaunchMotion(activeSessionId)) {
      return;
    }

    let timeout: number | null = null;
    const frame = window.requestAnimationFrame(() => {
      setLaunchMotion(true);
      timeout = window.setTimeout(() => setLaunchMotion(false), 1_200);
    });

    return () => {
      window.cancelAnimationFrame(frame);
      if (timeout) {
        window.clearTimeout(timeout);
      }
    };
  }, [activeSessionId, hasActiveMessages]);

  const handleTabChange = useCallback(
    (value: ChatTab) => {
      setTabState({
        sessionId: activeSessionId ?? null,
        value,
      });
    },
    [activeSessionId],
  );

  return (
    <div
      className={cn(
        "verevon-chat-page relative flex h-full min-h-0 w-full overflow-hidden bg-transparent text-[#202126] transition-colors dark:text-[#F7F8F8]",
        launchMotion ? "verevon-chat-page-launch" : "",
      )}
    >
      {launchMotion ? <div className="verevon-chat-launch-wash" aria-hidden="true" /> : null}
      <section className="relative z-10 flex min-w-0 flex-1 flex-col" aria-label="Verevon chat workspace">
        {hasActiveMessages ? (
          <ChatHeader
            activeSession={activeSession}
            onNewChat={chat.startNewChat}
            onRegenerate={chat.regenerate}
          />
        ) : null}
        {hasActiveMessages ? (
          <ChatTabs
            active={tab}
            onChange={handleTabChange}
            sourceCount={evidenceSources.length}
            stepCount={taskSteps.length}
            artifactCount={artifacts.length}
          />
        ) : null}
        {hasActiveMessages && tab === "chat" ? (
          <div
            ref={messageListRef}
            onScroll={handleScroll}
            className="min-h-0 flex-1 overflow-y-auto px-4 py-8 md:px-8"
          >
            <div className="mx-auto flex min-h-full w-full max-w-[840px] flex-col">
              <div className="verevon-chat-thread space-y-7 pb-16 md:pb-20">
                {activeSession?.messages.map((message, index) => {
                  const previous = index > 0 ? activeSession.messages[index - 1] : null;
                  const showDivider =
                    !previous || dayKey(previous.createdAt) !== dayKey(message.createdAt);
                  return (
                    <Fragment key={message.id}>
                      {showDivider ? <DateDivider value={message.createdAt} /> : null}
                      <MessageBlock
                        copied={copiedMessageId === message.id}
                        message={message}
                        onBranch={() => chat.branch(message.id)}
                        onCopy={() => {
                          void chat.copy(message);
                        }}
                        onEdit={(text) => chat.editAndResubmit(message.id, text)}
                        onRegenerate={chat.regenerate}
                      />
                    </Fragment>
                  );
                })}
              </div>
            </div>
          </div>
        ) : !hasActiveMessages ? (
          <EmptyChatState
            composerDraft={composerDraft}
            onSubmit={chat.submit}
          />
        ) : tab === "sources" ? (
          <SourcesPanel grounding={latestGrounding} sources={evidenceSources} />
        ) : tab === "artifacts" ? (
          <ArtifactsPanel artifacts={artifacts} />
        ) : (
          <StepsPanel steps={taskSteps} screen={agentScreen} onStopTask={chat.stopTask} />
        )}
        {hasActiveMessages && tab === "chat" ? (
          <div className="verevon-chat-composer-dock relative shrink-0 bg-[#FCFCFD]/92 px-4 pb-5 pt-3 backdrop-blur-xl dark:bg-[#101114]/90 md:px-6">
            {showScrollDown ? (
              <button
                type="button"
                onClick={() => scrollToBottom()}
                aria-label="Bla til bunnen"
                className="absolute -top-5 left-1/2 grid size-9 -translate-x-1/2 place-items-center rounded-full border border-[#E7E8EC] bg-white text-[#6F757E] shadow-[0_8px_20px_rgba(20,21,24,0.12)] transition hover:text-[#26282f] dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#AEB4C0] dark:hover:text-white"
              >
                <ArrowDown className="size-4" />
              </button>
            ) : null}
            <div className="mx-auto w-full max-w-[760px]">
              {isStreaming ? (
                <div className="mb-2 flex justify-center">
                  <button
                    type="button"
                    onClick={chat.stopTask}
                    className="inline-flex items-center gap-1.5 rounded-full border border-[#E2E3E9] bg-white px-3.5 py-1.5 text-[12px] font-semibold text-[#6F757E] shadow-sm transition hover:text-[#26282f] dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#AEB4C0] dark:hover:text-white"
                  >
                    <Square className="size-3" />
                    Stopp svar
                  </button>
                </div>
              ) : null}
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

type ChatTab = "chat" | "sources" | "steps" | "artifacts";

type EvidenceSource =
  | (Citation & { kind: "web" })
  | ChatGroundingSource;

function collectArtifacts(messages: ChatMessage[]): ChatArtifact[] {
  const byId = new Map<string, ChatArtifact>();
  for (const message of messages) {
    for (const artifact of message.artifacts ?? []) {
      const existing = byId.get(artifact.id);
      // Keep the highest version seen for a given artifact id.
      if (!existing || artifact.version >= existing.version) {
        byId.set(artifact.id, artifact);
      }
    }
  }
  return [...byId.values()];
}

function collectEvidenceSources(messages: ChatMessage[]): EvidenceSource[] {
  const seen = new Set<string>();
  const result: EvidenceSource[] = [];

  for (const message of messages) {
    for (const source of message.grounding?.sources ?? []) {
      const key = `knowledge:${source.documentId || source.id}`;
      if (!key || seen.has(key)) {
        continue;
      }
      seen.add(key);
      result.push(source);
    }

    for (const citation of message.citations ?? []) {
      const key = `web:${citation.url || citation.id}`;
      if (!key || seen.has(key)) {
        continue;
      }
      seen.add(key);
      result.push({ ...citation, kind: "web" });
    }
  }

  return result;
}

function collectLatestGrounding(messages: ChatMessage[]): ChatKnowledgeGrounding | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const grounding = messages[index]?.grounding;
    if (grounding) {
      return grounding;
    }
  }

  return null;
}

function hostname(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

function ChatTabs({
  active,
  onChange,
  sourceCount,
  stepCount,
  artifactCount,
}: {
  active: ChatTab;
  onChange: (tab: ChatTab) => void;
  sourceCount: number;
  stepCount: number;
  artifactCount: number;
}) {
  const tabs: Array<{ id: ChatTab; label: string; icon: ReactNode; count: number }> = [
    { id: "chat", label: "Chat", icon: <MessageSquare className="size-3.5" />, count: 0 },
    { id: "sources", label: "Kilder", icon: <Link2 className="size-3.5" />, count: sourceCount },
    { id: "artifacts", label: "Artefakter", icon: <FileCode2 className="size-3.5" />, count: artifactCount },
    { id: "steps", label: "Steg", icon: <ListChecks className="size-3.5" />, count: stepCount },
  ];

  return (
    <div className="flex shrink-0 items-center gap-1 border-b border-[#ECECF0]/80 bg-[#FCFCFD]/86 px-4 backdrop-blur-xl dark:border-[#25272D] dark:bg-[#101114]/86 md:px-6">
      {tabs.map((item) => {
        const selected = active === item.id;
        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onChange(item.id)}
            aria-current={selected}
            className={cn(
              "relative -mb-px flex items-center gap-1.5 border-b-2 px-3 py-2.5 text-[13px] font-medium transition",
              selected
                ? "border-[#C07B33] text-[#202126] dark:text-white"
                : "border-transparent text-[#858992] hover:text-[#202126] dark:text-[#8B929F] dark:hover:text-white",
            )}
          >
            {item.icon}
            {item.label}
            {item.count > 0 ? (
              <span className="rounded-full bg-[#F0F1F4] px-1.5 py-0.5 text-[10px] font-semibold text-[#6E737C] dark:bg-[#1C1E24] dark:text-[#AEB4C0]">
                {item.count}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

function EmptyPanel({
  icon,
  title,
  subtitle,
}: {
  icon: ReactNode;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center px-4 py-12 text-center">
      <div className="max-w-[320px]">
        <div className="mx-auto mb-3 grid size-11 place-items-center rounded-full border border-[#ECECEF] text-[#A8ADB5] dark:border-[#2A2C32] dark:text-[#6F7682]">
          {icon}
        </div>
        <p className="text-[15px] font-semibold text-[#202126] dark:text-white">{title}</p>
        <p className="mt-1 text-[13px] leading-[1.5] text-[#858992] dark:text-[#8B929F]">{subtitle}</p>
      </div>
    </div>
  );
}

function SourcesPanel({
  grounding,
  sources,
}: {
  grounding: ChatKnowledgeGrounding | null;
  sources: EvidenceSource[];
}) {
  if (sources.length === 0 && !grounding) {
    return (
      <EmptyPanel
        icon={<Link2 className="size-5" />}
        title="Ingen kilder ennå"
        subtitle="Interne kunnskapskilder og websøk dukker opp her når Verevon bruker dem i svaret."
      />
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-6 md:px-8">
      <div className="mx-auto w-full max-w-[760px] space-y-2.5">
        {grounding ? <GroundingOverviewCard grounding={grounding} /> : null}
        {sources.map((source, index) => (
          source.kind === "knowledge" ? (
            <article
              key={source.id}
              className="rounded-[14px] border border-[#ECECEF] bg-white p-4 dark:border-[#2A2C32] dark:bg-[#15161A]"
            >
              <div className="flex items-center gap-2 text-[12px] text-[#8A8F98] dark:text-[#8B929F]">
                <span className="grid size-5 shrink-0 place-items-center rounded-md bg-[#F0F1F4] text-[10px] font-semibold text-[#6E737C] dark:bg-[#1C1E24] dark:text-[#AEB4C0]">
                  {index + 1}
                </span>
                <span className="truncate">{source.provider} · {source.sourceType}</span>
                <span className="ml-auto rounded-full bg-[#F6EFE6] px-2 py-0.5 text-[10px] font-semibold text-[#B96E1D] dark:bg-[#2A2014] dark:text-[#E29A4D]">
                  Score {source.score.toFixed(2)}
                </span>
              </div>
              <p className="mt-1.5 text-[14px] font-semibold leading-snug text-[#202126] dark:text-white">
                {source.title}
              </p>
              <p className="mt-1 line-clamp-3 text-[13px] leading-[1.5] text-[#6E737C] dark:text-[#AEB4C0]">
                {source.snippet}
              </p>
              <div className="mt-3">
                <a
                  href={source.href}
                  className="inline-flex items-center gap-1.5 text-[12px] font-semibold text-[#C07B33] hover:text-[#A6631A] dark:text-[#E29A4D]"
                >
                  Open knowledge
                  <ChevronRight className="size-3.5" />
                </a>
              </div>
            </article>
          ) : (
            <a
              key={source.id || source.url || index}
              href={source.url}
              target="_blank"
              rel="noopener noreferrer"
              className="block rounded-[14px] border border-[#ECECEF] bg-white p-4 transition hover:border-[#E0E0E4] hover:shadow-[0_8px_24px_rgba(20,21,24,0.05)] dark:border-[#2A2C32] dark:bg-[#15161A] dark:hover:border-[#3A3D45]"
            >
              <div className="flex items-center gap-2 text-[12px] text-[#8A8F98] dark:text-[#8B929F]">
                <span className="grid size-5 shrink-0 place-items-center rounded-md bg-[#F0F1F4] text-[10px] font-semibold text-[#6E737C] dark:bg-[#1C1E24] dark:text-[#AEB4C0]">
                  {index + 1}
                </span>
                <span className="truncate">{hostname(source.url)}</span>
              </div>
              <p className="mt-1.5 text-[14px] font-semibold leading-snug text-[#202126] dark:text-white">
                {source.title || source.url}
              </p>
              {source.snippet ? (
                <p className="mt-1 line-clamp-2 text-[13px] leading-[1.5] text-[#6E737C] dark:text-[#AEB4C0]">
                  {source.snippet}
                </p>
              ) : null}
            </a>
          )
        ))}
      </div>
    </div>
  );
}

function GroundingOverviewCard({ grounding }: { grounding: ChatKnowledgeGrounding }) {
  return (
    <section className="rounded-[14px] border border-[#ECE7D8] bg-[#FCF8F1] p-4 dark:border-[#3A3123] dark:bg-[#19150F]">
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 rounded-full bg-[#F6EFE6] px-2.5 py-1 text-[11px] font-semibold text-[#B96E1D] dark:bg-[#2A2014] dark:text-[#E29A4D]">
          <Sparkles className="size-3.5" />
          Internal knowledge grounding
        </span>
        {grounding.lowConfidence ? (
          <span className="rounded-full bg-[#F9E6E2] px-2.5 py-1 text-[11px] font-semibold text-[#A2483B] dark:bg-[#2B1C1A] dark:text-[#E0897C]">
            Low confidence
          </span>
        ) : null}
      </div>
      <div className="mt-3 grid gap-2 sm:grid-cols-3">
        <GroundingMetric label="Sources" value={String(grounding.sourceCount)} />
        <GroundingMetric label="Facts" value={String(grounding.factCount)} />
        <GroundingMetric label="Graph nodes" value={String(grounding.graph?.nodes.length ?? 0)} />
      </div>
      {grounding.graph ? <GroundingGraphSummary graph={grounding.graph} traceId={grounding.traceId} /> : grounding.traceId ? (
        <p className="mt-3 text-[12px] text-[#8A8F98] dark:text-[#8B929F]">Trace {grounding.traceId}</p>
      ) : null}
    </section>
  );
}

function GroundingInlineSummary({ grounding }: { grounding: ChatKnowledgeGrounding }) {
  return (
    <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
      <span className="inline-flex items-center gap-1 rounded-full bg-[#F6EFE6] px-2 py-1 text-[10.5px] font-semibold text-[#B96E1D] dark:bg-[#2A2014] dark:text-[#E29A4D]">
        <Sparkles className="size-3" />
        {grounding.sourceCount} internal source{grounding.sourceCount === 1 ? "" : "s"}
      </span>
      <span className="rounded-full bg-[#F4F5F7] px-2 py-1 text-[10.5px] font-semibold text-[#6E737C] dark:bg-white/8 dark:text-[#C4CAD3]">
        {grounding.factCount} fact{grounding.factCount === 1 ? "" : "s"}
      </span>
      {grounding.graph?.nodes.length ? (
        <span className="rounded-full bg-[#F4F5F7] px-2 py-1 text-[10.5px] font-semibold text-[#6E737C] dark:bg-white/8 dark:text-[#C4CAD3]">
          {grounding.graph.nodes.length} graph node{grounding.graph.nodes.length === 1 ? "" : "s"}
        </span>
      ) : null}
      {grounding.lowConfidence ? (
        <span className="rounded-full bg-[#F9E6E2] px-2 py-1 text-[10.5px] font-semibold text-[#A2483B] dark:bg-[#2B1C1A] dark:text-[#E0897C]">
          Low confidence
        </span>
      ) : null}
    </div>
  );
}

function GroundingMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[10px] border border-[#EADFCC] bg-white/70 px-3 py-2 dark:border-[#3A3123] dark:bg-white/[0.04]">
      <p className="text-[11px] font-medium text-[#8A8F98] dark:text-[#8B929F]">{label}</p>
      <p className="mt-1 text-[14px] font-semibold text-[#202126] dark:text-white">{value}</p>
    </div>
  );
}

function GroundingGraphSummary({
  compact = false,
  graph,
  traceId,
}: {
  compact?: boolean;
  graph: ChatGroundingGraph;
  traceId?: string;
}) {
  return (
    <div className={cn(
      "mt-3 rounded-[10px] border border-[#EADFCC] bg-white/70 px-3 py-2 dark:border-[#3A3123] dark:bg-white/[0.04]",
      compact && "mt-0 border-0 bg-transparent px-0 py-0 dark:bg-transparent",
    )}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[11px] font-semibold text-[#202126] dark:text-white">Graph evidence</span>
        {traceId ? (
          <span className="text-[11px] text-[#8A8F98] dark:text-[#8B929F]">Trace {traceId}</span>
        ) : null}
      </div>
      {graph.communitySummaries.length > 0 ? (
        <div className="mt-2 space-y-1">
          {graph.communitySummaries.map((summary, index) => (
            <p key={`${graph.traceId ?? "graph"}-${index}`} className="text-[12px] leading-[1.45] text-[#6E737C] dark:text-[#AEB4C0]">
              {summary}
            </p>
          ))}
        </div>
      ) : null}
      {graph.nodes.length > 0 ? (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {graph.nodes.map((node) => (
            <span
              key={node.id}
              className="rounded-full bg-[#F4F5F7] px-2 py-1 text-[10.5px] font-semibold text-[#6E737C] dark:bg-white/8 dark:text-[#C4CAD3]"
            >
              {node.label}
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function StepsPanel({
  steps,
  screen,
  onStopTask,
}: {
  steps: AgentTaskStep[];
  screen?: ChatArtifact | null;
  onStopTask: () => void;
}) {
  if (steps.length === 0 && !screen) {
    return (
      <EmptyPanel
        icon={<ListChecks className="size-5" />}
        title="Ingen steg ennå"
        subtitle="Agentens arbeidssteg vises her mens en oppgave kjører."
      />
    );
  }

  const activeTask = steps.some((step) => step.status === "active" || step.status === "waiting");

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-6 md:px-8">
      <div className="mx-auto w-full max-w-[760px]">
        <div className="mb-4 flex items-center justify-between">
          <div>
            <p className="text-[14px] font-semibold text-[#202126] dark:text-white">Agent activity</p>
            <p className="text-[12px] font-medium text-[#8A8D96] dark:text-[#7A808B]">Live oppgavestatus</p>
          </div>
          <button
            type="button"
            onClick={onStopTask}
            disabled={!activeTask}
            className="inline-flex h-8 items-center gap-1.5 rounded-full border border-[#E2E3E9] bg-white px-3 text-[11px] font-semibold text-[#6F757E] transition hover:text-[#26282f] disabled:opacity-45 dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#AEB4C0] dark:hover:text-white"
          >
            <Square className="size-3" />
            Stopp
          </button>
        </div>
        {screen ? (
          <figure className="mb-4 overflow-hidden rounded-[14px] border border-[#ECECEF] bg-[#0B0B0D] dark:border-[#2A2C32]">
            {/* chat-parity Phase 3: latest agent screen (computer-use live view). */}
            {/* eslint-disable-next-line @next/next/no-img-element -- live agent frame, not a static asset */}
            <img
              src={imageArtifactSrc(screen.content)}
              alt={screen.title || "Agent screen"}
              className="mx-auto max-h-[420px] w-auto max-w-full object-contain"
            />
            <figcaption className="px-3 py-1.5 text-[11px] font-medium text-[#9AA0A9]">
              {screen.title || "Live screen"}
            </figcaption>
          </figure>
        ) : null}
        <div className="space-y-3">
          {steps.map((step, index) => (
            <TaskStep key={step.id} isLast={index === steps.length - 1} step={step} />
          ))}
        </div>
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes < 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatToolArgs(args: unknown): string {
  if (args == null) return "";
  if (typeof args === "string") return args;
  try {
    return JSON.stringify(args, null, 2);
  } catch {
    return String(args);
  }
}

const PROSE_ARTIFACT_KINDS = new Set(["markdown", "md", "doc", "text", "report"]);

function ArtifactsPanel({ artifacts }: { artifacts: ChatArtifact[] }) {
  if (artifacts.length === 0) {
    return (
      <EmptyPanel
        icon={<FileCode2 className="size-5" />}
        title="Ingen artefakter ennå"
        subtitle="Dokumenter, kode og andre artefakter Verevon lager dukker opp her."
      />
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-6 md:px-8">
      <div className="mx-auto w-full max-w-[820px] space-y-3">
        {artifacts.map((artifact) => (
          <ArtifactCard key={artifact.id} artifact={artifact} />
        ))}
      </div>
    </div>
  );
}

function ArtifactCard({ artifact }: { artifact: ChatArtifact }) {
  const [open, setOpen] = useState(true);
  const kind = artifact.kind.toLowerCase();
  const isImage = kind === "image";
  const isProse = PROSE_ARTIFACT_KINDS.has(kind);

  return (
    <div className="overflow-hidden rounded-[14px] border border-[#ECECEF] bg-white dark:border-[#2A2C32] dark:bg-[#15161A]">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-4 py-2.5 text-left"
      >
        <FileCode2 className="size-4 text-[#C07B33]" />
        <span className="truncate text-[13px] font-semibold text-[#202126] dark:text-white">
          {artifact.title || artifact.kind}
        </span>
        <span className="rounded-full bg-[#F0F1F4] px-1.5 py-0.5 text-[10px] font-medium text-[#6E737C] dark:bg-[#1C1E24] dark:text-[#AEB4C0]">
          {artifact.kind}
        </span>
        {artifact.version > 0 ? (
          <span className="text-[11px] font-medium text-[#A8ADB5] dark:text-[#6F7682]">v{artifact.version}</span>
        ) : null}
        <ChevronRight
          className={cn("ml-auto size-4 text-[#9AA0A9] transition-transform", open && "rotate-90")}
        />
      </button>
      {open ? (
        <div className="border-t border-[#ECECEF] px-4 py-3 dark:border-[#2A2C32]">
          {isImage ? (
            // eslint-disable-next-line @next/next/no-img-element -- data/remote URL, not a static asset
            <img
              src={imageArtifactSrc(artifact.content)}
              alt={artifact.title || "Generert bilde"}
              className="mx-auto max-h-[480px] w-auto max-w-full rounded-[10px] object-contain"
            />
          ) : isProse ? (
            <ChatMarkdown content={artifact.content} />
          ) : (
            <pre className="overflow-x-auto rounded-[10px] bg-[#FAFAF9] p-3 font-mono text-[12.5px] leading-relaxed text-[#3A3D45] dark:bg-[#16171B] dark:text-[#D6DAE2]">
              {artifact.content}
            </pre>
          )}
        </div>
      ) : null}
    </div>
  );
}

function ToolCallList({ calls }: { calls: ChatToolCall[] }) {
  if (calls.length === 0) {
    return null;
  }
  return (
    <div className="mt-2.5 space-y-1.5">
      {calls.map((call) => (
        <ToolCallCard key={call.id} call={call} />
      ))}
    </div>
  );
}

function ToolCallCard({ call }: { call: ChatToolCall }) {
  const [open, setOpen] = useState(false);
  const failed = Boolean(call.error) || call.status === "error";
  const running = !call.status || call.status === "running";
  const statusLabel = failed ? "feilet" : running ? "kjører …" : "fullført";
  const args = formatToolArgs(call.args);

  return (
    <div className="rounded-[10px] border border-[#ECECEF] bg-[#FBFBFA] dark:border-[#2A2C32] dark:bg-[#17181C]">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12.5px]"
      >
        <Wrench className="size-3.5 text-[#8A8F98]" />
        <span className="font-medium text-[#3A3D45] dark:text-[#D6DAE2]">{call.name}</span>
        <span
          className={cn(
            "ml-auto text-[11px] font-medium",
            failed
              ? "text-[#A2483B] dark:text-[#E0897C]"
              : running
                ? "text-[#8A8F98]"
                : "text-[#3E9E6E] dark:text-[#6FC79A]",
          )}
        >
          {statusLabel}
        </span>
        <ChevronRight className={cn("size-3.5 text-[#9AA0A9] transition-transform", open && "rotate-90")} />
      </button>
      {open && (args || call.output || call.error) ? (
        <div className="space-y-2 border-t border-[#ECECEF] px-3 py-2 dark:border-[#2A2C32]">
          {args ? (
            <pre className="overflow-x-auto rounded-[8px] bg-[#F5F5F4] p-2 font-mono text-[11.5px] text-[#3A3D45] dark:bg-[#1B1C21] dark:text-[#D6DAE2]">
              {args}
            </pre>
          ) : null}
          {call.output ? (
            <pre className="overflow-x-auto whitespace-pre-wrap rounded-[8px] bg-[#FAFAF9] p-2 font-mono text-[11.5px] text-[#3A3D45] dark:bg-[#16171B] dark:text-[#D6DAE2]">
              {call.output}
            </pre>
          ) : null}
          {call.error ? <p className="text-[12px] text-[#A2483B] dark:text-[#E0897C]">{call.error}</p> : null}
        </div>
      ) : null}
    </div>
  );
}

function GeneratedFiles({ files }: { files: GeneratedFile[] }) {
  if (files.length === 0) {
    return null;
  }
  return (
    <div className="mt-2.5 flex flex-wrap gap-2">
      {files.map((file) => (
        <a
          key={file.id}
          href={file.url}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center gap-1.5 rounded-full border border-[#E7E8EC] bg-white px-3 py-1 text-[11.5px] font-medium text-[#5C606B] transition hover:border-[#E0E0E4] dark:border-[#2A2C32] dark:bg-[#15161A] dark:text-[#C4CAD3]"
        >
          <Paperclip className="size-3" />
          {file.name}
          {file.size > 0 ? (
            <span className="text-[#B4B8C0] dark:text-[#5F6671]">{formatBytes(file.size)}</span>
          ) : null}
        </a>
      ))}
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
    <header className="flex h-[56px] shrink-0 items-center justify-between border-b border-[#ECECF0]/80 bg-[#FCFCFD]/86 px-4 backdrop-blur-xl dark:border-[#25272D] dark:bg-[#101114]/86 md:px-6">
      <div className="min-w-0">
        <h1 className="truncate text-[14px] font-semibold leading-5 text-[#202126] dark:text-white">
          {activeSession?.title ?? "Verevon Chat"}
        </h1>
        {activeSession ? (
          <p className="truncate text-[11px] font-medium leading-4 text-[#858992] dark:text-[#8B929F]">
            {activeSession.messages.length} messages · {activeSession.branchCount} regenerations
          </p>
        ) : null}
      </div>

      <div className="flex items-center gap-1.5">
        <TopLayerTooltip label="Regenerate latest response" placement="bottom">
          <VerevonIconButton
            type="button"
            aria-label="Regenerate latest response"
            disabled={!activeSession?.messages.some((message) => message.role === "user")}
            onClick={onRegenerate}
            size="md"
            radius="sm"
            className="text-[#6F757E] disabled:opacity-40 dark:text-[#AEB4C0]"
          >
            <RefreshCw className="size-4" />
          </VerevonIconButton>
        </TopLayerTooltip>
        <TopLayerTooltip label="New chat" placement="bottom">
          <VerevonIconButton
            type="button"
            aria-label="New chat"
            onClick={onNewChat}
            size="md"
            radius="sm"
            className="bg-[#111111] text-white hover:bg-[#2A2A2A] dark:bg-white dark:text-[#111111]"
          >
            <MessageSquarePlus className="size-4" />
          </VerevonIconButton>
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
    <div className="verevon-chat-empty flex min-h-0 flex-1 items-center justify-center px-4 py-10">
      <div className="w-full max-w-[980px] -translate-y-[2vh]">
        <div className="mb-8 flex items-center justify-center gap-3 text-center">
          <Sparkles className="size-7 text-[#DD7A1F]" strokeWidth={1.75} />
          <h1 className="text-[38px] font-[520] leading-none tracking-normal text-[#202126] dark:text-white sm:text-[50px]">
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

function dayKey(value: string) {
  return new Date(value).toDateString();
}

function formatDayLabel(value: string) {
  const date = new Date(value);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);

  if (date.toDateString() === today.toDateString()) {
    return "Today";
  }
  if (date.toDateString() === yesterday.toDateString()) {
    return "Yesterday";
  }

  return new Intl.DateTimeFormat("en", {
    weekday: "long",
    month: "short",
    day: "numeric",
  }).format(date);
}

function prettyModel(model: string) {
  const lower = model.toLowerCase();
  if (lower.includes("gpt-4o-mini")) return "GPT-4o Mini";
  if (lower.includes("gpt-4.1")) return "GPT-4.1";
  if (lower.includes("gpt-4o")) return "GPT-4o";
  if (lower.includes("claude")) return "Claude Sonnet";
  if (lower.includes("reason")) return "Verevon Reasoner";
  return model.length > 22 ? `${model.slice(0, 22)}…` : model;
}

function DateDivider({ value }: { value: string }) {
  return (
    <div className="verevon-chat-divider relative flex items-center justify-center py-1">
      <span className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-gradient-to-r from-transparent via-[#ECECEF] to-transparent dark:via-[#26282E]" />
      <span className="relative rounded-full px-3 text-[11px] font-semibold uppercase tracking-[0.09em] text-[#A8ADB5] dark:text-[#6F7682]">
        {formatDayLabel(value)}
      </span>
    </div>
  );
}

function ThinkingDots() {
  return (
    <span className="verevon-chat-thinking inline-flex items-center gap-2 text-[14px] font-medium text-[#8A8F98] dark:text-[#8B929F]">
      <span className="verevon-thinking-dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      Tenker
    </span>
  );
}

function formatLatency(ms: number) {
  if (ms < 1000) {
    return `${Math.round(ms)} ms`;
  }
  return `${(ms / 1000).toFixed(1)} s`;
}

/**
 * Collapsible reasoning trace (design ref image 3 — "Tenkte i Xs ›"). Renders
 * `message.reasoning` accumulated from `reasoning_delta` events. Auto-expands
 * while the model is still thinking, collapses once the answer is in.
 */
function ReasoningTrace({ text, streaming }: { text: string; streaming: boolean }) {
  const [open, setOpen] = useState(false);
  const trimmed = text.trim();
  if (!trimmed) {
    return null;
  }

  const expanded = open || streaming;

  return (
    <div className="mb-2.5">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={expanded}
        className="inline-flex items-center gap-1.5 text-[12px] font-medium text-[#8A8F98] transition hover:text-[#6E737C] dark:text-[#8B929F] dark:hover:text-[#C4CAD3]"
      >
        <Brain className="size-3.5 text-[#C07B33]" strokeWidth={1.9} />
        {streaming ? "Tenker …" : "Tenkte"}
        <ChevronRight className={cn("size-3.5 transition-transform", expanded && "rotate-90")} />
      </button>
      {expanded ? (
        <div className="mt-2 whitespace-pre-wrap border-l-2 border-[#ECECEF] pl-3 text-[13px] leading-[1.6] text-[#6E737C] dark:border-[#2A2C32] dark:text-[#A8AEB9]">
          {trimmed}
        </div>
      ) : null}
    </div>
  );
}

/**
 * Inline "Reasoning" chip that opens a popover with the real per-turn metrics
 * the Model Plane already emits (model, token counts, latency). No placeholder
 * fields — confidence/sector arrive once the backend emits `usage`/`reasoning`
 * events, then get added here.
 */
function ReasoningPopover({ message }: { message: ChatMessage }) {
  const [open, setOpen] = useState(false);
  const [activeTab, setActiveTab] = useState("general");
  const ref = useRef<HTMLDivElement>(null);

  const model = message.modelUsed ?? message.model;
  const { inputTokens, outputTokens, latencyMs, ttftMs, confidence, costUsd, reasoning, citations } =
    message;
  const groundingSources = message.grounding?.sources ?? [];
  const groundingGraph = message.grounding?.graph;
  const toolCalls = message.toolCalls ?? [];
  const hasMetrics =
    Boolean(model) ||
    inputTokens != null ||
    outputTokens != null ||
    latencyMs != null ||
    confidence != null ||
    costUsd != null ||
    Boolean(reasoning) ||
    groundingSources.length > 0 ||
    Boolean(groundingGraph) ||
    Boolean(citations?.length) ||
    toolCalls.length > 0;

  useEffect(() => {
    if (!open) {
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  if (!hasMetrics) {
    return null;
  }

  const rows: Array<{ label: string; value: string }> = [];
  if (model) rows.push({ label: "Modell", value: prettyModel(model) });
  if (inputTokens) rows.push({ label: "Input", value: `${inputTokens} tokens` });
  if (outputTokens) rows.push({ label: "Output", value: `${outputTokens} tokens` });
  if (ttftMs != null && ttftMs > 0) rows.push({ label: "Første token", value: formatLatency(ttftMs) });
  if (latencyMs != null && latencyMs > 0) rows.push({ label: "Total tid", value: formatLatency(latencyMs) });
  if (confidence != null) rows.push({ label: "Sikkerhet", value: `${Math.round(confidence * 100)}%` });
  if (costUsd != null && costUsd > 0) rows.push({ label: "Kostnad", value: `$${costUsd.toFixed(4)}` });

  const tabs: Array<{ id: string; label: string }> = [
    { id: "general", label: "Oversikt" },
    ...(reasoning ? [{ id: "insight", label: "Innsikt" }] : []),
    ...(toolCalls.length > 0 ? [{ id: "tools", label: "Verktøy" }] : []),
    ...(groundingSources.length > 0 || citations && citations.length > 0 || groundingGraph ? [{ id: "sources", label: "Kilder" }] : []),
  ];
  const currentTab = tabs.some((tabItem) => tabItem.id === activeTab) ? activeTab : "general";

  return (
    <div ref={ref} className="relative ml-auto">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-label="Vis reasoning-detaljer"
        className="inline-flex items-center gap-1.5 rounded-full border border-[#ECECEF] bg-[#FBFBFA] px-2.5 py-1 text-[10.5px] font-medium text-[#8A8F98] transition hover:border-[#E0E0E4] hover:text-[#6E737C] dark:border-[#2A2C32] dark:bg-[#17181C] dark:text-[#8B929F] dark:hover:text-[#C4CAD3]"
      >
        <Sparkles className="size-3 text-[#C07B33]" strokeWidth={2} />
        {model ? <span className="text-[#6E737C] dark:text-[#AEB4C0]">{prettyModel(model)}</span> : null}
        {outputTokens ? (
          <span className="text-[#B4B8C0] dark:text-[#5F6671]">· {outputTokens} tokens</span>
        ) : null}
      </button>

      {open ? (
        <div className="absolute bottom-full right-0 z-30 mb-2 w-80 rounded-[14px] border border-[#ECECEF] bg-white p-3 shadow-[0_18px_44px_rgba(20,21,24,0.12)] dark:border-[#2A2C32] dark:bg-[#15161A]">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-[13px] font-semibold text-[#26282f] dark:text-white">Reasoning</span>
            <button
              type="button"
              aria-label="Lukk"
              onClick={() => setOpen(false)}
              className="grid size-6 place-items-center rounded-md text-[#9AA0A9] transition hover:bg-[#F0F1F4] hover:text-[#26282f] dark:hover:bg-[#1C1E24] dark:hover:text-white"
            >
              <X className="size-3.5" />
            </button>
          </div>

          {tabs.length > 1 ? (
            <div className="mb-2.5 flex items-center gap-1 border-b border-[#ECECEF] dark:border-[#2A2C32]">
              {tabs.map((tabItem) => (
                <button
                  key={tabItem.id}
                  type="button"
                  onClick={() => setActiveTab(tabItem.id)}
                  className={cn(
                    "-mb-px border-b-2 px-2 py-1.5 text-[11.5px] font-medium transition",
                    currentTab === tabItem.id
                      ? "border-[#C07B33] text-[#26282f] dark:text-white"
                      : "border-transparent text-[#9AA0A9] hover:text-[#6E737C] dark:text-[#7A808B] dark:hover:text-[#C4CAD3]",
                  )}
                >
                  {tabItem.label}
                </button>
              ))}
            </div>
          ) : null}

          {currentTab === "general" ? (
            <dl className="space-y-1.5">
              {rows.map((row) => (
                <div key={row.label} className="flex items-center justify-between gap-3 text-[12px]">
                  <dt className="text-[#8A8F98] dark:text-[#8B929F]">{row.label}</dt>
                  <dd className="font-medium text-[#3A3D45] dark:text-[#E2E6EC]">{row.value}</dd>
                </div>
              ))}
            </dl>
          ) : null}

          {currentTab === "insight" && reasoning ? (
            <p className="max-h-48 overflow-y-auto whitespace-pre-wrap text-[12px] leading-[1.55] text-[#6E737C] dark:text-[#AEB4C0]">
              {reasoning}
            </p>
          ) : null}

          {currentTab === "tools" ? (
            <div className="space-y-1.5">
              {toolCalls.map((call) => {
                const failed = Boolean(call.error) || call.status === "error";
                const running = !call.status || call.status === "running";
                return (
                  <div key={call.id} className="flex items-center gap-2 text-[12px]">
                    <Wrench className="size-3.5 shrink-0 text-[#8A8F98]" />
                    <span className="truncate font-medium text-[#3A3D45] dark:text-[#D6DAE2]">{call.name}</span>
                    <span
                      className={cn(
                        "ml-auto text-[11px] font-medium",
                        failed
                          ? "text-[#A2483B] dark:text-[#E0897C]"
                          : running
                            ? "text-[#8A8F98]"
                            : "text-[#3E9E6E] dark:text-[#6FC79A]",
                      )}
                    >
                      {failed ? "feilet" : running ? "kjører …" : "fullført"}
                    </span>
                  </div>
                );
              })}
            </div>
          ) : null}

          {currentTab === "sources" ? (
            <div className="space-y-2">
              {groundingGraph ? (
                <GroundingGraphSummary compact graph={groundingGraph} traceId={message.grounding?.traceId} />
              ) : null}
              {groundingSources.map((source) => (
                <div key={source.id} className="rounded-[10px] border border-[#ECECEF] bg-[#FBFBFA] px-2.5 py-2 dark:border-[#2A2C32] dark:bg-[#17181C]">
                  <p className="truncate text-[11.5px] font-semibold text-[#26282f] dark:text-white">{source.title}</p>
                  <p className="mt-0.5 text-[11px] text-[#8A8F98] dark:text-[#8B929F]">
                    {source.provider} · {source.sourceType} · score {source.score.toFixed(2)}
                  </p>
                </div>
              ))}
              {citations?.map((citation) => (
                <a
                  key={citation.id}
                  href={citation.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  title={citation.snippet || citation.title}
                  className="block truncate text-[11.5px] font-medium text-[#C07B33] hover:underline dark:text-[#E29A4D]"
                >
                  {citation.title || citation.url}
                </a>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function ToolChips({ tools }: { tools: ChatMessage["tools"] }) {
  if (tools.length === 0) {
    return null;
  }

  return (
    <div className="mt-2.5 flex flex-wrap gap-1.5">
      {tools.map((tool) => (
        <span
          key={tool}
          className="inline-flex items-center rounded-full bg-[#F2F5FF] px-2 py-0.5 text-[10.5px] font-semibold text-[#3578F6] dark:bg-[#172236] dark:text-[#8BB7FF]"
        >
          {toolLabels[tool]}
        </span>
      ))}
    </div>
  );
}

function AttachmentChips({
  attachments,
  tone,
}: {
  attachments: ChatMessage["attachments"];
  tone: "assistant" | "user";
}) {
  if (attachments.length === 0) {
    return null;
  }

  return (
    <div className="mt-2.5 flex flex-wrap gap-2">
      {attachments.map((attachment) => (
        <AttachmentItem key={attachment.id} attachment={attachment} tone={tone} />
      ))}
    </div>
  );
}

function AttachmentItem({
  attachment,
  tone,
}: {
  attachment: ChatMessage["attachments"][number];
  tone: "assistant" | "user";
}) {
  const [failed, setFailed] = useState(false);
  const url = attachment.url;
  const isImage = Boolean(url) && attachment.type.startsWith("image/") && !failed;

  if (isImage && url) {
    return (
      <span className="block overflow-hidden rounded-[12px] border border-black/[0.06] dark:border-white/10">
        {/* eslint-disable-next-line @next/next/no-img-element -- object/data URL, not a remote asset */}
        <img
          src={url}
          alt={attachment.name}
          className="h-24 w-24 object-cover"
          onError={() => setFailed(true)}
        />
      </span>
    );
  }

  return (
    <span
      className={cn(
        "rounded-full px-2.5 py-1 text-[11px] font-medium ring-1",
        tone === "assistant"
          ? "bg-[#F4F5F7] text-[#5C606B] ring-[#E7E8EC] dark:bg-white/8 dark:text-[#C4CAD3] dark:ring-white/10"
          : "bg-white/70 text-[#5C606B] ring-black/5 dark:bg-white/10 dark:text-[#C4CAD3] dark:ring-white/10",
      )}
    >
      {attachment.name}
    </span>
  );
}

function ErrorNotice({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className="flex flex-col items-start gap-2.5 rounded-[12px] border border-[#F0D9D4] bg-[#FCF4F2] px-3.5 py-3 dark:border-[#3A2A28] dark:bg-[#1F1715]">
      <div className="flex items-start gap-2 text-[14px] leading-[1.55] text-[#A2483B] dark:text-[#E0897C]">
        <AlertCircle className="mt-0.5 size-4 shrink-0" />
        <span>{message}</span>
      </div>
      <button
        type="button"
        onClick={onRetry}
        className="inline-flex items-center gap-1.5 rounded-full border border-[#E7D3CE] bg-white px-3 py-1 text-[12px] font-semibold text-[#A2483B] transition hover:bg-[#FBEEEB] dark:border-[#3A2A28] dark:bg-[#241A18] dark:text-[#E0897C] dark:hover:bg-[#2A1E1B]"
      >
        <RefreshCw className="size-3.5" />
        Prøv igjen
      </button>
    </div>
  );
}

type MessageMenuItem = {
  label: string;
  icon: ReactNode;
  onClick: () => void;
};

/** Speak text via the browser Web Speech API — fully client-side. */
function readAloud(text: string) {
  if (typeof window === "undefined" || !("speechSynthesis" in window)) {
    return;
  }
  const clean = text.trim();
  if (!clean) {
    return;
  }
  window.speechSynthesis.cancel();
  const utterance = new SpeechSynthesisUtterance(clean);
  utterance.lang = "nb-NO";
  window.speechSynthesis.speak(utterance);
}

function MessageMenu({
  items,
  align = "start",
}: {
  items: MessageMenuItem[];
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <TopLayerTooltip label="Flere handlinger" placement="top">
        <button
          type="button"
          aria-label="Flere handlinger"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className="grid size-7 place-items-center rounded-[8px] text-[#9AA0A9] transition hover:bg-[#F0F1F4] hover:text-[#26282f] dark:text-[#7A808B] dark:hover:bg-[#1C1E24] dark:hover:text-white"
        >
          <MoreHorizontal className="size-3.5" />
        </button>
      </TopLayerTooltip>
      {open ? (
        <div
          className={cn(
            "absolute bottom-full z-30 mb-1 w-52 rounded-[12px] border border-[#ECECEF] bg-white p-1 shadow-[0_18px_44px_rgba(20,21,24,0.12)] dark:border-[#2A2C32] dark:bg-[#15161A]",
            align === "end" ? "right-0" : "left-0",
          )}
        >
          {items.map((item) => (
            <button
              key={item.label}
              type="button"
              onClick={() => {
                item.onClick();
                setOpen(false);
              }}
              className="flex w-full items-center gap-2.5 rounded-[8px] px-2.5 py-1.5 text-left text-[13px] text-[#3A3D45] transition hover:bg-[#F4F5F7] dark:text-[#D6DAE2] dark:hover:bg-[#1C1E24]"
            >
              <span className="text-[#8A8F98] dark:text-[#8B929F]">{item.icon}</span>
              {item.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function MessageBlock({
  copied,
  message,
  onBranch,
  onCopy,
  onEdit,
  onRegenerate,
}: {
  copied: boolean;
  message: ChatMessage;
  onBranch: () => void;
  onCopy: () => void;
  onEdit: (text: string) => void;
  onRegenerate: () => void;
}) {
  const [reaction, setReaction] = useState<"up" | "down" | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.content);
  const assistant = message.role === "assistant";
  const waiting = message.status === "waiting";
  const errored = message.status === "error";
  const stopped = message.status === "stopped";

  const startEditing = () => {
    setDraft(message.content);
    setEditing(true);
  };

  const submitEdit = () => {
    const next = draft.trim();
    if (!next) {
      return;
    }
    setEditing(false);
    onEdit(next);
  };

  if (assistant) {
    return (
      <article className="verevon-chat-message group flex flex-col">
        <div className="mb-2 flex items-center gap-2">
          <span className="grid size-6 place-items-center rounded-[8px] border border-[#E7E8ED] bg-white text-[#C07B33] shadow-sm dark:border-[#2D3037] dark:bg-[#17181D]">
            <Sparkles className="size-3.5" strokeWidth={1.8} />
          </span>
          <span className="text-[13px] font-semibold text-[#2B2D33] dark:text-white">Verevon</span>
          <span className="text-[11.5px] font-medium text-[#A8ADB5] dark:text-[#6F7682]">
            {formatRelative(message.createdAt)}
          </span>
        </div>

        <div className="min-w-0 pl-8">
          {message.reasoning ? (
            <ReasoningTrace text={message.reasoning} streaming={waiting} />
          ) : null}
          {waiting && (!message.content || isAssistantPlaceholder(message.content)) && !message.reasoning ? (
            <ThinkingDots />
          ) : errored ? (
            <ErrorNotice message={message.content} onRetry={onRegenerate} />
          ) : (
            <div className={cn(waiting && "verevon-chat-streaming")}>
              {message.content && !isAssistantPlaceholder(message.content) ? (
                <ChatMarkdown content={message.content} />
              ) : null}
              {stopped ? (
                <span className="mt-1 inline-flex items-center gap-1 text-[11.5px] font-medium text-[#A8ADB5] dark:text-[#6F7682]">
                  <Square className="size-3" />
                  Stoppet
                </span>
              ) : null}
            </div>
          )}
          {message.grounding ? <GroundingInlineSummary grounding={message.grounding} /> : null}
          <ToolChips tools={message.tools} />
          <AttachmentChips attachments={message.attachments} tone="assistant" />
          {message.toolCalls?.length ? <ToolCallList calls={message.toolCalls} /> : null}
          {message.files?.length ? <GeneratedFiles files={message.files} /> : null}
          {message.artifacts?.length ? (
            <div className="mt-2.5 flex flex-wrap gap-1.5">
              {message.artifacts.map((artifact) => (
                <span
                  key={artifact.id}
                  className="inline-flex items-center gap-1.5 rounded-full border border-[#ECECEF] bg-[#FBFBFA] px-2.5 py-1 text-[11px] font-medium text-[#6E737C] dark:border-[#2A2C32] dark:bg-[#17181C] dark:text-[#AEB4C0]"
                >
                  <FileCode2 className="size-3 text-[#C07B33]" />
                  {artifact.title || artifact.kind}
                </span>
              ))}
            </div>
          ) : null}

          {waiting || errored ? null : (
            <div className="mt-3 flex items-center gap-0.5">
              <MessageAction label={copied ? "Copied" : "Copy"} onClick={onCopy}>
                {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
              </MessageAction>
              <MessageAction
                active={reaction === "up"}
                label="Good response"
                onClick={() => setReaction((current) => (current === "up" ? null : "up"))}
              >
                <ThumbsUp className="size-3.5" />
              </MessageAction>
              <MessageAction
                active={reaction === "down"}
                label="Bad response"
                onClick={() => setReaction((current) => (current === "down" ? null : "down"))}
              >
                <ThumbsDown className="size-3.5" />
              </MessageAction>
              <MessageAction label="Regenerate" onClick={onRegenerate}>
                <RefreshCw className="size-3.5" />
              </MessageAction>
              <MessageMenu
                items={[
                  {
                    label: "Fortsett i ny chat",
                    icon: <MessageSquarePlus className="size-4" />,
                    onClick: onBranch,
                  },
                  {
                    label: "Les høyt",
                    icon: <Volume2 className="size-4" />,
                    onClick: () => readAloud(message.content),
                  },
                ]}
              />
              <ReasoningPopover message={message} />
            </div>
          )}
        </div>
      </article>
    );
  }

  return (
    <article className="verevon-chat-message group flex flex-col items-end">
      <div className="mb-1 flex items-center gap-2 pr-1 text-[11.5px] font-medium text-[#9AA0A9] dark:text-[#737A85]">
        <span className="font-semibold text-[#6E737C] dark:text-[#AEB4C0]">Meg</span>
        <span>{formatRelative(message.createdAt)}</span>
      </div>

      {editing ? (
        <div className="w-[440px] max-w-full rounded-[18px] border border-[#E2E3E9] bg-white p-3 shadow-sm dark:border-[#2C2E34] dark:bg-[#1A1B20]">
          <textarea
            autoFocus
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                submitEdit();
              } else if (event.key === "Escape") {
                setEditing(false);
                setDraft(message.content);
              }
            }}
            rows={Math.min(10, Math.max(2, draft.split("\n").length))}
            className="w-full resize-none bg-transparent text-[15px] leading-[1.6] text-[#26282D] outline-none placeholder:text-[#9AA0A9] dark:text-[#F2F4F7]"
          />
          <div className="mt-2 flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={() => {
                setEditing(false);
                setDraft(message.content);
              }}
              className="rounded-full px-3 py-1 text-[12px] font-semibold text-[#6F757E] transition hover:bg-[#F0F1F4] dark:text-[#AEB4C0] dark:hover:bg-[#1C1E24]"
            >
              Avbryt
            </button>
            <button
              type="button"
              onClick={submitEdit}
              disabled={!draft.trim()}
              className="rounded-full bg-[#111111] px-3.5 py-1 text-[12px] font-semibold text-white transition hover:bg-[#2A2A2A] disabled:opacity-40 dark:bg-white dark:text-[#111111]"
            >
              Send på nytt
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="verevon-chat-bubble w-fit max-w-[560px] rounded-[18px] rounded-tr-[6px] bg-[#F3F3F4] px-4 py-2.5 text-[15px] leading-[1.6] text-[#26282D] dark:bg-[#202227] dark:text-[#F2F4F7]">
            <span className="whitespace-pre-wrap">{message.content}</span>
            <ToolChips tools={message.tools} />
            <AttachmentChips attachments={message.attachments} tone="user" />
          </div>
          <div className="mt-1.5 flex items-center gap-0.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100">
            <MessageAction label="Rediger" onClick={startEditing}>
              <Pencil className="size-3.5" />
            </MessageAction>
            <MessageAction label={copied ? "Copied" : "Copy"} onClick={onCopy}>
              {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            </MessageAction>
            <MessageMenu
              align="end"
              items={[
                {
                  label: "Fortsett i ny chat",
                  icon: <MessageSquarePlus className="size-4" />,
                  onClick: onBranch,
                },
              ]}
            />
          </div>
        </>
      )}
    </article>
  );
}

function MessageAction({
  active = false,
  children,
  label,
  onClick,
}: {
  active?: boolean;
  children: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <TopLayerTooltip label={label} placement="top">
      <button
        type="button"
        aria-label={label}
        aria-pressed={active}
        onClick={onClick}
        className={cn(
          "grid size-7 place-items-center rounded-[8px] transition",
          active
            ? "bg-[#FBF0E4] text-[#C07B33] dark:bg-[#2A2014] dark:text-[#E29A4D]"
            : "text-[#9AA0A9] hover:bg-[#F0F1F4] hover:text-[#26282f] dark:text-[#7A808B] dark:hover:bg-[#1C1E24] dark:hover:text-white",
        )}
      >
        {children}
      </button>
    </TopLayerTooltip>
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
