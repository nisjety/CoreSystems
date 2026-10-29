import { defineConfig, devices } from '@playwright/test'

if (!process.env.REAL_AUTHORITY_FIXTURE_FILE || !process.env.REAL_AUTHORITY_BROWSER_FIXTURE_FILE) {
  throw new Error('real-authority Playwright fixture paths are required')
}

export default defineConfig({
  testDir: 'tests/e2e',
  testMatch: /real-authority-knowledge\.spec\.ts/,
  timeout: 120_000,
  expect: { timeout: 20_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: true,
  retries: 0,
  reporter: [['list']],
  outputDir: process.env.REAL_AUTHORITY_PLAYWRIGHT_OUTPUT_DIR,
  use: {
    baseURL: process.env.REAL_AUTHORITY_BASE_URL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  projects: [
    {
      name: 'real-authority',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
