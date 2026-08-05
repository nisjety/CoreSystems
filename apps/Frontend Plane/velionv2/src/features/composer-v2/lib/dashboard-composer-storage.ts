import {
  defaultComposerSettings,
  type ComposerSettings,
} from "@/features/dashboard-v2/lib/dashboard-composer-model";

const composerSettingsKey = "verevon-dashboard-composer-settings";
const composerReceiptTimeFormat = new Intl.DateTimeFormat("nb-NO", {
  hour: "2-digit",
  minute: "2-digit",
});

export function formatComposerTurnTime(date: Date) {
  return composerReceiptTimeFormat.format(date);
}

export function loadComposerSettings(): ComposerSettings {
  if (typeof window === "undefined") {
    return defaultComposerSettings;
  }

  try {
    const raw = window.localStorage.getItem(composerSettingsKey);
    return raw ? { ...defaultComposerSettings, ...JSON.parse(raw) } : defaultComposerSettings;
  } catch {
    return defaultComposerSettings;
  }
}

export function saveComposerSettings(settings: ComposerSettings) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.localStorage.setItem(composerSettingsKey, JSON.stringify(settings));
  } catch {
    // Local persistence is a convenience; the composer should still work without it.
  }
}
