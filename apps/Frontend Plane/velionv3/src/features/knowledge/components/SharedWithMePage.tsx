import { createResource, For, Show } from 'solid-js'

import { isGateOpen } from '@/shared/context/ownership-gate'
import { listSharedWithMe } from '@/shared/api/ownership-client'

/**
 * Read-only "shared with me" list, paged off the resource_grants ListVisible
 * facade. Gated by `isGateOpen()`: when per-user sharing isn't enforced (or
 * identity isn't live) we show no sharing surface at all — never a fabricated
 * "shared" list. Empty result → honest empty state, not invented rows.
 */
export function SharedWithMePage() {
  const [shared] = createResource(listSharedWithMe)

  return (
    <Show
      when={isGateOpen()}
      fallback={
        <section class="knowledge-shared-with-me">
          <h2>Shared with me</h2>
          <p class="knowledge-muted-copy">
            Per-document sharing isn't enabled in this workspace.
          </p>
        </section>
      }
    >
      <section class="knowledge-shared-with-me">
        <header>
          <h2>Shared with me</h2>
          <p class="knowledge-muted-copy">
            Documents other people have shared with you. View-only.
          </p>
        </header>
        <Show
          when={(shared()?.ids?.length ?? 0) > 0}
          fallback={<p class="knowledge-muted-copy">Nothing shared with you yet.</p>}
        >
          <ul class="knowledge-share-list">
            <For each={shared()?.ids ?? []}>
              {(id) => (
                <li class="knowledge-share-row">
                  <span class="knowledge-share-subject">{id}</span>
                  <span class="knowledge-muted-copy">Can view</span>
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
