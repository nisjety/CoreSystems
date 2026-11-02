import { Globe } from '@/shared/icons'
import { For, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { LinkPreviewsEntry, TurnEntryCardProps } from '../../lib/entry-registry'
import { registerTurnEntryCard } from '../../lib/entry-registry'
import './space-entries.css'

/**
 * The text-only link preview cards of a post (kind `link_previews`). Each card is a snapshot taken
 * when the post was sent: title, description and site name as captured, the host read from the link
 * itself, and a label that says so. No image is drawn or loaded. The link opens only when the shared
 * link rules accepted it (`entry-registry` drops a card whose link they refuse).
 */
export function LinkPreviewCards(props: TurnEntryCardProps<LinkPreviewsEntry>) {
  const i18n = useI18n()
  return (
    <div class="verevon-room-entry verevon-room-entry--previews" data-entry-kind="link_previews">
      <For each={props.entry.items}>
        {(item) => (
          <a
            class="verevon-room-link-preview"
            href={item.href ?? undefined}
            target="_blank"
            rel="noopener noreferrer nofollow"
          >
            <span class="verevon-room-link-preview__host">
              <Globe size={12} aria-hidden="true" />
              <span>{item.siteName || item.host}</span>
              <Show when={item.siteName && item.siteName.toLowerCase() !== item.host}>
                <span class="verevon-room-link-preview__domain">{item.host}</span>
              </Show>
            </span>
            <Show when={item.title}>
              <span class="verevon-room-link-preview__title">{item.title}</span>
            </Show>
            <Show when={item.description}>
              <span class="verevon-room-link-preview__description">{item.description}</span>
            </Show>
            <span class="verevon-room-link-preview__note">
              {i18n.tr('Forhåndsvisning hentet da innlegget ble sendt', 'Preview captured when sent')}
            </span>
          </a>
        )}
      </For>
    </div>
  )
}

registerTurnEntryCard<LinkPreviewsEntry>('link_previews', LinkPreviewCards)
