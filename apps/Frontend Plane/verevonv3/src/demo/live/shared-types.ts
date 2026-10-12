/**
 * Shared contracts of the interactive demo (stage 04 of the product loop).
 * Written first so the four parallel builders (engine, home world, demo entry
 * and hosting, site) code against one interface. See
 * apps/verevon-web/plans/product-demo/PLAN_2026-10-11.md.
 *
 * Scene mode and the films do not use this file.
 */

export type DemoRouteMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

export type DemoRouteContext = {
  method: DemoRouteMethod
  /** URL pathname, for example `/api/v1/navbar/search`. Never includes the query string. */
  path: string
  /** Raw query string including the leading `?`, or an empty string. */
  search: string
  url: URL
  /** Parsed JSON body (objects and arrays), the raw string, or undefined when there is none. */
  body: unknown
  headers: Headers
}

/**
 * What a route may return:
 *  - `undefined`: not handled, the interceptor tries the next route.
 *  - a `Response`: sent as is.
 *  - anything else: sent as a 200 JSON body.
 */
export type DemoRouteResult = unknown | Response | undefined

export type DemoRoute = {
  method: DemoRouteMethod
  /** Tested against `context.path` only. */
  match: RegExp
  respond: (context: DemoRouteContext) => DemoRouteResult | Promise<DemoRouteResult>
}

/** One row of `GET /api/v1/chat/threads` (the product's list shape is `{ sessions, retention }`). */
export type ThreadRow = {
  threadId: string
  title: string
  preview: string
  /** ISO 8601. */
  updatedAt: string
  pinned: boolean
  personaEnabled: boolean
}

/**
 * Options of `installDemoLive` (src/demo/live/index.ts, owned by the engine
 * builder, who may extend it). The demo entry builds the home world and passes
 * its routes and thread seed in; the engine owns every `/api/v1/chat/threads*`
 * endpoint, so the home world must NOT register those.
 */
export type DemoLiveOptions = {
  /** The fictional "now" in epoch ms (the demo clock starts here). */
  now: number
  /** Extra routes consulted after the engine's own and before the shared fixtures. */
  routes: DemoRoute[]
  /** Initial recent conversations. The engine may attach transcripts to the scenario threads. */
  threadSeed: ThreadRow[]
}

export type DemoLiveHandle = {
  /** Removes timers, open streams and listeners. */
  dispose(): void
  /** Clears threads, approvals and every demo storage key, as on a fresh visit. */
  reset(): void
  /** Start a scenario by id as if the visitor had picked its suggestion and sent it. */
  startScenario(id: string): void
}

/**
 * Result of `buildHomeWorld(now)` in src/demo/fixtures/fjordform-home.ts
 * (owned by the home-world builder). Pure and deterministic for a given `now`.
 */
export type HomeWorld = {
  /** Every dashboard/navbar/home endpoint except `/api/v1/chat/threads*` (the engine owns those). */
  routes: DemoRoute[]
  threadSeed: ThreadRow[]
  /** Pre-filled browser-storage keys the SPA reads on boot (preferences, session, org). In-memory only. */
  storageSeed: Record<string, string>
}

/** Messages across the iframe boundary, v1. Both directions carry `{ source: 'verevon-demo', v: 1, type }`. */
export type DemoToSiteMessage =
  | { source: 'verevon-demo'; v: 1; type: 'ready' }
  | { source: 'verevon-demo'; v: 1; type: 'escape' }
  | { source: 'verevon-demo'; v: 1; type: 'error'; message: string }

export type SiteToDemoMessage =
  | { source: 'verevon-demo'; v: 1; type: 'activate' }
  | { source: 'verevon-demo'; v: 1; type: 'deactivate' }
  | { source: 'verevon-demo'; v: 1; type: 'pause' }
  | { source: 'verevon-demo'; v: 1; type: 'resume' }
  | { source: 'verevon-demo'; v: 1; type: 'reset' }

/**
 * The fictional clock of the demo: Friday 9 October 2026, 12:00 in the visitor's own time zone (the
 * ISO text has no offset on purpose). The greeting, the day labels and every «i dag klokken 14» then
 * read the same wherever the visitor sits.
 */
export const DEMO_NOW_ISO = '2026-10-09T12:00:00'
