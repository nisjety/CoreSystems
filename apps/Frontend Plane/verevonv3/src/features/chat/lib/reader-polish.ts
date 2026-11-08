/**
 * Reader and appearance polish (F-t3-098): the size of the answer text, a dyslexia-friendly font, and the spacing of
 * the transcript. The page writes them as `data-reader-*` attributes on the chat workspace, and `chat-reader.css`
 * turns them into type and spacing.
 *
 * Behind `VITE_CHAT_READER_POLISH`, read through `readerPolishEnabled`. Off, no attribute is written, the settings are
 * not read, and the transcript renders as it did. The text size is the one the person stored in their appearance
 * settings. The font choice is kept on this device, like the transcript level's cache: it is a reading aid, and the
 * server does not need it. Spacing follows the transcript level (EP-14 S2) when that switch is on, and otherwise the
 * stored compact flag, which `UserPreferencesProvider` already puts on the document.
 */

import { createEffect, createSignal, type Accessor } from 'solid-js'
import { getAppearanceSettings, type TranscriptDensity } from '@/shared/api/navbar-client'
import { createResource } from '@/shared/lib/create-resource-compat'

/** Only the literal `true` enables the polish, as for the other chat switches. */
export function isReaderPolishEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const readerPolishEnabled = isReaderPolishEnabled(import.meta.env.VITE_CHAT_READER_POLISH)

/** The text sizes user-core stores, from `apps/Control Plane/user-core`. */
export const READER_FONT_SIZES = ['small', 'medium', 'large'] as const
export type ReaderFontSize = (typeof READER_FONT_SIZES)[number]
export const READER_FONT_SIZE_DEFAULT: ReaderFontSize = 'medium'

/** The stored text size when it is one of the three, otherwise the default. A stored value is never an error. */
export function resolveReaderFontSize(value: unknown): ReaderFontSize {
  return (READER_FONT_SIZES as readonly unknown[]).includes(value) ? (value as ReaderFontSize) : READER_FONT_SIZE_DEFAULT
}

export type ReaderSpacing = 'comfortable' | 'compact'

/**
 * The spacing for a transcript level. Only the levels that fold or hide work tighten the rhythm. With the level switch
 * off the spacing is left unset, so the stored compact flag decides it in CSS.
 */
export function readerSpacingFor(densityEnabled: boolean, level: TranscriptDensity): ReaderSpacing | undefined {
  if (!densityEnabled) return undefined
  return level === 'compact' || level === 'minimal' ? 'compact' : 'comfortable'
}

export type ReaderAppearance = {
  fontSize: ReaderFontSize
  dyslexiaFont: boolean
  spacing?: ReaderSpacing
}

export type ReaderAttributes = {
  'data-reader-font-size'?: ReaderFontSize
  'data-reader-font'?: 'dyslexia'
  'data-reader-spacing'?: ReaderSpacing
}

/**
 * The attributes for the workspace. A choice that is off comes back as `undefined`, which the page renders as no
 * attribute, so turning a choice off clears it.
 */
export function readerAttributes(appearance: ReaderAppearance): ReaderAttributes {
  return {
    'data-reader-font-size': appearance.fontSize,
    'data-reader-font': appearance.dyslexiaFont ? 'dyslexia' : undefined,
    'data-reader-spacing': appearance.spacing,
  }
}

/** The key is versioned, so a later format can sit beside this one. */
export const READER_DYSLEXIA_CACHE_KEY = 'verevon.reader.dyslexia.v1'

/** This device's storage, or nothing where the browser refuses it (a private window, blocked site data). */
function deviceStorage(): Storage | undefined {
  try {
    return globalThis.localStorage
  } catch {
    return undefined
  }
}

/** Whether the dyslexia-friendly font was chosen on this device. A missing, blocked or unknown value reads as off. */
export function readCachedDyslexiaFont(): boolean {
  try {
    return deviceStorage()?.getItem(READER_DYSLEXIA_CACHE_KEY) === '1'
  } catch {
    return false
  }
}

/** Remembers the choice on this device. A failed write only means the font resets on the next visit. */
export function writeCachedDyslexiaFont(on: boolean): void {
  try {
    if (on) deviceStorage()?.setItem(READER_DYSLEXIA_CACHE_KEY, '1')
    else deviceStorage()?.removeItem(READER_DYSLEXIA_CACHE_KEY)
  } catch {
    // Storage full or blocked. Nothing is lost: the font simply resets.
  }
}

/** The switch, and the settings call the page uses. Tests pass their own, so the switch need not be built in. */
export type ReaderAppearanceDeps = {
  enabled: boolean
  /** The stored appearance. Only its text size is read here. */
  load: () => Promise<{ fontSize?: unknown }>
}

export type ReaderAppearanceControl = {
  /** The workspace attributes at this spacing. Empty with the switch off, so the markup is unchanged. */
  attributes: (spacing: ReaderSpacing | undefined) => ReaderAttributes
  /** Whether the dyslexia-friendly font is on. Always off with the switch off. */
  dyslexiaFont: Accessor<boolean>
  setDyslexiaFont: (on: boolean) => void
}

const liveReaderDeps: ReaderAppearanceDeps = {
  enabled: readerPolishEnabled,
  load: () => getAppearanceSettings(),
}

/**
 * The chat page's reader state. The text size starts at the default and takes the stored size once the settings are
 * read. A read that fails keeps the default, so the transcript still renders.
 */
export function createReaderAppearance(deps: ReaderAppearanceDeps = liveReaderDeps): ReaderAppearanceControl {
  const { enabled } = deps
  const [fontSize, setFontSize] = createSignal<ReaderFontSize>(READER_FONT_SIZE_DEFAULT)
  const [dyslexiaFont, setDyslexiaFontSignal] = createSignal(enabled && readCachedDyslexiaFont())

  // With the switch off the source is false, so the settings are never fetched.
  const [settings] = createResource(
    () => enabled,
    () => deps.load(),
  )
  createEffect(
    () => settings(),
    (read) => {
      if (read) setFontSize(resolveReaderFontSize(read.fontSize))
    },
  )

  const setDyslexiaFont = (on: boolean) => {
    setDyslexiaFontSignal(on)
    writeCachedDyslexiaFont(on)
  }
  const attributes = (spacing: ReaderSpacing | undefined): ReaderAttributes =>
    enabled ? readerAttributes({ fontSize: fontSize(), dyslexiaFont: dyslexiaFont(), spacing }) : {}

  return { attributes, dyslexiaFont, setDyslexiaFont }
}
