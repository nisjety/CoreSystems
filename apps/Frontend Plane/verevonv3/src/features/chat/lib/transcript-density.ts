/**
 * Transcript density (EP-14 S2): how much of an agent turn's work the chat shows. A person picks one of four
 * levels. The level is stored in their appearance settings and cached on this device, so the transcript opens at
 * the level they chose before the server answers.
 *
 * Behind `VITE_CHAT_TRANSCRIPT_DENSITY`. Off, nothing here is read or saved, the rules are the unrestricted set, and
 * the chat renders as it did before. Only the literal `true` enables it, as for the other chat switches.
 */

import { createEffect, createSignal, type Accessor } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  getAppearanceSettings,
  saveTranscriptDensity,
  TRANSCRIPT_DENSITY_LEVELS,
  type TranscriptDensity,
} from '@/shared/api/navbar-client'

export function isTranscriptDensityEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const transcriptDensityEnabled = isTranscriptDensityEnabled(import.meta.env.VITE_CHAT_TRANSCRIPT_DENSITY)

/** The level a person who has not chosen one reads at. */
export const TRANSCRIPT_DENSITY_DEFAULT: TranscriptDensity = 'standard'

/** This device's last chosen level. The version in the key lets a later format sit beside this one. */
export const TRANSCRIPT_DENSITY_CACHE_KEY = 'verevon.chat.density.v1'

export function isTranscriptDensity(value: unknown): value is TranscriptDensity {
  return typeof value === 'string' && (TRANSCRIPT_DENSITY_LEVELS as readonly string[]).includes(value)
}

/**
 * The level a person reads at. A stored level wins. With none stored, the older compact flag still means compact,
 * and anything else reads at the default. A stored value outside the set is ignored, never an error.
 */
export function resolveDensity(
  settings: { transcriptDensity?: unknown; compactMode?: unknown } | null | undefined,
): TranscriptDensity {
  const stored = settings?.transcriptDensity
  if (isTranscriptDensity(stored)) return stored
  return settings?.compactMode === true ? 'compact' : TRANSCRIPT_DENSITY_DEFAULT
}

/**
 * What a level changes in the transcript. `foldSettled` folds settled turns to one row (EP-14 S1). `showProcess`
 * hides the tool chips and reasoning traces, so the answer, the decisions and the files are what is left.
 */
export type DensityRules = {
  foldSettled: boolean
  showProcess: boolean
}

const LEVEL_RULES: Record<TranscriptDensity, DensityRules> = {
  detailed: { foldSettled: false, showProcess: true },
  standard: { foldSettled: false, showProcess: true },
  compact: { foldSettled: true, showProcess: true },
  minimal: { foldSettled: true, showProcess: false },
}

/**
 * The rules with the switch off. They change nothing: folding still follows `VITE_CHAT_SETTLED_FOLD` alone, and the
 * process trail shows as it always did.
 */
export const UNRESTRICTED_RULES: DensityRules = { foldSettled: true, showProcess: true }

export function densityRules(level: TranscriptDensity): DensityRules {
  return LEVEL_RULES[level]
}

/** This device's storage, or nothing where the browser refuses it (a private window, blocked site data). */
function deviceStorage(): Storage | undefined {
  try {
    return globalThis.localStorage
  } catch {
    return undefined
  }
}

/** The level last chosen on this device. A missing, blocked or unknown value reads as nothing. */
export function readCachedDensity(): TranscriptDensity | undefined {
  try {
    const raw = deviceStorage()?.getItem(TRANSCRIPT_DENSITY_CACHE_KEY)
    return isTranscriptDensity(raw) ? raw : undefined
  } catch {
    return undefined
  }
}

/** Remembers the level on this device. A failed write only means the next visit waits for the server's copy. */
export function writeCachedDensity(level: TranscriptDensity): void {
  try {
    deviceStorage()?.setItem(TRANSCRIPT_DENSITY_CACHE_KEY, level)
  } catch {
    // Storage full or blocked. The server still holds the level, so nothing is lost.
  }
}

export type TranscriptDensityControl = {
  /** The level the transcript shows now. */
  level: Accessor<TranscriptDensity>
  /** What the level changes in the transcript. Unrestricted with the switch off. */
  rules: Accessor<DensityRules>
  /** Shows the level from now on, here and in the person's settings. A save that fails puts the old level back. */
  choose: (level: TranscriptDensity) => void
}

/** The switch, and the settings calls the page uses. Tests pass their own, so the switch need not be built in. */
export type TranscriptDensityDeps = {
  enabled: boolean
  load: () => Promise<{ transcriptDensity?: string; compactMode?: boolean }>
  save: (level: TranscriptDensity) => Promise<unknown>
}

const liveDensityDeps: TranscriptDensityDeps = {
  enabled: transcriptDensityEnabled,
  load: () => getAppearanceSettings(),
  save: (level) => saveTranscriptDensity(level),
}

/**
 * The chat page's density state. The level starts from this device's cache, and the stored level replaces it once
 * the settings are read, unless the person has already chosen one in the meantime.
 */
export function createTranscriptDensity(deps: TranscriptDensityDeps = liveDensityDeps): TranscriptDensityControl {
  const { enabled } = deps
  const cached = enabled ? readCachedDensity() : undefined
  const [level, setLevel] = createSignal<TranscriptDensity>(cached ?? TRANSCRIPT_DENSITY_DEFAULT)
  let chosen = false
  // Saves run one at a time. Each reads the appearance object it writes back, so two at once could lose one.
  let saves: Promise<unknown> = Promise.resolve()

  // With the switch off the source is false, so the settings are never fetched.
  const [settings] = createResource(
    () => enabled,
    () => deps.load(),
  )
  createEffect(
    () => settings(),
    (read) => {
      if (!read || chosen) return
      const next = resolveDensity(read)
      setLevel(next)
      writeCachedDensity(next)
    },
  )

  const choose = (next: TranscriptDensity) => {
    const previous = level()
    if (next === previous) return
    chosen = true
    setLevel(next)
    writeCachedDensity(next)
    saves = saves
      .then(() => deps.save(next))
      .catch(() => {
        // The save did not land. Show the level the person had, unless they have chosen another since.
        if (level() !== next) return
        setLevel(previous)
        writeCachedDensity(previous)
      })
  }

  const rules = () => (enabled ? densityRules(level()) : UNRESTRICTED_RULES)
  return { level, rules, choose }
}
