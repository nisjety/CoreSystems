import { useReducer } from "react";
import type { ComposerSubmitPayload, ComposerToolId } from "@/features/chat-v2/components/VerevonComposer";
import {
  defaultComposerSettings,
  type ComposerFile,
  type ComposerSettings,
  type ComposerTurn,
  type DashboardComposerModel,
  type DashboardComposerProps,
  type ResponseMode,
} from "@/features/dashboard-v2/lib/dashboard-composer-model";

const chatComposerTimeFormat = new Intl.DateTimeFormat("nb-NO", {
  hour: "2-digit",
  minute: "2-digit",
});

type ChatComposerState = {
  browseWeb: boolean;
  deepSearch: boolean;
  imageMode: boolean;
  files: ComposerFile[];
  historyOpen: boolean;
  message: string;
  modelOpen: boolean;
  responseMode: ResponseMode;
  selectedModel: DashboardComposerModel;
  settings: ComposerSettings;
  settingsOpen: boolean;
  suggestionsOpen: boolean;
  turns: ComposerTurn[];
  voiceMode: boolean;
};

type ChatComposerAction =
  | { type: "browseWeb"; value: boolean }
  | { type: "deepSearch"; value: boolean }
  | { type: "imageMode"; value: boolean }
  | { type: "files"; value: ComposerFile[] }
  | { type: "historyOpen"; value: boolean }
  | { type: "message"; value: string }
  | { type: "modelOpen"; value: boolean }
  | { type: "openAgentBuilder" }
  | { type: "responseMode"; value: ResponseMode }
  | { type: "selectedModel"; value: DashboardComposerModel }
  | { type: "settings"; value: ComposerSettings }
  | { type: "settingsOpen"; value: boolean }
  | { type: "submit"; value: ComposerTurn }
  | { type: "suggestionsOpen"; value: boolean }
  | { type: "voiceMode"; value: boolean };

type UseChatDashboardComposerOptions = {
  initialValue: string;
  onSubmit: (payload: ComposerSubmitPayload) => void;
};

function getInitialChatComposerState(initialValue: string): ChatComposerState {
  return {
    browseWeb: true,
    deepSearch: false,
    imageMode: false,
    files: [],
    historyOpen: false,
    message: initialValue,
    modelOpen: false,
    responseMode: "auto",
    selectedModel: "GPT-4o Mini",
    settings: defaultComposerSettings,
    settingsOpen: false,
    suggestionsOpen: false,
    turns: [],
    voiceMode: false,
  };
}

function chatComposerReducer(
  state: ChatComposerState,
  action: ChatComposerAction,
): ChatComposerState {
  switch (action.type) {
    case "browseWeb":
      return { ...state, browseWeb: action.value };
    case "deepSearch":
      return { ...state, deepSearch: action.value };
    case "imageMode":
      return { ...state, imageMode: action.value };
    case "files":
      return { ...state, files: action.value };
    case "historyOpen":
      return { ...state, historyOpen: action.value };
    case "message":
      return { ...state, message: action.value };
    case "modelOpen":
      return { ...state, modelOpen: action.value };
    case "openAgentBuilder":
      return {
        ...state,
        message: "Opprett en agent som håndterer kundesamtaler med kunnskapsbase, tone og eskaleringer.",
        suggestionsOpen: true,
      };
    case "responseMode":
      return { ...state, responseMode: action.value };
    case "selectedModel":
      return { ...state, selectedModel: action.value };
    case "settings":
      return { ...state, settings: action.value };
    case "settingsOpen":
      return { ...state, settingsOpen: action.value };
    case "submit":
      return {
        ...state,
        files: [],
        historyOpen: false,
        message: "",
        modelOpen: false,
        settingsOpen: false,
        suggestionsOpen: false,
        turns: [action.value, ...state.turns].slice(0, 6),
      };
    case "suggestionsOpen":
      return { ...state, suggestionsOpen: action.value };
    case "voiceMode":
      return { ...state, voiceMode: action.value };
  }
}

export function useChatDashboardComposer({
  initialValue,
  onSubmit,
}: UseChatDashboardComposerOptions): DashboardComposerProps {
  const [state, dispatch] = useReducer(
    chatComposerReducer,
    initialValue,
    getInitialChatComposerState,
  );

  const submitMessage = () => {
    const body = state.message.trim();

    if (!body && state.files.length === 0) {
      return;
    }

    const now = new Date();
    const submittedText = body || "Vedlegg sendt til Verevon.";
    const nextTurn: ComposerTurn = {
      id: `chat-turn-${now.getTime()}`,
      body: submittedText,
      model: state.selectedModel,
      responseMode: state.responseMode,
      browseWeb: state.browseWeb,
      deepSearch: state.deepSearch,
      files: state.files.map((file) => file.name),
      createdAt: chatComposerTimeFormat.format(now),
      createdAtIso: now.toISOString(),
    };

    const tools = getComposerTools({
      browseWeb: state.browseWeb,
      deepSearch: state.deepSearch,
      message: body,
      responseMode: state.responseMode,
    });
    if (state.imageMode && !tools.includes("image")) {
      tools.push("image");
    }

    onSubmit({
      text: submittedText,
      model: getGatewayModelForComposerModel(state.selectedModel),
      tools,
      attachments: state.files.map((file) => ({
        id: file.id,
        name: file.name,
        size: file.size,
        type: file.type || "application/octet-stream",
        url: file.url,
      })),
    });
    dispatch({ type: "submit", value: nextTurn });
  };

  return {
    browseWeb: state.browseWeb,
    deepSearch: state.deepSearch,
    imageMode: state.imageMode,
    files: state.files,
    historyOpen: state.historyOpen,
    message: state.message,
    modelOpen: state.modelOpen,
    responseMode: state.responseMode,
    selectedModel: state.selectedModel,
    settings: state.settings,
    settingsOpen: state.settingsOpen,
    suggestionsOpen: state.suggestionsOpen,
    turns: state.turns,
    voiceMode: state.voiceMode,
    onBrowseWebChange: (value) => dispatch({ type: "browseWeb", value }),
    onDeepSearchChange: (value) => dispatch({ type: "deepSearch", value }),
    onImageModeChange: (value) => dispatch({ type: "imageMode", value }),
    onFilesChange: (value) => dispatch({ type: "files", value }),
    onHistoryOpenChange: (value) => dispatch({ type: "historyOpen", value }),
    onMessageChange: (value) => dispatch({ type: "message", value }),
    onModelChange: (value) => dispatch({ type: "selectedModel", value }),
    onModelOpenChange: (value) => dispatch({ type: "modelOpen", value }),
    onOpenAgentBuilder: () => dispatch({ type: "openAgentBuilder" }),
    onResponseModeChange: (value) => dispatch({ type: "responseMode", value }),
    onSettingsChange: (value) => dispatch({ type: "settings", value }),
    onSettingsOpenChange: (value) => dispatch({ type: "settingsOpen", value }),
    onSubmit: submitMessage,
    onSuggestionsOpenChange: (value) => dispatch({ type: "suggestionsOpen", value }),
    onVoiceModeChange: (value) => dispatch({ type: "voiceMode", value }),
  };
}

export function getComposerTools({
  browseWeb,
  deepSearch,
  message,
  responseMode,
}: {
  browseWeb: boolean;
  deepSearch: boolean;
  message: string;
  responseMode: ResponseMode;
}): ComposerToolId[] {
  const tools: ComposerToolId[] = [];

  if (browseWeb) {
    tools.push("search");
  }
  if (responseMode === "deep") {
    tools.push("reason");
  }
  if (deepSearch) {
    tools.push("research");
  }
  if (message.trimStart().toLowerCase().startsWith("/image ")) {
    tools.push("image");
  }

  return [...new Set(tools)];
}

export function getGatewayModelForComposerModel(model: DashboardComposerModel) {
  switch (model) {
    case "GPT-4o Mini":
      return "gpt-4o-mini";
    case "Claude Sonnet":
      return "claude-sonnet-4-20250514";
    case "GPT-4.1":
    case "Verevon Reasoner":
      return undefined;
  }
}
