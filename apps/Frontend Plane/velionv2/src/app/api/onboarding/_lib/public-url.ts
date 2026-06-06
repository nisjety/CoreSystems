import { lookup } from "node:dns/promises";
import { isIP } from "node:net";

const DEFAULT_DNS_TIMEOUT_MS = 2_500;

export type PublicHttpUrlValidation =
  | { ok: true; url: string }
  | {
      ok: false;
      code:
        | "invalid_url"
        | "bad_scheme"
        | "no_hostname"
        | "dns_failed"
        | "private_address";
    };

export async function normalizePublicHttpUrl(
  input: string,
  dnsTimeoutMs = DEFAULT_DNS_TIMEOUT_MS,
): Promise<PublicHttpUrlValidation> {
  let candidate = input.trim();
  if (!candidate) return { ok: false, code: "invalid_url" };
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(candidate)) {
    candidate = `https://${candidate}`;
  }

  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return { ok: false, code: "invalid_url" };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { ok: false, code: "bad_scheme" };
  }
  if (!parsed.hostname) {
    return { ok: false, code: "no_hostname" };
  }
  if (isPrivateHostname(parsed.hostname)) {
    return { ok: false, code: "private_address" };
  }

  const host = stripIpv6Brackets(parsed.hostname);
  const ipVersion = isIP(host);
  if (ipVersion > 0) {
    return isPrivateIp(host)
      ? { ok: false, code: "private_address" }
      : { ok: true, url: parsed.toString() };
  }

  try {
    const records = await withTimeout(
      lookup(host, { all: true, verbatim: false }),
      dnsTimeoutMs,
    );
    if (records.length === 0) return { ok: false, code: "dns_failed" };
    if (records.some((record) => isPrivateIp(record.address))) {
      return { ok: false, code: "private_address" };
    }
  } catch {
    return { ok: false, code: "dns_failed" };
  }

  parsed.hash = "";
  return { ok: true, url: parsed.toString() };
}

function isPrivateHostname(hostname: string): boolean {
  const host = stripIpv6Brackets(hostname).toLowerCase();
  return host === "localhost" || host.endsWith(".localhost");
}

function isPrivateIp(address: string): boolean {
  const ip = stripIpv6Brackets(address).toLowerCase();
  if (isIP(ip) === 4) {
    const [a, b] = ip.split(".").map((part) => Number(part));
    return (
      a === 0 ||
      a === 10 ||
      a === 127 ||
      (a === 100 && b >= 64 && b <= 127) ||
      (a === 169 && b === 254) ||
      (a === 172 && b >= 16 && b <= 31) ||
      (a === 192 && b === 168) ||
      (a === 198 && (b === 18 || b === 19)) ||
      a >= 224
    );
  }
  if (isIP(ip) === 6) {
    if (ip === "::1" || ip === "::") return true;
    if (ip.startsWith("fe80:") || ip.startsWith("fc") || ip.startsWith("fd")) {
      return true;
    }
    const mapped = ip.match(/^::ffff:(\d+\.\d+\.\d+\.\d+)$/);
    return mapped ? isPrivateIp(mapped[1]) : false;
  }
  return true;
}

function stripIpv6Brackets(value: string): string {
  return value.replace(/^\[/, "").replace(/\]$/, "");
}

async function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => reject(new Error("timeout")), ms);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}
