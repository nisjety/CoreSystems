/**
 * Data Plane contracts barrel.
 *
 * Primary: `d4d5.ts` — Data Plane v2 (Rust + Go), mirrors
 * `apps/Data Plane v2/openapi/d4d5.yaml`. New code should import from here.
 *
 * Legacy: `graph_v1.ts` and `wiki_v1.ts` carry v1 shapes; they remain
 * exported so existing callers compile, but are DEPRECATED — migrate to
 * the d4d5 shapes when touching that code.
 */
export * from './d4d5';
export * from './graph_v1';
export * from './wiki_v1';

// `WikiPage` and `WikiPageVersion` are declared in both ./d4d5 (canonical v2)
// and ./wiki_v1 (legacy). Re-export the d4d5 definitions explicitly so the
// ambiguous `export *` re-exports resolve in favour of the primary source.
export type { WikiPage, WikiPageVersion } from './d4d5';
