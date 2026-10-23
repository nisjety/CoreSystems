import { createSignal, For, onCleanup, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { useI18n } from '@/shared/i18n'
import './browser-evidence.css'

/** The console filter (WS9.D): level chips and a substring match on the message text. */
export const CONSOLE_FILTER_MAX_CHARS = 200
export const CONSOLE_FILTER_DEBOUNCE_MS = 250
export const CONSOLE_LEVELS = ['error', 'warn', 'info', 'debug'] as const
export type ConsoleLevel = (typeof CONSOLE_LEVELS)[number]

export type ConsoleFilter = {
  /** Empty means every level. */
  levels: ConsoleLevel[]
  /** Case-insensitive substring of the message text. Empty means any text. */
  text: string
}

export function emptyConsoleFilter(): ConsoleFilter {
  return { levels: [], text: '' }
}

/**
 * Map a console level to one of the four chips. Mirrors the server aliases: `warning` is
 * `warn`, `log` is `info`, `verbose` is `debug`.
 */
export function normalizeConsoleLevel(level: string | null | undefined): ConsoleLevel {
  const value = (level ?? '').trim().toLowerCase()
  if (value.includes('error')) return 'error'
  if (value.includes('warn')) return 'warn'
  if (value.includes('debug') || value.includes('verbose')) return 'debug'
  return 'info'
}

export function consoleEntryMatches(
  entry: { level?: string | null; text?: string | null },
  filter: ConsoleFilter,
): boolean {
  if (filter.levels.length > 0 && !filter.levels.includes(normalizeConsoleLevel(entry.level))) return false
  const needle = filter.text.trim().toLowerCase()
  if (needle.length > 0 && !(entry.text ?? '').toLowerCase().includes(needle)) return false
  return true
}

export function filterConsoleEntries<T extends { level?: string | null; text?: string | null }>(
  entries: T[],
  filter: ConsoleFilter,
): T[] {
  if (filter.levels.length === 0 && filter.text.trim().length === 0) return entries
  return entries.filter((entry) => consoleEntryMatches(entry, filter))
}

const LEVEL_LABELS: Record<ConsoleLevel, { no: string; en: string }> = {
  error: { no: 'Feil', en: 'Errors' },
  warn: { no: 'Advarsler', en: 'Warnings' },
  info: { no: 'Info', en: 'Info' },
  debug: { no: 'Debug', en: 'Debug' },
}

/**
 * Filter bar above the console list. Chips apply at once. The text input is debounced by
 * {@link CONSOLE_FILTER_DEBOUNCE_MS}, capped at {@link CONSOLE_FILTER_MAX_CHARS} characters,
 * and Escape clears it. Filtering is done on the client; nothing is sent to the server.
 */
export function ConsoleFilterBar(props: {
  value: ConsoleFilter
  total: number
  shown: number
  onChange: (next: ConsoleFilter) => void
}): JSX.Element {
  const i18n = useI18n()
  const [draft, setDraft] = createSignal(untrack(() => props.value.text))
  let timer: ReturnType<typeof setTimeout> | undefined

  const clearTimer = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }
  onCleanup(clearTimer)

  const toggleLevel = (level: ConsoleLevel) => {
    const current = props.value.levels
    const levels = current.includes(level) ? current.filter((item) => item !== level) : [...current, level]
    props.onChange({ ...props.value, levels })
  }

  const onInput = (raw: string) => {
    const text = raw.slice(0, CONSOLE_FILTER_MAX_CHARS)
    setDraft(text)
    clearTimer()
    timer = setTimeout(() => {
      timer = undefined
      props.onChange({ ...props.value, text })
    }, CONSOLE_FILTER_DEBOUNCE_MS)
  }

  const onKeyDown = (key: string) => {
    if (key !== 'Escape') return
    clearTimer()
    setDraft('')
    props.onChange({ ...props.value, text: '' })
  }

  return (
    <div class="browser-evidence-filter" role="group" aria-label={i18n.tr('Filtrer konsoll', 'Filter console')}>
      <div class="browser-evidence-filter__levels">
        <For each={[...CONSOLE_LEVELS]}>
          {(level) => {
            const active = () => props.value.levels.includes(level)
            return (
              <button
                type="button"
                class={['browser-evidence-chip', { 'browser-evidence-chip--active': active() }]}
                aria-pressed={active() ? 'true' : 'false'}
                data-level={level}
                onClick={() => toggleLevel(level)}
              >
                {i18n.tr(LEVEL_LABELS[level].no, LEVEL_LABELS[level].en)}
              </button>
            )
          }}
        </For>
      </div>
      <input
        type="search"
        class="browser-evidence-filter__input"
        maxlength={CONSOLE_FILTER_MAX_CHARS}
        placeholder={i18n.tr('Søk i konsollen', 'Search the console')}
        aria-label={i18n.tr('Søk i konsollmeldinger', 'Search console messages')}
        value={draft()}
        onInput={(event) => onInput(event.currentTarget.value)}
        onKeyDown={(event) => onKeyDown(event.key)}
      />
      <p class="browser-evidence-filter__count" role="status">
        {i18n.tr('Viser', 'Showing')} {props.shown} {i18n.tr('av', 'of')} {props.total}
      </p>
    </div>
  )
}
