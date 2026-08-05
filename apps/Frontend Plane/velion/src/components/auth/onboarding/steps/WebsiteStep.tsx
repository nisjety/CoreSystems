'use client'

/**
 * Step 3 — website seed (Chatbase pattern, live crawl).
 *
 * Left pane: simplified URL input matching Chatbase's "Where can we
 * learn from?" — single big text field with `https://` prefix, an
 * optional "what should the agent help with?" textarea, a black
 * Continue CTA and a Skip link. Progress dots sit above so the user
 * always sees where they are in the 6-step flow.
 *
 * Right pane: live snippet drop. After submit the step opens an SSE
 * connection to `/api/onboarding/crawl-preview` which kicks a real
 * `crawl` job on `quarry-control`. Each `snippet` event is pushed onto
 * a queue and rendered as a falling card of one of four types — text
 * excerpt, image thumbnail, document icon, or link chip. Multiple
 * cards animate in parallel because each picks its own x-offset, fall
 * duration and start delay; that's what makes the folder feel busy
 * instead of synchronous.
 *
 * The same stream also delivers `branding` events from the runtime's
 * `branding_extracted` emit (favicon, theme color, logo candidate,
 * palette, og:image). We persist them on the wizard state — the
 * `OnboardingFrame` reads `machine.state.website.branding` and renders
 * a brand pill above the rounded modal card, so the user gets
 * immediate visual feedback that the site was understood, not just
 * crawled. This step intentionally doesn't render anything brand-
 * related itself.
 *
 * The component degrades open: if the SSE fails or no events arrive
 * within the timeout the step advances anyway so the user is never
 * stuck on a flaky control plane.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react'

import { formatOnboardingText, useOnboardingCopy } from '../i18n'
import type { OnboardingMachine } from '../state/useOnboardingMachine'
import type { BrandingSignals } from '../state/types'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

type SnippetKind = 'text' | 'image' | 'file' | 'link'

interface SnippetEvent {
  id: string
  kind: SnippetKind
  title: string
  excerpt?: string
  thumbUrl?: string
  url: string
  contentType?: string
}

/**
 * Render geometry for one falling card. Computed once when the
 * snippet enters and cached so re-renders don't shuffle the cards
 * around. Each card gets its own delay so multiple are in flight at
 * the same time.
 */
interface FallingCard extends SnippetEvent {
  /** Horizontal offset from centre, in % of the right pane width. */
  xOffsetPct: number
  /** Fall animation start delay in ms. */
  delayMs: number
  /** Fall duration in ms. */
  durationMs: number
}

const SAFE_ADVANCE_AFTER_MS = 12_000
const MIN_SNIPPETS_TO_ADVANCE = 4

export function WebsiteStep({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy()
  const initial = machine.state.website
  const [url, setUrl] = useState(initial?.url ?? '')
  const [brief, setBrief] = useState(initial?.agentBrief ?? '')
  const [submitted, setSubmitted] = useState(false)
  const [cards, setCards] = useState<FallingCard[]>([])
  const [doneCount, setDoneCount] = useState<number | null>(null)
  const [warning, setWarning] = useState<string | null>(null)

  // Keep a ref to the current count so the SSE handler can read it
  // without re-creating itself on every render.
  const cardsRef = useRef<FallingCard[]>([])
  cardsRef.current = cards

  // Subscribe to the live crawl preview when the user submits. The
  // SSE event-source pattern is rebuilt rather than imported because
  // EventSource ignores `cookie: include` cross-fetch and we hit the
  // route under the same origin — `fetch` + manual stream parsing is
  // tighter and easier to abort.
  useEffect(() => {
    if (!submitted) return undefined
    if (!url.trim()) return undefined

    const controller = new AbortController()
    let advanced = false
    const advanceOnce = () => {
      if (advanced) return
      advanced = true
      window.setTimeout(() => machine.goTo('connect'), 700)
    }

    const safetyTimer = window.setTimeout(() => {
      advanceOnce()
    }, SAFE_ADVANCE_AFTER_MS)

    void (async () => {
      try {
        const response = await fetch('/api/onboarding/crawl-preview', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            url: url.trim(),
            brief: brief.trim() || undefined,
            maxPages: 8,
          }),
          signal: controller.signal,
          cache: 'no-store',
        })

        if (!response.ok || !response.body) {
          throw new Error(`crawl-preview ${response.status}`)
        }

        const reader = response.body.getReader()
        const decoder = new TextDecoder()
        let buffer = ''
        while (true) {
          const { value, done } = await reader.read()
          if (done) break
          buffer += decoder.decode(value, { stream: true })
          let separator = buffer.indexOf('\n\n')
          while (separator !== -1) {
            const rawEvent = buffer.slice(0, separator)
            buffer = buffer.slice(separator + 2)
            separator = buffer.indexOf('\n\n')
            handleSseFrame(rawEvent)
          }
        }
      } catch (error) {
        if ((error as { name?: string }).name === 'AbortError') return
        setWarning(
          copy.website.warnings.unavailable,
        )
      } finally {
        // If we never reached MIN_SNIPPETS_TO_ADVANCE we still advance
        // after a short grace period so the user is not stuck.
        if (cardsRef.current.length >= MIN_SNIPPETS_TO_ADVANCE) {
          advanceOnce()
        } else {
          window.setTimeout(advanceOnce, 2_500)
        }
      }
    })()

    function handleSseFrame(frame: string): void {
      // Frame layout:
      //   event: snippet
      //   data: {...}
      let eventName = 'message'
      let dataLine = ''
      for (const line of frame.split('\n')) {
        if (line.startsWith('event: ')) eventName = line.slice(7).trim()
        else if (line.startsWith('data: ')) dataLine += line.slice(6)
      }
      if (!dataLine) return
      let payload: unknown
      try {
        payload = JSON.parse(dataLine)
      } catch {
        return
      }
      if (eventName === 'snippet') {
        const snippet = payload as SnippetEvent
        setCards((prev) => [...prev, toFallingCard(snippet, prev.length)])
      } else if (eventName === 'branding') {
        // Normalize the snake_case Rust payload into the camelCase TS
        // BrandingSignals shape and persist on wizard state. The
        // OnboardingFrame renders the brand strip above the modal — we
        // don't render anything brand-related inside the step itself.
        const normalized = normalizeBranding(payload)
        if (normalized) {
          const currentWebsite = machine.state.website
          machine.setWebsite({
            url: currentWebsite?.url ?? url.trim(),
            agentBrief: currentWebsite?.agentBrief ?? brief.trim(),
            crawlJobId: currentWebsite?.crawlJobId,
            branding: { ...currentWebsite?.branding, ...normalized },
          })
        }
      } else if (eventName === 'warning') {
        const w = payload as { message?: string; code?: string }
        const friendly = friendlyWarning(w.code, w.message, copy.website.warnings)
        if (friendly) setWarning(friendly)
      } else if (eventName === 'done') {
        const d = payload as { count?: number }
        setDoneCount(typeof d.count === 'number' ? d.count : null)
        advanceOnce()
      }
    }

    return () => {
      window.clearTimeout(safetyTimer)
      controller.abort()
    }
    // We intentionally only re-run when `submitted` flips — the inputs
    // are captured by closure on submit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [submitted])

  const submit = (event: React.FormEvent) => {
    event.preventDefault()
    if (!url.trim() || submitted) return
    machine.setWebsite({
      url: url.trim(),
      agentBrief: brief.trim(),
    })
    setSubmitted(true)
  }

  const fillPct = useMemo(() => {
    const target = doneCount ?? 8
    return Math.min(100, Math.round((cards.length / Math.max(1, target)) * 100))
  }, [cards.length, doneCount])

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.website.eyebrow}</StepEyebrow>
        <StepTitle>{copy.website.title}</StepTitle>
        <StepDescription>{copy.website.description}</StepDescription>

        <form onSubmit={submit} className="flex flex-col gap-5">
          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              {copy.website.urlLabel}
            </span>
            <div className="mt-2 flex items-stretch overflow-hidden rounded-md border border-[#D6D2CB] bg-white focus-within:border-[#1F1B17]">
              <span className="flex items-center bg-[#F7F4ED] px-3 font-inter text-[12px] text-[#6B6660]">
                https://
              </span>
              <input
                autoFocus
                required
                type="text"
                inputMode="url"
                disabled={submitted}
                value={url.replace(/^https?:\/\//, '')}
                onChange={(e) =>
                  setUrl(
                    e.target.value
                      .replace(/^https?:\/\//, '')
                      .replace(/\s+/g, ''),
                  )
                }
                placeholder="aquatiq.com"
                className="flex-1 bg-white px-3 py-2.5 font-inter text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>
          </label>

          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              {copy.website.briefLabel}
            </span>
            <textarea
              rows={2}
              disabled={submitted}
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              placeholder={copy.website.briefPlaceholder}
              className="mt-2 w-full resize-none rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[13px] leading-5 text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
            />
          </label>

          {!submitted ? (
            <div className="flex items-center gap-4">
              <PrimaryButton type="submit" disabled={!url.trim()}>
                {copy.website.continue}
              </PrimaryButton>
              <SkipLink onClick={() => machine.goTo('connect')}>
                {copy.website.skip}
              </SkipLink>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <p className="font-inter text-[12px] text-[#6B6660]">
                {formatOnboardingText(copy.website.fetching, { url })}
              </p>
              {warning && (
                <p className="font-inter text-[11px] text-[#B07C2E]">
                  {warning}
                </p>
              )}
            </div>
          )}
        </form>
      </LeftPane>

      <RightPane>
        <SnippetDropFolder cards={cards} fillPct={fillPct} />
      </RightPane>
    </>
  )
}

/**
 * Compute the per-card animation parameters once, when the card
 * enters. Centred around the folder with up to ±28% drift; durations
 * 1.6–2.6 s; delays staggered by index so a quick burst of snippets
 * still flows visually instead of stacking.
 */
function toFallingCard(snippet: SnippetEvent, index: number): FallingCard {
  // Deterministic-ish pseudo-randomness derived from the id so the
  // same snippet doesn't jitter on re-renders.
  const seed = hashString(snippet.id)
  const xOffsetPct = ((seed % 56) - 28) / 1 // -28..+28
  const delayMs = (index % 3) * 220 + Math.floor((seed >> 8) % 250)
  const durationMs = 1_600 + Math.floor((seed >> 16) % 1_000)
  return {
    ...snippet,
    xOffsetPct,
    delayMs,
    durationMs,
  }
}

function hashString(input: string): number {
  let hash = 0
  for (let i = 0; i < input.length; i += 1) {
    hash = (hash * 31 + input.charCodeAt(i)) | 0
  }
  return Math.abs(hash)
}

/**
 * Map the route's structured warning codes to user-friendly Norwegian
 * messages. Falls back to the server-supplied message when the code
 * is unknown (forward-compat for new codes added server-side without
 * a wizard update). Returns `null` when there's nothing to display.
 */
function friendlyWarning(
  code: string | undefined,
  fallback: string | undefined,
  warnings: Record<string, string>,
): string | null {
  switch (code) {
    case 'invalid_url':
      return warnings.invalidUrl
    case 'bad_scheme':
      return warnings.badScheme
    case 'no_hostname':
      return warnings.noHostname
    case 'dns_failed':
      return warnings.dnsFailed
    case 'private_address':
      return warnings.privateAddress
    case 'control_unreachable':
      return warnings.unavailable
    case 'crawl_failed':
      return warnings.crawlFailed
    case 'no_events':
      return warnings.noEvents
    case 'knowledge_store_unavailable':
      return warnings.knowledgeStoreUnavailable
    default:
      return fallback || null
  }
}

/**
 * Allow-list URL filter for branding payloads. The Rust runtime
 * forwards anything it scraped — favicon hrefs, og:image hrefs, logo
 * <img src> values — and those land directly in our <img src=…> in the
 * BrandStrip. We reject anything that isn't an absolute `http://` /
 * `https://` URL with a public-shaped hostname. Specifically blocked:
 * `data:` (could embed scripts in svgs), `javascript:`, `file:`,
 * `vbscript:`, relative paths (would resolve under verevon's own
 * origin), and absolute URLs with empty hostnames.
 *
 * Returns the cleaned URL string, or `undefined` to drop the field.
 */
function safeBrandingURL(input: string | undefined): string | undefined {
  if (!input) return undefined
  const trimmed = input.trim()
  if (!trimmed) return undefined
  let parsed: URL
  try {
    // Strict absolute parse — relative URLs throw without a base.
    parsed = new URL(trimmed)
  } catch {
    return undefined
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return undefined
  }
  if (!parsed.hostname) return undefined
  // Belt-and-braces: reject hostnames that look like RFC1918 / loopback
  // literals so a malicious crawled page can't leak the verevon
  // server's reachable LAN into the user's browser via an <img>.
  if (
    parsed.hostname === 'localhost' ||
    /^(127|10)\./.test(parsed.hostname) ||
    /^192\.168\./.test(parsed.hostname) ||
    /^172\.(1[6-9]|2\d|3[01])\./.test(parsed.hostname) ||
    /^169\.254\./.test(parsed.hostname) ||
    parsed.hostname === '::1' ||
    parsed.hostname.startsWith('fe80:') ||
    parsed.hostname.startsWith('fc') ||
    parsed.hostname.startsWith('fd')
  ) {
    return undefined
  }
  return parsed.toString()
}

/**
 * Convert the Rust runtime's snake_case `RenderedBranding` payload —
 * shipped over SSE under `event: branding` — into our camelCase
 * `BrandingSignals` shape. Every URL field is run through
 * `safeBrandingURL` before being persisted; palette colors are
 * filtered down to strict CSS hex values. Returns `null` if the
 * payload is malformed so the caller can skip the state write entirely.
 */
function normalizeBranding(input: unknown): BrandingSignals | null {
  if (!input || typeof input !== 'object') return null
  const root = input as Record<string, unknown>
  const eventUrl = typeof root.url === 'string' ? root.url : undefined
  const branding = root.branding
  if (!branding || typeof branding !== 'object') return null
  const b = branding as Record<string, unknown>
  const staticSignals = (b.static_signals && typeof b.static_signals === 'object'
    ? (b.static_signals as Record<string, unknown>)
    : {}) as Record<string, unknown>
  const str = (v: unknown): string | undefined =>
    typeof v === 'string' && v.length > 0 ? v : undefined
  // Palette must be `#rgb` / `#rrggbb` / `#rrggbbaa` strings. Anything
  // else gets dropped — the wizard inlines these into `backgroundColor`.
  const paletteIn = Array.isArray(b.palette) ? (b.palette as unknown[]) : []
  const palette = paletteIn
    .filter((x): x is string => typeof x === 'string')
    .filter((s) => /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(s.trim()))
    .map((s) => s.trim().toLowerCase())
  // theme_color: accept any safe CSS color string but only when it
  // matches our hex allow-list. The BrandStrip's safeColor() validates
  // again at render time as defence in depth.
  const themeColorRaw = str(staticSignals.theme_color)
  const themeColor =
    themeColorRaw &&
    /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(themeColorRaw)
      ? themeColorRaw.toLowerCase()
      : undefined
  const out: BrandingSignals = {
    url: safeBrandingURL(eventUrl),
    siteName: str(staticSignals.site_name),
    favicon: safeBrandingURL(str(staticSignals.favicon)),
    themeColor,
    ogImage: safeBrandingURL(str(staticSignals.og_image)),
    appleTouchIcon: safeBrandingURL(str(staticSignals.apple_touch_icon)),
    palette: palette.length > 0 ? palette : undefined,
    fontFamily: str(b.font_family),
    logoCandidate: safeBrandingURL(str(b.logo_candidate)),
    bodyBackground: str(b.body_background),
  }
  // Drop the record entirely if no fields were extracted — the SSE
  // emitter shouldn't have sent it in that case, but be defensive.
  const hasAny = Object.values(out).some(
    (v) =>
      v !== undefined &&
      (typeof v !== 'object' || (Array.isArray(v) && v.length > 0)),
  )
  return hasAny ? out : null
}

function SnippetDropFolder({
  cards,
  fillPct,
}: {
  cards: FallingCard[]
  fillPct: number
}) {
  return (
    <div className="relative flex h-full w-full items-end justify-center px-10 pb-10">
      {/* Falling layer fills the upper 60% of the pane. Each card
          animates from top to ~70% of the layer so it visually lands
          on the folder lip below. */}
      <div className="pointer-events-none absolute inset-x-0 top-0 h-[58%] overflow-hidden">
        {cards.map((card) => (
          <SnippetCard key={card.id} card={card} />
        ))}
      </div>

      <FolderCard count={cards.length} fillPct={fillPct} />

      <style>{`
        @keyframes verevon-snippet-drop {
          0% {
            transform: translate(var(--vx, -50%), -20%) rotate(var(--vrot, 0deg));
            opacity: 0;
          }
          15% { opacity: 1; }
          100% {
            transform: translate(var(--vx, -50%), 70%) rotate(var(--vrot, 0deg));
            opacity: 1;
          }
        }
      `}</style>
    </div>
  )
}

function SnippetCard({ card }: { card: FallingCard }) {
  const { copy } = useOnboardingCopy()
  const style: React.CSSProperties = {
    left: `calc(50% + ${card.xOffsetPct}%)`,
    animationDelay: `${card.delayMs}ms`,
    animationDuration: `${card.durationMs}ms`,
    animationName: 'verevon-snippet-drop',
    animationTimingFunction: 'ease-out',
    animationFillMode: 'forwards',
    // Custom property the keyframes pick up via `var(--vx)` so the
    // card lands at its own x-offset rather than dead-centre.
    ['--vx' as string]: '-50%',
    ['--vrot' as string]: `${((card.id.length * 7) % 7) - 3}deg`,
  }
  return (
    <div
      style={style}
      className="absolute top-0 will-change-transform"
    >
      {renderCardBody(card, copy.website.kindLabels)}
    </div>
  )
}

function renderCardBody(
  card: FallingCard,
  kindLabel: Readonly<Record<SnippetKind, string>>,
): React.ReactElement {
  switch (card.kind) {
    case 'image':
      return (
        <div className="w-[10.5rem] overflow-hidden rounded-md border border-[#E5DFD3] bg-white shadow-[0_8px_18px_rgba(31,27,23,0.10)]">
          <div className="relative h-20 w-full bg-[#F1ECDF]">
            {card.thumbUrl && (
              <img
                src={card.thumbUrl}
                alt=""
                referrerPolicy="no-referrer"
                onError={(e) => {
                  // CORS-blocked → leave the placeholder background.
                  ;(e.currentTarget as HTMLImageElement).style.display =
                    'none'
                }}
                className="h-full w-full object-cover"
              />
            )}
            <span className="absolute left-2 top-2 rounded-full bg-white/90 px-2 py-0.5 font-inter text-[9px] uppercase tracking-[0.14em] text-[#1F1B17]">
              {kindLabel.image}
            </span>
          </div>
          <div className="px-2.5 py-2">
            <p className="truncate font-inter text-[11px] text-[#1F1B17]">
              {card.title}
            </p>
          </div>
        </div>
      )
    case 'file':
      return (
        <div className="flex w-[12rem] items-center gap-2.5 rounded-md border border-[#E5DFD3] bg-white px-3 py-2.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)]">
          <div className="flex h-9 w-7 items-center justify-center rounded-sm bg-[#1F1B17] font-inter text-[9px] font-semibold uppercase tracking-[0.08em] text-white">
            {fileExt(card.contentType)}
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate font-inter text-[10px] uppercase tracking-[0.12em] text-[#A09890]">
              {kindLabel.file}
            </p>
            <p className="truncate font-inter text-[11px] text-[#1F1B17]">
              {card.title}
            </p>
          </div>
        </div>
      )
    case 'link':
      return (
        <div className="flex w-[11rem] items-center gap-2 rounded-full border border-[#E5DFD3] bg-white px-3 py-1.5 shadow-[0_6px_14px_rgba(31,27,23,0.08)]">
          <span className="h-1.5 w-1.5 rounded-full bg-[#34D399]" />
          <span className="truncate font-inter text-[11px] text-[#1F1B17]">
            {card.title}
          </span>
        </div>
      )
    case 'text':
    default:
      return (
        <div className="w-[12.5rem] rounded-md border border-[#E5DFD3] bg-white px-3 py-2.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)]">
          <p className="font-inter text-[10px] uppercase tracking-[0.12em] text-[#A09890]">
            {card.title}
          </p>
          {card.excerpt && (
            <p className="mt-1 line-clamp-2 font-inter text-[11px] leading-4 text-[#1F1B17]">
              {card.excerpt}
            </p>
          )}
        </div>
      )
  }
}

function fileExt(contentType: string | undefined): string {
  const ct = (contentType ?? '').toLowerCase()
  if (ct === 'application/pdf') return 'PDF'
  if (ct.includes('word')) return 'DOC'
  if (ct.includes('excel') || ct.includes('spreadsheet')) return 'XLS'
  if (ct === 'text/csv') return 'CSV'
  return 'FILE'
}

function FolderCard({
  count,
  fillPct,
}: {
  count: number
  fillPct: number
}) {
  const { copy } = useOnboardingCopy()

  return (
    <div className="relative w-[18rem] rounded-2xl border border-[#E5DFD3] bg-white shadow-[0_8px_24px_rgba(31,27,23,0.10)]">
      <div className="absolute -top-3 left-6 h-3 w-20 rounded-t-md border border-b-0 border-[#E5DFD3] bg-white" />
      <div className="px-5 pt-6 pb-4">
        <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
          {copy.website.folder.title}
        </p>
        <p
          className="mt-2 text-[20px] font-normal leading-[1.05] tracking-normal text-[#1F1B17]"
          style={{
            fontFamily: 'var(--font-geist-sans), Arial, sans-serif',
          }}
        >
          {formatOnboardingText(copy.website.folder.gathered, { count })}
        </p>
        <p className="mt-1 font-inter text-[11px] text-[#6B6660]">
          {copy.website.folder.description}
        </p>
      </div>
      <div className="border-t border-[#F1ECDF] px-5 py-3">
        <div className="flex items-center justify-between">
          <span className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
            {copy.website.folder.progress}
          </span>
          <span className="font-inter text-[11px] tabular-nums text-[#1F1B17]">
            {fillPct}%
          </span>
        </div>
        <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-[#F1ECDF]">
          <div
            className="h-full rounded-full bg-[#1F1B17] transition-[width] duration-500"
            style={{ width: `${fillPct}%` }}
          />
        </div>
      </div>
    </div>
  )
}
