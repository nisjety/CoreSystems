import { Cookie, Gauge, Globe2, Search } from 'lucide-solid'
import { Match, Switch } from 'solid-js'
import type { PulseServiceKey } from './ConnectedServices'

export function ConnectorMark(props: { provider: PulseServiceKey }) {
  return <span class="pulse-services__brand" aria-hidden="true"><Switch fallback={<Search/>}>
    <Match when={props.provider === 'search_console' || props.provider === 'ga4'}>
      <svg viewBox="0 0 48 48" fill="none"><path fill="#4285F4" d="M43.6 24.5c0-1.4-.1-2.8-.4-4.2H24v8h11c-.5 2.6-2 4.8-4.1 6.3v5.2h6.6c3.9-3.6 6.1-8.9 6.1-15.3Z"/><path fill="#34A853" d="M24 44c5.5 0 10.1-1.8 13.5-4.9l-6.6-5.2c-1.8 1.2-4.1 2-6.9 2-5.3 0-9.8-3.6-11.4-8.4H5.8v5.3A20.4 20.4 0 0 0 24 44Z"/><path fill="#FBBC05" d="M12.6 27.5a12.3 12.3 0 0 1 0-7V15H5.8a20.4 20.4 0 0 0 0 18l6.8-5.5Z"/><path fill="#EA4335" d="M24 12.1c3 0 5.7 1 7.8 3l5.9-5.9A19.6 19.6 0 0 0 24 4 20.4 20.4 0 0 0 5.8 15l6.8 5.5C14.2 15.6 18.7 12.1 24 12.1Z"/></svg>
    </Match>
    <Match when={props.provider === 'clarity'}><svg viewBox="0 0 48 48"><path fill="#F25022" d="M4 4h19v19H4z"/><path fill="#7FBA00" d="M25 4h19v19H25z"/><path fill="#00A4EF" d="M4 25h19v19H4z"/><path fill="#FFB900" d="M25 25h19v19H25z"/></svg></Match>
    <Match when={props.provider === 'website'}><Globe2/></Match>
    <Match when={props.provider === 'web_vitals'}><Gauge/></Match>
    <Match when={props.provider === 'cookiebot'}><Cookie/></Match>
  </Switch></span>
}
