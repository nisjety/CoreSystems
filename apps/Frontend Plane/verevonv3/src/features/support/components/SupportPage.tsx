import { useSearchParams } from '@solidjs/router'
import { Match, Switch } from 'solid-js'
import InboxPage from '@/features/inbox/components/InboxPage'
import TicketingPage from '@/features/tickets/components/TicketingPage'
import SupportOutboundPage from '@/features/support/components/SupportOutboundPage'
import RemoteSupportPage from '@/features/support/components/RemoteSupportPage'
import { VerevonDraftsQueue } from '@/features/support/components/VerevonDraftsQueue'
import { SupportTopTabs } from '@/features/support/components/SupportTopTabs'
import { parseSupportSurface } from '@/features/support/lib/support-navigation'

/** One operator workspace: conversations are the communication stream,
 * drafts are what Verevon has proposed and a person has not yet acted on,
 * tickets are the durable work state, and remote support is the live
 * screen-sharing session with a customer's computer. Legacy /inbox and
 * /tickets routes stay available for deep links, while /support is the
 * unified entry point.
 *
 * The surfaces are a flat `Switch` rather than nested `Show` fallbacks: with
 * five of them the fallback chain was four levels deep, and adding one more
 * meant editing the middle of it. */
export default function SupportPage() {
  const [searchParams] = useSearchParams()
  const surface = () => parseSupportSurface(searchParams.surface)
  return (
    <div class="verevon-support-workspace">
      <SupportTopTabs active={surface()} />
      <Switch fallback={<InboxPage />}>
        <Match when={surface() === 'drafts'}><VerevonDraftsQueue /></Match>
        <Match when={surface() === 'tickets'}><TicketingPage /></Match>
        <Match when={surface() === 'outbound'}><SupportOutboundPage /></Match>
        <Match when={surface() === 'remote'}><RemoteSupportPage /></Match>
      </Switch>
    </div>
  )
}
