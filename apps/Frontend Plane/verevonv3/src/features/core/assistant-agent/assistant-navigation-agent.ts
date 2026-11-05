import { ApiError, requestJson } from '@/shared/api/http'
import { runWebSearch, type WebSearchPayload } from '@/shared/api/search-client'
import { assistantTaskProposalSchema, type AssistantNavigationAction, type AssistantTaskProposal } from '@/shared/actions/assistant-navigation'
import { assistantBrowserTarget } from './assistant-browser-agent'

export type AssistantNavigationAgentState = {
  status: 'idle' | 'thinking' | 'stopped' | 'error'
  error: string | null
  /** The durable navigation run (and its thread) from model-gateway, when known. */
  runId?: string | null
  threadId?: string | null
}

/** The model proposes only. Public URLs must come from a trusted data source. */
export function parseAssistantTaskProposal(text: string): AssistantTaskProposal | null {
  if (text.length > 2048) return null
  try {
    const result = assistantTaskProposalSchema.safeParse(JSON.parse(text))
    return result.success ? result.data : null
  } catch { return null }
}

const publicProviders = new Set(['searxng', 'stract', 'brave', 'serper'])

function publicSearchTargets(search: WebSearchPayload): URL[] {
  if (search.mode !== 'search' || !Array.isArray(search.results)) return []
  const urls: URL[] = []
  for (const hit of search.results) {
    if (!hit || typeof hit.url !== 'string' || !publicProviders.has(hit.provider ?? '')) continue
    const target = assistantBrowserTarget(hit.url)
    if (target) urls.push(target)
  }
  return urls
}

// A named destination in an explicit "go to X and…" instruction constrains
// the search. An unrelated result is not permission to browse another site.
function requestedSiteName(text: string): string | null {
  return /\b(?:gå\s+(?:inn\s+)?(?:på|til)|(?:go|navigate)\s+(?:to|on))\s+([a-z\d][a-z\d-]{1,62})(?=\s+(?:og|and|for\s+å|to)\b)/i
    .exec(text)?.[1]?.toLowerCase() ?? null
}

const siteHost = (url: URL) => url.hostname.replace(/^www\./, '')
const hasDestination = (url: URL) => url.pathname !== '/' || Boolean(url.search)

type Dependencies = {
  infer: (prompt: string, signal: AbortSignal) => Promise<{ content: string; runId?: string | null; threadId?: string | null }>
  search?: (query: string, signal: AbortSignal) => Promise<WebSearchPayload>
}
type Active = { abort: AbortController; scope: string; cancelled: boolean }

const inferNavigation: Dependencies['infer'] = (prompt, signal) => requestJson('/api/v1/ai/assistant-navigation', {
  method: 'POST', body: JSON.stringify({ prompt }), signal,
})

/** One isolated model call per task. Only a completed, schema-validated proposal is dispatched. */
export function createAssistantNavigationAgent(options: {
  scope: () => string | null
  onChange: (state: AssistantNavigationAgentState) => void
  onAction: (action: AssistantNavigationAction) => boolean
  currentBrowserTarget?: (signal: AbortSignal) => string | null | Promise<string | null>
  onBrowserTask?: (goal: string, target: string) => Promise<void>
  tr?: (no: string, en: string) => string
  dependencies?: Dependencies
}) {
  const api: Dependencies = options.dependencies ?? { infer: inferNavigation }
  const search = api.search ?? ((query, signal) => runWebSearch({ query, publicOnly: true, limit: 8 }, signal))
  const tr = options.tr ?? ((no: string) => no)
  let state: AssistantNavigationAgentState = { status: 'idle', error: null }
  let active: Active | null = null
  let generation = 0
  let disposed = false
  const publish = (patch: Partial<AssistantNavigationAgentState>) => {
    state = { ...state, ...patch }
    if (!disposed) options.onChange(state)
  }
  const stop = () => {
    const run = active
    if (!run || run.cancelled) return
    run.cancelled = true
    // Abort the local request and ignore late responses. The model endpoint does
    // not expose a provider-side cancellation receipt.
    run.abort.abort()
    publish({ status: 'stopped', error: null })
  }
  const reset = () => { stop(); generation += 1; active = null; publish({ status: 'idle', error: null }) }
  const dispose = () => { disposed = true; stop(); generation += 1; active = null }

  async function send(text: string): Promise<void> {
    const input = text.trim()
    const scope = options.scope()
    if (disposed || !scope) throw new Error(tr('Ingen innlogget arbeidsflate er tilgjengelig.', 'No authenticated workspace is available.'))
    if (!input || input.length > 2000) throw new Error(tr('Skriv en oppgave på opptil 2000 tegn.', 'Enter a task of at most 2000 characters.'))
    if (active) throw new Error(tr('En oppgave er allerede i gang.', 'A task is already running.'))
    const run: Active = { abort: new AbortController(), scope, cancelled: false }
    active = run
    const mine = ++generation
    const current = () => !disposed && active === run && generation === mine && options.scope() === scope
    const requireCurrent = () => {
      if (!current() || run.cancelled) throw new Error(tr('Oppgaven ble stoppet.', 'The task was stopped.'))
    }
    let handedOff = false
    publish({ status: 'thinking', error: null, runId: null, threadId: null })
    try {
      const result = await api.infer(input, run.abort.signal)
      requireCurrent()
      if (!result || typeof result.content !== 'string') throw new Error(tr('Modellen returnerte et ugyldig svar.', 'The model returned an invalid response.'))
      // The navigation classification is a durable run: surface its ids so the
      // task can be followed in the Model Plane trace and audit.
      if (result.runId || result.threadId) {
        publish({ runId: result.runId ?? null, threadId: result.threadId ?? null })
      }
      const action = parseAssistantTaskProposal(result.content)
      if (!action || action.type === 'unsupported') throw new Error(tr('Hvilken side eller nettside vil du at jeg skal jobbe med?', 'Which app page or website should I work with?'))
      if (action.type === 'assistant.navigation.guide') {
        if (!options.onAction(action)) throw new Error(tr('Siden er ikke tilgjengelig i navigasjonen akkurat nå.', 'That page is not available in navigation right now.'))
        publish({ status: 'idle', error: null })
        return
      }
      if (!options.onBrowserTask) throw new Error(tr('Nettleseragenten er ikke tilgjengelig.', 'The browser agent is unavailable.'))
      const namedSite = action.type === 'assistant.browser.search' ? requestedSiteName(input) : null
      let target: string | null
      if (action.type === 'assistant.browser.current') {
        const address = await options.currentBrowserTarget?.(run.abort.signal)
        target = address ? assistantBrowserTarget(address)?.href ?? null : null
        if (!target) throw new Error(tr('Åpne en side i Nett først, eller si hvilken nettside jeg skal finne.', 'Open a page in Web first, or tell me which website to find.'))
      } else {
        const initialTargets = publicSearchTargets(await search(action.query, run.abort.signal))
        requireCurrent()
        if (!namedSite) {
          target = initialTargets[0]?.href ?? null
        } else {
          const sameName = (url: URL) => siteHost(url).split('.').at(-2) === namedSite
          const matches = initialTargets.filter(sameName)
          const destination = matches.find(hasDestination)
          if (destination) {
            target = destination.href
          } else {
            // Resolve the site's own domain from a public result before
            // querying it with site:. Never construct a URL from the name.
            const home = matches[0] ?? publicSearchTargets(await search(`${namedSite} official website`, run.abort.signal)).find(sameName)
            requireCurrent()
            if (!home) throw new Error(tr('Jeg fant ikke det navngitte nettstedet. Prøv å oppgi nettadressen.', 'I could not find the named website. Try providing its address.'))
            const domain = siteHost(home)
            const scoped = publicSearchTargets(await search(`site:${domain} ${action.query}`, run.abort.signal))
            target = (scoped.find(url => siteHost(url) === domain && hasDestination(url)) ?? home).href
          }
        }
        if (!target) throw new Error(tr('Jeg fant ingen offentlig nettside for oppgaven. Prøv et nettsted eller litt mer informasjon.', 'I found no public website for the task. Try naming a site or adding a detail.'))
      }
      requireCurrent()
      handedOff = true
      const starting = options.onBrowserTask(input, target)
      publish({ status: 'idle', error: null })
      try {
        await starting
      } catch (error) {
        const address = new URL(target)
        if (!namedSite || !hasDestination(address) || !(error instanceof ApiError) ||
          error.status !== 504 || error.code !== 'TIMEOUT' || error.details?.op !== 'navigate') throw error
        // A ranked result can stall before the agent starts. For an explicit
        // named-site task, make one attempt from the same site's homepage.
        requireCurrent()
        await options.onBrowserTask(input, new URL('/', address).href)
      }
    } catch (error) {
      if (!handedOff && current() && !run.cancelled) publish({ status: 'error', error: error instanceof Error ? error.message : tr('Oppgaven kunne ikke utføres.', 'The task could not be completed.') })
      throw error
    } finally {
      if (active === run) active = null
    }
  }
  return { send, stop, reset, dispose, state: () => state }
}
