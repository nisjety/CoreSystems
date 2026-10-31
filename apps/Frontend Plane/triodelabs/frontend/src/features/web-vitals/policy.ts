import { publicPage } from "../analytics/consent-tracking";

export const VITAL_LIMITS = { LCP: 120000, INP: 60000, CLS: 100 } as const;
export type VitalsSample = Readonly<{
  sampleId: string;
  sequence: number;
  metric: keyof typeof VITAL_LIMITS;
  value: number;
  viewport: "narrow" | "wide";
  path: string;
}>;

export function vitalsPage(value: string, development: boolean): string | null {
  try {
    const url = new URL(value);
    if (url.search || url.hash || url.username || url.password) return null;
    const local =
      development &&
      ["http://localhost:3000", "http://127.0.0.1:3000", "http://localhost:3001", "http://127.0.0.1:3001"].includes(url.origin);
    return publicPage(local ? `https://www.triodelab.no${url.pathname}` : value)
      ?.location
      ? url.pathname
      : null;
  } catch {
    return null;
  }
}

export function safeVitalsSample(value: unknown): VitalsSample | null {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    return null;
  const v = value as Record<string, unknown>;
  if (
    Object.keys(v).sort().join(",") !==
      "metric,path,sampleId,sequence,value,viewport" ||
    typeof v.sampleId !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      v.sampleId,
    ) ||
    !Number.isSafeInteger(v.sequence) ||
    Number(v.sequence) < 1 ||
    Number(v.sequence) > 32 ||
    typeof v.metric !== "string" ||
    !Object.hasOwn(VITAL_LIMITS, v.metric) ||
    typeof v.value !== "number" ||
    !Number.isFinite(v.value) ||
    v.value < 0 ||
    v.value > VITAL_LIMITS[v.metric as keyof typeof VITAL_LIMITS] ||
    !["narrow", "wide"].includes(v.viewport as string) ||
    typeof v.path !== "string" ||
    v.path.length > 256 ||
    /[?#%\\\s]/.test(v.path) ||
    !v.path.startsWith("/") ||
    vitalsPage(`https://www.triodelab.no${v.path}`, false) !== v.path
  )
    return null;
  return {
    sampleId: v.sampleId,
    sequence: Number(v.sequence),
    metric: v.metric as keyof typeof VITAL_LIMITS,
    value: v.value,
    viewport: v.viewport as "narrow" | "wide",
    path: v.path,
  };
}
