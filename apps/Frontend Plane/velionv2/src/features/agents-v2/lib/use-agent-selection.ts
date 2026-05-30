"use client";

import { createContext, createElement, use, useSyncExternalStore, type ReactNode } from "react";
import {
  agentSelectionEventName,
  defaultAgentSelectionId,
  getAgentFeatureFromSearch,
  getDefaultAgentFeatureForRole,
  defaultChatbotAddOnId,
  defaultChatbotBuilderSectionId,
  defaultWorkflowBuilderToolId,
  getAgentSelectionFromSearch,
  getChatbotAddOnFromSearch,
  getChatbotBuilderSectionFromSearch,
  getWorkflowBuilderToolFromSearch,
  isAgentFeatureForRole,
  isAgentSelectionId,
  isChatbotAddOnId,
  isChatbotBuilderSectionId,
  isCoreAgentRoleId,
  isWorkflowBuilderToolId,
  type AgentFeatureId,
  type ChatbotAddOnId,
  type ChatbotBuilderSectionId,
  type AgentSelectionId,
  type WorkflowBuilderToolId,
} from "@/features/agents-v2/lib/agent-roles";

const AgentSelectionInitialContext = createContext<AgentSelectionId>(defaultAgentSelectionId);

export function AgentSelectionInitialProvider({
  children,
  selection,
}: {
  children: ReactNode;
  selection: AgentSelectionId;
}) {
  return createElement(AgentSelectionInitialContext.Provider, { value: selection }, children);
}

export function useAgentSelection() {
  const initialSelection = use(AgentSelectionInitialContext);
  const locationSnapshot = useAgentLocationSnapshot();
  const selection = getAgentSelectionFromSnapshot(locationSnapshot, initialSelection);

  const updateSelection = (nextSelection: AgentSelectionId) => {
    pushAgentLocation((url) => {
      if (nextSelection === defaultAgentSelectionId) {
        url.searchParams.delete("agent");
        url.searchParams.delete("stage");
        url.searchParams.delete("feature");
        url.searchParams.delete("view");
        url.searchParams.delete("addon");
      } else if (nextSelection === "chatbot") {
        url.searchParams.set("agent", nextSelection);
        url.searchParams.delete("stage");
        url.searchParams.delete("feature");
        url.searchParams.delete("tool");
        if (!isChatbotBuilderSectionId(url.searchParams.get("view"))) {
          url.searchParams.set("view", defaultChatbotBuilderSectionId);
        }
        if (!isChatbotAddOnId(url.searchParams.get("addon"))) {
          url.searchParams.set("addon", defaultChatbotAddOnId);
        }
      } else if (nextSelection === "workflow") {
        url.searchParams.set("agent", nextSelection);
        url.searchParams.delete("stage");
        url.searchParams.delete("feature");
        url.searchParams.delete("view");
        url.searchParams.delete("addon");
        if (!isWorkflowBuilderToolId(url.searchParams.get("tool"))) {
          url.searchParams.set("tool", defaultWorkflowBuilderToolId);
        }
      } else if (isCoreAgentRoleId(nextSelection)) {
        url.searchParams.set("agent", nextSelection);
        url.searchParams.delete("stage");
        url.searchParams.delete("view");
        url.searchParams.delete("addon");
        url.searchParams.delete("tool");
        if (!isAgentFeatureForRole(nextSelection, url.searchParams.get("feature"))) {
          url.searchParams.set("feature", getDefaultAgentFeatureForRole(nextSelection));
        }
      }
    });
  };

  const updateSelectionFromValue = (value: string) => {
    updateSelection(isAgentSelectionId(value) ? value : defaultAgentSelectionId);
  };

  return [selection, updateSelection, updateSelectionFromValue] as const;
}

function getAgentSelectionFromSnapshot(snapshot: string, fallbackSelection: AgentSelectionId): AgentSelectionId {
  if (!snapshot) {
    return fallbackSelection;
  }

  const selection = getAgentSelectionFromSearch(getSearchFromLocationSnapshot(snapshot));
  if (selection !== defaultAgentSelectionId) {
    return selection;
  }

  if (getPathnameFromLocationSnapshot(snapshot).startsWith("/agents/chatbots")) {
    return "chatbot";
  }

  return selection;
}

export function useAgentFeature(agentSelection: AgentSelectionId) {
  const locationSnapshot = useAgentLocationSnapshot();
  const selectedRole = getAgentSelectionFromSnapshot(locationSnapshot, agentSelection);
  const feature = isCoreAgentRoleId(selectedRole)
    ? getAgentFeatureFromSearch(getSearchFromLocationSnapshot(locationSnapshot), selectedRole)
    : getDefaultAgentFeatureForRole("service");

  const updateFeature = (nextFeature: AgentFeatureId) => {
    if (!isCoreAgentRoleId(agentSelection)) {
      return;
    }

    const safeFeature = isAgentFeatureForRole(agentSelection, nextFeature)
      ? nextFeature
      : getDefaultAgentFeatureForRole(agentSelection);
    pushAgentLocation((url) => {
      url.searchParams.set("agent", agentSelection);
      url.searchParams.set("feature", safeFeature);
      url.searchParams.delete("stage");
      url.searchParams.delete("view");
      url.searchParams.delete("addon");
      url.searchParams.delete("tool");
    });
  };

  const updateFeatureFromValue = (value: string) => {
    if (!isCoreAgentRoleId(agentSelection)) {
      return;
    }

    updateFeature(isAgentFeatureForRole(agentSelection, value) ? value : getDefaultAgentFeatureForRole(agentSelection));
  };

  return [feature, updateFeature, updateFeatureFromValue] as const;
}

export function useChatbotBuilderSection() {
  const locationSnapshot = useAgentLocationSnapshot();
  const section = getChatbotBuilderSectionFromSearch(getSearchFromLocationSnapshot(locationSnapshot));

  const updateSection = (nextSection: ChatbotBuilderSectionId) => {
    pushAgentLocation((url) => {
      url.searchParams.set("agent", "chatbot");
      url.searchParams.set("view", nextSection);
      if (!isChatbotAddOnId(url.searchParams.get("addon"))) {
        url.searchParams.set("addon", defaultChatbotAddOnId);
      }
      url.searchParams.delete("stage");
      url.searchParams.delete("tool");
    });
  };

  const updateSectionFromValue = (value: string) => {
    updateSection(isChatbotBuilderSectionId(value) ? value : defaultChatbotBuilderSectionId);
  };

  return [section, updateSection, updateSectionFromValue] as const;
}

export function useChatbotAddOn() {
  const locationSnapshot = useAgentLocationSnapshot();
  const addOn = getChatbotAddOnFromSearch(getSearchFromLocationSnapshot(locationSnapshot));

  const updateAddOn = (nextAddOn: ChatbotAddOnId) => {
    pushAgentLocation((url) => {
      url.searchParams.set("agent", "chatbot");
      url.searchParams.set("addon", nextAddOn);
      if (!isChatbotBuilderSectionId(url.searchParams.get("view"))) {
        url.searchParams.set("view", defaultChatbotBuilderSectionId);
      }
      url.searchParams.delete("stage");
      url.searchParams.delete("tool");
    });
  };

  const updateAddOnFromValue = (value: string) => {
    updateAddOn(isChatbotAddOnId(value) ? value : defaultChatbotAddOnId);
  };

  return [addOn, updateAddOn, updateAddOnFromValue] as const;
}

export function useWorkflowBuilderTool() {
  const locationSnapshot = useAgentLocationSnapshot();
  const tool = getWorkflowBuilderToolFromSearch(getSearchFromLocationSnapshot(locationSnapshot));

  const updateTool = (nextTool: WorkflowBuilderToolId) => {
    pushAgentLocation((url) => {
      url.searchParams.set("agent", "workflow");
      url.searchParams.set("tool", nextTool);
      url.searchParams.delete("stage");
      url.searchParams.delete("view");
      url.searchParams.delete("addon");
    });
  };

  const updateToolFromValue = (value: string) => {
    updateTool(isWorkflowBuilderToolId(value) ? value : defaultWorkflowBuilderToolId);
  };

  return [tool, updateTool, updateToolFromValue] as const;
}

function useAgentLocationSnapshot() {
  return useSyncExternalStore(subscribeToAgentLocation, getAgentLocationSnapshot, getServerAgentLocationSnapshot);
}

function subscribeToAgentLocation(onStoreChange: () => void) {
  if (typeof window === "undefined") {
    return () => {};
  }

  window.addEventListener("popstate", onStoreChange);
  window.addEventListener(agentSelectionEventName, onStoreChange);

  return () => {
    window.removeEventListener("popstate", onStoreChange);
    window.removeEventListener(agentSelectionEventName, onStoreChange);
  };
}

function getAgentLocationSnapshot() {
  if (typeof window === "undefined") {
    return "";
  }

  return `${window.location.pathname}${window.location.search}`;
}

function getServerAgentLocationSnapshot() {
  return "";
}

function getSearchFromLocationSnapshot(snapshot: string) {
  const queryStart = snapshot.indexOf("?");
  return queryStart === -1 ? "" : snapshot.slice(queryStart);
}

function getPathnameFromLocationSnapshot(snapshot: string) {
  const queryStart = snapshot.indexOf("?");
  return queryStart === -1 ? snapshot : snapshot.slice(0, queryStart);
}

function pushAgentLocation(updateUrl: (url: URL) => void) {
  if (typeof window === "undefined") {
    return;
  }

  const url = new URL(window.location.href);
  updateUrl(url);
  window.history.pushState(null, "", `${url.pathname}${url.search}${url.hash}`);
  window.dispatchEvent(new Event(agentSelectionEventName));
}
