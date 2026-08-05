import "server-only";

import type { RequestActor } from "@/lib/integrations/request-actor";

type ApiEnvelope<T> = { data?: T; error?: unknown };

export type ConversationSummary = {
  id: string;
  org_id: string;
  inbox_id: string;
  title: string;
  status: string;
  priority: string;
  channel: string;
  provider?: string;
  assignee_user_id?: string;
  assignee_name?: string;
  last_message_preview?: string;
  last_message_at?: string;
  contact?: {
    id?: string;
    name?: string;
    email?: string;
  };
  tags?: string[];
  created_at: string;
  updated_at: string;
};

export type ConversationMessage = {
  id: string;
  conversation_id: string;
  direction: string;
  sender_type: string;
  sender_name?: string;
  sender_email?: string;
  body_text: string;
  body_html?: string;
  internal: boolean;
  occurred_at: string;
  created_at: string;
};

export type ConversationDetail = ConversationSummary & {
  messages: ConversationMessage[];
};

export type SupportTicket = {
  id: string;
  number: string;
  title: string;
  state: { id: number; name: string };
  priority: { id: number; name: string };
  group: { id: number; name: string };
  owner: { id: string; firstname: string; lastname: string; email: string } | null;
  customer: { id: string; firstname: string; lastname: string; email: string } | null;
  tags: string[];
  created_at: string;
  updated_at: string;
  article_count?: number;
  _conversation?: ConversationSummary;
};

export type SupportArticle = {
  id: string;
  ticket_id: string;
  type: string;
  internal: boolean;
  body: string;
  from: string;
  sender: string;
  created_at: string;
};

const getConversationCoreUrl = () =>
  (
    process.env.CONVERSATION_CORE_URL ||
    process.env.CONVERSATION_CORE_GO_URL ||
    "http://conversation-core-go:3160"
  ).replace(/\/+$/, "");

const getInternalApiKey = () =>
  (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || "").trim();

const getSupportOrgId = () =>
  (
    process.env.SUPPORT_ORG_ID ||
    process.env.DEFAULT_ORG_ID ||
    process.env.NEXT_PUBLIC_DEFAULT_ORG_ID ||
    "default-org"
  ).trim();

function buildHeaders(actor: RequestActor, orgId = getSupportOrgId()) {
  const key = getInternalApiKey();
  if (!key) {
    throw new ConversationCoreError(500, "missing_internal_api_key", "INTERNAL_API_KEY is required.");
  }
  return {
    "Content-Type": "application/json",
    "x-internal-api-key": key,
    "x-org-id": orgId,
    "x-user-id": actor.userId,
    ...(actor.email ? { "x-user-email": actor.email } : {}),
    ...(actor.name ? { "x-user-name": actor.name } : {}),
  };
}

export class ConversationCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

async function requestConversationCore<T>(
  actor: RequestActor,
  path: string,
  init?: RequestInit & { orgId?: string },
): Promise<T> {
  const response = await fetch(`${getConversationCoreUrl()}${path}`, {
    ...init,
    headers: {
      ...buildHeaders(actor, init?.orgId),
      ...(init?.headers ?? {}),
    },
    cache: "no-store",
    signal: AbortSignal.timeout(8_000),
  });
  const payload = (await response.json().catch(() => null)) as ApiEnvelope<T> | T | null;
  if (!response.ok) {
    const code = typeof payload === "object" && payload && "error" in payload ? "conversation_core_error" : "conversation_core_error";
    throw new ConversationCoreError(response.status, code, `conversation-core returned ${response.status}`);
  }
  if (payload && typeof payload === "object" && "data" in payload) {
    return (payload as ApiEnvelope<T>).data as T;
  }
  return payload as T;
}

export async function listSupportTickets(actor: RequestActor, params: URLSearchParams) {
  const query = new URLSearchParams();
  query.set("limit", params.get("limit") || "50");
  const state = params.get("state");
  if (state) query.set("state", state);
  const assigned = params.get("assigned");
  if (assigned) query.set("assigned", assigned === "mine" ? actor.userId : assigned);
  const channel = params.get("channel");
  if (channel && channel !== "all") query.set("channel", channel);
  const data = await requestConversationCore<ConversationSummary[]>(actor, `/api/v1/conversations?${query}`);
  return data.map(toSupportTicket);
}

export async function createSupportTicket(actor: RequestActor, input: { title: string; body: string; customer_email: string }) {
  const now = new Date().toISOString();
  const payload = {
    idempotency_key: `verevonv2:create:${actor.userId}:${input.customer_email}:${input.title}:${now}`,
    org_id: getSupportOrgId(),
    provider: "verevon",
    provider_event_id: crypto.randomUUID(),
    provider_message_id: crypto.randomUUID(),
    provider_thread_id: crypto.randomUUID(),
    direction: "inbound",
    subject: input.title,
    from: { email: input.customer_email, name: input.customer_email },
    body_text: input.body,
    occurred_at: now,
  };
  const result = await requestConversationCore<{ detail?: ConversationDetail }>(actor, "/internal/conversation-events", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  const detail = result.detail;
  return detail ? toSupportTicket(detail) : result;
}

export async function getSupportTicket(actor: RequestActor, id: string) {
  const detail = await getConversation(actor, id);
  return toSupportTicket(detail);
}

export async function patchSupportTicket(actor: RequestActor, id: string, patch: { state_id?: number; owner_id?: number; tags?: string[] }) {
  let detail = await getConversation(actor, id);
  if (patch.state_id !== undefined) {
    detail = await requestConversationCore<ConversationDetail>(actor, `/api/v1/conversations/${encodeURIComponent(id)}/status`, {
      method: "PATCH",
      body: JSON.stringify({ status: statusFromStateId(patch.state_id) }),
    });
  }
  if (patch.owner_id !== undefined) {
    detail = await requestConversationCore<ConversationDetail>(actor, `/api/v1/conversations/${encodeURIComponent(id)}/assignment`, {
      method: "PATCH",
      body: JSON.stringify({ assignee_user_id: String(patch.owner_id), assignee_name: `Agent ${patch.owner_id}` }),
    });
  }
  if (patch.tags) {
    const current = new Set(detail.tags ?? []);
    const wanted = new Set(patch.tags);
    for (const tag of wanted) {
      if (!current.has(tag)) {
        detail = await requestConversationCore<ConversationDetail>(actor, `/api/v1/conversations/${encodeURIComponent(id)}/tags`, {
          method: "POST",
          body: JSON.stringify({ tag }),
        });
      }
    }
    for (const tag of current) {
      if (!wanted.has(tag)) {
        detail = await requestConversationCore<ConversationDetail>(actor, `/api/v1/conversations/${encodeURIComponent(id)}/tags/${encodeURIComponent(tag)}`, {
          method: "DELETE",
        });
      }
    }
  }
  return toSupportTicket(detail);
}

export async function listSupportArticles(actor: RequestActor, id: string) {
  const detail = await getConversation(actor, id);
  return detail.messages.map(toSupportArticle);
}

export async function addSupportArticle(actor: RequestActor, id: string, input: { body: string; internal: boolean }) {
  const path = input.internal ? "notes" : "messages";
  const message = await requestConversationCore<ConversationMessage>(actor, `/api/v1/conversations/${encodeURIComponent(id)}/${path}`, {
    method: "POST",
    body: JSON.stringify({
      body_text: input.body,
      internal: input.internal,
      actor_name: actor.name ?? "You",
      actor_email: actor.email ?? "",
    }),
  });
  return toSupportArticle(message);
}

export async function listSupportGroups(actor: RequestActor) {
  const inboxes = await requestConversationCore<Array<{ id: string; name: string }>>(actor, "/api/v1/inboxes");
  return inboxes.map((inbox, index) => ({ id: index + 1, name: inbox.name }));
}

export function listSupportAgents(actor: RequestActor) {
  return [{
    id: actor.userId,
    firstname: actor.name?.split(" ")[0] ?? "Verevon",
    lastname: actor.name?.split(" ").slice(1).join(" ") || "Agent",
    email: actor.email ?? "agent@verevon.local",
  }];
}

export function listSupportMacros() {
  return [
    { id: "assign-to-me", name: "Assign to me + open" },
    { id: "close", name: "Resolved - waiting for confirmation" },
  ];
}

export async function executeSupportMacro(actor: RequestActor, macroId: string, ticketId: string) {
  if (macroId === "close") {
    return patchSupportTicket(actor, ticketId, { state_id: 4 });
  }
  if (macroId === "assign-to-me") {
    return requestConversationCore<ConversationDetail>(actor, `/api/v1/conversations/${encodeURIComponent(ticketId)}/assignment`, {
      method: "PATCH",
      body: JSON.stringify({ assignee_user_id: actor.userId, assignee_name: actor.name ?? actor.email ?? "You" }),
    });
  }
  return { ok: true };
}

async function getConversation(actor: RequestActor, id: string) {
  return await requestConversationCore<ConversationDetail>(actor, `/api/v1/conversations/${encodeURIComponent(id)}`);
}

export function toSupportTicket(conversation: ConversationSummary | ConversationDetail): SupportTicket {
  const [firstname, ...rest] = (conversation.contact?.name || conversation.contact?.email || "Unknown").split(" ");
  return {
    id: conversation.id,
    number: conversation.id.replace(/^conv_/, "").slice(0, 12),
    title: conversation.title,
    state: stateFromStatus(conversation.status),
    priority: priorityFromValue(conversation.priority),
    group: { id: 1, name: "Email" },
    owner: conversation.assignee_user_id ? {
      id: conversation.assignee_user_id,
      firstname: conversation.assignee_name?.split(" ")[0] || "Assigned",
      lastname: conversation.assignee_name?.split(" ").slice(1).join(" ") || "Agent",
      email: "",
    } : null,
    customer: {
      id: conversation.contact?.id || conversation.contact?.email || "unknown",
      firstname: firstname || "Unknown",
      lastname: rest.join(" "),
      email: conversation.contact?.email || "",
    },
    tags: conversation.tags ?? [],
    created_at: conversation.created_at,
    updated_at: conversation.updated_at,
    article_count: "messages" in conversation ? conversation.messages.length : undefined,
    _conversation: conversation,
  };
}

export function toSupportArticle(message: ConversationMessage): SupportArticle {
  const agentMessage = message.sender_type === "agent" || message.direction === "outbound";
  return {
    id: message.id,
    ticket_id: message.conversation_id,
    type: message.internal ? "note" : "email",
    internal: message.internal,
    body: message.body_html || message.body_text,
    from: message.sender_name || message.sender_email || (agentMessage ? "Verevon Support" : "Customer"),
    sender: agentMessage ? "Agent" : "Customer",
    created_at: message.occurred_at || message.created_at,
  };
}

function stateFromStatus(status: string) {
  switch (status) {
    case "pending":
      return { id: 6, name: "pending" };
    case "solved":
    case "closed":
      return { id: 4, name: "closed" };
    default:
      return { id: 2, name: "open" };
  }
}

function statusFromStateId(id: number) {
  switch (id) {
    case 4:
      return "solved";
    case 6:
      return "pending";
    default:
      return "open";
  }
}

function priorityFromValue(priority: string) {
  switch (priority) {
    case "high":
      return { id: 3, name: "high" };
    case "low":
      return { id: 1, name: "low" };
    default:
      return { id: 2, name: "normal" };
  }
}
