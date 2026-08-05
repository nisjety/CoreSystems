import type { ReactNode } from "react";
import {
  AlignCenter,
  AlignJustify,
  AlignLeft,
  Image as ImageIcon,
  Lightbulb,
  Upload,
  WandSparkles,
  Zap,
} from "lucide-react";
import type {
  ComposerTone,
  DashboardComposerModel,
  ResponseMode,
} from "@/features/dashboard-v2/lib/dashboard-composer-model";
import type { AutocompleteItem, HistoryPanelPosition, SettingsPanelPosition } from "@/features/composer-v2/lib/dashboard-composer-types";

export const modelOptions = ["GPT-4o Mini", "GPT-4.1", "Claude Sonnet", "Verevon Reasoner"] as const satisfies readonly DashboardComposerModel[];
export const responseModes: Array<{ id: ResponseMode; label: string; announcement: string; icon: ReactNode }> = [
  { id: "auto", label: "Auto", announcement: "Auto mode", icon: <WandSparkles className="size-4" /> },
  { id: "quick", label: "Quick response", announcement: "Quick response activated", icon: <Zap className="size-4" /> },
  { id: "deep", label: "Deep research", announcement: "Deep research mode", icon: <Lightbulb className="size-4" /> },
];

export const promptSuggestions = [
  "Finn de viktigste kundesakene fra siste uke.",
  "Lag et kort svarutkast med kildehenvisninger.",
  "Oppsummer kunnskapsbasen og pek på mangler.",
] as const;

export const dayEntries = [
  { name: "Monday", dayIndex: 1 },
  { name: "Tuesday", dayIndex: 2 },
  { name: "Wednesday", dayIndex: 3 },
  { name: "Thursday", dayIndex: 4 },
  { name: "Friday", dayIndex: 5 },
  { name: "Saturday", dayIndex: 6 },
  { name: "Sunday", dayIndex: 0 },
] as const;

export const slashCommands: AutocompleteItem[] = [
  {
    id: "cmd-file",
    icon: <Upload className="size-4" />,
    label: "File upload",
    meta: "slash",
    action: "file",
  },
  {
    id: "cmd-image",
    icon: <ImageIcon className="size-4" />,
    label: "Generate image",
    meta: "slash",
    action: "image",
  },
] as const;

export const voiceLanguages = [
  { value: "en-US", label: "English (US)" },
  { value: "en-GB", label: "English (UK)" },
  { value: "nb-NO", label: "Norwegian (Bokmål)" },
  { value: "nn-NO", label: "Norwegian (Nynorsk)" },
  { value: "sv-SE", label: "Swedish" },
  { value: "da-DK", label: "Danish" },
  { value: "de-DE", label: "German" },
  { value: "fr-FR", label: "French" },
  { value: "es-ES", label: "Spanish" },
  { value: "pt-BR", label: "Portuguese (BR)" },
] as const;

export const toneOptions: Array<{ value: ComposerTone; label: string; Icon: typeof AlignLeft }> = [
  { value: "concise", label: "Concise", Icon: AlignLeft },
  { value: "balanced", label: "Balanced", Icon: AlignCenter },
  { value: "detailed", label: "Detailed", Icon: AlignJustify },
];

export const defaultHistoryPanelPosition: HistoryPanelPosition = {
  bottom: 0,
  right: 0,
  maxHeight: 400,
};

export const defaultSettingsPanelPosition: SettingsPanelPosition = {
  top: 0,
  left: 0,
  maxHeight: 460,
};
