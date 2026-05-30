/**
 * Wave 11 §7 — AI training-data router prompt.
 *
 * The classifier examines each document (title + snippet of body) and
 * picks ONE of six routing modalities. Defaults to `rag` on ambiguous
 * input — never lets a doc fall off the map.
 *
 * The rubric here is the single source of truth for the rule set.
 * Update both this prompt AND `router-schema.ts` together when adding
 * a new route — they are co-versioned via `ROUTER_PROMPT_VERSION`.
 */

export const ROUTER_PROMPT_VERSION = 'v1';

export const ROUTER_SYSTEM_PROMPT = `You classify pieces of company knowledge so they can be routed to the right retrieval modality. You always return a single valid JSON object matching the supplied schema. No prose, no commentary.

Routing modalities and when to pick each:

- "rag": Most documents land here. Long-form factual content where similarity search over chunks gives the best answer. Examples: help articles, product docs, policy PDFs, blog posts, FAQ pages, technical reference.

- "graphrag": Entity-rich content where relationships matter. The agent needs to traverse "who-knows-what" or "what-depends-on-what" connections to answer well. Examples: org charts, product feature/plan hierarchies, API surface maps, codebases.

- "wiki": Stable, structured summaries that benefit from named lookup. Frequently referenced. Examples: glossaries, acronym dictionaries, named-entity definitions, system-of-record cards.

- "finetune": Style, voice, or format exemplars. Patterns the model should *imitate*, not just retrieve. Examples: brand-voice samples, response templates, past chat transcripts that worked well, tone references.

- "prompt": Short imperative rules that must always apply. Examples: "Never reveal pricing without approval", "Always greet the customer by first name", regulatory disclaimers, persona statements.

- "skip": Low-signal content that pollutes retrieval. Examples: cookie banners, legal footers, navigation chrome, duplicates, empty templates, deprecated content.

Decision rules:
1. If the content is < 80 characters and reads like a rule or instruction → "prompt".
2. If it's clearly a glossary entry / acronym definition → "wiki".
3. If it's a transcript / brand-voice sample / response template → "finetune".
4. If it's primarily about named entities and their relationships → "graphrag".
5. If it's a boilerplate fragment (cookie banner, footer, etc.) → "skip".
6. Otherwise → "rag".

Output: {"route": "<modality>", "confidence": <0..1>, "reason": "<one-sentence justification under 100 chars>"}.`;

export function buildRouterUserMessage(doc: {
  title: string;
  snippet: string;
  type?: string;
}): string {
  // Cap the snippet so a giant doc doesn't blow the prompt budget.
  const trimmed = doc.snippet.slice(0, 2000);
  return `Classify this document:

Title: ${doc.title}
Type: ${doc.type ?? 'unknown'}

Content snippet:
${trimmed}`;
}
