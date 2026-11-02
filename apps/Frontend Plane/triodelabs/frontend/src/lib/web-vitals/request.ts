import {
  safeVitalsSample,
  type VitalsSample,
} from "../../features/web-vitals/policy";

export class VitalsRequestError extends Error {
  constructor(public readonly status: number) {
    super("invalid_web_vitals_request");
  }
}

// A flat primitive-only envelope lets us reject duplicate JSON keys before
// JSON.parse could silently keep a later value. No DOM/attribution objects fit.
export function parseSampleJSON(raw: string): VitalsSample {
  let index = 0;
  const skip = () => {
    while (/\s/.test(raw[index] ?? "") && index < raw.length) index++;
  };
  const string = (): string => {
    const start = index;
    if (raw[index++] !== '"') throw new VitalsRequestError(400);
    while (index < raw.length) {
      const char = raw[index++];
      if (char === "\\") {
        index++;
        continue;
      }
      if (char === '"') return JSON.parse(raw.slice(start, index)) as string;
    }
    throw new VitalsRequestError(400);
  };
  try {
    skip();
    if (raw[index++] !== "{") throw new VitalsRequestError(400);
    const fields: Record<string, unknown> = Object.create(null);
    skip();
    while (raw[index] !== "}") {
      const key = string();
      skip();
      if (Object.hasOwn(fields, key) || raw[index++] !== ":")
        throw new VitalsRequestError(400);
      skip();
      if (raw[index] === '"') fields[key] = string();
      else {
        const number = raw
          .slice(index)
          .match(/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/);
        if (!number) throw new VitalsRequestError(400);
        fields[key] = Number(number[0]);
        index += number[0].length;
      }
      skip();
      if (raw[index] !== ",") break;
      index++;
      skip();
      if (raw[index] === "}") throw new VitalsRequestError(400);
    }
    if (raw[index++] !== "}") throw new VitalsRequestError(400);
    skip();
    const sample = index === raw.length ? safeVitalsSample(fields) : null;
    if (!sample) throw new VitalsRequestError(400);
    return sample;
  } catch {
    throw new VitalsRequestError(400);
  }
}

export async function readSample(request: Request): Promise<VitalsSample> {
  if (
    request.headers.get("content-type")?.split(";")[0].trim().toLowerCase() !==
    "application/json"
  )
    throw new VitalsRequestError(415);
  const length = request.headers.get("content-length");
  if (length && (!/^[0-9]+$/.test(length) || Number(length) > 2048))
    throw new VitalsRequestError(413);
  const reader = request.body?.getReader();
  if (!reader) throw new VitalsRequestError(400);
  const chunks: Uint8Array[] = [];
  let size = 0;
  const deadline = AbortSignal.timeout(3000);
  const timedOut = new Promise<never>((_, reject) =>
    deadline.addEventListener(
      "abort",
      () => reject(new VitalsRequestError(408)),
      { once: true },
    ),
  );
  try {
    while (true) {
      const chunk = await Promise.race([reader.read(), timedOut]);
      if (chunk.done) break;
      size += chunk.value.byteLength;
      if (size > 2048) throw new VitalsRequestError(413);
      chunks.push(chunk.value);
    }
    const raw = Buffer.concat(chunks.map((chunk) => Buffer.from(chunk)));
    return parseSampleJSON(
      new TextDecoder("utf-8", { fatal: true }).decode(raw),
    );
  } catch (error) {
    void reader.cancel().catch(() => undefined);
    if (error instanceof VitalsRequestError) throw error;
    throw new VitalsRequestError(400);
  } finally {
    reader.releaseLock();
  }
}
