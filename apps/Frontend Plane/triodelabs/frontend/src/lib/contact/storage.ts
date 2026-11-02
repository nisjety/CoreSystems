import { createHmac } from "node:crypto";
import { isIP } from "node:net";
import { z } from "zod";
import type { ContactRedisConfig } from "./config";
import type { ContactSubmission } from "./request";

const rateScript = `-- CONTACT_RATE_LIMIT
local count = redis.call('INCR', KEYS[1])
if count == 1 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end
if count > tonumber(ARGV[2]) then return {0, redis.call('TTL', KEYS[1])} end
local total = redis.call('INCR', KEYS[2])
if total == 1 then redis.call('EXPIRE', KEYS[2], ARGV[1]) end
if total > tonumber(ARGV[3]) then return {0, redis.call('TTL', KEYS[2])} end
return {1, 0}`;

const leadScript = `-- CONTACT_RECORD_LEAD
if redis.call('EXISTS', KEYS[1]) == 1 then return 0 end
redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
redis.call('HINCRBY', KEYS[2], ARGV[3], 1)
redis.call('HINCRBY', KEYS[2], ARGV[4], 1)
redis.call('EXPIRE', KEYS[2], ARGV[5])
return 1`;

export async function redisCommand(
  config: ContactRedisConfig,
  command: (string | number)[],
): Promise<unknown> {
  const response = await fetch(config.url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${config.token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(command),
    signal: AbortSignal.timeout(4000),
    cache: "no-store",
  });
  if (!response.ok) throw new Error("contact_storage_failed");
  const body: unknown = await response.json();
  const parsed = z
    .object({ result: z.unknown(), error: z.string().optional() })
    .safeParse(body);
  if (!parsed.success || parsed.data.error)
    throw new Error("contact_storage_failed");
  return parsed.data.result;
}

function hash(config: ContactRedisConfig, purpose: string, value: string): string {
  return createHmac("sha256", config.token)
    .update(`${purpose}:${value}`)
    .digest("hex");
}

export function submissionFingerprint(
  config: ContactRedisConfig,
  submission: ContactSubmission,
): string {
  return hash(config, "contact-submission", JSON.stringify(submission));
}

export async function checkRateLimit(
  config: ContactRedisConfig,
  request: Request,
): Promise<number | null> {
  // On Vercel this header is supplied by the trusted edge, not the browser.
  const header =
    process.env.VERCEL === "1"
      ? request.headers.get("x-vercel-forwarded-for")
      : null;
  const ip = header?.trim();
  const identity = ip && isIP(ip) ? ip : "shared-fallback";
  const result = await redisCommand(config, [
    "EVAL",
    rateScript,
    2,
    `contact:rate:${hash(config, "contact-rate", identity)}`,
    "contact:rate:global",
    600,
    5,
    100,
  ]);
  const parsed = z
    .tuple([z.union([z.literal(0), z.literal(1)]), z.number().int()])
    .parse(result);
  return parsed[0] === 1 ? null : Math.max(1, parsed[1]);
}

const receiptSchema = z.object({
  fingerprint: z.string().length(64),
  providerId: z.string().min(1).max(128),
});

export async function getReceipt(config: ContactRedisConfig, submissionId: string) {
  const value = await redisCommand(config, [
    "GET",
    `contact:receipt:${submissionId}`,
  ]);
  if (value === null) return null;
  if (typeof value !== "string") throw new Error("contact_receipt_invalid");
  return receiptSchema.parse(JSON.parse(value));
}

export async function recordLead(
  config: ContactRedisConfig,
  submission: ContactSubmission,
  providerId: string,
): Promise<void> {
  const day = new Date().toLocaleDateString("en-CA", {
    timeZone: "Europe/Oslo",
  });
  const result = await redisCommand(config, [
    "EVAL",
    leadScript,
    2,
    `contact:receipt:${submission.submissionId}`,
    `contact:leads:${day}`,
    JSON.stringify({
      fingerprint: submissionFingerprint(config, submission),
      providerId,
    }),
    48 * 60 * 60,
    `projectType:${submission.projectType}`,
    `source:${submission.source}`,
    400 * 24 * 60 * 60,
  ]);
  if (result !== 0 && result !== 1) throw new Error("contact_metrics_invalid");
}
