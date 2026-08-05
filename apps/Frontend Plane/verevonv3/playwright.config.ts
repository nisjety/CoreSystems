import { defineConfig, devices } from '@playwright/test'

/**
 * Authenticated cross-plane E2E harness (audit proof-ladder L4).
 *
 * Runs against the already-running dockerized stack (SPA on :5199, gateway
 * on :3185) — it does NOT boot its own server. Bring the stack up first:
 *   (cd "apps/Frontend Plane/verevonv3" && docker compose up -d)
 *   (cd "apps/Control Plane" && docker compose up -d)
 * then, one time: pnpm exec playwright install chromium
 *
 * The `setup` project seeds a verified account + org and saves its session
 * as storageState; the `e2e` project reuses that state so every spec starts
 * signed in.
 *
 * The `local-setup`/`local` projects are a second, independent pair targeting
 * the dockerized v3 *dev* stack instead (bind-mounted Vite on :5173, HMR) as
 * `local@verevon.dev` — used by the Phase 6 `browser-workspace-*.spec.ts`
 * family, which needs a real Quarry-org-token-minting account against the
 * source-mounted dev stack specifically. `testMatch`/`testIgnore` below keep
 * the two pairs from picking up each other's spec/setup files.
 *
 * `workers: 1` (all projects, not just `local`): the `browser-workspace-*`
 * specs drive real Chromium sessions through the live Quarry-edge stack, and
 * more than one worker running them concurrently against the SAME external
 * origin (e.g. two specs both using `EXAMPLE_URL`) reliably reproduces a
 * real, pre-existing bug — `KnowledgeComposer.tsx`'s link-mode submit fires
 * a second, *concurrent* `/v1/scrape` render (via `scrapePreview`) alongside
 * the interactive session create, and quarry-edge's scrape/fetch driver
 * pools its browser lease by domain host, not by run id
 * (`BrowserDriverAdapter::do_fetch`, `quarry-runtime/src/browser_driver.rs`).
 * Two concurrent scrape-previews for the same host can poison and evict each
 * other's lease, which was observed to cascade into the *unrelated*
 * interactive session's own live-frame polling failing with sustained 502s
 * until the page closed. That is a genuine backend concurrency bug worth
 * fixing in quarry-edge separately — out of scope for this E2E-coverage
 * pass — so this suite avoids triggering it by never running these specs in
 * parallel, rather than papering over the flakiness with looser assertions.
 */
const BASE_URL = process.env.E2E_BASE_URL || 'http://localhost:5199'
const STORAGE_STATE = 'tests/e2e/.auth/state.json'
const LOCAL_BASE_URL = process.env.LOCAL_E2E_BASE_URL || 'http://localhost:5173'
const LOCAL_STORAGE_STATE = 'tests/e2e/.auth/local-state.json'
const BROWSER_WORKSPACE_SPECS = /browser-workspace-.*\.spec\.ts/

export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : [['list']],
  use: {
    baseURL: BASE_URL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    // testMatch regexes are evaluated against the full file path Playwright
    // resolves for each candidate file, not the testDir-relative basename —
    // an unqualified `^...$` anchor never matches anything (the string
    // always starts with `/`). Anchor on a preceding "/" or start-of-string
    // instead, so `auth.setup.ts` never also matches `local-auth.setup.ts`.
    { name: 'setup', testMatch: /(^|\/)auth\.setup\.ts$/ },
    {
      name: 'e2e',
      testMatch: /.*\.spec\.ts/,
      testIgnore: BROWSER_WORKSPACE_SPECS,
      dependencies: ['setup'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    {
      name: 'local-setup',
      testMatch: /(^|\/)local-auth\.setup\.ts$/,
      use: { baseURL: LOCAL_BASE_URL },
    },
    {
      name: 'local',
      testMatch: BROWSER_WORKSPACE_SPECS,
      dependencies: ['local-setup'],
      use: { ...devices['Desktop Chrome'], baseURL: LOCAL_BASE_URL, storageState: LOCAL_STORAGE_STATE },
    },
  ],
})
