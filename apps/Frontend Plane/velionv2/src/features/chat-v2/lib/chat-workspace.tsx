"use client";

import { createContext, use, useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import type {
  ComposerAttachment,
  ComposerSubmitPayload,
  ComposerToolId,
} from "@/features/chat-v2/components/VelionComposer";
import { toolLabels } from "@/features/chat-v2/lib/chat-format";
import { streamChat, type ChatStreamChunk, type ChatTiming } from "@/features/chat-v2/lib/chat-stream";

const STORAGE_KEY = "velion:v2:chat:sessions";
const LAUNCH_MOTION_KEY = "velion:v2:chat:launch-motion";
const LAUNCH_MOTION_TTL_MS = 4_000;
const SESSION_LIMIT = 30;
const WAITING_ASSISTANT_CONTENT = "Awaiting the live Velion agent stream.";
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
};

export type Citation = {
  id: string;
  title: string;
  url: string;
  snippet: string;
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

export function VelionChatWorkspaceProvider({ children }: { children: ReactNode }) {
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
      content: "Regeneration queued for the live Velion agent stream.",
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

export function useVelionChatWorkspace() {
  const context = use(ChatWorkspaceContext);

  if (!context) {
    throw new Error("useVelionChatWorkspace must be used within VelionChatWorkspaceProvider.");
  }

  return context;
}

export function useVelionChatWorkspaceSafe() {
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

  try {
    for await (const chunk of streamChat({
      browseWeb: payload.tools.includes("search"),
      content: payload.text,
      // Opt into the rich event families the chat UI renders (chat-parity §2):
      // real usage (insight chip), citations (Sources), reasoning (thinking).
      features: ["usage", "citations", "reasoning"],
      model: payload.model,
      sessionId,
      signal: controller.signal,
      tools: payload.tools,
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
    case "done":
      if (chunk.timing) {
        // Latency breakdown for the just-finished turn — see where slow chats
        // spend time. ttftMs ≈ totalMs means the gateway didn't stream tokens.
        console.info("[velion-chat timing]", {
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
  }
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
        content: `${previousContent}${delta}`,
        requestId: requestId ?? message.requestId,
        status: "waiting",
      };
    }),
    updatedAt: new Date().toISOString(),
  }));
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
  const failedAt = new Date().toISOString();

  updateChatSession(sessionId, (session) => ({
    ...session,
    messages: session.messages.map((chatMessage) => (
      chatMessage.id === assistantMessageId
        ? {
            ...chatMessage,
            content: MODEL_STREAM_ERROR_CONTENT,
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

function isAbortError(error: unknown) {
  return error instanceof Error && error.name === "AbortError";
}

function isAssistantPlaceholder(content: string) {
  return content === WAITING_ASSISTANT_CONTENT ||
    content === "Regeneration queued for the live Velion agent stream.";
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
