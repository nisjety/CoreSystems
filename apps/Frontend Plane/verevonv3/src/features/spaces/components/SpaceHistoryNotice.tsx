import { History } from '@/shared/icons'

import { useI18n } from '@/shared/i18n'

/**
 * WS04-S11: a room kept in `since_join` mode shows a member the conversations from the moment
 * they joined. Control signs the floor and Session Core applies it; this notice only says so, once,
 * above the posts. It is worded as "may", because the room's mode is a label and the two floor
 * flags (user-core and session-core) can be off, in which case the reader still sees everything.
 * Presentation only: it grants and hides nothing.
 */
export function SpaceHistoryNotice() {
  const i18n = useI18n()
  return (
    <div class="verevon-space-restricted" data-variant="history" role="status">
      <History size={16} aria-hidden="true" />
      <div class="verevon-space-restricted__body">
        <strong>
          {i18n.tr('Dette rommet viser historikk fra du ble med', 'This room shows history from when you joined')}
        </strong>
        <p>
          {i18n.tr(
            'Samtaler fra før du ble med er skjult for deg. Rommets eier kan endre dette.',
            'Conversations from before you joined may be hidden from you. The room owner can change this.',
          )}
        </p>
      </div>
    </div>
  )
}
