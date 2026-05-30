import type { ReactNode } from "react";

export type EntityKind = "date" | "file" | "person";

export type EntityToken = {
  start: number;
  text: string;
  kind: EntityKind;
};

export type AutocompleteItem = {
  id: string;
  icon: ReactNode;
  label: string;
  meta?: EntityKind | "slash";
  action?: "file" | "image";
};

export type AutocompleteState = {
  items: AutocompleteItem[];
  category: string;
  triggerStart: number;
  triggerLen: number;
};

export type TriggerContext =
  | { type: "date"; dayIndex: number; start: number; rawLen: number }
  | { type: "slash"; query: string; start: number; rawLen: number }
  | { type: "person"; query: string; start: number; rawLen: number };

export type HistoryPanelPosition = {
  bottom: number;
  right: number;
  maxHeight: number;
};

export type SettingsPanelPosition = {
  top: number;
  left: number;
  maxHeight: number;
};
