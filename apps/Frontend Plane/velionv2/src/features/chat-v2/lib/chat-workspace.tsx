"use client";

import { createContext, use, useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import type {
  ComposerAttachment,
  ComposerSubmitPayload,
  ComposerToolId,
} from "@/features/chat-v2/components/VelionComposer";
import { toolLabels } from "@/features/chat-v2/lib/chat-format";

const STORAGE_KEY = "velion:v2:chat:sessions";
const SESSION_LIMIT = 30;

export type MessageRole = "user" | "assistant";
export type TaskStepStatus = "done" | "active" | "waiting" | "error" | "stopped";

export type ChatMessage = {
  id: string;
  role: MessageRole;
  content: string;
  createdAt: string;
  tools: ComposerToolId[];
  attachments: ComposerAttachment[];
  status?: "waiting" | "stopped";
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
    updateChatStore((current) => {
      const submittedAt = new Date().toISOString();
      const targetSessionId = current.activeSessionId ?? createId();
      const existingSession = current.sessions.find((session) => session.id === targetSessionId);
      const userMessage: ChatMessage = {
        id: createId(),
        role: "user",
        content: payload.text,
        createdAt: submittedAt,
        tools: payload.tools,
        attachments: payload.attachments,
      };
      const assistantStatus: ChatMessage = {
        id: createId(),
        role: "assistant",
        content: "Awaiting the live Velion agent stream.",
        createdAt: submittedAt,
        tools: payload.tools,
        attachments: [],
        status: "waiting",
      };
      const nextSession: ChatSession = {
        id: targetSessionId,
        title: existingSession?.messages.length ? existingSession.title : createTitle(payload.text),
        preview: createPreview(payload.text),
        updatedAt: submittedAt,
        messages: [...(existingSession?.messages ?? []), userMessage, assistantStatus],
        taskSteps: buildTaskSteps(payload, submittedAt, "submit"),
        branchCount: existingSession?.branchCount ?? 0,
      };
      const withoutTarget = current.sessions.filter((session) => session.id !== targetSessionId);

      return {
        activeSessionId: targetSessionId,
        composerDraft: "",
        sessions: [nextSession, ...withoutTarget].slice(0, SESSION_LIMIT),
      };
    });
  };

  const startNewChat = () => {
    updateChatStore((current) => ({
      ...current,
      activeSessionId: null,
      composerDraft: "",
    }));
  };

  const stopTask = () => {
    updateChatStore((current) => {
      if (!current.activeSessionId) {
        return current;
      }

      const stoppedAt = new Date().toISOString();

      return {
        ...current,
        sessions: current.sessions.map((session) => (
          session.id === current.activeSessionId
            ? {
                ...session,
                updatedAt: stoppedAt,
                messages: session.messages.map((message) => (
                  message.status === "waiting" ? { ...message, content: "Task stopped.", status: "stopped" } : message
                )),
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
    updateChatStore((current) => {
      const active = current.sessions.find((session) => session.id === current.activeSessionId);
      const lastUserMessage = [...(active?.messages ?? [])].reverse().find((message) => message.role === "user");

      if (!active || !lastUserMessage) {
        return current;
      }

      const regeneratedAt = new Date().toISOString();
      const payload: ComposerSubmitPayload = {
        text: lastUserMessage.content,
        tools: lastUserMessage.tools,
        attachments: lastUserMessage.attachments,
      };
      const assistantStatus: ChatMessage = {
        id: createId(),
        role: "assistant",
        content: "Regeneration queued for the live Velion agent stream.",
        createdAt: regeneratedAt,
        tools: lastUserMessage.tools,
        attachments: [],
        status: "waiting",
      };

      return {
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
      };
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
      detail: "Ready to request the signed-in user's inbox, knowledge, and connected source context.",
      status: "active",
      createdAt,
    },
    {
      id: createId(),
      title: "Model gateway",
      detail: "Waiting for the production agent stream endpoint. The UI does not generate fallback answers.",
      status: "waiting",
      createdAt,
    },
  ];
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
