import { useSearchParams } from '@solidjs/router'
import { Show } from 'solid-js'
import InboxPage from '@/features/inbox/components/InboxPage'
import TicketingPage from '@/features/tickets/components/TicketingPage'
import SupportOutboundPage from '@/features/support/components/SupportOutboundPage'
import { SupportTopTabs } from '@/features/support/components/SupportTopTabs'
import { parseSupportSurface } from '@/features/support/lib/support-navigation'

/** One operator workspace: conversations are the communication stream and
 * tickets are the durable work state. Legacy /inbox and /tickets routes stay
 * available for deep links, while /support is the unified entry point. */
export default function SupportPage() {
  const [searchParams] = useSearchParams()
  const surface = () => parseSupportSurface(searchParams.surface)
  return (
    <div class="verevon-support-workspace">
      <SupportTopTabs active={surface()} />
      <Show when={surface() === 'outbound'} fallback={<Show when={surface() === 'tickets'} fallback={<InboxPage />}><TicketingPage /></Show>}>
        <SupportOutboundPage />
      </Show>
    </div>
  )
}
