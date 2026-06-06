import { fail } from "@/lib/api/envelope";

const DEFAULT_INFORMATION_CORE_URL = "http://information-core:3190";

export function getInformationCoreUrl() {
  return (
    process.env.INFORMATION_CORE_URL?.trim() ||
    process.env.APPLICATION_INFORMATION_CORE_URL?.trim() ||
    DEFAULT_INFORMATION_CORE_URL
  ).replace(/\/+$/, "");
}

export function buildInformationCoreHeaders() {
  const internalApiKey =
    process.env.INTERNAL_API_KEY?.trim() ||
    process.env.INTEGRATION_INTERNAL_API_KEY?.trim();

  if (!internalApiKey) {
    throw new Error("INTERNAL_API_KEY or INTEGRATION_INTERNAL_API_KEY is required for information-core.");
  }

  return {
    "Content-Type": "application/json",
    "X-Internal-Api-Key": internalApiKey,
  };
}

export function mapInformationCoreError(message: string, fallbackCode: string) {
  return fail({
    code: fallbackCode,
    message: message || "Information service is unavailable.",
  });
}

