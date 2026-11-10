import { Show } from 'solid-js'
import { useNavigate, useParams } from '@solidjs/router'
import { ApiError } from '@/shared/api/http'
import { readSharedChatSnapshot } from '@/shared/api/snapshot-client'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { ThreadReplayView } from './ThreadReplayView'
import './SharedSnapshotPage.css'

/**
 * The page a member opens from a shared conversation snapshot, `/chat/shared/:snapshotId` (W4-SH-6, F-t3-046).
 * It shows the owner's redacted copy as static text in the existing replay view: no model runs, no tool or
 * connector is reached, and no widget or app is mounted. A snapshot that is revoked, in another organization or not
 * shared with the reader answers the same way as one that never existed, so this page cannot tell them apart
 * either. The route is mounted only while `VITE_CHAT_SNAPSHOTS` is `true`.
 */
export default function SharedSnapshotPage() {
  const i18n = useI18n()
  const navigate = useNavigate()
  const params = useParams<{ snapshotId: string }>()
  const [shared] = createResource(
    () => params.snapshotId?.trim() || undefined,
    (snapshotId) => readSharedChatSnapshot(snapshotId),
  )
  const unavailable = () => shared.error instanceof ApiError && shared.error.status === 404

  return (
    <main class="verevon-shared-snapshot">
      <header class="verevon-shared-snapshot__header">
        <h1>{i18n.tr('Delt samtale', 'Shared conversation')}</h1>
        <p>
          {i18n.tr(
            'Dette er en redigert kopi som eieren delte med deg. Den kan bare leses.',
            'This is a redacted copy its owner shared with you. It is read only.',
          )}
        </p>
      </header>
      <Show when={shared.loading && shared() === undefined}>
        <p class="verevon-shared-snapshot__status" role="status">
          {i18n.tr('Åpner samtalen …', 'Opening the conversation …')}
        </p>
      </Show>
      <Show when={shared.error}>
        <p class="verevon-shared-snapshot__status" role="alert">
          {unavailable()
            ? i18n.tr(
                'Denne samtalen er ikke tilgjengelig. Den kan være trukket tilbake, eller ikke delt med deg.',
                'This conversation is not available. It may have been revoked, or not shared with you.',
              )
            : translateApiError(shared.error, i18n.tr, {
                no: 'Samtalen kunne ikke åpnes. Prøv igjen.',
                en: 'The conversation could not be opened. Try again.',
              })}
        </p>
      </Show>
      <Show when={shared()}>
        {(snapshot) => (
          <ThreadReplayView conversation={snapshot().conversation} onClose={() => navigate('/chat')} />
        )}
      </Show>
    </main>
  )
}
