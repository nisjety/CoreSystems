import { createHash } from "node:crypto";
import { createClient } from "redis";
import {
  safeVitalsSample,
  type VitalsSample,
} from "../../features/web-vitals/policy";
import type { VitalsConfig } from "./config";

export type StoredSample = VitalsSample & Readonly<{ receivedAt: string }>;
export type RecordResult =
  | "stored"
  | "duplicate"
  | "conflict"
  | "rate_limited"
  | "capacity";
const day = 86400_000;

export function storagePrefix(config: VitalsConfig) {
  const id = createHash("sha256")
    .update(
      JSON.stringify([
        config.websiteId,
        config.websiteRevision,
        config.site,
        config.environment,
        config.reportingOrigin,
      ]),
    )
    .digest("hex");
  return `web-vitals:v1:{${id}}:`;
}
function calendar(now: Date) {
  const start = Date.UTC(
    now.getUTCFullYear(),
    now.getUTCMonth(),
    now.getUTCDate(),
  );
  return Array.from({ length: 7 }, (_, i) =>
    new Date(start - i * day).toISOString().slice(0, 10),
  );
}
export const recordScript = `-- WEB_VITALS_RECORD
local count=redis.call('INCR',KEYS[10])
if count==1 then redis.call('PEXPIREAT',KEYS[10],ARGV[5]) end
if count>120 then return 'rate_limited' end
local incoming=cjson.decode(ARGV[1])
local pageRaw=redis.call('GET',KEYS[9])
local page=nil
if pageRaw then page=cjson.decode(pageRaw)
  if page.path~=incoming.path or page.viewport~=incoming.viewport then return 'conflict' end
end
local oldRaw=redis.call('GET',KEYS[8])
if oldRaw then
  if not page then return redis.error_reply('invalid_vitals_store') end
  local old=cjson.decode(oldRaw)
  if incoming.sequence<old.sequence then return 'duplicate' end
  if incoming.sequence==old.sequence then
    if incoming.value~=old.value then return 'conflict' end
    return 'duplicate'
  end
  incoming.receivedAt=old.receivedAt
end
local total=0
for i=1,7 do total=total+redis.call('SCARD',KEYS[i]) end
if not oldRaw and total>=5000 then return 'capacity' end
if not page then page={path=incoming.path,viewport=incoming.viewport,day=ARGV[3],expiresAt=tonumber(ARGV[4])} end
local index=nil
for i=1,7 do if string.sub(KEYS[i],-10)==page.day then index=KEYS[i] end end
if not index then return redis.error_reply('invalid_vitals_window') end
redis.call('SET',KEYS[8],cjson.encode(incoming),'PXAT',page.expiresAt)
if not pageRaw then redis.call('SET',KEYS[9],cjson.encode(page),'PXAT',page.expiresAt) end
redis.call('SADD',index,ARGV[2])
redis.call('PEXPIREAT',index,page.expiresAt)
return 'stored'`;

export const exportScript = `-- WEB_VITALS_EXPORT
local total=0
for i=1,7 do total=total+redis.call('SCARD',KEYS[i]) end
if total>5000 then return redis.error_reply('invalid_vitals_capacity') end
local result={}
for i=1,7 do
  for _,member in ipairs(redis.call('SMEMBERS',KEYS[i])) do
    table.insert(result,member)
  end
end
return result`;

const exportSamplesScript = `-- WEB_VITALS_EXPORT_SAMPLES
local result={}
for i=1,#KEYS do
  local value=redis.call('GET',KEYS[i])
  if value then table.insert(result,value) end
end
return result`;

async function evaluate(
  config: VitalsConfig,
  script: string,
  keys: string[],
  args: string[],
): Promise<unknown> {
  const client = createClient({
    url: config.redisUrl,
    socket: { connectTimeout: 3000, reconnectStrategy: false },
    disableOfflineQueue: true,
    commandsQueueMaxLength: 16,
  });
  client.on("error", () => undefined); // Never log credentials or remote errors.
  let connectionDeadline: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      client.connect(),
      new Promise<never>((_, reject) => {
        connectionDeadline = setTimeout(
          () => reject(new Error("web_vitals_storage_unavailable")),
          3000,
        );
      }),
    ]);
    if (connectionDeadline) clearTimeout(connectionDeadline);
    return await client
      .withCommandOptions({ abortSignal: AbortSignal.timeout(3000) })
      .eval(script, { keys, arguments: args });
  } finally {
    if (connectionDeadline) clearTimeout(connectionDeadline);
    if (client.isOpen) client.destroy();
  }
}

export async function recordVital(
  config: VitalsConfig,
  sample: VitalsSample,
  now: Date,
): Promise<RecordResult> {
  const prefix = storagePrefix(config);
  const days = calendar(now);
  const expiry = Date.parse(`${days[0]}T00:00:00.000Z`) + 7 * day;
  const member = `${sample.sampleId}:${sample.metric}`;
  const result = await evaluate(
    config,
    recordScript,
    [
      ...days.map((date) => `${prefix}day:${date}`),
      `${prefix}sample:${member}`,
      `${prefix}page:${sample.sampleId}`,
      `${prefix}rate:${Math.floor(now.getTime() / 60000)}`,
    ],
    [
      JSON.stringify({ ...sample, receivedAt: now.toISOString() }),
      member,
      days[0],
      String(expiry),
      String((Math.floor(now.getTime() / 60000) + 1) * 60000),
    ],
  );
  if (
    !["stored", "duplicate", "conflict", "rate_limited", "capacity"].includes(
      String(result),
    )
  )
    throw new Error("web_vitals_storage_unavailable");
  return result as RecordResult;
}

export async function readVitals(
  config: VitalsConfig,
  now: Date,
): Promise<StoredSample[]> {
  const prefix = storagePrefix(config),
    days = calendar(now);
  const members = await evaluate(
    config,
    exportScript,
    days.map((date) => `${prefix}day:${date}`),
    [],
  );
  if (
    !Array.isArray(members) ||
    members.length > 5000 ||
    members.some(
      (member) =>
        typeof member !== "string" ||
        !/^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}:(LCP|INP|CLS)$/.test(
          member,
        ),
    ) ||
    new Set(members).size !== members.length
  )
    throw new Error("web_vitals_storage_unavailable");
  if (members.length === 0) return [];
  const raw = await evaluate(
    config,
    exportSamplesScript,
    members.map((member) => `${prefix}sample:${member}`),
    [now.toISOString()],
  );
  if (!Array.isArray(raw) || raw.length > 5000)
    throw new Error("web_vitals_storage_unavailable");
  const seen = new Set<string>(),
    contexts = new Map<string, string>();
  const earliest = Date.parse(`${days[6]}T00:00:00.000Z`);
  return (
    raw
      .map((value) => {
        if (typeof value !== "string" || Buffer.byteLength(value) > 1024)
          throw new Error("web_vitals_storage_unavailable");
        const parsed = JSON.parse(value) as Record<string, unknown>;
        if (
          typeof parsed !== "object" ||
          parsed === null ||
          Object.keys(parsed).sort().join(",") !==
            "metric,path,receivedAt,sampleId,sequence,value,viewport"
        )
          throw new Error("web_vitals_storage_unavailable");
        const { receivedAt, ...fields } = parsed;
        const safe = safeVitalsSample(fields),
          stamp = typeof receivedAt === "string" ? Date.parse(receivedAt) : NaN;
        if (
          !safe ||
          !Number.isFinite(stamp) ||
          stamp < earliest ||
          new Date(stamp).toISOString() !== receivedAt
        )
          throw new Error("web_vitals_storage_unavailable");
        const identity = `${safe.sampleId}:${safe.metric}`,
          context = `${safe.path}:${safe.viewport}`;
        if (
          seen.has(identity) ||
          (contexts.has(safe.sampleId) &&
            contexts.get(safe.sampleId) !== context)
        )
          throw new Error("web_vitals_storage_unavailable");
        seen.add(identity);
        contexts.set(safe.sampleId, context);
        return { ...safe, receivedAt: receivedAt as string };
      })
      // Validate every stored row before the explicit snapshot cutoff. Concurrent
      // later receipts remain in Redis for the next export, never as future data.
      .filter((sample) => Date.parse(sample.receivedAt) <= now.getTime())
      .sort(
        (a, b) =>
          a.receivedAt.localeCompare(b.receivedAt) ||
          a.sampleId.localeCompare(b.sampleId) ||
          a.metric.localeCompare(b.metric),
      )
  );
}
