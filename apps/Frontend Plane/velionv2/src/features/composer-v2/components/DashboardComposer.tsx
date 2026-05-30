"use client";

import {
  ArrowUp,
  AudioWaveform,
  CirclePlus,
  Globe2,
  Lightbulb,
  Mic,
  Telescope,
} from "lucide-react";
import { type DashboardComposerProps } from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { DashboardComposerControls } from "@/features/composer-v2/components/DashboardComposerControls";
import { AutocompleteDropdown, AttachmentPreview, EntityOverlay, SplitText, ToolbarIcon, TurnReceipt } from "@/features/composer-v2/components/DashboardComposerInputParts";
import { HistoryPanel, SettingsPanel } from "@/features/composer-v2/components/DashboardComposerPanels";
import { DashboardComposerSuggestions } from "@/features/composer-v2/components/DashboardComposerSuggestions";
import { RealtimeVoiceModal } from "@/features/composer-v2/components/RealtimeVoiceModal";
import { useDashboardComposerController } from "@/features/composer-v2/hooks/use-dashboard-composer-controller";
import { responseModes } from "@/features/composer-v2/lib/dashboard-composer-options";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import { cn } from "@/lib/utils";

export function DashboardComposer({
  browseWeb,
  deepSearch,
  files,
  historyOpen,
  message,
  modelOpen,
  responseMode,
  selectedModel,
  settings,
  settingsOpen,
  suggestionsOpen,
  turns,
  voiceMode,
  onBrowseWebChange,
  onDeepSearchChange,
  onFilesChange,
  onHistoryOpenChange,
  onMessageChange,
  onModelChange,
  onModelOpenChange,
  onOpenAgentBuilder,
  onResponseModeChange,
  onSettingsChange,
  onSettingsOpenChange,
  onSubmit,
  onSuggestionsOpenChange,
  onVoiceModeChange,
}: DashboardComposerProps) {
  const {
    fileInputRef,
    textareaRef,
    composerRootRef,
    historyTriggerRef,
    settingsTriggerRef,
    historyPanelPosition,
    settingsPanelPosition,
    autocomplete,
    autocompleteIndex,
    entities,
    modeAnnouncement,
    isRecording,
    hasContent,
    setAutocomplete,
    setAutocompleteIndex,
    addFiles,
    removeFile,
    updateSettings,
    openHistoryPanel,
    openSettingsPanel,
    handleMessageChange,
    handleComposerKeyDown,
    handleResponseMode,
    enhanceAttachments,
    toggleRecording,
    submitComposer,
    addScreenshotFile,
    applyAutocompleteSelection,
  } = useDashboardComposerController({
    files,
    historyOpen,
    message,
    modelOpen,
    responseMode,
    settings,
    settingsOpen,
    suggestionsOpen,
    onFilesChange,
    onHistoryOpenChange,
    onMessageChange,
    onModelOpenChange,
    onResponseModeChange,
    onSettingsChange,
    onSettingsOpenChange,
    onSubmit,
    onSuggestionsOpenChange,
  });

  return (
    <div ref={composerRootRef} className="relative w-full">
      <DashboardComposerControls
        historyOpen={historyOpen}
        historyTriggerRef={historyTriggerRef}
        modelOpen={modelOpen}
        selectedModel={selectedModel}
        settingsOpen={settingsOpen}
        settingsTriggerRef={settingsTriggerRef}
        onHistoryOpenChange={onHistoryOpenChange}
        onModelChange={onModelChange}
        onModelOpenChange={onModelOpenChange}
        onOpenAgentBuilder={onOpenAgentBuilder}
        onSettingsOpenChange={onSettingsOpenChange}
        onSuggestionsOpenChange={onSuggestionsOpenChange}
        openHistoryPanel={openHistoryPanel}
        openSettingsPanel={openSettingsPanel}
      />

      <div className="relative">
        <input ref={fileInputRef} type="file" accept="*/*" multiple className="sr-only" aria-label="Add files" onChange={addFiles} />

        {suggestionsOpen ? (
          <DashboardComposerSuggestions
            onMessageChange={onMessageChange}
            onSuggestionsOpenChange={onSuggestionsOpenChange}
          />
        ) : null}

        {autocomplete ? (
          <div className="absolute bottom-full left-4 z-[95] mb-2">
            <AutocompleteDropdown
              category={autocomplete.category}
              items={autocomplete.items}
              selectedIndex={autocompleteIndex}
              onHover={setAutocompleteIndex}
              onSelect={applyAutocompleteSelection}
            />
          </div>
        ) : null}

        <form
          className="velion-dashboard-composer-card velion-composer-shell ring-1 ring-black/[0.03] dark:ring-white/[0.06]"
          action={() => {
            void submitComposer();
          }}
        >
          <AttachmentPreview attachments={files} onEnhance={enhanceAttachments} onRemove={removeFile} />

          <div className="relative px-4">
            {entities.length > 0 ? (
              <div
                aria-hidden="true"
                className="pointer-events-none absolute inset-0 overflow-hidden px-5 text-[15px] leading-relaxed text-[#1a1a1a] dark:text-[#F7F8F8]"
              >
                <EntityOverlay entities={entities} message={message} />
              </div>
            ) : null}

            {modeAnnouncement && !message ? (
              <div
                aria-hidden="true"
                data-mode-announcement={modeAnnouncement}
                className="pointer-events-none absolute inset-0 flex items-start px-5 text-[15px] font-normal leading-relaxed text-[#c0bab5] dark:text-[#62666D]"
              >
                <SplitText text={modeAnnouncement} />
              </div>
            ) : null}

            <label className="sr-only" htmlFor="velion-dashboard-input">
              Message Velion
            </label>
            <textarea
              ref={textareaRef}
              id="velion-dashboard-input"
              aria-label="Message Velion"
              aria-busy={isRecording}
              value={message}
              onChange={handleMessageChange}
              onKeyDown={handleComposerKeyDown}
              placeholder={modeAnnouncement ? "" : "How can I help you today?"}
              rows={1}
              className="velion-dashboard-textarea w-full resize-none bg-transparent px-1 text-[#1a1a1a] placeholder:text-[#B8B4AF] focus:outline-none disabled:opacity-50 dark:text-[#F7F8F8] dark:placeholder:text-[#62666D]"
              style={{
                color: entities.length > 0 ? "transparent" : undefined,
                caretColor: "currentColor",
              }}
              disabled={isRecording}
            />
          </div>

          {isRecording ? (
            <div className="mx-4 mb-2 h-px overflow-hidden rounded-full bg-black/6 dark:bg-white/10">
              <div className="velion-loading-bar h-full w-1/2 rounded-full bg-linear-to-r from-orange-300 via-blue-400 to-orange-300" />
            </div>
          ) : null}

          <div className="flex flex-col gap-3 px-3 lg:flex-row lg:items-center lg:justify-between">
            <div className="flex min-w-0 flex-wrap items-center gap-1.5">
              <TopLayerTooltip label="Add files" placement="top">
                <button
                  type="button"
                  onClick={() => fileInputRef.current?.click()}
                  className="group flex items-center gap-1.5 text-[#888] transition-colors hover:text-[#333] disabled:opacity-40 dark:hover:text-white"
                  aria-label="Add files"
                  disabled={isRecording}
                >
                  <span className="flex size-8 items-center justify-center rounded-full bg-black/6 transition-colors group-hover:bg-black/10 dark:bg-white/10">
                    <CirclePlus className="size-4" />
                  </span>
                  <span className="text-[13px]">add files</span>
                </button>
              </TopLayerTooltip>
              <div className="mx-1 hidden h-4 w-px bg-black/10 dark:bg-white/10 sm:block" />
              <ToolbarIcon
                active={suggestionsOpen}
                label="Suggestions"
                onClick={() => {
                  onSuggestionsOpenChange(!suggestionsOpen);
                  onModelOpenChange(false);
                  onHistoryOpenChange(false);
                  onSettingsOpenChange(false);
                  setAutocomplete(null);
                }}
              >
                <Lightbulb className="size-4" />
              </ToolbarIcon>
              <ToolbarIcon active={deepSearch} label="Deep search" onClick={() => onDeepSearchChange(!deepSearch)}>
                <Telescope className="size-4" />
              </ToolbarIcon>
              <TopLayerTooltip label="Browse web" placement="top">
                <button
                  type="button"
                  aria-pressed={browseWeb}
                  onClick={() => onBrowseWebChange(!browseWeb)}
                  className={cn(
                    "flex items-center gap-1.5 rounded-lg px-3 py-[7px] text-[13px] font-semibold transition-colors",
                    browseWeb ? "bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-300" : "text-[#777] hover:bg-blue-50/60 hover:text-blue-500 dark:text-[#AEB4C0]",
                  )}
                  aria-label="Browse web"
                >
                  <Globe2 className="size-4" />
                  Search
                </button>
              </TopLayerTooltip>
            </div>

            <div className="flex items-center justify-end gap-1.5">
              <div className="flex items-center gap-0.5 rounded-xl bg-black/6 p-[3px] dark:bg-white/10">
                {responseModes.map((mode) => (
                  <ToolbarIcon
                    key={mode.id}
                    active={responseMode === mode.id}
                    label={mode.label}
                    onClick={() => handleResponseMode(mode.id)}
                  >
                    {mode.icon}
                  </ToolbarIcon>
                ))}
              </div>
              <ToolbarIcon active={voiceMode} label="Voice mode" onClick={() => onVoiceModeChange(true)}>
                <AudioWaveform className="size-4" />
              </ToolbarIcon>
              <ToolbarIcon active={isRecording} label="Voice input" onClick={() => void toggleRecording()}>
                <Mic className="size-4" />
              </ToolbarIcon>
              <button
                type="submit"
                disabled={!hasContent || isRecording}
                aria-label="Send message"
                title="Send message"
                className={cn(
                  "grid size-11 place-items-center rounded-[14px] transition-colors",
                  hasContent && !isRecording ? "bg-[#111111] text-white hover:bg-[#2A2A2A]" : "bg-[#F0F0F1] text-[#B0B3BC]",
                )}
              >
                <ArrowUp className="size-4" />
              </button>
            </div>
          </div>
        </form>
      </div>

      {turns.length > 0 ? <TurnReceipt turn={turns[0]} /> : null}
      {historyOpen ? <HistoryPanel position={historyPanelPosition} turns={turns} onClose={() => onHistoryOpenChange(false)} /> : null}
      {settingsOpen ? (
        <SettingsPanel
          position={settingsPanelPosition}
          settings={settings}
          onAddFiles={() => {
            onSettingsOpenChange(false);
            fileInputRef.current?.click();
          }}
          onScreenshot={() => {
            onSettingsOpenChange(false);
            void addScreenshotFile();
          }}
          onSettingsChange={updateSettings}
        />
      ) : null}
      <RealtimeVoiceModal
        language={settings.voiceLang}
        model={selectedModel}
        open={voiceMode}
        onClose={() => onVoiceModeChange(false)}
      />
    </div>
  );
}
