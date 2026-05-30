import { z } from 'zod';

/**
 * Wave 11 §7 — AI router output schema. Anything the classifier returns
 * is parsed against this; non-conforming results default to `rag` (the
 * safe fallback) so we never lose a doc to malformed JSON.
 *
 * Co-versioned with `router-prompt.ts` via `ROUTER_PROMPT_VERSION`.
 */

export const ROUTER_ROUTES = [
  'rag',
  'graphrag',
  'wiki',
  'finetune',
  'prompt',
  'skip',
] as const;

export type RouterRoute = (typeof ROUTER_ROUTES)[number];

export const routerOutputSchema = z.object({
  route: z.enum(ROUTER_ROUTES),
  confidence: z.number().min(0).max(1),
  reason: z.string().min(1).max(160),
});

export type RouterOutput = z.infer<typeof routerOutputSchema>;

/**
 * Parse a model response; fall back to a safe `rag` classification
 * when the LLM produced something we can't trust. The caller logs the
 * fallback so we can detect when the prompt drifts.
 */
export function parseRouterOutput(raw: unknown): RouterOutput {
  const parsed = routerOutputSchema.safeParse(raw);
  if (parsed.success) return parsed.data;
  return {
    route: 'rag',
    confidence: 0.5,
    reason: 'Fell back to rag — classifier returned malformed output',
  };
}
