import { vi } from "vitest";

export function configuredEnvironment() {
  const values = {
    NODE_ENV: "development",
    WEB_VITALS_ENABLED: "true",
    WEB_VITALS_WEBSITE_ID: "web_12345678901234567890123456789012",
    WEB_VITALS_WEBSITE_REVISION: "7",
    WEB_VITALS_SITE: "https://triodelab.no/",
    WEB_VITALS_ENVIRONMENT: "development",
    WEB_VITALS_REPORTING_ORIGIN: "http://localhost:3000",
    WEB_VITALS_REDIS_URL: "redis://localhost:6379/15",
    WEB_VITALS_REDIS_URL_FILE: "",
    WEB_VITALS_EXPORT_TOKEN: "t".repeat(43),
    WEB_VITALS_EXPORT_TOKEN_FILE: "",
  };
  for (const [name, value] of Object.entries(values)) vi.stubEnv(name, value);
}
