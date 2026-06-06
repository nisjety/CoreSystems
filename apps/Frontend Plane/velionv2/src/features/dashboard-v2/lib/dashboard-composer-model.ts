export type ResponseMode = "auto" | "quick" | "deep";
export type ComposerTone = "concise" | "balanced" | "detailed";

export type ComposerFile = {
  id: string;
  name: string;
  size: number;
  type: string;
  url: string;
};

export type ComposerTurn = {
  id: string;
  body: string;
  model: string;
  responseMode: ResponseMode;
  browseWeb: boolean;
  deepSearch: boolean;
  files: string[];
  createdAt: string;
  createdAtIso: string;
};

export type ComposerSettings = {
  voiceLang: string;
  tone: ComposerTone;
};

export type DashboardComposerModel = "GPT-4o Mini" | "GPT-4.1" | "Claude Sonnet" | "Velion Reasoner";

export type DashboardComposerProps = {
  browseWeb: boolean;
  deepSearch: boolean;
  /** Image-generation intent toggle. Optional — when `onImageModeChange` is
   *  omitted the "Bilde" pill is hidden (e.g. the dashboard-home composer). */
  imageMode?: boolean;
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
  onBrowseWebChange: (active: boolean) => void;
  onDeepSearchChange: (active: boolean) => void;
  onImageModeChange?: (active: boolean) => void;
  onFilesChange: (files: ComposerFile[]) => void;
  onHistoryOpenChange: (open: boolean) => void;
  onMessageChange: (message: string) => void;
  onModelChange: (model: DashboardComposerModel) => void;
  onModelOpenChange: (open: boolean) => void;
  onOpenAgentBuilder: () => void;
  onResponseModeChange: (mode: ResponseMode) => void;
  onSettingsChange: (settings: ComposerSettings) => void;
  onSettingsOpenChange: (open: boolean) => void;
  onSubmit: () => void;
  onSuggestionsOpenChange: (open: boolean) => void;
  onVoiceModeChange: (active: boolean) => void;
};

export const defaultComposerSettings: ComposerSettings = {
  voiceLang: "en-US",
  tone: "balanced",
};
