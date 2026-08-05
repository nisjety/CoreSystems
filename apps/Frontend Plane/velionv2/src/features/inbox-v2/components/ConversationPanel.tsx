"use client";

import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import {
  CheckCheck,
  ChevronDown,
  Clock3,
  Image as ImageIcon,
  Link2,
  Mail,
  MessageCircle,
  MoreHorizontal,
  Paperclip,
  PenLine,
  Plus,
  Search,
  Sparkles,
  Star,
  Tag,
  type LucideIcon,
  X,
  Zap,
} from "lucide-react";
import {
  customerInitials,
  customerName,
  formatDate,
  formatRelativeTime,
  formatTimestamp,
  stripHtml,
  type Agent,
  type Group,
  type TicketSentiment,
  type ZammadArticle,
  type ZammadTicket,
} from "@/features/inbox-v2/lib/inbox-model";
import { SentimentBadge } from "@/features/inbox-v2/components/SentimentBadge";
import type { InboxModalRequest } from "@/features/inbox-v2/components/InboxWorkModal";
import { cn } from "@/lib/utils";

const stateOptions = [
  { id: 1, label: "New" },
  { id: 2, label: "Open" },
  { id: 4, label: "Closed" },
  { id: 6, label: "Pending reminder" },
] as const;

const priorityOptions = [
  { id: 1, label: "Low" },
  { id: 2, label: "Normal" },
  { id: 3, label: "High" },
] as const;

const fieldSelectClass = "h-8 cursor-pointer appearance-none rounded-[8px] border border-[#E1DAD1] bg-white py-1 pl-2 pr-7 text-[12px] text-[#3F3A35] outline-none transition focus:border-[#DD7A1F] focus:ring-2 focus:ring-[#DD7A1F]/10 dark:border-[#303238] dark:bg-[#17181C] dark:text-[#D8DDE6]";

export function ConversationPanel({
  agents,
  articles,
  articlesLoading,
  groups,
  notice,
  onAddTag,
  onOpenModal,
  onPatchTicket,
  onRemoveTag,
  onSendReply,
  onSuggestReply,
  replyText,
  replySending,
  selectedTicket,
  sentiment,
  setReplyText,
}: {
  agents: Agent[];
  articles: ZammadArticle[];
  articlesLoading: boolean;
  groups: Group[];
  notice: string | null;
  onAddTag: (tag: string) => void;
  onOpenModal: (modal: InboxModalRequest) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void;
  onRemoveTag: (tag: string) => void;
  onSendReply: (text: string, internal: boolean) => void;
  onSuggestReply: () => void;
  replyText: string;
  replySending: boolean;
  selectedTicket: ZammadTicket | null;
  sentiment: TicketSentiment | null;
  setReplyText: (value: string) => void;
}) {
  const [isInternal, setIsInternal] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!selectedTicket || articlesLoading) return;
    const frame = window.requestAnimationFrame(() => {
      const container = scrollRef.current;
      if (!container) return;
      if (typeof container.scrollTo === "function") {
        container.scrollTo({ top: container.scrollHeight, behavior: "smooth" });
        return;
      }
      container.scrollTop = container.scrollHeight;
    });

    return () => window.cancelAnimationFrame(frame);
  }, [articles.length, articlesLoading, selectedTicket]);

  if (!selectedTicket) {
    return <ConversationEmptyState />;
  }

  const contactReason = sentiment?.sentiment ? titleCase(sentiment.sentiment) : selectedTicket.priority?.name ?? "Support request";

  return (
    <main className="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-[16px] border border-[#D8D2C8] bg-white text-[#111111] dark:border-[#2A2C31] dark:bg-[#15161A] dark:text-white">
      <ConversationHeader
        agents={agents}
        contactReason={contactReason}
        groups={groups}
        onAddTag={onAddTag}
        onOpenModal={onOpenModal}
        onPatchTicket={onPatchTicket}
        onRemoveTag={onRemoveTag}
        selectedTicket={selectedTicket}
        sentiment={sentiment}
      />
      <ConversationTranscript
        articles={articles}
        articlesLoading={articlesLoading}
        scrollRef={scrollRef}
        selectedTicket={selectedTicket}
      />
      <ConversationReplyComposer
        isInternal={isInternal}
        notice={notice}
        onOpenModal={onOpenModal}
        onPatchTicket={onPatchTicket}
        onSendReply={onSendReply}
        onSuggestReply={onSuggestReply}
        replySending={replySending}
        replyText={replyText}
        selectedTicket={selectedTicket}
        setIsInternal={setIsInternal}
        setReplyText={setReplyText}
      />
    </main>
  );
}

function ConversationEmptyState() {
  return (
    <main className="relative flex min-h-[520px] min-w-0 items-center justify-center overflow-hidden rounded-[16px] border border-[#D8D2C8] bg-white dark:border-[#2A2C31] dark:bg-[#15161A]">
      <div className="-mt-6 max-w-sm px-8 text-center">
        <div className="mx-auto grid size-16 place-items-center rounded-[18px] border border-[#DDD6CC] bg-[#F8F5F1]">
          <MessageCircle className="size-8 text-[#C8C1B7]" strokeWidth={1.45} />
        </div>
        <p className="mt-[18px] text-[15px] font-medium text-[#626260]">
          Select a ticket to view the conversation
        </p>
        <p className="mt-2 text-[13px] leading-5 text-[#9C9A96]">
          Customer details, conversation history, and the reply composer will appear here.
        </p>
      </div>
    </main>
  );
}

function ConversationHeader({
  agents,
  contactReason,
  groups,
  onAddTag,
  onOpenModal,
  onPatchTicket,
  onRemoveTag,
  selectedTicket,
  sentiment,
}: {
  agents: Agent[];
  contactReason: string;
  groups: Group[];
  onAddTag: (tag: string) => void;
  onOpenModal: (modal: InboxModalRequest) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void;
  onRemoveTag: (tag: string) => void;
  selectedTicket: ZammadTicket;
  sentiment: TicketSentiment | null;
}) {
  return (
    <div className="shrink-0 border-b border-[#E7E1D8] bg-white dark:border-[#2A2C31] dark:bg-[#15161A]">
      <div className="flex min-h-[72px] items-start justify-between gap-3 px-5 py-4">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate text-[20px] font-semibold leading-6 tracking-normal">{selectedTicket.title}</h2>
            <span className="shrink-0 text-[12px] text-[#7B7B78]">#{selectedTicket.number}</span>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-2 text-[12px] text-[#626260]">
            <span className="inline-flex items-center gap-1.5">
              <Clock3 className="size-3.5" />
              Updated {formatRelativeTime(selectedTicket.updated_at)} ago
            </span>
            {sentiment ? <SentimentBadge sentiment={sentiment.sentiment} /> : null}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          <HeaderIconButton
            label="Star conversation"
            onClick={() => onOpenModal({
              type: "work",
              title: "Watch conversation",
              description: "Keep this conversation in a monitored queue and let Verevon surface changes, SLA risk, and customer replies here.",
              primaryAction: "Start watch",
            })}
          >
            <Star className="size-4" />
          </HeaderIconButton>
          <HeaderIconButton
            label="More conversation actions"
            onClick={() => onOpenModal({
              type: "work",
              title: "Conversation actions",
              description: "Run assignment, status, priority, tags, side conversations, and audit actions in-place without leaving the inbox.",
              primaryAction: "Save action",
            })}
          >
            <MoreHorizontal className="size-4" />
          </HeaderIconButton>
          <button
            type="button"
            onClick={() => onPatchTicket({ state_id: 4 })}
            className="verevon-button verevon-button-primary verevon-button-xs verevon-button-radius-sm ml-1 px-3 font-semibold"
          >
            <CheckCheck className="size-3.5" />
            Close
          </button>
        </div>
      </div>

      <ConversationToolbar
        agents={agents}
        groups={groups}
        onAddTag={onAddTag}
        onPatchTicket={onPatchTicket}
        onRemoveTag={onRemoveTag}
        selectedTicket={selectedTicket}
      />

      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[#EEE8E0] px-5 py-3 text-[13px] dark:border-[#2A2C31]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="text-[#626260]">Contact reason:</span>
          <span className="font-semibold text-[#111111] dark:text-white">{contactReason}</span>
        </div>
        <button
          type="button"
          onClick={() => onOpenModal({
            type: "work",
            title: "Conversation intelligence",
            description: "Review intent, sentiment, SLA, ownership, and suggested next actions for the selected ticket in this modal.",
            primaryAction: "Update context",
          })}
          className="text-[13px] font-medium text-[#006ADC] hover:underline"
        >
          Show more
        </button>
      </div>
    </div>
  );
}

function ConversationToolbar({
  agents,
  groups,
  onAddTag,
  onPatchTicket,
  onRemoveTag,
  selectedTicket,
}: {
  agents: Agent[];
  groups: Group[];
  onAddTag: (tag: string) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void;
  onRemoveTag: (tag: string) => void;
  selectedTicket: ZammadTicket;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-[#EEE8E0] px-5 py-3 dark:border-[#2A2C31]">
      {/* Backend handoff: keep these controls wired to ticket PATCH APIs and extend them with collision-safe optimistic updates. */}
      <button
        type="button"
        onClick={() => onPatchTicket({ state_id: 4 })}
        className="verevon-button verevon-button-secondary verevon-button-xs verevon-button-radius-sm px-3 font-medium"
      >
        <CheckCheck className="size-3.5" />
        Close
      </button>
      <TagEditor tags={selectedTicket.tags ?? []} onAdd={onAddTag} onRemove={onRemoveTag} />
      <SelectShell>
        <select
          aria-label="Conversation status"
          value={selectedTicket.state?.id ?? 2}
          onChange={(event) => onPatchTicket({ state_id: Number(event.target.value) })}
          className={fieldSelectClass}
        >
          {stateOptions.map((option) => (
            <option key={option.id} value={option.id}>{option.label}</option>
          ))}
        </select>
      </SelectShell>
      <SelectShell>
        <select
          aria-label="Conversation priority"
          value={selectedTicket.priority?.id ?? 2}
          onChange={(event) => onPatchTicket({ priority_id: Number(event.target.value) })}
          className={fieldSelectClass}
        >
          {priorityOptions.map((option) => (
            <option key={option.id} value={option.id}>{option.label}</option>
          ))}
        </select>
      </SelectShell>
      {agents.length ? (
        <SelectShell>
          <select
            aria-label="Assignee"
            value={selectedTicket.owner?.id ?? 0}
            onChange={(event) => onPatchTicket({ owner_id: Number(event.target.value) })}
            className={fieldSelectClass}
          >
            <option value={0}>Unassigned</option>
            {agents.map((agent) => (
              <option key={agent.id} value={agent.id}>{agent.firstname} {agent.lastname}</option>
            ))}
          </select>
        </SelectShell>
      ) : null}
      {groups.length ? (
        <SelectShell>
          <select
            aria-label="Group"
            value={selectedTicket.group?.id ?? 0}
            onChange={(event) => onPatchTicket({ group_id: Number(event.target.value) })}
            className={fieldSelectClass}
          >
            {groups.map((group) => (
              <option key={group.id} value={group.id}>{group.name}</option>
            ))}
          </select>
        </SelectShell>
      ) : null}
    </div>
  );
}

function ConversationTranscript({
  articles,
  articlesLoading,
  scrollRef,
  selectedTicket,
}: {
  articles: ZammadArticle[];
  articlesLoading: boolean;
  scrollRef: RefObject<HTMLDivElement | null>;
  selectedTicket: ZammadTicket;
}) {
  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto bg-white px-5 py-6 dark:bg-[#15161A]">
      <div className="mb-5 flex justify-center">
        <span className="rounded-full border border-[#E7E1D8] bg-[#FAF8F5] px-3 py-1 text-[11px] font-medium text-[#7B7B78] dark:border-[#2A2C31] dark:bg-[#202229]">
          {formatDate(selectedTicket.created_at)}
        </span>
      </div>

      {articlesLoading ? (
        <div className="space-y-4">
          {[1, 2].map((item) => (
            <div key={item} className="h-24 animate-pulse rounded-[16px] bg-[#F1ECE5]" />
          ))}
        </div>
      ) : articles.length ? (
        <div className="space-y-6">
          {articles.map((article) => (
            <ArticleBubble key={article.id} article={article} ticket={selectedTicket} />
          ))}
        </div>
      ) : (
        <p className="pt-8 text-center text-[14px] text-[#7B7B78]">No articles in this conversation.</p>
      )}
    </div>
  );
}

function ConversationReplyComposer({
  isInternal,
  notice,
  onOpenModal,
  onPatchTicket,
  onSendReply,
  onSuggestReply,
  replySending,
  replyText,
  selectedTicket,
  setIsInternal,
  setReplyText,
}: {
  isInternal: boolean;
  notice: string | null;
  onOpenModal: (modal: InboxModalRequest) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void;
  onSendReply: (text: string, internal: boolean) => void;
  onSuggestReply: () => void;
  replySending: boolean;
  replyText: string;
  selectedTicket: ZammadTicket;
  setIsInternal: (isInternal: boolean) => void;
  setReplyText: (value: string) => void;
}) {
  return (
    <div className="shrink-0 bg-white p-4 dark:bg-[#15161A]">
      {notice ? <p className="mb-2 text-[12px] font-medium text-[#006ADC]">{notice}</p> : null}
      <div className="overflow-hidden rounded-[12px] border border-[#E1DAD1] bg-white focus-within:border-[#DD7A1F] focus-within:ring-2 focus-within:ring-[#DD7A1F]/10 dark:border-[#303238] dark:bg-[#17181C]">
        <ConversationReplyComposerHeader
          isInternal={isInternal}
          selectedTicket={selectedTicket}
          setIsInternal={setIsInternal}
        />
        <MacroSearchButton onOpenModal={onOpenModal} />
        <textarea
          rows={4}
          value={replyText}
          onChange={(event) => setReplyText(event.target.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
              event.preventDefault();
              onSendReply(replyText, isInternal);
            }
          }}
          className="min-h-[126px] w-full resize-none bg-transparent p-4 text-[15px] leading-6 text-[#111111] outline-none placeholder:text-[#9C9A96] dark:text-white"
          placeholder={isInternal ? "Add an internal note…" : `Reply to ${customerName(selectedTicket)}…`}
          aria-label={isInternal ? "Add an internal note" : `Reply to ${customerName(selectedTicket)}`}
        />
        <ConversationReplyComposerFooter
          isInternal={isInternal}
          onOpenModal={onOpenModal}
          onPatchTicket={onPatchTicket}
          onSendReply={onSendReply}
          onSuggestReply={onSuggestReply}
          replySending={replySending}
          replyText={replyText}
        />
      </div>
    </div>
  );
}

function ConversationReplyComposerHeader({
  isInternal,
  selectedTicket,
  setIsInternal,
}: {
  isInternal: boolean;
  selectedTicket: ZammadTicket;
  setIsInternal: (isInternal: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-[#EEE8E0] px-3 py-2 dark:border-[#2A2C31]">
      <ModeButton active={!isInternal} icon={MessageCircle} label="Reply" onClick={() => setIsInternal(false)} />
      <ModeButton active={isInternal} icon={PenLine} label="Internal note" onClick={() => setIsInternal(true)} warning />
      <div className="ml-auto flex min-w-0 items-center gap-2 text-[13px] text-[#626260]">
        <span className="text-[#9C9A96]">To:</span>
        <span className="truncate font-medium text-[#111111] dark:text-white">{selectedTicket.customer?.email ?? customerName(selectedTicket)}</span>
        <ChevronDown className="size-3.5" />
      </div>
    </div>
  );
}

function MacroSearchButton({ onOpenModal }: { onOpenModal: (modal: InboxModalRequest) => void }) {
  return (
    <button
      type="button"
      onClick={() => onOpenModal({
        type: "work",
        title: "Macros",
        description: "Search, preview, and execute macros inside the inbox composer. Backend should return executable macro steps and required approvals here.",
        primaryAction: "Run macro",
      })}
      className="flex w-full items-center gap-2 border-b border-[#EEE8E0] px-3 py-2 text-left text-[13px] transition-colors hover:bg-[#FAF8F5] dark:border-[#2A2C31] dark:hover:bg-white/5"
    >
      <Zap className="size-4 text-[#111111] dark:text-white" />
      <Search className="size-4 text-[#9C9A96]" />
      <span className="truncate text-[#9C9A96]">Search macros by name, tags or body…</span>
      <ChevronDown className="ml-auto size-4 text-[#626260]" />
    </button>
  );
}

function ConversationReplyComposerFooter({
  isInternal,
  onOpenModal,
  onPatchTicket,
  onSendReply,
  onSuggestReply,
  replySending,
  replyText,
}: {
  isInternal: boolean;
  onOpenModal: (modal: InboxModalRequest) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void;
  onSendReply: (text: string, internal: boolean) => void;
  onSuggestReply: () => void;
  replySending: boolean;
  replyText: string;
}) {
  return (
    <div className="border-t border-[#EEE8E0] px-3 py-2 dark:border-[#2A2C31]">
      <SuggestedMacroChips onOpenModal={onOpenModal} />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-1">
          <IconButton label="AI assist" onClick={onSuggestReply}><Sparkles className="size-4" /></IconButton>
          <ComposerToolButtons onOpenModal={onOpenModal} />
          <span className="ml-2 hidden text-[12px] text-[#9C9A96] sm:inline">Use ⌘K for shortcuts</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            disabled={!replyText.trim() || replySending}
            onClick={() => onSendReply(replyText, isInternal)}
            className="verevon-button verevon-button-secondary verevon-button-sm verevon-button-radius-sm px-4 disabled:opacity-40"
          >
            {replySending ? "Sending…" : "Send"}
          </button>
          <button
            type="button"
            disabled={!replyText.trim() || replySending}
            onClick={() => {
              onSendReply(replyText, isInternal);
              onPatchTicket({ state_id: 4 });
            }}
            className="verevon-button verevon-button-primary verevon-button-sm verevon-button-radius-sm px-4 font-semibold disabled:opacity-40"
          >
            Send & Close
          </button>
        </div>
      </div>
    </div>
  );
}

function SuggestedMacroChips({ onOpenModal }: { onOpenModal: (modal: InboxModalRequest) => void }) {
  return (
    <div className="mb-2 flex flex-wrap items-center gap-2 text-[12px] text-[#7B7B78]">
      <span>Suggested macros</span>
      {["Generic: Sign Off", "Refund", "Shipping update"].map((macro) => (
        <button
          key={macro}
          type="button"
          onClick={() => onOpenModal({
            type: "work",
            title: macro,
            description: `Preview and execute the ${macro} macro without leaving this conversation.`,
            primaryAction: "Run macro",
          })}
          className="rounded-[6px] border border-[#D8D2C8] bg-[#F5F1EC] px-2 py-1 text-[12px] font-medium text-[#111111] hover:bg-[#EEE8E0]"
        >
          {macro}
        </button>
      ))}
    </div>
  );
}

function ComposerToolButtons({ onOpenModal }: { onOpenModal: (modal: InboxModalRequest) => void }) {
  return (
    <>
      <IconButton
        label="Attach file"
        onClick={() => onOpenModal({
          type: "work",
          title: "Attach file",
          description: "Attach files to this reply while preserving the current conversation state.",
          primaryAction: "Attach",
        })}
      >
        <Paperclip className="size-4" />
      </IconButton>
      <IconButton
        label="Attach image"
        onClick={() => onOpenModal({
          type: "work",
          title: "Attach image",
          description: "Attach images or screenshots to this reply without changing pages.",
          primaryAction: "Attach image",
        })}
      >
        <ImageIcon className="size-4" />
      </IconButton>
      <IconButton
        label="Insert link"
        onClick={() => onOpenModal({
          type: "work",
          title: "Insert link",
          description: "Add a source, order, tracker, or knowledge-base link directly into the composer.",
          primaryAction: "Insert link",
        })}
      >
        <Link2 className="size-4" />
      </IconButton>
    </>
  );
}

function ArticleBubble({ article, ticket }: { article: ZammadArticle; ticket: ZammadTicket }) {
  const agentMessage = article.sender?.toLowerCase() === "agent";
  const body = stripHtml(article.body ?? "");

  return (
    <article className="flex gap-3">
      <div className={cn("mt-1 grid size-9 shrink-0 place-items-center rounded-full text-[12px] font-semibold", agentMessage ? "bg-[#F0ECE6] text-[#626260]" : "bg-[#111111] text-white")}>
        {agentMessage ? "A" : customerInitials(ticket)}
      </div>
      <div className="min-w-0 flex-1">
        <div className="mb-2 flex flex-wrap items-center gap-2 text-[13px]">
          <span className={cn("font-semibold", agentMessage ? "text-[#006ADC]" : "text-[#111111] dark:text-white")}>
            {article.from || (agentMessage ? "Verevon Support" : customerName(ticket))}
          </span>
          <Mail className="size-3.5 text-[#9C9A96]" />
          <span className="text-[#9C9A96]">{formatTimestamp(article.created_at)}</span>
        </div>
        <div
          className={cn(
            "max-w-[760px] whitespace-pre-wrap rounded-[12px] px-4 py-3 text-[15px] leading-6",
            article.internal
              ? "border border-[#F2D497] bg-[#FFF7E8] text-[#6B4500]"
              : agentMessage
                ? "border border-[#E7E1D8] bg-white text-[#111111]"
                : "bg-[#F4F2EF] text-[#111111]",
          )}
        >
          {body || "No message body."}
        </div>
        {article.internal ? <span className="mt-2 inline-flex rounded-[5px] bg-[#FFF0C2] px-1.5 py-0.5 text-[10px] font-medium text-[#8A5A00]">Internal note</span> : null}
      </div>
    </article>
  );
}

function TagEditor({ tags, onAdd, onRemove }: { tags: string[]; onAdd: (tag: string) => void; onRemove: (tag: string) => void }) {
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");
  const focusTagInput = (node: HTMLInputElement | null) => {
    node?.focus();
  };

  function submit() {
    onAdd(draft);
    setDraft("");
    setAdding(false);
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <Tag className="size-3.5 text-[#7B7B78]" />
      {tags.map((tag) => (
        <span key={tag} className="inline-flex h-8 items-center gap-1 rounded-[8px] border border-[#D8D2C8] bg-[#FAF8F5] px-2 text-[12px] font-medium text-[#3F3A35]">
          {tag}
          <button type="button" onClick={() => onRemove(tag)} aria-label={`Remove tag ${tag}`}>
            <X className="size-3" />
          </button>
        </span>
      ))}
      {adding ? (
        <input
          ref={focusTagInput}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={submit}
          onKeyDown={(event) => {
            if (event.key === "Enter") submit();
            if (event.key === "Escape") setAdding(false);
          }}
          className="h-8 w-24 rounded-[8px] border border-[#D8D2C8] px-2 text-[12px] outline-none focus:border-[#DD7A1F]"
          placeholder="tag…"
          aria-label="New tag"
        />
      ) : (
        <button type="button" onClick={() => setAdding(true)} className="inline-flex h-8 items-center gap-1 rounded-[8px] px-2 text-[12px] font-medium text-[#626260] transition-colors hover:bg-[#F5F1EC]">
          <Plus className="size-3.5" />
          Add Tags
        </button>
      )}
    </div>
  );
}

function ModeButton({ active, icon: Icon, label, onClick, warning = false }: { active: boolean; icon: LucideIcon; label: string; onClick: () => void; warning?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-8 items-center gap-1.5 rounded-[8px] px-2 text-[13px] font-semibold transition-colors",
        active
          ? warning ? "bg-[#FFF7E8] text-[#9A6300]" : "bg-[#F5F1EC] text-[#111111]"
          : "text-[#626260] hover:bg-[#F5F1EC] hover:text-[#111111]",
      )}
    >
      <Icon className="size-3.5" />
      {label}
      {label === "Reply" ? <ChevronDown className="size-3.5" /> : null}
    </button>
  );
}

function IconButton({ children, label, onClick }: { children: ReactNode; label: string; onClick?: () => void }) {
  return (
    <button type="button" onClick={onClick} aria-label={label} title={label} className="verevon-icon-button verevon-icon-button-xs verevon-button-radius-sm">
      {children}
    </button>
  );
}

function HeaderIconButton({ children, label, onClick }: { children: ReactNode; label: string; onClick?: () => void }) {
  return (
    <button type="button" onClick={onClick} aria-label={label} title={label} className="verevon-icon-button verevon-button-radius-sm">
      {children}
    </button>
  );
}

function SelectShell({ children }: { children: ReactNode }) {
  return (
    <div className="relative">
      {children}
      <ChevronDown className="pointer-events-none absolute right-2 top-1/2 size-3 -translate-y-1/2 text-[#8B8780]" />
    </div>
  );
}

function titleCase(value: string) {
  return value
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join(" ");
}
