import { getWebsiteIntakeConfig, usesWebsiteIntake } from "./website-intake";
import { z } from "zod";

const emailSchema = z.string().trim().email();

// Accepts both "post@triodelab.no" and "TriodeLab <post@triodelab.no>".
const senderSchema = z
  .string()
  .trim()
  .refine((value) => {
    const match = value.match(/^[^<>]*<([^<>]+)>$/);
    return emailSchema.safeParse(match ? match[1] : value).success;
  });

const redisSchema = z.object({
  url: z
    .string()
    .trim()
    .url()
    .refine((value) => {
      const url = new URL(value);
      return url.protocol === "https:" && !url.username && !url.password;
    }),
  token: z.string().trim().min(1),
});

const configSchema = z.object({
  apiKey: z.string().trim().min(1),
  from: senderSchema,
  to: emailSchema,
});

export type ContactRedisConfig = z.infer<typeof redisSchema>;
export type ContactConfig = z.infer<typeof configSchema> & {
  // Optional: without Upstash the form still sends, but without rate limiting,
  // retry receipts or lead counts. Resend's idempotency key still prevents
  // duplicate emails for the same submission.
  redis: ContactRedisConfig | null;
};

function env(...names: string[]): string | undefined {
  for (const name of names) {
    const value = process.env[name]?.trim();
    if (value) return value;
  }
  return undefined;
}

function readRedisConfig(): ContactRedisConfig | null {
  const result = redisSchema.safeParse({
    // The Vercel Marketplace integration may expose the KV_* names instead.
    url: env("UPSTASH_REDIS_REST_URL", "KV_REST_API_URL"),
    token: env("UPSTASH_REDIS_REST_TOKEN", "KV_REST_API_TOKEN"),
  });
  return result.success ? result.data : null;
}

// Only server code imports this module. No credentials are sent to the browser.
export function getContactConfig(): ContactConfig | null {
  const result = configSchema.safeParse({
    apiKey: env("RESEND_API_KEY"),
    // Any address on the domain verified in Resend works; this is the default.
    from: env("CONTACT_FROM_EMAIL") ?? "TriodeLab <skjema@triodelab.no>",
    to: env("CONTACT_TO_EMAIL") ?? "post@triodelab.no",
  });
  if (!result.success) {
    // Names only, never values, so the cause is visible in the Vercel logs.
    const invalid = result.error.issues.map((issue) => issue.path.join("."));
    console.error(`contact_config_invalid: ${invalid.join(", ")}`);
    return null;
  }
  return { ...result.data, redis: readRedisConfig() };
}

export function isContactConfigured(): boolean {
  return usesWebsiteIntake()
    ? getWebsiteIntakeConfig() !== null
    : getContactConfig() !== null;
}

// Safe to expose: only reports whether each setting is present, never values.
export function getContactConfigStatus() {
  return {
    configured: isContactConfigured(),
    ...(usesWebsiteIntake() ? { delivery: "leads" } : {}),
    resendApiKey: Boolean(env("RESEND_API_KEY")),
    fromEmail: Boolean(env("CONTACT_FROM_EMAIL")),
    toEmail: Boolean(env("CONTACT_TO_EMAIL")),
    upstash: readRedisConfig() !== null,
  };
}
