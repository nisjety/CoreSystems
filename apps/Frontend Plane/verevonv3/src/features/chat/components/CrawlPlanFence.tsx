/**
 * The crawl-plan card (E4.3-055c). When the assistant proposes a crawl, execution-core's
 * `knowledge.crawl_plan` emits a closed ```crawl-plan fence. This component turns that fence into the
 * existing page picker in plan mode, so the person sees the proposed pages and scope and approves
 * before anything is crawled.
 *
 * Authority: the fence is model-authored and untrusted (`parseCrawlPlanFence` validates it). Nothing
 * here starts a crawl on its own. The two approval handlers run only on the person's click and call
 * the same gateway routes the dashboard uses, under the person's own token: the selected pages go to
 * `/api/v1/knowledge/crawl` as a batch (`crawlSelectedPages`), the whole scope goes to the same route
 * as a site crawl (`startCrawl`, the `knowledge.crawl_site` contract). The org comes from the verified
 * session context, never from the fence. A typed refusal (zero data retention, approval required, a
 * quota) is shown as text and never hidden. No new action id.
 *
 * With `VITE_CRAWL_PLAN_CARD` off, or a fence that does not parse, the caller's plain code block shows.
 */

import { createMemo, createSignal, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { CrawlPagePicker } from '@/features/dashboard/home/CrawlPagePicker'
import { getSessionContext } from '@/shared/api/auth-client'
import { crawlSelectedPages, startCrawl, type CrawlJob } from '@/shared/api/knowledge-client'
import { resolveCrawlIngest } from '@/shared/api/settings-client'
import { LocalizedError, translateApiError, useI18n } from '@/shared/i18n'
import { crawlPlanCardEnabled, parseCrawlPlanFence, type CrawlPlan } from '../lib/crawl-plan'

export function CrawlPlanFence(props: { text: string; fallback: JSX.Element }) {
  const i18n = useI18n()
  const [dismissed, setDismissed] = createSignal(false)
  const [submitting, setSubmitting] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [started, setStarted] = createSignal<CrawlJob | null>(null)

  // The plan to show, or null for the plain code block (switch off, or a fence that does not parse).
  const plan = createMemo<CrawlPlan | null>(() => {
    if (!crawlPlanCardEnabled()) return null
    const parsed = parseCrawlPlanFence(props.text)
    return parsed.ok ? parsed.plan : null
  })

  // Same org lookup as the dashboard composer: the verified session context, never the fence.
  const resolveOrgId = async (): Promise<string> => {
    const ctx = await getSessionContext()
    const orgId = ctx.orgId ?? ctx.orgs[0]?.id ?? ''
    if (!orgId) {
      throw new LocalizedError({
        no: 'Velg en organisasjon først.',
        en: 'Select an organization first.',
      })
    }
    return orgId
  }

  // Same shared resolver and prompt as KnowledgeComposer's selected-pages crawl.
  const resolveIngest = () =>
    resolveCrawlIngest(() =>
      window.confirm(
        i18n.tr(
          'Lagre disse sidene i kunnskapsbasen? (synlig for organisasjonen din)',
          'Save these pages to the knowledge base? (visible to your organization)',
        ),
      ),
    )

  // Runs the approved crawl; every failure is shown as text under the card.
  const approve = async (start: (orgId: string, ingest: boolean) => Promise<CrawlJob>) => {
    if (submitting()) return
    setError(null)
    setSubmitting(true)
    try {
      const orgId = await resolveOrgId()
      const ingest = await resolveIngest()
      setStarted(await start(orgId, ingest))
    } catch (reason) {
      setError(
        translateApiError(reason, i18n.tr, {
          no: 'Crawl kunne ikke startes.',
          en: 'Crawl could not be started.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  const crawlPages = (urls: string[]) => {
    if (urls.length === 0) return
    void approve((orgId, ingest) => crawlSelectedPages(orgId, urls, ingest))
  }

  const crawlScope = (current: CrawlPlan, scope: CrawlPlan['scope']) => {
    void approve((orgId, ingest) =>
      startCrawl(orgId, {
        url: current.url,
        ingest,
        ...(scope.maxPages === undefined ? {} : { maxPages: scope.maxPages }),
        ...(scope.maxDepth === undefined ? {} : { maxDepth: scope.maxDepth }),
        ...(scope.includePaths === undefined ? {} : { includePaths: scope.includePaths }),
        ...(scope.excludePaths === undefined ? {} : { excludePaths: scope.excludePaths }),
        ...(scope.allowSubdomains === undefined ? {} : { allowSubdomains: scope.allowSubdomains }),
      }),
    )
  }

  return (
    <Show when={plan()} fallback={props.fallback} keyed>
      {(current) => (
        <Show when={!dismissed()}>
          <div class="crawl-plan-fence">
            <Show
              when={started()}
              fallback={
                <CrawlPagePicker
                  discovery={{ url: current.url, pages: current.pages, count: current.pages.length }}
                  submitting={submitting()}
                  onCrawl={crawlPages}
                  onDiscard={() => setDismissed(true)}
                  plan={{ ...(current.goal ? { goal: current.goal } : {}), scope: current.scope }}
                  onCrawlScope={(scope) => crawlScope(current, scope)}
                />
              }
            >
              {(job) => (
                <p class="crawl-plan-fence__started" role="status">
                  {i18n.tr('Crawl startet.', 'Crawl started.')}
                  <Show when={job().id}>{` ${i18n.tr('Kjøring', 'Run')} ${job().id}`}</Show>
                </p>
              )}
            </Show>
            <Show when={error()}>
              {(message) => (
                <p class="crawl-plan-fence__error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
          </div>
        </Show>
      )}
    </Show>
  )
}
