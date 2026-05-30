import { useEffect, useRef, useState } from "react";
import type React from "react";
import { FileText, User } from "lucide-react";
import {
  type ComposerSettings,
  type DashboardComposerProps,
  type ResponseMode,
} from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { getUpcomingDates, detectTrigger } from "@/features/composer-v2/lib/dashboard-composer-autocomplete";
import {
  defaultHistoryPanelPosition,
  defaultSettingsPanelPosition,
  responseModes,
  slashCommands,
} from "@/features/composer-v2/lib/dashboard-composer-options";
import { saveComposerSettings } from "@/features/composer-v2/lib/dashboard-composer-storage";
import type {
  AutocompleteItem,
  AutocompleteState,
  EntityKind,
  EntityToken,
  HistoryPanelPosition,
  SettingsPanelPosition,
} from "@/features/composer-v2/lib/dashboard-composer-types";
import { searchNavbar } from "@/features/shell-v2/lib/navbar-data";

type DashboardComposerControllerProps = Pick<
  DashboardComposerProps,
  | "files"
  | "historyOpen"
  | "message"
  | "modelOpen"
  | "responseMode"
  | "settings"
  | "settingsOpen"
  | "suggestionsOpen"
  | "onFilesChange"
  | "onHistoryOpenChange"
  | "onMessageChange"
  | "onModelOpenChange"
  | "onResponseModeChange"
  | "onSettingsChange"
  | "onSettingsOpenChange"
  | "onSubmit"
  | "onSuggestionsOpenChange"
>;

export function useDashboardComposerController({
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
}: DashboardComposerControllerProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const composerRootRef = useRef<HTMLDivElement>(null);
  const historyTriggerRef = useRef<HTMLSpanElement>(null);
  const settingsTriggerRef = useRef<HTMLSpanElement>(null);
  const pendingFilePositionRef = useRef<number | null>(null);
  const autocompleteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const autocompleteAbortRef = useRef<AbortController | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const audioChunksRef = useRef<Blob[]>([]);
  const modeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [historyPanelPosition, setHistoryPanelPosition] = useState<HistoryPanelPosition>(defaultHistoryPanelPosition);
  const [settingsPanelPosition, setSettingsPanelPosition] = useState<SettingsPanelPosition>(defaultSettingsPanelPosition);
  const [autocomplete, setAutocomplete] = useState<AutocompleteState | null>(null);
  const [autocompleteIndex, setAutocompleteIndex] = useState(0);
  const [entities, setEntities] = useState<EntityToken[]>([]);
  const [modeAnnouncement, setModeAnnouncement] = useState<string | null>(null);
  const [isRecording, setIsRecording] = useState(false);
  const hasContent = message.trim().length > 0 || files.length > 0;

  useEffect(() => {
    const element = textareaRef.current;

    if (!element) {
      return;
    }

    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, 128)}px`;
  }, [message]);

  useEffect(
    () => () => {
      if (autocompleteTimerRef.current) {
        clearTimeout(autocompleteTimerRef.current);
      }
      if (modeTimerRef.current) {
        clearTimeout(modeTimerRef.current);
      }
      autocompleteAbortRef.current?.abort();
      mediaRecorderRef.current?.stream.getTracks().forEach((track) => track.stop());
    },
    [],
  );

  useEffect(() => {
    if (!modelOpen && !historyOpen && !settingsOpen && !suggestionsOpen) {
      return;
    }

    const closePanels = () => {
      onModelOpenChange(false);
      onHistoryOpenChange(false);
      onSettingsOpenChange(false);
      onSuggestionsOpenChange(false);
    };

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;

      if (!(target instanceof Node)) {
        return;
      }

      if (composerRootRef.current?.contains(target)) {
        return;
      }

      if (target instanceof Element && target.closest("[data-composer-floating-panel='true']")) {
        return;
      }

      closePanels();
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closePanels();
      }
    };

    document.addEventListener("pointerdown", handlePointerDown, true);
    document.addEventListener("keydown", handleKeyDown);

  
  return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [
    historyOpen,
    modelOpen,
    onHistoryOpenChange,
    onModelOpenChange,
    onSettingsOpenChange,
    onSuggestionsOpenChange,
    settingsOpen,
    suggestionsOpen,
  ]);

  const addFiles = (event: React.ChangeEvent<HTMLInputElement>) => {
    const selectedFiles = Array.from(event.currentTarget.files ?? []);

    if (selectedFiles.length === 0) {
      return;
    }

    const pendingPosition = pendingFilePositionRef.current;

    if (pendingPosition !== null) {
      const file = selectedFiles[0];
      const nextMessage = message.slice(0, pendingPosition) + file.name + message.slice(pendingPosition);
      pendingFilePositionRef.current = null;
      onMessageChange(nextMessage);
      setEntities((current) => [
        ...current,
        {
          start: pendingPosition,
          text: file.name,
          kind: "file",
        },
      ]);
      setAutocomplete(null);
      event.currentTarget.value = "";
      window.requestAnimationFrame(() => {
        const caret = pendingPosition + file.name.length;
        textareaRef.current?.focus();
        textareaRef.current?.setSelectionRange(caret, caret);
      });
      return;
    }

    const selectedAttachments = selectedFiles.map((file) => ({
      id: `${file.name}-${file.size}-${file.lastModified}`,
      name: file.name,
      size: file.size,
      type: file.type,
      url: URL.createObjectURL(file),
    }));

    onFilesChange([
      ...files,
      ...selectedAttachments.filter((file) => !files.some((current) => current.id === file.id)),
    ]);
    event.currentTarget.value = "";
  };

  const removeFile = (fileId: string) => {
    const file = files.find((current) => current.id === fileId);

    if (file?.url.startsWith("blob:")) {
      URL.revokeObjectURL(file.url);
    }

    onFilesChange(files.filter((current) => current.id !== fileId));
  };

  const updateSettings = (nextSettings: ComposerSettings) => {
    onSettingsChange(nextSettings);
    saveComposerSettings(nextSettings);
  };

  const openHistoryPanel = () => {
    if (!historyOpen) {
      const rect = historyTriggerRef.current?.getBoundingClientRect();

      if (rect) {
        setHistoryPanelPosition({
          bottom: window.innerHeight - rect.top + 8,
          right: window.innerWidth - rect.right,
          maxHeight: Math.max(180, Math.min(400, rect.top - 16)),
        });
      }
    }

    onHistoryOpenChange(!historyOpen);
    onSettingsOpenChange(false);
    onModelOpenChange(false);
    onSuggestionsOpenChange(false);
  };

  const openSettingsPanel = () => {
    if (!settingsOpen) {
      const rect = settingsTriggerRef.current?.getBoundingClientRect();

      if (rect) {
        const width = 256;
        const gap = 8;
        const left = Math.min(rect.right + gap, window.innerWidth - width - gap);

        setSettingsPanelPosition({
          top: rect.top,
          left: Math.max(gap, left),
          maxHeight: Math.max(220, Math.min(460, window.innerHeight - rect.top - 16)),
        });
      }
    }

    onSettingsOpenChange(!settingsOpen);
    onHistoryOpenChange(false);
    onModelOpenChange(false);
    onSuggestionsOpenChange(false);
  };

  const updateAutocomplete = (text: string, position: number) => {
    if (autocompleteTimerRef.current) {
      clearTimeout(autocompleteTimerRef.current);
      autocompleteTimerRef.current = null;
    }
    autocompleteAbortRef.current?.abort();
    autocompleteAbortRef.current = null;

    const trigger = detectTrigger(text, position);

    if (!trigger) {
      setAutocomplete(null);
      return;
    }

    if (trigger.type === "date") {
      const items = getUpcomingDates(trigger.dayIndex);
      setAutocomplete(items.length > 0 ? {
        items,
        category: "Schedule",
        triggerStart: trigger.start,
        triggerLen: trigger.rawLen,
      } : null);
      return;
    }

    if (trigger.type === "slash") {
      const commandMatches = slashCommands.filter(
        (command) =>
          command.action?.startsWith(trigger.query) ||
          command.label.toLowerCase().startsWith(trigger.query),
      );

      if (commandMatches.length > 0) {
        setAutocomplete({
          items: commandMatches,
          category: "Attachment",
          triggerStart: trigger.start,
          triggerLen: trigger.rawLen,
        });
        return;
      }
    }

    if (trigger.query.length === 0) {
      setAutocomplete(null);
      return;
    }

    autocompleteTimerRef.current = setTimeout(() => {
      const controller = new AbortController();
      autocompleteAbortRef.current = controller;
      searchNavbar(trigger.query, controller.signal)
        .then((payload) => {
          const results = payload.results;
          const items = results.slice(0, trigger.type === "person" ? 6 : 5).map((result) => ({
            id: result.id,
            icon: trigger.type === "person" ? <User className="size-4" /> : <FileText className="size-4" />,
            label: result.label,
            meta: trigger.type === "person" ? "person" as const : "file" as const,
          }));

          setAutocomplete(items.length > 0 ? {
            items,
            category: trigger.type === "person" ? "People" : "Documents",
            triggerStart: trigger.start,
            triggerLen: trigger.rawLen,
          } : null);
        })
        .catch(() => {
          if (!controller.signal.aborted) {
            setAutocomplete(null);
          }
        });
    }, 180);
  };

  const handleMessageChange = (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const nextMessage = event.target.value;
    const cursorPosition = event.target.selectionStart ?? nextMessage.length;

    if (modeAnnouncement) {
      setModeAnnouncement(null);
      if (modeTimerRef.current) {
        clearTimeout(modeTimerRef.current);
        modeTimerRef.current = null;
      }
    }

    onMessageChange(nextMessage);
    updateAutocomplete(nextMessage, cursorPosition);
    setEntities((current) =>
      current.filter((entity) => {
        const end = entity.start + entity.text.length;
        return end <= nextMessage.length && nextMessage.slice(entity.start, end) === entity.text;
      }),
    );
  };

  const applyAutocompleteSelection = (item: AutocompleteItem) => {
    if (!autocomplete) {
      return;
    }

    const command = slashCommands.find((candidate) => candidate.id === item.id);

    if (command?.action === "file") {
      const nextMessage =
        message.slice(0, autocomplete.triggerStart) +
        message.slice(autocomplete.triggerStart + autocomplete.triggerLen);
      pendingFilePositionRef.current = autocomplete.triggerStart;
      onMessageChange(nextMessage);
      setAutocomplete(null);
      fileInputRef.current?.click();
      return;
    }

    if (command?.action === "image") {
      const before = message.slice(0, autocomplete.triggerStart);
      const after = message.slice(autocomplete.triggerStart + autocomplete.triggerLen);
      const inserted = `${before}/image ${after}`;
      const caret = before.length + "/image ".length;
      onMessageChange(inserted);
      setAutocomplete(null);
      window.requestAnimationFrame(() => {
        textareaRef.current?.focus();
        textareaRef.current?.setSelectionRange(caret, caret);
      });
      return;
    }

    const before = message.slice(0, autocomplete.triggerStart);
    const after = message.slice(autocomplete.triggerStart + autocomplete.triggerLen);
    const inserted = item.meta === "person" ? `@${item.label}` : item.label;
    const nextMessage = before + inserted + after;

    onMessageChange(nextMessage);

    if (item.meta === "date" || item.meta === "person" || item.meta === "file") {
      const kind: EntityKind = item.meta;
      setEntities((current) => [
        ...current,
        {
          start: autocomplete.triggerStart,
          text: inserted,
          kind,
        },
      ]);
    }

    setAutocomplete(null);
    window.requestAnimationFrame(() => {
      const caret = autocomplete.triggerStart + inserted.length;
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(caret, caret);
    });
  };

  const handleComposerKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (autocomplete) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setAutocompleteIndex((current) => Math.min(current + 1, autocomplete.items.length - 1));
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setAutocompleteIndex((current) => Math.max(current - 1, 0));
        return;
      }

      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        applyAutocompleteSelection(autocomplete.items[autocompleteIndex]);
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        setAutocomplete(null);
        return;
      }
    }

    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submitComposer();
    }
  };

  const handleResponseMode = (mode: ResponseMode) => {
    if (mode === responseMode) {
      return;
    }

    const nextMode = responseModes.find((candidate) => candidate.id === mode);
    onResponseModeChange(mode);

    if (nextMode) {
      setModeAnnouncement(nextMode.announcement);
      if (modeTimerRef.current) {
        clearTimeout(modeTimerRef.current);
      }
      modeTimerRef.current = setTimeout(() => setModeAnnouncement(null), 3000);
    }
  };

  const enhanceAttachments = () => {
    if (files.length === 0) {
      return;
    }

    const names = files.map((file) => file.name).join(", ");
    const body = message.trim();
    onMessageChange(body ? `Analyze the attached file(s) (${names}) and ${body}` : `Describe and analyze the attached file(s): ${names}`);
    textareaRef.current?.focus();
  };

  const toggleRecording = async () => {
    if (isRecording) {
      mediaRecorderRef.current?.stop();
      return;
    }

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      audioChunksRef.current = [];
      const recorder = new MediaRecorder(stream);
      mediaRecorderRef.current = recorder;

      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) {
          audioChunksRef.current.push(event.data);
        }
      };

      recorder.onstop = async () => {
        stream.getTracks().forEach((track) => track.stop());
        const audio = new Blob(audioChunksRef.current, { type: "audio/webm" });
        audioChunksRef.current = [];
        setIsRecording(false);

        try {
          const formData = new FormData();
          formData.append("audio", audio, "recording.webm");
          const response = await fetch(`/api/audio/transcribe?lang=${encodeURIComponent(settings.voiceLang)}`, {
            method: "POST",
            body: formData,
          });

          if (response.ok) {
            const payload = (await response.json()) as { text?: string; transcript?: string };
            const transcript = payload.text ?? payload.transcript;

            if (transcript) {
              onMessageChange(transcript);
            }
          }
        } catch {
          // Voice transcription is optional in local development; failed calls keep the typed input intact.
        }
      };

      recorder.onerror = () => {
        stream.getTracks().forEach((track) => track.stop());
        setIsRecording(false);
      };

      recorder.start();
      setIsRecording(true);
    } catch {
      setIsRecording(false);
    }
  };

  const submitComposer = async () => {
    if (autocomplete) {
      return;
    }

    const imagePrompt = message.trimStart().toLowerCase().startsWith("/image ")
      ? message.trimStart().slice("/image ".length).trim()
      : "";

    if (imagePrompt) {
      try {
        const response = await fetch("/api/ai/images", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          credentials: "include",
          body: JSON.stringify({ prompt: imagePrompt, size: "1024x1024", quality: "auto" }),
        });

        if (response.ok) {
          const payload = (await response.json()) as { data_url?: string };

          if (payload.data_url) {
            onFilesChange([
              ...files,
              {
                id: `img-${Date.now()}`,
                name: `${imagePrompt.slice(0, 40)}.png`,
                size: 0,
                type: "image/png",
                url: payload.data_url,
              },
            ]);
            onMessageChange(`Generated image for: "${imagePrompt}"`);
          }
        }
      } catch {
        // Image generation depends on the model gateway; keep normal submit available when it is offline.
      }
    }

    setAutocomplete(null);
    setModeAnnouncement(null);
    onSubmit();
  };

  const addScreenshotFile = async () => {
    if (!navigator.mediaDevices?.getDisplayMedia) {
      return;
    }

    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({ video: true });
      const video = document.createElement("video");
      video.srcObject = stream;

      await new Promise<void>((resolve) => {
        video.onloadedmetadata = () => resolve();
      });

      await video.play();
      await new Promise((resolve) => window.setTimeout(resolve, 100));

      const canvas = document.createElement("canvas");
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      canvas.getContext("2d")?.drawImage(video, 0, 0);
      stream.getTracks().forEach((track) => track.stop());

      const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));

      if (!blob) {
        return;
      }

      const file = new File([blob], `screenshot-${Date.now()}.png`, { type: "image/png" });
      onFilesChange([
        ...files,
        {
          id: `${file.name}-${file.size}-${file.lastModified}`,
          name: file.name,
          size: file.size,
          type: file.type,
          url: URL.createObjectURL(file),
        },
      ]);
    } catch {
      // The browser throws when the user cancels screen capture.
    }
  };


  return {
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
  };
}
