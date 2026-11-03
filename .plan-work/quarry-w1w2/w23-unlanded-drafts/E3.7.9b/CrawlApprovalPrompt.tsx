import { Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { localeIntl } from '@/shared/i18n/locales'
import {
  approvalTargetSummary,
  type ApprovalCardModel,
  type ApprovalDecisionRequest,
} from '@/features/browser/approval/approval-model'
import { BrowserApprovalDecisionCard } from '@/features/browser/approval/BrowserApprovalDecisionCard'

/**
 * Inline approval prompt for a knowledge crawl that waits for the person (W3-E3.7.9b).
 *
 * A crawl above the page threshold needs approval (owner-delegated decision of
 * 2026-10-23: above 50 pages; the gateway's CRAWL_APPROVAL_PAGES is tighten-only,
 * default 50). This prompt sits inline in the crawl composer or the chat. It shows
 * the page estimate and the price band, and decides the pending approval with the
 * E3.2-b card (BrowserApprovalDecisionCard): same keys, same decision request, same
 * orchestration decide endpoint.
 *
 * The prompt starts and drops nothing itself. Approve and reject both come back
 * through `onDecided`: the caller starts the crawl on approve, drops it on reject,
 * and removes the prompt.
 *
 * It never prices a crawl. The page count and the price band come from the gateway
 * (the job price estimate, quarry JobPrice in micro-USD). A missing band reads as
 * "not ready yet"; the SPA never fills it in from rates of its own.
 *
 * Ships dark: it renders nothing unless VITE_KNOWLEDGE_CRAWL_APPROVAL_ENABLED is "true".
 */

/** Pages above which a crawl needs approval. The estimate may carry the gateway's own threshold. */
export const CRAWL_APPROVAL_PAGES_DEFAULT = 50

/** Micro-USD per US dollar (quarry JobPrice units). */
const MICRO_USD_PER_USD = 1_000_000

/** The price band of the job price estimate. The server sets both ends. */
export type CrawlPriceBand = {
  lowMicroUsd: number
  highMicroUsd: number
}

/** The page estimate and price band the person is asked to approve. */
export type CrawlApprovalEstimate = {
  /** Pages the crawl would fetch. */
  pages: number
  /** True when a ceiling was applied, so the estimate covers fewer pages than the request asked for. */
  clamped?: boolean
  /** Pages above which approval is needed. Absent: CRAWL_APPROVAL_PAGES_DEFAULT. */
  approvalThreshold?: number
  /** Absent or null: the prompt says the price is not ready yet. */
  priceBand?: CrawlPriceBand | null
}

/** The pending crawl approval. */
export type CrawlApprovalRequest = {
  /** The orchestration approval id the decision goes to. Blank, or containing whitespace: no prompt. */
  approvalId: string
  /** The crawl's seed URL. Only its origin and path are shown; the query and fragment are dropped. */
  seedUrl: string
}

/** Build flag. Off unless VITE_KNOWLEDGE_CRAWL_APPROVAL_ENABLED is "true". */
export function crawlApprovalPromptEnabled(): boolean {
  return (import.meta.env as Record<string, string | undefined>).VITE_KNOWLEDGE_CRAWL_APPROVAL_ENABLED === 'true'
}

/** Whole, non-negative page count. Anything else reads as zero. */
export function crawlPageCount(pages: number): number {
  return Number.isFinite(pages) && pages > 0 ? Math.floor(pages) : 0
}

/** Two finite, non-negative micro-USD amounts with the high end at or above the low end. */
export function isCrawlPriceBand(band: CrawlPriceBand | null | undefined): band is CrawlPriceBand {
  return (
    band !== null &&
    band !== undefined &&
    Number.isFinite(band.lowMicroUsd) &&
    Number.isFinite(band.highMicroUsd) &&
    band.lowMicroUsd >= 0 &&
    band.highMicroUsd >= band.lowMicroUsd
  )
}

/** The card puts the approval id into a DOM id, so the decision needs an id without whitespace. */
export function isDecidableApprovalId(approvalId: string): boolean {
  const id = approvalId.trim()
  return id.length > 0 && !/\s/.test(id)
}

/** Origin and path of the seed URL. The query and fragment are dropped before anything is shown. */
export function crawlTargetSummary(seedUrl: string): string {
  const withoutQuery = seedUrl.split(/[?#]/)[0] ?? ''
  return approvalTargetSummary(withoutQuery, '')
}

/** The host alone, for the heading. */
function seedHost(seedUrl: string): string {
  try {
    return new URL(seedUrl.trim()).host
  } catch {
    return crawlTargetSummary(seedUrl).split('/')[0] ?? ''
  }
}

function usdAmount(microUsd: number, intlLocale: string): string {
  return new Intl.NumberFormat(intlLocale, {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 4,
  }).format(microUsd / MICRO_USD_PER_USD)
}

/** One amount when the band is flat, otherwise "low – high". Pass only a band that passed isCrawlPriceBand. */
export function crawlPriceRangeText(band: CrawlPriceBand, intlLocale: string): string {
  const low = usdAmount(band.lowMicroUsd, intlLocale)
  const high = usdAmount(band.highMicroUsd, intlLocale)
  return low === high ? low : `${low} – ${high}`
}

/**
 * The crawl approval prompt. Renders the page estimate, the price band and the
 * E3.2-b decision card for one pending crawl approval.
 */
export function CrawlApprovalPrompt(props: {
  request: CrawlApprovalRequest
  estimate: CrawlApprovalEstimate
  /** Sends the decision. Defaults to the orchestration client, as the card does. */
  decide?: (request: ApprovalDecisionRequest) => Promise<unknown>
  /**
   * Called after the decision landed. Approve: the caller starts the crawl. Reject:
   * the caller drops it. The caller then removes the prompt.
   */
  onDecided?: (decision: ApprovalDecisionRequest) => void
}) {
  const i18n = useI18n()
  const intlLocale = () => localeIntl(i18n.locale())
  const count = (value: number) => new Intl.NumberFormat(intlLocale()).format(value)
  const pages = () => crawlPageCount(props.estimate.pages)
  const threshold = () => {
    const value = props.estimate.approvalThreshold
    return value !== undefined && Number.isFinite(value) && value >= 0
      ? Math.floor(value)
      : CRAWL_APPROVAL_PAGES_DEFAULT
  }
  const overThreshold = () => pages() > threshold()
  const visible = () => crawlApprovalPromptEnabled() && isDecidableApprovalId(props.request.approvalId)

  const pagesLine = () => {
    const n = count(pages())
    if (props.estimate.clamped === true) {
      return i18n.tr(
        `Estimatet gjelder de første ${n} sidene. Crawlen kan hente flere.`,
        `The estimate covers the first ${n} pages. The crawl may fetch more.`,
      )
    }
    return pages() === 1
      ? i18n.tr(`Ca. ${n} side.`, `About ${n} page.`)
      : i18n.tr(`Ca. ${n} sider.`, `About ${n} pages.`)
  }

  const priceLine = () => {
    const band = props.estimate.priceBand
    if (!isCrawlPriceBand(band)) {
      return i18n.tr('Prisestimatet er ikke klart ennå.', 'The price estimate is not ready yet.')
    }
    const range = crawlPriceRangeText(band, intlLocale())
    return i18n.tr(
      `Estimert pris: ${range} (estimat, ikke faktura).`,
      `Estimated price: ${range} (an estimate, not an invoice).`,
    )
  }

  const reasonText = () => {
    if (!overThreshold()) {
      return i18n.tr('Crawlen må godkjennes før den starter.', 'This crawl needs approval before it starts.')
    }
    const n = count(pages())
    const limit = count(threshold())
    return i18n.tr(
      `Crawlen henter ca. ${n} sider, over grensen på ${limit} sider. Den starter først når den er godkjent.`,
      `The crawl fetches about ${n} pages, over the limit of ${limit} pages. It starts only once it is approved.`,
    )
  }

  // source 'run': the approval comes from an orchestration run (the same queue the chat
  // deck reads), so the card's decide endpoint is the run approval endpoint.
  const model = (): ApprovalCardModel => ({
    approvalId: props.request.approvalId.trim(),
    source: 'run',
    reason: reasonText(),
    actionType: i18n.tr('Crawl av nettsted', 'Website crawl'),
    target: crawlTargetSummary(props.request.seedUrl),
    riskCategory: '',
  })

  return (
    <Show when={visible()}>
      <section
        class={['crawl-page-picker', 'crawl-approval-prompt']}
        aria-label={i18n.tr('Godkjenn crawl', 'Approve crawl')}
      >
        <div class="crawl-page-picker__head">
          <div class="crawl-page-picker__heading">
            <span class="crawl-page-picker__tag">{i18n.tr('Krever godkjenning', 'Approval needed')}</span>
            <p class="crawl-page-picker__title">{seedHost(props.request.seedUrl)}</p>
            <span class="crawl-page-picker__found">{pagesLine()}</span>
          </div>
          <span class={['crawl-page-picker__stat', { 'crawl-page-picker__stat--over': overThreshold() }]}>
            {overThreshold()
              ? i18n.tr(`Over ${count(threshold())} sider`, `Over ${count(threshold())} pages`)
              : i18n.tr(`Innenfor ${count(threshold())} sider`, `Within ${count(threshold())} pages`)}
          </span>
        </div>
        <p class="crawl-page-picker__found">{priceLine()}</p>
        <BrowserApprovalDecisionCard
          approval={model()}
          allowRedirect={false}
          decide={props.decide}
          onDecided={props.onDecided}
        />
      </section>
    </Show>
  )
}
