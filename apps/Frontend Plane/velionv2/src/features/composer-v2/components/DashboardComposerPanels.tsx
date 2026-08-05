"use client";

import Link from "next/link";
import type { Route } from "next";
import { useState, useSyncExternalStore } from "react";
import type React from "react";
import { createPortal } from "react-dom";
import {
  ArrowLeft,
  Blocks,
  Briefcase,
  Camera,
  Check,
  ChevronRight,
  FolderPlus,
  LayoutGrid,
  Loader2,
  MessageSquare,
  Mic,
  Paperclip,
} from "lucide-react";
import type { ComposerSettings, ComposerTurn } from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { toneOptions, voiceLanguages } from "@/features/composer-v2/lib/dashboard-composer-options";
import type { HistoryPanelPosition, SettingsPanelPosition } from "@/features/composer-v2/lib/dashboard-composer-types";

export function HistoryPanel({
  onClose,
  position,
  turns,
}: {
  onClose: () => void;
  position: HistoryPanelPosition;
  turns: ComposerTurn[];
}) {
  if (typeof document === "undefined") {
    return null;
  }

  const groupedTurns = groupTurnsByDate(turns);
  const groups = [
    { label: "Today", items: groupedTurns.today, isToday: true },
    { label: "Yesterday", items: groupedTurns.yesterday, isToday: false },
    { label: "Earlier", items: groupedTurns.earlier, isToday: false },
  ] as const;

  return createPortal(
    <div
      data-composer-floating-panel="true"
      className="verevon-popover verevon-popover-up verevon-floating-panel verevon-floating-panel-sm verevon-floating-panel-compact"
      style={{
        position: "fixed",
        bottom: position.bottom,
        right: position.right,
        zIndex: "var(--verevon-z-popover)",
        maxHeight: position.maxHeight,
        transformOrigin: "bottom right",
      }}
    >
      <div className="overflow-y-auto p-2" style={{ maxHeight: position.maxHeight }}>
        {turns.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 py-6">
            <MessageSquare className="size-5 text-[#ccc]" strokeWidth={1.5} />
            <p className="text-[12px] text-[#bbb]">No conversations yet</p>
          </div>
        ) : (
          groups.map((group) =>
            group.items.length === 0 ? null : (
              <div key={group.label}>
                <p className="px-3 pb-0.5 pt-1.5 text-[10px] font-semibold uppercase tracking-wider text-[#bbb]">
                  {group.label}
                </p>
                {group.items.map((turn) => (
                  <button
                    key={turn.id}
                    type="button"
                    onClick={onClose}
                    className="verevon-menu-row text-left"
                  >
                    <MessageSquare className="size-4 shrink-0 text-[#333] dark:text-[#F7F8F8]" strokeWidth={1.7} />
                    <span className="min-w-0 flex-1">
                      <span className="verevon-menu-label block truncate">{turn.body || "Untitled"}</span>
                      <span className="verevon-menu-meta block truncate">{turn.model}</span>
                    </span>
                    <span className="verevon-menu-meta shrink-0 text-[#bbb]">{formatTurnTime(turn, group.isToday)}</span>
                  </button>
                ))}
              </div>
            ),
          )
        )}

        <div className="mx-1 my-1 h-px bg-black/8 dark:bg-white/10" />
        <Link
          href={"/chat" as Route}
          onClick={onClose}
          className="verevon-menu-row text-left"
        >
          <LayoutGrid className="size-4 shrink-0 text-[#333] dark:text-[#F7F8F8]" strokeWidth={1.7} />
          <span className="verevon-menu-label">View all conversations</span>
        </Link>
      </div>
    </div>,
    document.body,
  );
}

function groupTurnsByDate(turns: ComposerTurn[]) {
  const now = new Date();
  const today = now.toDateString();
  const yesterday = new Date(now.getTime() - 86_400_000).toDateString();

  return turns.reduce<{
    today: ComposerTurn[];
    yesterday: ComposerTurn[];
    earlier: ComposerTurn[];
  }>(
    (groups, turn) => {
      const date = new Date(turn.createdAtIso).toDateString();

      if (date === today) {
        return { ...groups, today: [...groups.today, turn] };
      }

      if (date === yesterday) {
        return { ...groups, yesterday: [...groups.yesterday, turn] };
      }

      return { ...groups, earlier: [...groups.earlier, turn] };
    },
    { today: [], yesterday: [], earlier: [] },
  );
}

function formatTurnTime(turn: ComposerTurn, isToday: boolean) {
  const date = new Date(turn.createdAtIso);

  if (Number.isNaN(date.getTime())) {
    return turn.createdAt;
  }

  return isToday
    ? date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function SettingsPanel({
  onAddFiles,
  onScreenshot,
  onSettingsChange,
  position,
  settings,
}: {
  onAddFiles: () => void;
  onScreenshot: () => void;
  onSettingsChange: (settings: ComposerSettings) => void;
  position: SettingsPanelPosition;
  settings: ComposerSettings;
}) {
  const [view, setView] = useState<"main" | "skills" | "projects" | "connectors">("main");
  const currentLangLabel = voiceLanguages.find((language) => language.value === settings.voiceLang)?.label ?? settings.voiceLang;

  if (typeof document === "undefined") {
    return null;
  }

  const updateSetting = <K extends keyof ComposerSettings>(key: K, value: ComposerSettings[K]) => {
    onSettingsChange({ ...settings, [key]: value });
  };

  return createPortal(
    <div
      data-composer-floating-panel="true"
      className="verevon-popover verevon-popover-side verevon-floating-panel verevon-floating-panel-xs verevon-floating-panel-compact"
      style={{
        position: "fixed",
        top: position.top,
        left: position.left,
        zIndex: "var(--verevon-z-popover)",
        transformOrigin: "top left",
      }}
    >
      <div className="overflow-y-auto p-2" style={{ maxHeight: position.maxHeight }}>
        {view === "main" ? (
          <div className="verevon-settings-view">
            <div className="verevon-menu-row">
              <Mic className="size-[17px] shrink-0 text-[#333] dark:text-[#F7F8F8]" strokeWidth={1.7} />
              <span className="verevon-menu-label min-w-0 flex-1 truncate">
                Voice language
              </span>
              <select
                value={settings.voiceLang}
                onChange={(event) => updateSetting("voiceLang", event.target.value)}
                title={currentLangLabel}
                className="max-w-24 cursor-pointer truncate border-none bg-transparent text-right text-[12px] text-[#888] outline-none dark:text-[#AEB4C0]"
              >
                {voiceLanguages.map((language) => (
                  <option key={language.value} value={language.value}>
                    {language.label}
                  </option>
                ))}
              </select>
            </div>

            {toneOptions.map(({ value, label, Icon }) => (
              <ComposerMenuRow
                key={value}
                icon={<Icon className="size-[17px]" strokeWidth={1.7} />}
                label={label}
                onClick={() => updateSetting("tone", value)}
                right={settings.tone === value ? <Check className="size-3.5 shrink-0 text-[#1a1a1a] dark:text-white" strokeWidth={2.5} /> : <span className="w-3.5" />}
              />
            ))}

            <div className="mx-2 my-1 h-px bg-black/8 dark:bg-white/10" />

            <ComposerMenuRow icon={<Paperclip className="size-[17px]" strokeWidth={1.7} />} label="Add files or photos" onClick={onAddFiles} />
            <ComposerMenuRow icon={<Camera className="size-[17px]" strokeWidth={1.7} />} label="Take a screenshot" onClick={onScreenshot} />
            <ComposerMenuRow
              icon={<FolderPlus className="size-[17px]" strokeWidth={1.7} />}
              label="Add to project"
              onClick={() => setView("projects")}
              right={<ChevronRight className="size-3.5 shrink-0 text-[#bbb]" />}
            />

            <div className="mx-2 my-1 h-px bg-black/8 dark:bg-white/10" />

            <ComposerMenuRow
              icon={<Blocks className="size-[17px]" strokeWidth={1.7} />}
              label="Skills"
              onClick={() => setView("skills")}
              right={<ChevronRight className="size-3.5 shrink-0 text-[#bbb]" />}
            />
            <ComposerMenuRow
              icon={<LayoutGrid className="size-[17px]" strokeWidth={1.7} />}
              label="Connectors"
              onClick={() => setView("connectors")}
              right={<ChevronRight className="size-3.5 shrink-0 text-[#bbb]" />}
            />
          </div>
        ) : null}

        {view === "skills" ? (
          <SettingsSubView title="Skills" onBack={() => setView("main")}>
            <RemoteSettingsList
              emptyLabel="No skills are available yet."
              endpoint="/api/skills"
              itemKey="skills"
              manageHref={"/skills" as Route}
              manageLabel="Manage skills"
            />
          </SettingsSubView>
        ) : null}

        {view === "projects" ? (
          <SettingsSubView title="Add to project" onBack={() => setView("main")}>
            <RemoteSettingsList
              emptyLabel="No projects are available yet."
              endpoint="/api/projects"
              itemKey="projects"
              manageHref={"/projects" as Route}
              manageLabel="Start a new project"
            />
          </SettingsSubView>
        ) : null}

        {view === "connectors" ? (
          <SettingsSubView title="Connectors" onBack={() => setView("main")}>
            <RemoteSettingsList
              emptyLabel="No connectors are connected yet."
              endpoint="/api/knowledge/integrations"
              itemKey="providers"
              manageHref={"/settings/integrations" as Route}
              manageLabel="Connect more"
            />
          </SettingsSubView>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}

function SettingsSubView({
  children,
  onBack,
  title,
}: {
  children: React.ReactNode;
  onBack: () => void;
  title: string;
}) {
  return (
    <div className="verevon-settings-view">
      <button
        type="button"
        onClick={onBack}
        className="verevon-menu-row mb-0.5 gap-2 text-[#888]"
      >
        <ArrowLeft className="size-3.5" strokeWidth={1.8} />
        <span className="text-[12px] font-medium">{title}</span>
      </button>
      <div className="mx-2 mb-1 h-px bg-black/8 dark:bg-white/10" />
      {children}
    </div>
  );
}

type RemoteSettingsItem = {
  id: string;
  name: string;
  description?: string;
  label?: string;
  connected?: boolean;
};

type RemoteSettingsState = {
  items: RemoteSettingsItem[];
  loading: boolean;
  error: string | null;
};

type RemoteSettingsRecord = {
  listeners: Set<() => void>;
  request: Promise<void> | null;
  state: RemoteSettingsState;
};

const initialRemoteSettingsState: RemoteSettingsState = {
  items: [],
  loading: true,
  error: null,
};

const remoteSettingsRecords = new Map<string, RemoteSettingsRecord>();

function getRemoteSettingsKey(endpoint: string, itemKey: string) {
  return `${endpoint}::${itemKey}`;
}

function getRemoteSettingsRecord(endpoint: string, itemKey: string) {
  const key = getRemoteSettingsKey(endpoint, itemKey);
  let record = remoteSettingsRecords.get(key);

  if (!record) {
    record = {
      listeners: new Set<() => void>(),
      request: null,
      state: initialRemoteSettingsState,
    };
    remoteSettingsRecords.set(key, record);
  }

  return record;
}

function notifyRemoteSettings(record: RemoteSettingsRecord) {
  for (const listener of record.listeners) {
    listener();
  }
}

function extractRemoteSettingsItems(payload: Record<string, unknown>, itemKey: string): RemoteSettingsItem[] {
  const source: unknown[] =
    Array.isArray(payload[itemKey])
      ? (payload[itemKey] as unknown[])
      : payload.data && typeof payload.data === "object" && Array.isArray((payload.data as Record<string, unknown>)[itemKey])
        ? ((payload.data as Record<string, unknown>)[itemKey] as unknown[])
        : [];

  return source
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === "object")
    .map((item, index) => ({
      id: typeof item.id === "string" ? item.id : typeof item.key === "string" ? item.key : `${itemKey}-${index}`,
      name:
        typeof item.name === "string"
          ? item.name
          : typeof item.title === "string"
            ? item.title
            : typeof item.label === "string"
              ? item.label
              : "Untitled",
      description:
        typeof item.description === "string"
          ? item.description
          : typeof item.sub === "string"
            ? item.sub
            : undefined,
      connected: typeof item.connected === "boolean" ? item.connected : undefined,
    }));
}

async function loadRemoteSettings(record: RemoteSettingsRecord, endpoint: string, itemKey: string) {
  try {
    const response = await fetch(endpoint, { cache: "no-store", credentials: "include" });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    const payload = await response.json() as Record<string, unknown>;
    record.state = { items: extractRemoteSettingsItems(payload, itemKey), loading: false, error: null };
  } catch (error) {
    record.state = {
      items: [],
      loading: false,
      error: error instanceof Error ? error.message : "Service unavailable",
    };
  }

  notifyRemoteSettings(record);
}

function ensureRemoteSettingsRequest(endpoint: string, itemKey: string) {
  const record = getRemoteSettingsRecord(endpoint, itemKey);
  record.request ??= loadRemoteSettings(record, endpoint, itemKey).finally(() => {
    record.request = null;
  });
}

function subscribeRemoteSettings(endpoint: string, itemKey: string, listener: () => void) {
  const record = getRemoteSettingsRecord(endpoint, itemKey);
  record.listeners.add(listener);
  ensureRemoteSettingsRequest(endpoint, itemKey);

  return () => {
    record.listeners.delete(listener);
  };
}

function getRemoteSettingsSnapshot(endpoint: string, itemKey: string) {
  return getRemoteSettingsRecord(endpoint, itemKey).state;
}

function getInitialRemoteSettingsSnapshot() {
  return initialRemoteSettingsState;
}

function useRemoteSettingsList(endpoint: string, itemKey: string) {
  return useSyncExternalStore(
    (listener) => subscribeRemoteSettings(endpoint, itemKey, listener),
    () => getRemoteSettingsSnapshot(endpoint, itemKey),
    getInitialRemoteSettingsSnapshot,
  );
}

function RemoteSettingsList({
  emptyLabel,
  endpoint,
  itemKey,
  manageHref,
  manageLabel,
}: {
  emptyLabel: string;
  endpoint: string;
  itemKey: string;
  manageHref: Route;
  manageLabel: string;
}) {
  const state = useRemoteSettingsList(endpoint, itemKey);

  if (state.loading) {
    return (
      <div className="flex items-center gap-2 p-3 text-[12px] text-[#888]">
        <Loader2 className="size-3.5 animate-spin" />
        Loading…
      </div>
    );
  }

  return (
    <>
      {state.items.length === 0 ? (
        <div className="px-3 py-2.5">
          <p className="text-[12px] font-medium text-[#999] dark:text-[#AEB4C0]">{emptyLabel}</p>
          {state.error ? <p className="mt-0.5 break-all text-[10px] text-[#bbb]">{state.error}</p> : null}
        </div>
      ) : (
        state.items.map((item) => (
          <button
            key={item.id}
            type="button"
            className="verevon-menu-row text-left"
          >
            <span className="grid size-7 shrink-0 place-items-center rounded-xl bg-black/[0.04] dark:bg-white/10">
              <Briefcase className="size-3.5 text-[#555] dark:text-[#D3D7DE]" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="verevon-menu-label block truncate">{item.name}</span>
              {item.description ? <span className="verevon-menu-meta mt-0.5 block line-clamp-2 text-[#888]">{item.description}</span> : null}
            </span>
            {typeof item.connected === "boolean" ? (
              <span className="verevon-menu-meta shrink-0 font-medium text-[#888]">{item.connected ? "Manage" : "Open"}</span>
            ) : null}
          </button>
        ))
      )}
      <div className="mx-2 my-1 h-px bg-black/8 dark:bg-white/10" />
      <Link
        href={manageHref}
        className="verevon-menu-row text-left"
      >
        <span className="grid size-7 shrink-0 place-items-center rounded-xl bg-black/[0.04] dark:bg-white/10">
          <Briefcase className="size-3.5 text-[#555] dark:text-[#D3D7DE]" />
        </span>
        <span className="verevon-menu-label">{manageLabel}</span>
      </Link>
    </>
  );
}

function ComposerMenuRow({
  icon,
  label,
  onClick,
  right,
}: {
  icon?: React.ReactNode;
  label: string;
  onClick: () => void;
  right?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      className="verevon-menu-row text-left"
    >
      {icon ? <span className="shrink-0 text-[#333] dark:text-[#F7F8F8]">{icon}</span> : null}
      <span className="verevon-menu-label min-w-0 flex-1 truncate">{label}</span>
      {right}
    </button>
  );
}
