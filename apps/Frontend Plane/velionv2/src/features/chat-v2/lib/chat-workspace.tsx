"use client";

import { createContext, use, useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import type {
  ComposerAttachment,
  ComposerSubmitPayload,
  ComposerToolId,
} from "@/features/chat-v2/components/VerevonComposer";
import { toolLabels } from "@/features/chat-v2/lib/chat-format";
import {
  cancelChat,
  loadThreadHistory,
  streamChat,
  type ChatStreamChunk,
  type ChatTiming,
} from "@/features/chat-v2/lib/chat-stream";
import type { ChatKnowledgeGrounding } from "@/features/chat-v2/lib/chat-grounding";

const STORAGE_KEY = "verevon:v2:chat:sessions";
const LAUNCH_MOTION_KEY = "verevon:v2:chat:launch-motion";
const LAUNCH_MOTION_TTL_MS = 4_000;
const SESSION_LIMIT = 30;
const WAITING_ASSISTANT_CONTENT = "Awaiting the live Verevon agent stream.";
const EMPTY_MODEL_RESPONSE_CONTENT = "Model Plane completed without returning text.";
const MODEL_STREAM_ERROR_CONTENT = "I couldn't connect to the Model Plane stream. Try again in a moment.";

export type MessageRole = "user" | "assistant";
export type TaskStepStatus = "done" | "active" | "waiting" | "error" | "stopped";

export type ChatMessage = {
  id: string;
  role: MessageRole;
  content: string;
  createdAt: string;
  tools: ComposerToolId[];
  attachments: ComposerAttachment[];
  status?: "waiting" | "stopped" | "error";
  model?: string;
  requestId?: string;
  modelUsed?: string;
  inputTokens?: number;
  outputTokens?: number;
  latencyMs?: number;
  ttftMs?: number;
  // chat-parity §2: rich event data rendered by the chat UI.
  reasoning?: string;
  citations?: Citation[];
  costUsd?: number;
  confidence?: number;
  toolCalls?: ChatToolCall[];
  artifacts?: ChatArtifact[];
  files?: GeneratedFile[];
  grounding?: ChatKnowledgeGrounding;
};

export type Citation = {
  id: string;
  title: string;
  url: string;
  snippet: string;
};

export type ChatToolCall = {
  id: string;
  name: string;
  args?: unknown;
  status?: string;
  output?: string;
  error?: string;
};

export type ChatArtifact = {
  id: string;
  kind: string;
  title: string;
  content: string;
  version: number;
};

export type GeneratedFile = {
  id: string;
  name: string;
  mime: string;
  url: string;
  size: number;
};

export type AgentTaskStep = {
  id: string;
  title: string;
  detail: string;
  status: TaskStepStatus;
  createdAt: string;
};

export type ChatSession = {
  id: string;
  title: string;
  preview: string;
  updatedAt: string;
  messages: ChatMessage[];
  taskSteps: AgentTaskStep[];
  branchCount: number;
};

type ChatPageState = {
  activeSessionId: string | null;
  composerDraft: string;
  sessions: ChatSession[];
};

export type ChatWorkspaceValue = {
  activeSession: ChatSession | null;
  activeSessionId: string | null;
  composerDraft: string;
  copiedMessageId: string | null;
  sessions: ChatSession[];
  branch: (messageId: string) => void;
  clearHistory: () => void;
  copy: (message: ChatMessage) => Promise<void>;
  editAndResubmit: (messageId: string, text: string) => void;
  regenerate: () => void;
  selectSession: (sessionId: string) => void;
  startNewChat: () => void;
  stopTask: () => void;
  submit: (payload: ComposerSubmitPayload) => void;
};

const ChatWorkspaceContext = createContext<ChatWorkspaceValue | null>(null);
const hydrationSafeInitialState: ChatPageState = {
  activeSessionId: null,
  composerDraft: "",
  sessions: [],
};

type ChatStoreUpdate = ChatPageState | ((current: ChatPageState) => ChatPageState);

let chatStoreState = hydrationSafeInitialState;
let chatStoreHydrated = false;
const chatStoreListeners = new Set<() => void>();
const assistantStreamControllers = new Map<string, AbortController>();

export function VerevonChatWorkspaceProvider({ children }: { children: ReactNode }) {
  const chatState = useSyncExternalStore(
    subscribeChatStore,
    getChatStoreSnapshot,
    getServerChatStoreSnapshot,
  );
  const [copiedMessageId, setCopiedMessageId] = useState<string | null>(null);
  const { activeSessionId, composerDraft, sessions } = chatState;

  useEffect(() => {
    if (!composerDraft || typeof window === "undefined" || !window.location.search.includes("prompt=")) {
      return;
    }

    window.history.replaceState(null, "", window.location.pathname);
  }, [composerDraft]);

  const activeSession = sessions.find((session) => session.id === activeSessionId) ?? null;

  const submit = (payload: ComposerSubmitPayload) => {
    submitChatPayload(payload);
  };

  const startNewChat = () => {
    updateChatStore((current) => ({
      ...current,
      activeSessionId: null,
      composerDraft: "",
    }));
  };

  const stopTask = () => {
    hydrateChatStore();
    const activeTaskSessionId = chatStoreState.activeSessionId;
    if (!activeTaskSessionId) {
      return;
    }

    assistantStreamControllers.get(activeTaskSessionId)?.abort();
    assistantStreamControllers.delete(activeTaskSessionId);

    // chat-parity §4: the abort only halts the local read — also tell the
    // server to stop generating, keyed by the in-flight assistant message's
    // request_id (captured from the streamed deltas). Best-effort.
    const cancelSession = chatStoreState.sessions.find((session) => session.id === activeTaskSessionId);
    const waitingMessage = cancelSession?.messages.find(
      (message) => message.role === "assistant" && message.status === "waiting",
    );
    if (waitingMessage) {
      // Drain buffered tokens (and cancel the pending flush) before we freeze
      // the partial, so the kept content has the tail and no late rAF re-adds.
      flushAssistantDelta(activeTaskSessionId, waitingMessage.id);
    }
    if (waitingMessage?.requestId) {
      void cancelChat(waitingMessage.requestId);
    }

    updateChatStore((current) => {
      const stoppedAt = new Date().toISOString();

      return {
        ...current,
        sessions: current.sessions.map((session) => (
          session.id === current.activeSessionId
            ? {
                ...session,
                updatedAt: stoppedAt,
                messages: session.messages.map((message) => {
                  if (message.status !== "waiting") {
                    return message;
                  }
                  // Preserve whatever streamed in before the user hit stop.
                  // Empty when nothing arrived yet — the UI shows a "Stoppet" tag.
                  const partial = isAssistantPlaceholder(message.content) ? "" : message.content.trim();
                  return {
                    ...message,
                    content: partial,
                    status: "stopped" as const,
                  };
                }),
                taskSteps: session.taskSteps.map((step) => (
                  step.status === "active" || step.status === "waiting"
                    ? { ...step, status: "stopped", detail: "Stopped by the user before the live stream returned." }
                    : step
                )),
              }
            : session
        )),
      };
    });
  };

  const regenerate = () => {
    hydrateChatStore();
    const current = chatStoreState;
    const active = current.sessions.find((session) => session.id === current.activeSessionId);
    const lastUserMessage = [...(active?.messages ?? [])].reverse().find((message) => message.role === "user");

    if (!active || !lastUserMessage) {
      return;
    }

    const regeneratedAt = new Date().toISOString();
    const payload: ComposerSubmitPayload = {
      text: lastUserMessage.content,
      model: lastUserMessage.model,
      tools: lastUserMessage.tools,
      attachments: lastUserMessage.attachments,
    };
    const assistantStatus: ChatMessage = {
      id: createId(),
      role: "assistant",
      content: "Regeneration queued for the live Verevon agent stream.",
      createdAt: regeneratedAt,
      model: payload.model,
      tools: lastUserMessage.tools,
      attachments: [],
      status: "waiting",
    };

    updateChatStore({
      ...current,
      sessions: current.sessions.map((session) => (
        session.id === active.id
          ? {
              ...session,
              branchCount: session.branchCount + 1,
              preview: createPreview(lastUserMessage.content),
              updatedAt: regeneratedAt,
              messages: [...session.messages, assistantStatus],
              taskSteps: buildTaskSteps(payload, regeneratedAt, "regenerate"),
            }
          : session
      )),
    });
    startAssistantStream({
      assistantMessageId: assistantStatus.id,
      payload,
      sessionId: active.id,
    });
  };

  const editAndResubmit = (messageId: string, text: string) => {
    hydrateChatStore();
    const current = chatStoreState;
    const active = current.sessions.find((session) => session.id === current.activeSessionId);
    if (!active) {
      return;
    }

    const index = active.messages.findIndex((message) => message.id === messageId);
    if (index < 0) {
      return;
    }

    const original = active.messages[index];
    const nextText = text.trim();
    if (!nextText) {
      return;
    }

    // Abort any in-flight stream, then drop the edited message and everything
    // after it. Resubmitting appends a fresh user turn + assistant stream,
    // reusing the original turn's model / tools / attachments.
    assistantStreamControllers.get(active.id)?.abort();
    assistantStreamControllers.delete(active.id);

    updateChatSession(active.id, (session) => ({
      ...session,
      messages: session.messages.slice(0, index),
      updatedAt: new Date().toISOString(),
    }));

    submitChatPayload({
      text: nextText,
      model: original.model,
      tools: original.tools,
      attachments: original.attachments,
    });
  };

  const branch = (messageId: string) => {
    updateChatStore((current) => {
      const active = current.sessions.find((session) => session.id === current.activeSessionId);
      const messageIndex = active?.messages.findIndex((message) => message.id === messageId) ?? -1;

      if (!active || messageIndex < 0) {
        return current;
      }

      const branchAt = new Date().toISOString();
      const branchMessages = active.messages.slice(0, messageIndex + 1).map((message) => ({
        ...message,
        id: createId(),
      }));
      const branchSession: ChatSession = {
        id: createId(),
        title: `${active.title} branch`,
        preview: branchMessages.at(-1)?.content ?? active.preview,
        updatedAt: branchAt,
        messages: branchMessages,
        taskSteps: active.taskSteps.map((step) => ({ ...step, id: createId() })),
        branchCount: 0,
      };

      return {
        activeSessionId: branchSession.id,
        composerDraft: "",
        sessions: [branchSession, ...current.sessions].slice(0, SESSION_LIMIT),
      };
    });
  };

  const copy = async (message: ChatMessage) => {
    await navigator.clipboard.writeText(message.content).catch(() => undefined);
    setCopiedMessageId(message.id);
    window.setTimeout(() => setCopiedMessageId(null), 1200);
  };

  const clearHistory = () => {
    updateChatStore({
      activeSessionId: null,
      composerDraft: "",
      sessions: [],
    });
  };

  const selectSession = (sessionId: string) => {
    updateChatStore((current) => ({
      ...current,
      activeSessionId: sessionId,
      composerDraft: "",
    }));
    // chat-parity §1 — cross-device resume. The local sessionId IS the
    // session-core threadId (the BFF stream sends thread_id = sessionId), so
    // an empty local session can be rehydrated from the server (e.g. opened
    // on another device or after localStorage was cleared). Best-effort.
    void hydrateSessionFromServer(sessionId);
  };

  const value: ChatWorkspaceValue = {
    activeSession,
    activeSessionId,
    branch,
    clearHistory,
    composerDraft,
    copiedMessageId,
    copy,
    editAndResubmit,
    regenerate,
    selectSession,
    sessions,
    startNewChat,
    stopTask,
    submit,
  };

  return (
    <ChatWorkspaceContext.Provider value={value}>
      {children}
    </ChatWorkspaceContext.Provider>
  );
}

export function useVerevonChatWorkspace() {
  const context = use(ChatWorkspaceContext);

  if (!context) {
    throw new Error("useVerevonChatWorkspace must be used within VerevonChatWorkspaceProvider.");
  }

  return context;
}

export function useVerevonChatWorkspaceSafe() {
  return use(ChatWorkspaceContext);
}

export function launchChatSessionFromComposer(payload: ComposerSubmitPayload) {
  return submitChatPayload(payload, { targetSessionId: createId() });
}

function submitChatPayload(
  payload: ComposerSubmitPayload,
  options: { targetSessionId?: string } = {},
) {
  hydrateChatStore();

  const current = chatStoreState;
  const submittedAt = new Date().toISOString();
  const targetSessionId = options.targetSessionId ?? current.activeSessionId ?? createId();
  const existingSession = current.sessions.find((session) => session.id === targetSessionId);
  const nextSession = createSubmittedChatSession({
    existingSession: existingSession ?? null,
    payload,
    submittedAt,
    targetSessionId,
  });
  const withoutTarget = current.sessions.filter((session) => session.id !== targetSessionId);

  updateChatStore({
    activeSessionId: targetSessionId,
    composerDraft: "",
    sessions: [nextSession, ...withoutTarget].slice(0, SESSION_LIMIT),
  });

  const assistantMessage = [...nextSession.messages]
    .reverse()
    .find((message) => message.role === "assistant" && message.status === "waiting");
  if (assistantMessage) {
    startAssistantStream({
      assistantMessageId: assistantMessage.id,
      payload,
      sessionId: nextSession.id,
    });
  }

  return nextSession;
}

export function writeChatLaunchMotion(sessionId: string, prompt: string) {
  if (typeof sessionStorage === "undefined") {
    return;
  }

  try {
    sessionStorage.setItem(
      LAUNCH_MOTION_KEY,
      JSON.stringify({
        prompt,
        sessionId,
        createdAt: Date.now(),
      }),
    );
  } catch {
    // Motion metadata is optional; the chat launch itself must still succeed.
  }
}

export function consumeChatLaunchMotion(sessionId: string) {
  if (typeof sessionStorage === "undefined") {
    return false;
  }

  try {
    const parsed = JSON.parse(sessionStorage.getItem(LAUNCH_MOTION_KEY) ?? "null") as {
      createdAt?: unknown;
      sessionId?: unknown;
    } | null;

    sessionStorage.removeItem(LAUNCH_MOTION_KEY);

    return parsed?.sessionId === sessionId &&
      typeof parsed.createdAt === "number" &&
      Date.now() - parsed.createdAt < LAUNCH_MOTION_TTL_MS;
  } catch {
    sessionStorage.removeItem(LAUNCH_MOTION_KEY);
    return false;
  }
}

function subscribeChatStore(listener: () => void) {
  chatStoreListeners.add(listener);
  hydrateChatStore();

  return () => {
    chatStoreListeners.delete(listener);
  };
}

function getChatStoreSnapshot() {
  return chatStoreState;
}

function getServerChatStoreSnapshot() {
  return hydrationSafeInitialState;
}

function hydrateChatStore() {
  if (chatStoreHydrated || typeof window === "undefined") {
    return;
  }

  const restoredSessions = readStoredSessions();
  chatStoreState = {
    activeSessionId: restoredSessions[0]?.id ?? null,
    composerDraft: readPromptDraft(),
    sessions: restoredSessions,
  };
  chatStoreHydrated = true;
  queueMicrotask(emitChatStoreChange);
}

function updateChatStore(update: ChatStoreUpdate) {
  hydrateChatStore();
  const nextState = typeof update === "function" ? update(chatStoreState) : update;

  if (Object.is(nextState, chatStoreState)) {
    return;
  }

  chatStoreState = nextState;
  writeStoredSessions(nextState.sessions);
  emitChatStoreChange();
}

function startAssistantStream({
  assistantMessageId,
  payload,
  sessionId,
}: {
  assistantMessageId: string;
  payload: ComposerSubmitPayload;
  sessionId: string;
}) {
  if (typeof window === "undefined") {
    return;
  }

  assistantStreamControllers.get(sessionId)?.abort();
  const controller = new AbortController();
  assistantStreamControllers.set(sessionId, controller);

  void runAssistantStream({
    assistantMessageId,
    controller,
    payload,
    sessionId,
  });
}

type StreamAttachment = { kind?: string; url?: string; data_base64?: string; mime_type?: string };

function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : "");
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

/**
 * Read picked image attachments (object/data URLs) into base64 for the gateway
 * vision path. Non-images and unreadable entries are skipped so the text turn
 * still streams. Runs only in the browser (called from runAssistantStream).
 */
async function toStreamAttachments(attachments: ComposerAttachment[]): Promise<StreamAttachment[]> {
  const out: StreamAttachment[] = [];
  for (const attachment of attachments) {
    if (!attachment.url || !attachment.type.startsWith("image/")) {
      continue;
    }
    try {
      let dataUrl = attachment.url;
      if (!dataUrl.startsWith("data:")) {
        const response = await fetch(dataUrl);
        dataUrl = await blobToDataUrl(await response.blob());
      }
      const commaIndex = dataUrl.indexOf(",");
      if (commaIndex < 0) {
        continue;
      }
      const mime = dataUrl.slice(5, commaIndex).split(";")[0] || attachment.type;
      const base64 = dataUrl.slice(commaIndex + 1);
      if (base64) {
        out.push({ kind: "image", data_base64: base64, mime_type: mime });
      }
    } catch {
      // Unreadable attachment — skip; the text turn still streams.
    }
  }
  return out;
}

const HISTORY_MAX_TURNS = 8;
const HISTORY_MAX_CONTENT = 600;

/**
 * Build a compact conversation context prefix (most-recent N turns from this
 * session, excluding placeholder/in-flight messages and the assistant message
 * we're currently generating). Mirrors v1 reasoning-plane's
 * `buildConversationContext` so follow-up turns ("kan den være som et blåbære
 * i uke 7?") keep the prior context. Returns "" when there's no history.
 */
function buildConversationContext({
  sessionId,
  assistantMessageId,
}: {
  sessionId: string;
  assistantMessageId: string;
}): string {
  const session = chatStoreState.sessions.find((entry) => entry.id === sessionId);
  if (!session) {
    return "";
  }

  const prior: Array<{ role: MessageRole; content: string }> = [];
  for (const message of session.messages) {
    if (message.id === assistantMessageId) continue;
    if (message.status === "waiting" || message.status === "error") continue;
    const trimmed = message.content.trim();
    if (!trimmed || isAssistantPlaceholder(trimmed)) continue;
    prior.push({ role: message.role, content: trimmed });
  }

  const recent = prior.slice(-HISTORY_MAX_TURNS);
  if (recent.length === 0) {
    return "";
  }

  const lines = recent.map((message) => {
    const speaker = message.role === "assistant" ? "Assistant" : "User";
    const body = message.content.length > HISTORY_MAX_CONTENT
      ? `${message.content.slice(0, HISTORY_MAX_CONTENT)}…`
      : message.content;
    return `${speaker}: ${body}`;
  });

  return [
    "[Conversation context]",
    ...lines,
    "",
    "Answer the latest user request while keeping the prior conversation in mind when it is relevant.",
  ].join("\n");
}

async function runAssistantStream({
  assistantMessageId,
  controller,
  payload,
  sessionId,
}: {
  assistantMessageId: string;
  controller: AbortController;
  payload: ComposerSubmitPayload;
  sessionId: string;
}) {
  let completed = false;

  // Multimodal input (chat-parity §2): read picked image attachments to base64
  // so the gateway routes the turn through vision; `/image` intent → image-gen.
  const attachments = await toStreamAttachments(payload.attachments);
  const generateImage = payload.tools.includes("image") || undefined;

  // Inject prior conversation context so follow-up turns aren't answered cold
  // ("kan den være som et blåbære i uke 7?" → needs the prior pregnancy turn).
  // The gateway's session-core persistence isn't yet wired for unary turns;
  // prefixing here keeps context intact regardless and is harmless when it is.
  const contextBlock = buildConversationContext({ sessionId, assistantMessageId });
  const finalContent = contextBlock ? `${contextBlock}\n\n${payload.text}` : payload.text;

  try {
    for await (const chunk of streamChat({
      browseWeb: payload.tools.includes("search"),
      content: finalContent,
      // Opt into the rich event families the chat UI renders (chat-parity §2):
      // real usage (insight chip), citations (Sources), reasoning (thinking).
      features: ["usage", "citations", "reasoning", "steps", "tools", "artifacts"],
      model: payload.model,
      sessionId,
      signal: controller.signal,
      tools: payload.tools,
      attachments: attachments.length > 0 ? attachments : undefined,
      generateImage,
    })) {
      if (controller.signal.aborted) {
        return;
      }

      if (applyStreamChunk({ assistantMessageId, chunk, sessionId })) {
        completed = true;
        return;
      }
    }

    if (!completed && !controller.signal.aborted) {
      failAssistantStream({
        assistantMessageId,
        message: "Model Plane stream closed before completion.",
        sessionId,
      });
    }
  } catch (error) {
    if (controller.signal.aborted || isAbortError(error)) {
      return;
    }

    failAssistantStream({
      assistantMessageId,
      message: error instanceof Error ? error.message : "Model Plane stream failed.",
      sessionId,
    });
  } finally {
    if (assistantStreamControllers.get(sessionId) === controller) {
      assistantStreamControllers.delete(sessionId);
    }
  }
}

function applyStreamChunk({
  assistantMessageId,
  chunk,
  sessionId,
}: {
  assistantMessageId: string;
  chunk: ChatStreamChunk;
  sessionId: string;
}) {
  switch (chunk.type) {
    case "connected":
      markAssistantStreamConnected(sessionId);
      return false;
    case "delta":
      appendAssistantDelta({
        assistantMessageId,
        delta: chunk.delta,
        requestId: chunk.requestId,
        sessionId,
      });
      return false;
    case "grounding":
      updateAssistantMessage(sessionId, assistantMessageId, (message) => ({
        ...message,
        grounding: chunk.grounding,
      }));
      return false;
    case "done":
      if (chunk.timing) {
        // Latency breakdown for the just-finished turn — see where slow chats
        // spend time. ttftMs ≈ totalMs means the gateway didn't stream tokens.
        console.info("[verevon-chat timing]", {
          sessionId,
          modelUsed: chunk.modelUsed,
          ...chunk.timing,
        });
      }
      finishAssistantStream({
        assistantMessageId,
        inputTokens: chunk.inputTokens,
        modelUsed: chunk.modelUsed,
        outputTokens: chunk.outputTokens,
        timing: chunk.timing,
        sessionId,
      });
      return true;
    case "error":
      failAssistantStream({
        assistantMessageId,
        message: chunk.message,
        sessionId,
      });
      return true;
    case "reasoning_delta":
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        reasoning: `${m.reasoning ?? ""}${chunk.delta}`,
      }));
      return false;
    case "citation":
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        citations: [
          ...(m.citations ?? []),
          { id: chunk.id, title: chunk.title, url: chunk.url, snippet: chunk.snippet },
        ],
      }));
      return false;
    case "usage":
      // Real usage for the insight chip + reasoning popover (no placeholders).
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        inputTokens: chunk.inputTokens,
        outputTokens: chunk.outputTokens,
        latencyMs: chunk.latencyMs,
        costUsd: chunk.costUsd,
        confidence: chunk.confidence,
      }));
      return false;
    case "step_update":
      upsertTaskStep({ sessionId, step: chunk });
      return false;
    case "artifact":
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        artifacts: upsertById(m.artifacts, {
          id: chunk.id,
          kind: chunk.kind,
          title: chunk.title,
          content: chunk.content,
          version: chunk.version,
        }),
      }));
      return false;
    case "tool_call":
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        toolCalls: upsertById(m.toolCalls, {
          id: chunk.id,
          name: chunk.name,
          args: chunk.args,
          status: "running",
        }),
      }));
      return false;
    case "tool_result":
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        toolCalls: (m.toolCalls ?? []).map((call) =>
          call.id === chunk.id
            ? { ...call, status: chunk.status, output: chunk.output, error: chunk.error }
            : call,
        ),
      }));
      return false;
    case "attachment":
      updateAssistantMessage(sessionId, assistantMessageId, (m) => ({
        ...m,
        files: upsertById(m.files, {
          id: chunk.id,
          name: chunk.name,
          mime: chunk.mime,
          url: chunk.url,
          size: chunk.size,
        }),
      }));
      return false;
    case "stopped":
      // Terminal server-side stop ack — finalize the waiting message, keep partial.
      flushAssistantDelta(sessionId, assistantMessageId);
      updateAssistantMessage(sessionId, assistantMessageId, (m) => {
        if (m.status !== "waiting") {
          return m;
        }
        const partial = isAssistantPlaceholder(m.content) ? "" : m.content.trim();
        return { ...m, content: partial, status: "stopped" };
      });
      return true;
  }
}

/** Insert-or-replace an item by `id` in an optional array (immutable). */
function upsertById<T extends { id: string }>(list: T[] | undefined, item: T): T[] {
  const existing = list ?? [];
  const index = existing.findIndex((entry) => entry.id === item.id);
  if (index < 0) {
    return [...existing, item];
  }
  const next = existing.slice();
  next[index] = { ...next[index], ...item };
  return next;
}

function coerceTaskStepStatus(value: string): TaskStepStatus {
  switch (value) {
    case "done":
    case "active":
    case "waiting":
    case "error":
    case "stopped":
      return value;
    default:
      return "active";
  }
}

/// Insert or update a live agent step (drives the Steps tab + activity card).
function upsertTaskStep({
  sessionId,
  step,
}: {
  sessionId: string;
  step: { id: string; title: string; detail: string; status: string };
}) {
  updateChatSession(sessionId, (session) => {
    const status = coerceTaskStepStatus(step.status);
    const existingIndex = session.taskSteps.findIndex((existing) => existing.id === step.id);

    if (existingIndex >= 0) {
      const taskSteps = session.taskSteps.slice();
      taskSteps[existingIndex] = {
        ...taskSteps[existingIndex],
        title: step.title || taskSteps[existingIndex].title,
        detail: step.detail || taskSteps[existingIndex].detail,
        status,
      };
      return { ...session, taskSteps };
    }

    return {
      ...session,
      taskSteps: [
        ...session.taskSteps,
        {
          id: step.id || createId(),
          title: step.title,
          detail: step.detail,
          status,
          createdAt: new Date().toISOString(),
        },
      ],
    };
  });
}

/// Apply an update to one assistant message within a session.
function updateAssistantMessage(
  sessionId: string,
  assistantMessageId: string,
  updater: (message: ChatMessage) => ChatMessage,
) {
  updateChatSession(sessionId, (session) => ({
    ...session,
    messages: session.messages.map((message) =>
      message.id === assistantMessageId ? updater(message) : message,
    ),
  }));
}

function markAssistantStreamConnected(sessionId: string) {
  updateChatSession(sessionId, (session) => ({
    ...session,
    taskSteps: session.taskSteps.map((step) => {
      if (step.title === "Context route prepared") {
        return {
          ...step,
          detail: "Request authorized and routed to the Model Plane gateway.",
          status: "done",
        };
      }
      if (step.title === "Model gateway") {
        return {
          ...step,
          detail: "Streaming response from Model Plane.",
          status: "active",
        };
      }
      return step;
    }),
  }));
}

// Streaming deltas arrive token-by-token (often many per frame). Each store
// write forces a synchronous useSyncExternalStore re-render, so writing per
// token overflows React's nested-update limit ("Maximum update depth") under a
// fast burst. Coalesce tokens into ONE store update per animation frame: buffer
// the text, schedule a single flush, and reconcile on the next frame. Terminal
// events (done/error/stopped) MUST call flushAssistantDelta first so no buffered
// tail is lost and no late flush races the finalized message.
type PendingDelta = { text: string; requestId?: string; frame: number | null };
const pendingDeltas = new Map<string, PendingDelta>();

function deltaKey(sessionId: string, assistantMessageId: string) {
  return `${sessionId}:${assistantMessageId}`;
}

function flushAssistantDelta(sessionId: string, assistantMessageId: string) {
  const key = deltaKey(sessionId, assistantMessageId);
  const pending = pendingDeltas.get(key);
  if (!pending) {
    return;
  }

  if (pending.frame !== null && typeof cancelAnimationFrame !== "undefined") {
    cancelAnimationFrame(pending.frame);
  }
  pendingDeltas.delete(key);

  const { text, requestId } = pending;
  if (!text) {
    return;
  }

  updateChatSession(sessionId, (session) => ({
    ...session,
    messages: session.messages.map((message) => {
      if (message.id !== assistantMessageId) {
        return message;
      }

      const previousContent = message.status === "waiting" && isAssistantPlaceholder(message.content)
        ? ""
        : message.content;

      return {
        ...message,
        content: `${previousContent}${text}`,
        requestId: requestId ?? message.requestId,
        status: "waiting",
      };
    }),
    updatedAt: new Date().toISOString(),
  }));
}

function appendAssistantDelta({
  assistantMessageId,
  delta,
  requestId,
  sessionId,
}: {
  assistantMessageId: string;
  delta: string;
  requestId?: string;
  sessionId: string;
}) {
  if (!delta) {
    return;
  }

  const key = deltaKey(sessionId, assistantMessageId);
  const pending = pendingDeltas.get(key) ?? { text: "", requestId, frame: null };
  pending.text += delta;
  if (requestId) {
    pending.requestId = requestId;
  }

  if (pending.frame === null) {
    pending.frame = typeof requestAnimationFrame !== "undefined"
      ? requestAnimationFrame(() => flushAssistantDelta(sessionId, assistantMessageId))
      : (setTimeout(() => flushAssistantDelta(sessionId, assistantMessageId), 16) as unknown as number);
  }

  pendingDeltas.set(key, pending);
}

function finishAssistantStream({
  assistantMessageId,
  inputTokens,
  modelUsed,
  outputTokens,
  timing,
  sessionId,
}: {
  assistantMessageId: string;
  inputTokens: number;
  modelUsed: string;
  outputTokens: number;
  timing?: ChatTiming;
  sessionId: string;
}) {
  // Drain any tokens still buffered for coalescing so the finalized content
  // includes the tail and no late rAF flush races this terminal write.
  flushAssistantDelta(sessionId, assistantMessageId);
  const finishedAt = new Date().toISOString();

  updateChatSession(sessionId, (session) => {
    let assistantContent = "";

    const messages = session.messages.map((message) => {
      if (message.id !== assistantMessageId) {
        return message;
      }

      const streamedContent = isAssistantPlaceholder(message.content) ? "" : message.content;
      assistantContent = streamedContent.trim() ? streamedContent : EMPTY_MODEL_RESPONSE_CONTENT;
      return {
        ...message,
        content: assistantContent,
        inputTokens,
        modelUsed,
        outputTokens,
        latencyMs: timing?.totalMs,
        ttftMs: timing?.ttftMs,
        status: undefined,
      };
    });

    return {
      ...session,
      messages,
      preview: assistantContent ? createPreview(assistantContent) : session.preview,
      taskSteps: session.taskSteps.map((step) => {
        if (step.title === "Context route prepared") {
          return { ...step, status: "done" };
        }
        if (step.title === "Model gateway") {
          const tokenSummary = outputTokens > 0 ? `${outputTokens} output tokens` : "no output token count";
          return {
            ...step,
            detail: `Completed via ${modelUsed || "Model Plane"} with ${tokenSummary}.`,
            status: "done",
          };
        }
        return step;
      }),
      updatedAt: finishedAt,
    };
  });
}

function failAssistantStream({
  assistantMessageId,
  message,
  sessionId,
}: {
  assistantMessageId: string;
  message: string;
  sessionId: string;
}) {
  // Cancel/drain any buffered tokens so a late flush can't re-add content
  // after the error message has replaced it.
  flushAssistantDelta(sessionId, assistantMessageId);
  const failedAt = new Date().toISOString();
  // Surface the REAL failure reason (was hidden in the agent-activity card,
  // which is removed). Keep the generic line only when no detail is available.
  const detail = message.trim();
  const content = detail && detail !== "Model Plane stream failed."
    ? detail
    : MODEL_STREAM_ERROR_CONTENT;

  updateChatSession(sessionId, (session) => ({
    ...session,
    messages: session.messages.map((chatMessage) => (
      chatMessage.id === assistantMessageId
        ? {
            ...chatMessage,
            content,
            status: "error",
          }
        : chatMessage
    )),
    taskSteps: session.taskSteps.map((step) => (
      step.title === "Model gateway"
        ? {
            ...step,
            detail: message,
            status: "error",
          }
        : step
    )),
    updatedAt: failedAt,
  }));
}

function updateChatSession(sessionId: string, updater: (session: ChatSession) => ChatSession) {
  updateChatStore((current) => ({
    ...current,
    sessions: current.sessions.map((session) => (
      session.id === sessionId ? updater(session) : session
    )),
  }));
}

/**
 * Rehydrate a session's transcript from session-core (chat-parity §1,
 * cross-device resume). Best-effort and conservative:
 *   - skips if a live stream is in flight for this session (never clobber it);
 *   - skips if the session already has local messages (server is the fallback,
 *     not the source of truth, once the device has its own copy);
 *   - only user/assistant turns are rendered (system/tool rows are dropped);
 *   - any failure is swallowed — the session simply stays empty.
 */
export async function hydrateSessionFromServer(sessionId: string): Promise<void> {
  if (!sessionId || assistantStreamControllers.has(sessionId)) {
    return;
  }
  const existing = chatStoreState.sessions.find((session) => session.id === sessionId);
  if (existing && existing.messages.length > 0) {
    return;
  }

  const history = await loadThreadHistory(sessionId);
  if (history.length === 0) {
    return;
  }

  const messages: ChatMessage[] = history
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => ({
      id: createId(),
      role: m.role as MessageRole,
      content: m.content,
      createdAt: new Date().toISOString(),
      tools: [],
      attachments: [],
    }));
  if (messages.length === 0) {
    return;
  }

  // Re-check the in-flight guard: a stream may have started during the await.
  if (assistantStreamControllers.has(sessionId)) {
    return;
  }
  updateChatStore((current) => {
    const found = current.sessions.find((session) => session.id === sessionId);
    // Don't overwrite if the session gained messages while we were fetching.
    if (found && found.messages.length > 0) {
      return current;
    }
    const lastText = messages[messages.length - 1]?.content ?? "";
    const hydrated: ChatSession = found
      ? { ...found, messages, preview: createPreview(lastText) }
      : {
          id: sessionId,
          title: createTitle(messages[0]?.content ?? "Conversation"),
          preview: createPreview(lastText),
          updatedAt: new Date().toISOString(),
          messages,
          taskSteps: [],
          branchCount: 0,
        };
    const others = current.sessions.filter((session) => session.id !== sessionId);
    return { ...current, sessions: [hydrated, ...others] };
  });
}

function isAbortError(error: unknown) {
  return error instanceof Error && error.name === "AbortError";
}

export function isAssistantPlaceholder(content: string) {
  return content === WAITING_ASSISTANT_CONTENT ||
    content === "Regeneration queued for the live Verevon agent stream.";
}

function emitChatStoreChange() {
  for (const listener of chatStoreListeners) {
    listener();
  }
}

function buildTaskSteps(payload: ComposerSubmitPayload, createdAt: string, mode: "submit" | "regenerate"): AgentTaskStep[] {
  const selectedTools = payload.tools.length > 0
    ? payload.tools.map((tool) => toolLabels[tool]).join(", ")
    : "default chat";
  const attachmentSummary = payload.attachments.length === 0
    ? "no attachments"
    : `${payload.attachments.length} attachment${payload.attachments.length === 1 ? "" : "s"}`;

  return [
    {
      id: createId(),
      title: mode === "regenerate" ? "Regenerate requested" : "Prompt received",
      detail: `${payload.text.length} characters captured with ${selectedTools} and ${attachmentSummary}.`,
      status: "done",
      createdAt,
    },
    {
      id: createId(),
      title: "Context route prepared",
      detail: "Ready to request the signed-in user's Model Plane context.",
      status: "active",
      createdAt,
    },
    {
      id: createId(),
      title: "Model gateway",
      detail: "Opening the production Model Plane stream endpoint.",
      status: "waiting",
      createdAt,
    },
  ];
}

function createSubmittedChatSession({
  existingSession,
  payload,
  submittedAt,
  targetSessionId,
}: {
  existingSession: ChatSession | null;
  payload: ComposerSubmitPayload;
  submittedAt: string;
  targetSessionId: string;
}): ChatSession {
  const userMessage: ChatMessage = {
    id: createId(),
    role: "user",
    content: payload.text,
    createdAt: submittedAt,
    model: payload.model,
    tools: payload.tools,
    attachments: payload.attachments,
  };
  const assistantStatus: ChatMessage = {
    id: createId(),
    role: "assistant",
    content: WAITING_ASSISTANT_CONTENT,
    createdAt: submittedAt,
    model: payload.model,
    tools: payload.tools,
    attachments: [],
    status: "waiting",
  };

  return {
    id: targetSessionId,
    title: existingSession?.messages.length ? existingSession.title : createTitle(payload.text),
    preview: createPreview(payload.text),
    updatedAt: submittedAt,
    messages: [...(existingSession?.messages ?? []), userMessage, assistantStatus],
    taskSteps: buildTaskSteps(payload, submittedAt, "submit"),
    branchCount: existingSession?.branchCount ?? 0,
  };
}

function readStoredSessions(): ChatSession[] {
  if (typeof localStorage === "undefined") {
    return [];
  }

  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed.filter(isChatSession).slice(0, SESSION_LIMIT);
  } catch {
    return [];
  }
}

function writeStoredSessions(sessions: ChatSession[]) {
  if (typeof localStorage === "undefined") {
    return;
  }

  localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions.slice(0, SESSION_LIMIT)));
}

function readPromptDraft() {
  if (typeof window === "undefined") {
    return "";
  }

  return new URLSearchParams(window.location.search).get("prompt")?.trim() ?? "";
}

function isChatSession(value: unknown): value is ChatSession {
  if (!value || typeof value !== "object") {
    return false;
  }

  const session = value as Partial<ChatSession>;
  return typeof session.id === "string"
    && typeof session.title === "string"
    && typeof session.preview === "string"
    && typeof session.updatedAt === "string"
    && Array.isArray(session.messages)
    && Array.isArray(session.taskSteps);
}

function createTitle(text: string) {
  const words = text.replace(/\s+/g, " ").trim().split(" ").slice(0, 7).join(" ");
  return words.length > 0 ? words : "New conversation";
}

function createPreview(text: string) {
  const normalized = text.replace(/\s+/g, " ").trim();
  return normalized.length > 74 ? `${normalized.slice(0, 74)}…` : normalized;
}

function createId() {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
