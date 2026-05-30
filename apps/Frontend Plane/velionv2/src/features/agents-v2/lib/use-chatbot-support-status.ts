import { useSyncExternalStore } from "react";

export type SupportIntegrationStatus = {
  status: "loading" | "connected" | "not-configured" | "error";
  agents: number;
  groups: number;
  macros: number;
  message: string;
};

type ChatbotRuntimeResponse = {
  support: {
    configured: boolean;
    connected: boolean;
    agents: number;
    groups: number;
    macros: number;
    message: string;
  };
};

const initialSupportIntegrationStatus: SupportIntegrationStatus = {
  status: "loading",
  agents: 0,
  groups: 0,
  macros: 0,
  message: "Checking support integration…",
};

const supportStatusListeners = new Set<() => void>();
let supportStatusSnapshot = initialSupportIntegrationStatus;
let supportStatusRequest: Promise<void> | null = null;

function notifySupportStatusListeners() {
  for (const listener of supportStatusListeners) {
    listener();
  }
}

function setSupportStatusSnapshot(nextSnapshot: SupportIntegrationStatus) {
  supportStatusSnapshot = nextSnapshot;
  notifySupportStatusListeners();
}

async function loadSupportStatus() {
  if (typeof fetch !== "function") {
    setSupportStatusSnapshot({
      status: "not-configured",
      agents: 0,
      groups: 0,
      macros: 0,
      message: "Support APIs are not available in this runtime.",
    });
    return;
  }

  let response: Response;
  let payload: ChatbotRuntimeResponse | null;

  try {
    response = await fetch("/api/agents/chatbot/runtime", { cache: "no-store" });
    payload = await response.json().catch(() => null) as ChatbotRuntimeResponse | null;
  } catch {
    setSupportStatusSnapshot({
      status: "error",
      agents: 0,
      groups: 0,
      macros: 0,
      message: "Support integration could not be checked.",
    });
    return;
  }

  if (response.ok && payload) {
    const support = payload.support;
    setSupportStatusSnapshot({
      status: support.connected ? "connected" : support.configured ? "error" : "not-configured",
      agents: support.agents,
      groups: support.groups,
      macros: support.macros,
      message: support.message,
    });
    return;
  }

  setSupportStatusSnapshot({
    status: "error",
    agents: 0,
    groups: 0,
    macros: 0,
    message: "Support integration could not be checked.",
  });
}

function ensureSupportStatusRequest() {
  supportStatusRequest ??= loadSupportStatus().finally(() => {
    supportStatusRequest = null;
  });
}

function subscribeSupportStatus(listener: () => void) {
  supportStatusListeners.add(listener);
  ensureSupportStatusRequest();

  return () => {
    supportStatusListeners.delete(listener);
  };
}

function getSupportStatusSnapshot() {
  return supportStatusSnapshot;
}

export function useChatbotSupportStatus() {
  return useSyncExternalStore(
    subscribeSupportStatus,
    getSupportStatusSnapshot,
    getSupportStatusSnapshot,
  );
}
