// Shared knowledge-filter store. Module-level Solid signals (the established
// module-singleton pattern — see src/shared/context/ownership-gate.ts): the
// left sidebar (CoreSidebarKnowledgePanel) and the /knowledge main pane render
// in separate component trees, so filter intent has to travel through a shared
// store rather than props. KnowledgePage aliases these signals as its single
// source of truth for search filtering; the sidebar writes into them.
import { createSignal } from 'solid-js'

/**
 * Mirrors KnowledgePage's KnowledgeView union so callers can deep-link a tab
 * switch alongside a filter. Kept as its own literal union (structurally
 * identical) because KnowledgePage's type is file-local.
 */
export type KnowledgeRequestedView = 'overview' | 'operating-map' | 'graph' | 'chunks'

/**
 * The free-text query the /knowledge pane filters folders/files/sources/graph
 * by (KnowledgePage's filterKnowledgePayload). Sidebar folder/source/tag
 * clicks push their label here; a second click on the same item clears it.
 */
export const [knowledgeSearchQuery, setKnowledgeSearchQuery] = createSignal('')

/**
 * One-shot view request: KnowledgePage applies it via setActiveView and
 * resets it to null. Null means "no pending request".
 */
export const [knowledgeRequestedView, setKnowledgeRequestedView] = createSignal<KnowledgeRequestedView | null>(null)

/**
 * One-shot request to open the Add Source modal on /knowledge (raised by the
 * sidebar's "Legg til samling" button, possibly before the page mounts).
 * KnowledgePage consumes it: opens the modal and resets the flag.
 */
export const [knowledgeAddSourceRequested, setKnowledgeAddSourceRequested] = createSignal(false)
