import { useSearchParams } from '@solidjs/router'
import { Show } from 'solid-js'
import InboxPage from '@/features/inbox/components/InboxPage'
import TicketingPage from '@/features/tickets/components/TicketingPage'
import SupportOutboundPage from '@/features/support/components/SupportOutboundPage'
import RemoteSupportPage from '@/features/support/components/RemoteSupportPage'
import { SupportTopTabs } from '@/features/support/components/SupportTopTabs'
import { parseSupportSurface } from '@/features/support/lib/support-navigation'

/** One operator workspace: conversations are the communication stream,
 * tickets are the durable work state, and remote support is the live
 * screen-sharing session with a customer's computer. Legacy /inbox and
 * /tickets routes stay available for deep links, while /support is the
 * unified entry point. */
export default function SupportPage() {
  const [searchParams] = useSearchParams()
  const surface = () => parseSupportSurface(searchParams.surface)
  return (
    <div class="verevon-support-workspace">
      <SupportTopTabs active={surface()} />
      <Show when={surface() === 'remote'} fallback={
        <Show when={surface() === 'outbound'} fallback={<Show when={surface() === 'tickets'} fallback={<InboxPage />}><TicketingPage /></Show>}>
          <SupportOutboundPage />
        </Show>
      }>
        <RemoteSupportPage />
      </Show>
    </div>
  )
}
