import { createSignal, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Check, Copy, Download, Info } from '@/shared/icons'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { runStatusGroup } from '@/features/ingestions/lib/runs'

/** Small building blocks shared by the Innhenting views. */

export type Tr = (no: string, en: string) => string

export function StatusBadge(props: { status: string }) {
  const tone = () => {
    const status = props.status.toLowerCase()
    if (['connected', 'ready'].includes(status)) return 'ingestions-status--success'
    if (status === 'paused') return 'ingestions-status--neutral'
    switch (runStatusGroup(status)) {
      case 'succeeded':
        return 'ingestions-status--success'
      case 'active':
        return status === 'active' ? 'ingestions-status--success' : 'ingestions-status--info'
      default:
        return 'ingestions-status--warning'
    }
  }
  return (
    <span class={cn('ingestions-status', tone())}>
      <Show when={runStatusGroup(props.status) === 'active' && props.status.toLowerCase() !== 'active'}>
        <span class="ingestions-live-dot" aria-hidden="true" />
      </Show>
      {props.status.replace(/_/g, ' ')}
    </span>
  )
}

/** A capability the page shows but cannot run yet, with the reason on hover and for screen readers. */
export function NeedsBackend(props: { reason: string; label?: string }) {
  const i18n = useI18n()
  return (
    <span class="ingestions-stub-badge" title={props.reason}>
      <Info class="size-3" strokeWidth={2} aria-hidden="true" />
      {props.label ?? i18n.tr('Krever backend', 'Needs backend')}
      <span class="sr-only">: {props.reason}</span>
    </span>
  )
}

export function CopyButton(props: { text: () => string; label?: string; class?: string }) {
  const i18n = useI18n()
  const [copied, setCopied] = createSignal(false)
  async function copy() {
    try {
      await navigator.clipboard.writeText(props.text())
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      setCopied(false)
    }
  }
  return (
    <button type="button" class={cn('ingestions-icon-button', props.class)} onClick={() => void copy()}>
      <Show when={copied()} fallback={<Copy class="size-3.5" strokeWidth={1.9} />}>
        <Check class="size-3.5" strokeWidth={2.2} />
      </Show>
      {copied() ? i18n.tr('Kopiert', 'Copied') : (props.label ?? i18n.tr('Kopier', 'Copy'))}
    </button>
  )
}

/** Save `content` as a file in the browser; nothing leaves the page. */
export function downloadText(filename: string, content: string, type = 'application/json') {
  const url = URL.createObjectURL(new Blob([content], { type }))
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.click()
  URL.revokeObjectURL(url)
}

export function DownloadButton(props: { filename: string; content: () => string; type?: string; label?: string }) {
  const i18n = useI18n()
  return (
    <button
      type="button"
      class="ingestions-icon-button"
      onClick={() => downloadText(props.filename, props.content(), props.type)}
    >
      <Download class="size-3.5" strokeWidth={1.9} />
      {props.label ?? i18n.tr('Last ned', 'Download')}
    </button>
  )
}

export function EmptyState(props: { icon?: JSX.Element; title: string; children?: JSX.Element }) {
  return (
    <div class="ingestions-empty-state">
      <Show when={props.icon}>{(icon) => <div class="ingestions-empty-state__icon">{icon()}</div>}</Show>
      <strong>{props.title}</strong>
      <Show when={props.children}>
        <p>{props.children}</p>
      </Show>
    </div>
  )
}

export function relativeTime(value: string | null | undefined, i18n: ReturnType<typeof useI18n>) {
  if (!value) return i18n.tr('Ukjent', 'Unknown')
  const time = Date.parse(value)
  if (!Number.isFinite(time)) return value
  const deltaMs = time - Date.now()
  const rtf = new Intl.RelativeTimeFormat(localeDateTime(i18n.locale()), { numeric: 'auto' })
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['day', 86_400_000],
    ['hour', 3_600_000],
    ['minute', 60_000],
  ]
  for (const [unit, amount] of units) {
    if (Math.abs(deltaMs) >= amount || unit === 'minute') {
      return rtf.format(Math.round(deltaMs / amount), unit)
    }
  }
  return value
}

export function absoluteTime(value: string | null | undefined, i18n: ReturnType<typeof useI18n>) {
  if (!value) return '—'
  const time = Date.parse(value)
  if (!Number.isFinite(time)) return value
  return new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'medium' }).format(time)
}

export function kindLabel(kind: string, tr: Tr) {
  switch (kind) {
    case 'scrape':
      return 'Scrape'
    case 'crawl':
      return 'Crawl'
    case 'extract':
      return tr('Uttrekk', 'Extract')
    case 'search':
      return tr('Søk', 'Search')
    case 'map':
      return tr('Kart', 'Map')
    case 'batch':
      return 'Batch'
    case 'agent':
      return 'Agent'
    default:
      return kind
  }
}
