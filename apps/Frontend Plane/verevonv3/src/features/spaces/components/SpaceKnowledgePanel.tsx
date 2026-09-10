import { BookOpen, FileText } from '@/shared/icons'
import { For, Show } from 'solid-js'

import { getSpaceKnowledge, type SpaceKnowledge } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { formatWhenWithTime } from '../lib/space-thread-presentation'

/**
 * What this room knows about.
 *
 * This tab said "Data Plane has not published a Space projection for this yet"
 * from the day the cockpit shipped, and the gap was further upstream than a
 * missing endpoint. `documents-api` verified a Control Space import decision on
 * every create, answered `X-Space-Import-Authority-Accepted: true`, and then
 * discarded the Space — so no document row could say which room it belonged to.
 * There was nothing to list, not a listing nobody had written.
 *
 * # Two sections because there are two mechanisms
 *
 * Documents come from `documents.space_ref`, the edge Data now records from
 * that verified decision. Wiki pages come from the workspace this Space's
 * `space_retrieval_bindings` row names. They fail independently, so each states
 * its own gap beside whatever the other resolved.
 *
 * # The binding is shown, not hidden
 *
 * Which archive a room reads from is the most consequential fact about its
 * answers. A room bound to a workspace nobody expected produces confidently
 * wrong grounding, and that is only visible if the binding is on screen.
 */
export interface SpaceKnowledgePanelProps {
  readonly spaceRef: string
}

function sectionLabel(section: string, tr: (no: string, en: string) => string): string {
  if (section === 'knowledge') return tr('Kunnskap: ', 'Knowledge: ')
  if (section === 'documents') return tr('Dokumenter: ', 'Documents: ')
  if (section === 'wiki_pages') return tr('Wiki: ', 'Wiki: ')
  return `${section}: `
}

/**
 * The gap in the reader's language.
 *
 * Same contract as the Work tab: the server sends a stable `code` beside its
 * English sentence, a known code is translated, and an unknown one falls back
 * to the server's own words — a gap stated in the wrong language beats a gap
 * not stated, and inventing a translation for a reason this build does not know
 * would be guessing at what went wrong.
 */
function gapReason(
  gap: { readonly code?: string; readonly reason: string },
  tr: (no: string, en: string) => string,
): string {
  switch (gap.code) {
    case 'knowledge_read_not_authorized':
      return tr(
        'Du kan ikke lese kunnskapen i dette rommet. Tilgang til kunnskap gis separat fra samtale.',
        'You cannot read this Space’s knowledge. Knowledge access is granted separately from chat.',
      )
    case 'knowledge_binding_unavailable':
      return tr(
        'Rommet er ikke koblet til et kunnskapsarkiv ennå.',
        'This Space is not connected to a knowledge archive yet.',
      )
    case 'knowledge_upstream_unavailable':
      return tr(
        'Kunnskapen i rommet kunne ikke hentes.',
        'This Space’s knowledge could not be loaded.',
      )
    case 'space_binding_has_no_wiki_workspace':
      return tr(
        'Rommets kobling peker ikke på et wiki-arbeidsområde.',
        'This Space’s binding names no wiki workspace.',
      )
    default:
      return gap.reason
  }
}

export function SpaceKnowledgePanel(props: SpaceKnowledgePanelProps) {
  const i18n = useI18n()
  const [knowledge] = createResource(() => props.spaceRef, getSpaceKnowledge)

  const resolved = (): SpaceKnowledge | undefined =>
    knowledge.error ? undefined : knowledge()
  const documents = () => resolved()?.documents ?? []
  const wikiPages = () => resolved()?.wiki_pages ?? []
  const boundWorkspace = () => {
    const workspace = resolved()?.binding?.workspace_id
    return typeof workspace === 'string' && workspace.trim() ? workspace.trim() : undefined
  }
  // "Nothing here" is only sayable when both sections resolved. With a gap
  // present, an empty list means "we could not find out" — a different fact.
  const genuinelyEmpty = () => {
    const current = resolved()
    if (!current) return false
    return (
      current.unavailable.length === 0 &&
      current.documents.length === 0 &&
      current.wiki_pages.length === 0
    )
  }

  return (
    <section class="verevon-space-view" aria-labelledby="space-knowledge-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Kunnskap i rommet', 'Knowledge in this Space')}</p>
          <h2 id="space-knowledge-title">{i18n.tr('Kunnskap', 'Knowledge')}</h2>
          <p>{i18n.tr(
            'Dokumentene og wiki-sidene agenter i dette rommet kan bygge svar på.',
            'The documents and wiki pages agents in this Space can ground answers on.',
          )}</p>
        </div>
      </div>

      <Show when={knowledge.loading}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter kunnskapen i rommet …', 'Loading this Space’s knowledge…')}
        </p>
      </Show>

      {/* A failed read is not an empty archive. */}
      <Show when={knowledge.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Vi fikk ikke hentet kunnskapen i rommet. Arkivet er uendret.',
            'We could not load this Space’s knowledge. The archive itself is unchanged.',
          )}
        </p>
      </Show>

      <For each={resolved()?.unavailable ?? []}>
        {(gap) => (
          <p class="verevon-space-work__gap" role="status">
            {sectionLabel(gap.section, i18n.tr)}
            {gapReason(gap, i18n.tr)}
          </p>
        )}
      </For>

      {/* Which archive this room actually reads from. */}
      <Show when={boundWorkspace()}>
        {(workspace) => (
          <p class="verevon-space-knowledge__binding">
            {i18n.tr('Leser fra arbeidsområdet ', 'Reads from workspace ')}
            <code>{workspace()}</code>
          </p>
        )}
      </Show>

      <Show when={documents().length > 0}>
        <div class="verevon-space-knowledge__section">
          <h3>{i18n.tr('Dokumenter', 'Documents')}</h3>
          <ul class="verevon-space-knowledge__list">
            <For each={documents()}>
              {(document) => (
                <li class="verevon-space-knowledge__item">
                  <FileText size={15} aria-hidden="true" />
                  <div>
                    <p class="verevon-space-knowledge__title">
                      {document.title.trim() ||
                        i18n.tr('Dokument uten tittel', 'Untitled document')}
                    </p>
                    <p class="verevon-space-knowledge__meta">
                      <Show when={document.source}>{(source) => <span>{source()}</span>}</Show>
                      <Show when={document.updated_at}>
                        {(updated) => <span>{formatWhenWithTime(updated())}</span>}
                      </Show>
                      {/* A document still being processed cannot ground an
                          answer yet, and a reader who cannot tell will wonder
                          why the agent ignored it.

                          The ternary is load-bearing: `a && a !== b` yields the
                          BOOLEAN true, and `<Show>`'s callback would hand that
                          to the child, where a boolean renders as nothing. */}
                      <Show
                        when={
                          document.status && document.status !== 'completed'
                            ? document.status
                            : undefined
                        }
                      >
                        {(status) => (
                          <span class="verevon-space-knowledge__status">{status()}</span>
                        )}
                      </Show>
                    </p>
                  </div>
                </li>
              )}
            </For>
          </ul>
          <Show when={resolved()?.documents_truncated}>
            <p class="verevon-space-knowledge__more" role="status">
              {i18n.tr(
                'Dette er de nyeste dokumentene i rommet, ikke hele arkivet.',
                'These are the most recent documents in this Space, not the whole archive.',
              )}
            </p>
          </Show>
        </div>
      </Show>

      <Show when={wikiPages().length > 0}>
        <div class="verevon-space-knowledge__section">
          <h3>{i18n.tr('Wiki-sider', 'Wiki pages')}</h3>
          <ul class="verevon-space-knowledge__list">
            <For each={wikiPages()}>
              {(page) => (
                <li class="verevon-space-knowledge__item">
                  <BookOpen size={15} aria-hidden="true" />
                  <div>
                    <p class="verevon-space-knowledge__title">
                      {page.title.trim() || i18n.tr('Side uten tittel', 'Untitled page')}
                    </p>
                    <p class="verevon-space-knowledge__meta">
                      <Show when={page.path}>{(path) => <span>{path()}</span>}</Show>
                      <Show when={page.updated_at}>
                        {(updated) => <span>{formatWhenWithTime(updated())}</span>}
                      </Show>
                    </p>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>

      <Show when={genuinelyEmpty()}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr(
            'Ingen dokumenter eller wiki-sider er knyttet til dette rommet ennå.',
            'No documents or wiki pages are connected to this Space yet.',
          )}
        </p>
      </Show>
    </section>
  )
}
