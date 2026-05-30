/**
 * Wave 11.C-a — feature flags for the Logseq-style GraphRAG viewer +
 * LLM Wiki editor. Both surfaces consume D4/D5 Data Plane endpoints,
 * so the flags default OFF and are opt-in per environment.
 *
 * Enable per environment via:
 *   KNOWLEDGE_GRAPH_ENABLED=1   # /knowledge/graph route + sub-nav item
 *   KNOWLEDGE_WIKI_ENABLED=1    # /knowledge/wiki/* routes + sub-nav item
 *
 * Each access re-reads `process.env` so Next.js dev mode picks up
 * `.env.local` edits without a server restart. The first
 * implementation cached the values at module load, which made it
 * impossible to flip flags without `pnpm dev` restart — a Proxy
 * getter wraps `process.env` so HMR is friction-free.
 */

function envFlag(value: string | undefined): boolean {
  if (!value) return false;
  const normalized = value.trim().toLowerCase();
  return normalized === '1' || normalized === 'true' || normalized === 'yes' || normalized === 'on';
}

interface KnowledgeFlags {
  readonly graph: boolean;
  readonly wiki: boolean;
}

/**
 * Public flag object. Property access dispatches a fresh `process.env`
 * read each time — keep this as a Proxy (not a frozen const) so
 * dev-time env changes don't require a server restart.
 */
export const KNOWLEDGE_FEATURE_FLAGS: KnowledgeFlags = new Proxy(
  {} as KnowledgeFlags,
  {
    get(_target, prop): boolean {
      if (prop === 'graph') return envFlag(process.env.KNOWLEDGE_GRAPH_ENABLED);
      if (prop === 'wiki') return envFlag(process.env.KNOWLEDGE_WIKI_ENABLED);
      return false;
    },
  },
);

/**
 * Server-side helper used by feature-gated routes. When the flag is
 * off the route 404s instead of pretending to work — the UI's sub-nav
 * never surfaces the route in that case anyway, this is belt-and-braces
 * for direct URL access.
 */
export function ensureGraphEnabled(): true | { error: string; status: 404 } {
  if (!KNOWLEDGE_FEATURE_FLAGS.graph) {
    return { error: 'GraphRAG viewer is disabled in this environment', status: 404 };
  }
  return true;
}

export function ensureWikiEnabled(): true | { error: string; status: 404 } {
  if (!KNOWLEDGE_FEATURE_FLAGS.wiki) {
    return { error: 'LLM Wiki is disabled in this environment', status: 404 };
  }
  return true;
}
