import Link from "next/link";
import type { Route } from "next";
import type { RefObject } from "react";
import { Check, ChevronDown, Clock3, SlidersHorizontal, Sparkles, Zap } from "lucide-react";
import type { DashboardComposerProps } from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { IconChip } from "@/features/composer-v2/components/DashboardComposerInputParts";
import { modelOptions } from "@/features/composer-v2/lib/dashboard-composer-options";
import { cn } from "@/lib/utils";

type DashboardComposerControlsProps = Pick<
  DashboardComposerProps,
  | "historyOpen"
  | "modelOpen"
  | "selectedModel"
  | "settingsOpen"
  | "onHistoryOpenChange"
  | "onModelChange"
  | "onModelOpenChange"
  | "onOpenAgentBuilder"
  | "onSettingsOpenChange"
  | "onSuggestionsOpenChange"
> & {
  historyTriggerRef: RefObject<HTMLSpanElement | null>;
  settingsTriggerRef: RefObject<HTMLSpanElement | null>;
  openHistoryPanel: () => void;
  openSettingsPanel: () => void;
};

export function DashboardComposerControls({
  historyOpen,
  historyTriggerRef,
  modelOpen,
  selectedModel,
  settingsOpen,
  settingsTriggerRef,
  onHistoryOpenChange,
  onModelChange,
  onModelOpenChange,
  onOpenAgentBuilder,
  onSettingsOpenChange,
  onSuggestionsOpenChange,
  openHistoryPanel,
  openSettingsPanel,
}: DashboardComposerControlsProps) {
  return (
    <div className="mb-3 flex items-center justify-between px-1">
      <div className="flex items-center gap-2">
        <div className="relative">
          <button
            type="button"
            aria-expanded={modelOpen}
            onClick={() => {
              onModelOpenChange(!modelOpen);
              onHistoryOpenChange(false);
              onSettingsOpenChange(false);
              onSuggestionsOpenChange(false);
            }}
            title="Velg AI-modell"
            className="velion-composer-control flex h-10 max-w-[150px] items-center gap-2 rounded-[12px] border border-black/[0.07] bg-white/90 px-3 text-[13px] font-medium text-[#333] shadow-[0_8px_20px_rgba(0,0,0,0.05)] backdrop-blur-sm transition-all hover:bg-white active:scale-[0.98] dark:border-[#2A2C31] dark:bg-[#17181C]/90 dark:text-[#F7F8F8] dark:hover:bg-[#23252A] sm:max-w-none sm:px-4"
          >
            <Zap className="size-4 text-[#12B76A]" />
            <span className="min-w-0 truncate whitespace-nowrap">{selectedModel}</span>
            <ChevronDown className={cn("size-4 text-[#777] transition-transform", modelOpen ? "rotate-180" : "")} />
          </button>

          {modelOpen ? (
            <div className="velion-popover absolute bottom-full left-0 z-[90] mb-2 w-64 rounded-2xl border border-black/[0.06] bg-white/95 p-1 text-sm shadow-[0_12px_40px_rgba(0,0,0,0.12)] backdrop-blur-xl dark:border-[#2A2C31] dark:bg-[#141516]/95">
              {modelOptions.map((model) => (
                <button
                  key={model}
                  type="button"
                  title={`Bruk ${model}`}
                  onClick={() => {
                    onModelChange(model);
                    onModelOpenChange(false);
                  }}
                  className={cn(
                    "flex w-full items-center justify-between rounded-xl px-3 py-2.5 text-left text-[13px] transition-colors hover:bg-gray-50 dark:hover:bg-white/10",
                    selectedModel === model ? "bg-blue-50/60 font-medium text-blue-700 dark:bg-white/10 dark:text-white" : "text-gray-800 dark:text-[#F7F8F8]",
                  )}
                >
                  <span className="flex min-w-0 items-center gap-3">
                    <span className="grid size-6 shrink-0 place-items-center rounded-full bg-gray-50 dark:bg-white/10">
                      <Zap className="size-3 text-gray-600 dark:text-[#D3D7DE]" />
                    </span>
                    <span className="truncate">{model}</span>
                  </span>
                  {selectedModel === model ? <Check className="size-4 text-[#12B76A]" /> : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>

        <Link
          href={"/agents" as Route}
          onClick={onOpenAgentBuilder}
          title="Opprett agent"
          className="velion-composer-control flex h-10 items-center gap-2 rounded-[12px] bg-[#2a2a2a] px-3 text-[13px] font-medium text-white shadow-md transition-all hover:bg-[#1a1a1a] active:scale-[0.97] sm:px-4"
        >
          <Sparkles className="size-[13px] shrink-0" />
          <span className="hidden whitespace-nowrap sm:inline">Create agent</span>
        </Link>
      </div>

      <div className="relative flex items-center gap-1.5">
        <span ref={historyTriggerRef}>
          <IconChip active={historyOpen} label="Historikk" onClick={openHistoryPanel}>
            <Clock3 className="size-3.5" />
          </IconChip>
        </span>
        <span ref={settingsTriggerRef}>
          <IconChip active={settingsOpen} label="Innstillinger" onClick={openSettingsPanel}>
            <SlidersHorizontal className="size-3.5" />
          </IconChip>
        </span>
      </div>
    </div>
  );
}
