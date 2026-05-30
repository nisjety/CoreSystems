import { clamp } from "@/lib/utils";

export interface CapabilityScore {
  name: string;
  current: number;
  target: number;
}

export function computeReadinessScore(scores: CapabilityScore[]) {
  if (scores.length === 0) return 0;

  const total = scores.reduce((sum, score) => {
    const normalized = score.target === 0 ? 0 : score.current / score.target;
    return sum + clamp(normalized, 0, 1);
  }, 0);

  return Math.round((total / scores.length) * 100);
}
