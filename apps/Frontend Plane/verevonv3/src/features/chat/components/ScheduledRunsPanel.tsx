import { useI18n } from '@/shared/i18n'
import { AutomationsList } from '@/features/automations/components/AutomationsList'

/**
 * "Planlagte kjøringer" in chat (chat-wave2 "schedule" spec §2.1). A compact mount of the
 * automations list over the signed-in person's own schedules (EP-6-S4): the first five rows,
 * and a link to the full hub. Chat and `/agents/automations` share one list, so the WS20
 * guards (renewal on a lapsed approval, no bare re-enable, the results flag) hold in both.
 */
export function ScheduledRunsPanel() {
  const i18n = useI18n()

  return (
    <div class="verevon-scheduled-runs-panel">
      <AutomationsList scope="mine" compact limit={5} />
      <a href="/agents/automations" link>
        {i18n.tr('Se alle automatiseringer', 'See all automations')}
      </a>
    </div>
  )
}
