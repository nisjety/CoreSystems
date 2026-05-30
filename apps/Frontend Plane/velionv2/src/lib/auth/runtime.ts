import { randomUUID } from "node:crypto";

const devSecret = `${randomUUID()}${randomUUID()}`;

function readOptionalEnv(name: string): string | undefined {
  const value = process.env[name]?.trim();
  return value ? value : undefined;
}

export function getAuthBaseUrl() {
  return (
    readOptionalEnv("BETTER_AUTH_URL") ??
    readOptionalEnv("NEXT_PUBLIC_APP_URL") ??
    "http://localhost:3000"
  );
}

export function getAuthSecret() {
  const configured = readOptionalEnv("BETTER_AUTH_SECRET");
  if (configured) {
    return configured;
  }

  if (process.env.NODE_ENV === "production") {
    if (process.env.NEXT_PHASE === "phase-production-build") {
      return devSecret;
    }
    throw new Error("BETTER_AUTH_SECRET must be configured in production.");
  }

  return devSecret;
}

export function getTrustedOrigins() {
  const configured = readOptionalEnv("BETTER_AUTH_TRUSTED_ORIGINS");
  const values = configured
    ? configured.split(",").flatMap((origin) => {
        const trimmed = origin.trim();
        return trimmed ? [trimmed] : [];
      })
    : [];

  const baseUrl = getAuthBaseUrl();
  return Array.from(new Set([baseUrl, ...values]));
}

export function isSecureCookieRuntime() {
  return process.env.NODE_ENV === "production";
}

export const authRuntimePlan = {
  sessionMaxAgeDays: 7,
  sessionRefreshHours: 12,
  twoFactorChallengeMinutes: 10,
  trustedDeviceDays: 30,
  passwordMinimumLength: 12,
  supportedPrimaryMethods: ["email-password", "microsoft-oauth", "google-oauth", "passkey"],
  supportedSecondFactors: ["totp", "otp", "backup-code"],
} as const;
