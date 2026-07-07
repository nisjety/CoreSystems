import { defineConfig, devices } from '@playwright/test'

/**
 * Authenticated cross-plane E2E harness (audit proof-ladder L4).
 *
 * Runs against the already-running dockerized stack (SPA on :5199, gateway
 * on :3185) — it does NOT boot its own server. Bring the stack up first:
 *   (cd "apps/Frontend Plane/velionv3" && docker compose up -d)
 *   (cd "apps/Control Plane" && docker compose up -d)
 * then, one time: pnpm exec playwright install chromium
 *
 * The `setup` project seeds a verified account + org and saves its session
 * as storageState; the `e2e` project reuses that state so every spec starts
 * signed in.
 */
const BASE_URL = process.env.E2E_BASE_URL || 'http://localhost:5199'
const STORAGE_STATE = 'tests/e2e/.auth/state.json'

export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : [['list']],
  use: {
    baseURL: BASE_URL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'setup', testMatch: /auth\.setup\.ts/ },
    {
      name: 'e2e',
      testMatch: /.*\.spec\.ts/,
      dependencies: ['setup'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
  ],
})
