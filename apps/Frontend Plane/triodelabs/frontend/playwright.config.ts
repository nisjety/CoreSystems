import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.PLAYWRIGHT_PORT ?? 3000);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("PLAYWRIGHT_PORT must be a valid TCP port");
}
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./e2e",
  use: {
    baseURL,
    channel: process.env.PLAYWRIGHT_BROWSER_CHANNEL,
    trace: "on-first-retry",
  },
  webServer: {
    command: `npm run dev -- -p ${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    url: baseURL,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
