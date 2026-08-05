import { createResource, For, Show } from 'solid-js'

import { isGateOpen } from '@/shared/context/ownership-gate'
import { listSharedWithMe } from '@/shared/api/ownership-client'
import { useI18n } from '@/shared/i18n'

/**
 * Read-only "shared with me" list, paged off the resource_grants ListVisible
 * facade. Gated by `isGateOpen()`: when per-user sharing isn't enforced (or
 * identity isn't live) we show no sharing surface at all — never a fabricated
 * "shared" list. Empty result → honest empty state, not invented rows.
 */
export function SharedWithMePage() {
  const i18n = useI18n()
  const [shared] = createResource(listSharedWithMe)

  return (
    <Show
      when={isGateOpen()}
      fallback={
        <section class="knowledge-shared-with-me">
          <h2>{i18n.tr('Delt med meg', 'Shared with me')}</h2>
          <p class="knowledge-muted-copy">
            {i18n.tr('Deling per dokument er ikke aktivert i dette arbeidsområdet.', "Per-document sharing isn't enabled in this workspace.")}
          </p>
        </section>
      }
    >
      <section class="knowledge-shared-with-me">
        <header>
          <h2>{i18n.tr('Delt med meg', 'Shared with me')}</h2>
          <p class="knowledge-muted-copy">
            {i18n.tr('Dokumenter andre har delt med deg. Kun visning.', 'Documents other people have shared with you. View-only.')}
          </p>
        </header>
        <Show
          when={(shared()?.ids?.length ?? 0) > 0}
          fallback={<p class="knowledge-muted-copy">{i18n.tr('Ingenting delt med deg ennå.', 'Nothing shared with you yet.')}</p>}
        >
          <ul class="knowledge-share-list">
            <For each={shared()?.ids ?? []}>
              {(id) => (
                <li class="knowledge-share-row">
                  <span class="knowledge-share-subject">{id}</span>
                  <span class="knowledge-muted-copy">{i18n.tr('Kan vise', 'Can view')}</span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </section>
    </Show>
  )
}

export default SharedWithMePage
