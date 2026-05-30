"use client";

import { useEffect, useEffectEvent, useState } from "react";
import {
  Bot,
  CalendarDays,
  CheckCircle2,
  Clock3,
  FileText,
  MessageCircle,
  Play,
  Settings,
  Tag,
  X,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  customerName,
  type Agent,
  type Group,
  type ZammadTicket,
} from "@/features/inbox-v2/lib/inbox-model";
import { apiSend } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

export type InboxModalRequest =
  | {
    type: "view";
    title: string;
    description: string;
    sourceHref?: string;
  }
  | {
    type: "work";
    title: string;
    description: string;
    primaryAction?: string;
  }
  | {
    type: "velion";
    prompt?: string;
  };

type ToolActionId =
  | "draft-reply"
  | "schedule-follow-up"
  | "raise-priority"
  | "route-support"
  | "internal-note"
  | "close-ticket";

type ToolRunLog = {
  id: string;
  label: string;
  status: "done" | "failed" | "running";
  detail: string;
};

type CalendarEvent = {
  id: string;
  title: string;
  start: string;
  end: string;
  type: string;
  status: string;
  createdAt: string;
};

const toolActions: Array<{
  id: ToolActionId;
  label: string;
  description: string;
  icon: LucideIcon;
}> = [
  {
    id: "draft-reply",
    label: "Draft reply",
    description: "Prepare a customer-ready answer in the composer.",
    icon: MessageCircle,
  },
  {
    id: "schedule-follow-up",
    label: "Schedule follow-up",
    description: "Create a calendar item linked to the conversation.",
    icon: CalendarDays,
  },
  {
    id: "raise-priority",
    label: "Raise priority",
    description: "Move urgent conversations to high priority.",
    icon: Zap,
  },
  {
    id: "route-support",
    label: "Route to team",
    description: "Move the ticket to the best available team queue.",
    icon: Tag,
  },
  {
    id: "internal-note",
    label: "Add internal note",
    description: "Write a private operator note to the conversation.",
    icon: FileText,
  },
  {
    id: "close-ticket",
    label: "Close ticket",
    description: "Resolve the conversation after the action plan is complete.",
    icon: CheckCircle2,
  },
];

export function InboxWorkModal({
  agents,
  groups,
  modal,
  onClose,
  onInsertReply,
  onPatchTicket,
  onRefreshTicket,
  onSendReply,
  selectedTicket,
}: {
  agents: Agent[];
  groups: Group[];
  modal: InboxModalRequest | null;
  onClose: () => void;
  onInsertReply: (text: string) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void | Promise<void>;
  onRefreshTicket: () => void;
  onSendReply: (text: string, internal: boolean) => void | Promise<void>;
  selectedTicket: ZammadTicket | null;
}) {
  const closeModal = useEffectEvent(() => {
    onClose();
  });

  useEffect(() => {
    if (!modal) return;

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeModal();
    }

    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [modal]);

  if (!modal) return null;

  const title = modal.type === "velion" ? "Velion workspace" : modal.title;
  const sizeClass = modal.type === "velion" ? "velion-modal-shell-wide" : "velion-modal-shell-md";

  return (
    <dialog
      open
      className="velion-modal-backdrop"
      aria-label={title}
    >
      <button
        type="button"
        aria-label="Dismiss modal backdrop"
        className="absolute inset-0 cursor-default"
        onClick={onClose}
      />
      <div className={cn("velion-modal-shell relative z-10 max-h-[calc(100vh-104px)]", sizeClass)}>
        <div className="velion-modal-header">
          <div className="min-w-0">
            <h2 className="velion-modal-title truncate">{title}</h2>
            <p className="velion-type-sm mt-0.5 text-[#7B7B78] dark:text-[#AEB4C0]">Inbox context stays active.</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="velion-icon-button shrink-0"
            aria-label="Close modal"
            title="Close modal"
          >
            <X className="size-4" />
          </button>
        </div>

        {modal.type === "velion" ? (
          <VelionExecutionPanel
            agents={agents}
            groups={groups}
            initialPrompt={modal.prompt ?? ""}
            onClose={onClose}
            onInsertReply={onInsertReply}
            onPatchTicket={onPatchTicket}
            onRefreshTicket={onRefreshTicket}
            onSendReply={onSendReply}
            selectedTicket={selectedTicket}
          />
        ) : (
          <ContextWorkPanel modal={modal} selectedTicket={selectedTicket} />
        )}
      </div>
    </dialog>
  );
}

function ContextWorkPanel({
  modal,
  selectedTicket,
}: {
  modal: Exclude<InboxModalRequest, { type: "velion" }>;
  selectedTicket: ZammadTicket | null;
}) {
  return (
    <div className="max-h-[calc(100vh-150px)] overflow-y-auto p-5">
      <div className="rounded-[14px] border border-[#E7E1D8] bg-[#FAF8F5] p-4 dark:border-[#2A2C31] dark:bg-[#17181C]">
        <p className="text-[13px] leading-5 text-[#3F3A35] dark:text-[#D8DDE6]">{modal.description}</p>
        {selectedTicket ? (
          <div className="mt-4 rounded-[10px] bg-white p-3 text-[12px] text-[#626260] dark:bg-[#101114] dark:text-[#AEB4C0]">
            <div className="font-semibold text-[#111111] dark:text-white">{selectedTicket.title}</div>
            <div className="mt-1">#{selectedTicket.number} · {selectedTicket.customer?.email ?? customerName(selectedTicket)}</div>
          </div>
        ) : null}
      </div>

      {modal.type === "view" ? (
        <>
          <section className="mt-4 grid gap-2 sm:grid-cols-2">
            <ModalMetric label="Scope" value={modal.title} />
            <ModalMetric label="Mode" value="Local view" />
          </section>
          <p className="mt-4 text-[12px] leading-5 text-[#7B7B78] dark:text-[#AEB4C0]">
            This view can become a saved AI-managed inbox rule without leaving the page.
          </p>
        </>
      ) : (
        <>
          <label className="mt-4 block text-[12px] font-semibold text-[#626260] dark:text-[#AEB4C0]" htmlFor="inbox-work-modal-note">
            Working note
          </label>
          <textarea
            id="inbox-work-modal-note"
            rows={4}
            placeholder="Capture the action, owner, or backend payload…"
            className="velion-textarea mt-2"
          />
          <button
            type="button"
            className="velion-button velion-button-primary velion-button-sm mt-3 px-3 text-[12px] font-semibold disabled:opacity-50"
          >
            {modal.primaryAction ?? "Save in inbox"}
          </button>
        </>
      )}

      <p className="mt-4 text-[11px] leading-5 text-[#9C9A96]">
        Backend handoff: wire this modal to durable inbox resources instead of routing the operator to separate pages.
      </p>
    </div>
  );
}

function VelionExecutionPanel({
  agents,
  groups,
  initialPrompt,
  onClose,
  onInsertReply,
  onPatchTicket,
  onRefreshTicket,
  onSendReply,
  selectedTicket,
}: {
  agents: Agent[];
  groups: Group[];
  initialPrompt: string;
  onClose: () => void;
  onInsertReply: (text: string) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void | Promise<void>;
  onRefreshTicket: () => void;
  onSendReply: (text: string, internal: boolean) => void | Promise<void>;
  selectedTicket: ZammadTicket | null;
}) {
  const [prompt, setPrompt] = useState(initialPrompt);
  const [selectedActions, setSelectedActions] = useState<Set<ToolActionId>>(() => new Set(["draft-reply", "schedule-follow-up", "internal-note"]));
  const [runningAction, setRunningAction] = useState<ToolActionId | null>(null);
  const [logs, setLogs] = useState<ToolRunLog[]>([]);

  const selectedActionList = toolActions
    .filter((action) => selectedActions.has(action.id))
    .sort((a, b) => Number(a.id === "draft-reply") - Number(b.id === "draft-reply"));

  function toggleAction(actionId: ToolActionId) {
    setSelectedActions((current) => {
      const next = new Set(current);
      if (next.has(actionId)) {
        next.delete(actionId);
      } else {
        next.add(actionId);
      }
      return next;
    });
  }

  async function runSelectedActions() {
    if (!selectedTicket || runningAction || selectedActionList.length === 0) return;
    setLogs([]);

    // Keep selected actions serial: several patch the same ticket or append
    // ordered logs, so parallel execution can race the visible outcome.
    await selectedActionList.reduce<Promise<void>>(
      (chain, action) => chain.then(() => runAction(action.id, action.label)),
      Promise.resolve(),
    );

    onRefreshTicket();
  }

  async function runAction(actionId: ToolActionId, label: string) {
    if (!selectedTicket) return;

    setRunningAction(actionId);
    appendLog(label, "running", "Running tool call…");

    // Backend handoff: replace this client dispatcher with a server-owned Velion
    // tool runner that returns planned tool calls, approval scopes, audit logs,
    // and rollback metadata before committing support, calendar, commerce, or CRM work.
    try {
      if (actionId === "draft-reply") {
        onInsertReply(buildVelionReply(selectedTicket, prompt));
        appendLog(label, "done", "Reply draft inserted in composer.");
      }

      if (actionId === "schedule-follow-up") {
        await scheduleFollowUp(selectedTicket);
        appendLog(label, "done", "Follow-up saved to the shared calendar endpoint.");
      }

      if (actionId === "raise-priority") {
        await onPatchTicket({ priority_id: 3 });
        appendLog(label, "done", "Ticket priority set to high.");
      }

      if (actionId === "route-support") {
        const groupId = selectedTicket.group?.id ?? groups[0]?.id;
        if (!groupId) {
          appendLog(label, "failed", "No support group is available.");
          setRunningAction(null);
          return;
        }
        await onPatchTicket({ group_id: groupId });
        appendLog(label, "done", "Ticket routed to the available support queue.");
      }

      if (actionId === "internal-note") {
        await onSendReply(buildInternalNote(prompt), true);
        appendLog(label, "done", "Internal note added to the conversation.");
      }

      if (actionId === "close-ticket") {
        await onPatchTicket({ state_id: 4 });
        appendLog(label, "done", "Ticket marked closed.");
      }
    } catch (error) {
      appendLog(label, "failed", error instanceof Error ? error.message : "Tool call failed.");
    }

    setRunningAction(null);
  }

  function appendLog(label: string, status: ToolRunLog["status"], detail: string) {
    setLogs((current) => [
      ...current.filter((item) => !(item.label === label && item.status === "running")),
      {
        id: `${label}-${status}-${Date.now()}`,
        label,
        status,
        detail,
      },
    ]);
  }

  return (
    <div className="grid max-h-[calc(100vh-150px)] overflow-y-auto lg:grid-cols-[minmax(0,1fr)_300px]">
      <div className="min-w-0 border-b border-[#E7E1D8] p-5 lg:border-b-0 lg:border-r dark:border-[#2A2C31]">
        <div className="rounded-[14px] border border-[#E7E1D8] bg-[#FAF8F5] p-4 dark:border-[#2A2C31] dark:bg-[#17181C]">
          <div className="mb-3 flex items-center gap-2">
            <Bot className="size-4 text-[#DD7A1F]" />
            <h3 className="text-[13px] font-semibold text-[#111111] dark:text-white">Autonomous inbox operator</h3>
          </div>
          <textarea
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            rows={5}
            placeholder="Tell Velion what outcome to handle…"
            aria-label="Inbox operator prompt"
            className="velion-textarea"
          />
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void runSelectedActions()}
              disabled={!selectedTicket || runningAction !== null || selectedActionList.length === 0}
              className="velion-button velion-button-primary velion-button-sm px-3 text-[12px] font-semibold disabled:opacity-45"
            >
              <Play className="size-3.5" />
              {runningAction ? "Running…" : "Run selected"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="velion-button velion-button-secondary velion-button-sm px-3 text-[12px] font-semibold"
            >
              Keep monitoring
            </button>
          </div>
        </div>

        <div className="mt-4 grid gap-2 sm:grid-cols-2">
          {toolActions.map((action) => {
            const Icon = action.icon;
            const active = selectedActions.has(action.id);

            return (
              <button
                key={action.id}
                type="button"
                onClick={() => toggleAction(action.id)}
                className={cn(
                  "rounded-[12px] border p-3 text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]",
                  active
                    ? "border-[#111111] bg-white shadow-[0_1px_3px_rgba(16,24,40,0.08)] dark:border-white dark:bg-[#17181C]"
                    : "border-[#E1DAD1] bg-[#FAF8F5] hover:bg-white dark:border-[#303238] dark:bg-[#17181C]/70 dark:hover:bg-[#17181C]",
                )}
                aria-pressed={active}
              >
                <div className="flex items-center gap-2">
                  <Icon className={cn("size-4", active ? "text-[#DD7A1F]" : "text-[#7B7B78]")} />
                  <span className="text-[12px] font-semibold text-[#111111] dark:text-white">{action.label}</span>
                </div>
                <p className="mt-1 text-[12px] leading-5 text-[#626260] dark:text-[#AEB4C0]">{action.description}</p>
              </button>
            );
          })}
        </div>
      </div>

      <aside className="min-w-0 bg-[#FAF8F5] p-5 dark:bg-[#17181C]">
        <h3 className="text-[13px] font-semibold text-[#111111] dark:text-white">Tool run</h3>
        <div className="mt-3 space-y-2">
          <ToolState label="Context" value={selectedTicket ? `#${selectedTicket.number}` : "No ticket"} icon={MessageCircle} />
          <ToolState label="Approval" value="Human supervised" icon={Settings} />
          <ToolState label="Agent" value={agents[0] ? `${agents[0].firstname} ${agents[0].lastname}` : "Velion"} icon={Bot} />
        </div>
        <div className="mt-4 rounded-[12px] border border-[#E1DAD1] bg-white p-3 dark:border-[#303238] dark:bg-[#101114]">
          <div className="mb-2 flex items-center gap-2">
            <Clock3 className="size-4 text-[#7B7B78]" />
            <span className="text-[12px] font-semibold text-[#111111] dark:text-white">Audit stream</span>
          </div>
          {!logs.length ? <p className="text-[12px] leading-5 text-[#7B7B78] dark:text-[#AEB4C0]">No tool calls yet.</p> : null}
          <div className="space-y-2">
            {logs.map((log) => <ToolLogRow key={log.id} log={log} />)}
          </div>
        </div>
        <p className="mt-4 text-[11px] leading-5 text-[#9C9A96]">
          Backend handoff: Velion should execute through scoped tools with audit events, permissions, retries, and operator review.
        </p>
      </aside>
    </div>
  );
}

function ToolState({ icon: Icon, label, value }: { icon: LucideIcon; label: string; value: string }) {
  return (
    <div className="flex items-center gap-2 rounded-[10px] bg-white px-3 py-2 text-[12px] dark:bg-[#101114]">
      <Icon className="size-3.5 text-[#7B7B78]" />
      <span className="text-[#7B7B78]">{label}</span>
      <span className="ml-auto truncate font-semibold text-[#111111] dark:text-white">{value}</span>
    </div>
  );
}

function ToolLogRow({ log }: { log: ToolRunLog }) {
  return (
    <div className="rounded-[9px] border border-[#EEE8E0] px-2.5 py-2 text-[12px] dark:border-[#2A2C31]">
      <div className="flex items-center gap-2">
        <span
          className={cn(
            "size-1.5 rounded-full",
            log.status === "done" ? "bg-[#12B76A]" : "",
            log.status === "running" ? "bg-[#DD7A1F]" : "",
            log.status === "failed" ? "bg-[#D92D20]" : "",
          )}
        />
        <span className="font-semibold text-[#111111] dark:text-white">{log.label}</span>
      </div>
      <p className="mt-1 leading-5 text-[#626260] dark:text-[#AEB4C0]">{log.detail}</p>
    </div>
  );
}

function ModalMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[12px] border border-[#E1DAD1] bg-white p-3 dark:border-[#303238] dark:bg-[#17181C]">
      <div className="text-[11px] font-semibold uppercase text-[#9C9A96]">{label}</div>
      <div className="mt-1 truncate text-[13px] font-semibold text-[#111111] dark:text-white">{value}</div>
    </div>
  );
}

async function scheduleFollowUp(ticket: ZammadTicket) {
  const start = new Date();
  start.setDate(start.getDate() + 1);
  start.setHours(9, 0, 0, 0);
  const end = new Date(start);
  end.setMinutes(end.getMinutes() + 30);

  await apiSend<{ event: CalendarEvent }>("/api/v1/navbar/calendar", {
    title: `Follow up: ${ticket.title}`.slice(0, 160),
    start: start.toISOString(),
    end: end.toISOString(),
    type: "inbox-ai-follow-up",
  });
}

function buildVelionReply(ticket: ZammadTicket, prompt: string) {
  const customer = customerName(ticket);
  const instruction = prompt.trim();

  return [
    `Hi ${customer},`,
    "",
    "Thanks for the context. I am checking the account and will handle the next step from here.",
    instruction ? `I am using this instruction: ${instruction}` : "I will follow up with the right team and keep this moving.",
    "",
    "Best,",
    "Velion",
  ].join("\n");
}

function buildInternalNote(prompt: string) {
  return `Velion action note: ${prompt.trim() || "Review completed. Draft, follow-up, and routing actions were prepared in the inbox workspace."}`;
}
