import type { Component } from 'solid-js'
import type { LucideProps } from 'lucide-solid'

export type SupportProvider =
  | 'email'
  | 'google'
  | 'microsoft'
  | 'messenger'
  | 'instagram'
  | 'whatsapp'
  | 'threads'
  | 'slack'
  | 'teams'
  | 'discord'
  | 'linkedin'
  | 'x'
  | 'sms'

type ProviderIconProps = LucideProps & { provider: SupportProvider }

/** Compact provider marks for dense Support navigation. Email providers retain
 * their familiar brand construction so operators can distinguish mailboxes at
 * a glance; the remaining channel marks follow the sidebar's ink treatment. */
export function SupportProviderIcon(props: ProviderIconProps) {
  const strokeWidth = () => props.strokeWidth ?? 1.7
  return (
    <svg
      aria-hidden="true"
      class={props.class ? `${props.class} verevon-support-provider-icon` : 'verevon-support-provider-icon'}
      fill="none"
      height={props.size ?? 16}
      stroke="currentColor"
      stroke-linecap="round"
      stroke-linejoin="round"
      stroke-width={strokeWidth()}
      viewBox="0 0 24 24"
      width={props.size ?? 16}
    >
      {props.provider === 'email' && <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="m4 7 8 6 8-6" /></>}
      {props.provider === 'google' && <>
        <path fill="#fff" stroke="none" d="M4.5 5.2h15A1.5 1.5 0 0 1 21 6.7v10.6a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.3V6.7a1.5 1.5 0 0 1 1.5-1.5Z" />
        <path fill="#EA4335" stroke="none" d="M3 6.7 12 13l9-6.3v2.5L12 15.5 3 9.2V6.7Z" />
        <path fill="#4285F4" stroke="none" d="M3 9.2 7 12v6.8H4.5A1.5 1.5 0 0 1 3 17.3V9.2Z" />
        <path fill="#34A853" stroke="none" d="m17 12 4-2.8v8.1a1.5 1.5 0 0 1-1.5 1.5H17V12Z" />
        <path fill="#FBBC04" stroke="none" d="m7 12 5 3.5 5-3.5v6.8H7V12Z" />
      </>}
      {props.provider === 'microsoft' && <>
        <path fill="#0A5CAD" stroke="none" d="M10 6.1h8.4A2.6 2.6 0 0 1 21 8.7v7.6a2.6 2.6 0 0 1-2.6 2.6H10V6.1Z" />
        <path fill="#28A8EA" stroke="none" d="M10 7.3h8.3L20 9.2 15.2 13 10 9.3v-2Z" />
        <path fill="#0078D4" stroke="none" d="M3 5.2h9.2v13.6H3V5.2Z" />
        <path d="M5.5 8.3h2.8c1.2 0 2 .9 2 2.2v3.1c0 1.3-.8 2.2-2 2.2H5.5V8.3Zm1.2 1.1v5.3h1.4c.6 0 1-.4 1-1.1v-3.1c0-.7-.4-1.1-1-1.1H6.7Z" fill="#fff" stroke="none" />
      </>}
      {props.provider === 'messenger' && <><path d="M12 4C7.1 4 3 7.4 3 11.7c0 2.4 1.3 4.5 3.4 5.8L6 21l3.4-2c.8.2 1.7.3 2.6.3 4.9 0 9-3.4 9-7.6S16.9 4 12 4Z" /><path d="m7.5 13 2.7-2.8 2.2 1.9 3.8-2.1-2.7 2.8-2.2-1.9L7.5 13Z" /></>}
      {props.provider === 'instagram' && <><rect x="4" y="4" width="16" height="16" rx="4" /><circle cx="12" cy="12" r="3.5" /><circle cx="17.3" cy="6.8" r=".7" fill="currentColor" stroke="none" /></>}
      {props.provider === 'whatsapp' && <><path d="M12 4a8 8 0 0 0-6.8 12.2L4 20l3.9-1.2A8 8 0 1 0 12 4Z" /><path d="M9.2 8.8c.2-.4.4-.4.7-.4h.5c.2 0 .4.1.5.4l.7 1.5c.1.2.1.4-.1.6l-.5.6c.7 1.1 1.5 1.8 2.7 2.3l.5-.6c.2-.2.4-.2.6-.1l1.5.7c.3.1.4.3.3.6-.2.8-.8 1.4-1.6 1.5-1.1.1-3.2-.9-4.6-2.1-1.4-1.3-2.5-3.3-2.4-4.4.1-.3.2-.5.4-.6Z" /></>}
      {props.provider === 'threads' && <><path d="M12 4.2c4.5 0 7.4 2.7 7.4 7.3 0 5.2-2.7 8.3-7.2 8.3-4.7 0-7.6-3.1-7.6-7.9 0-4.6 2.8-7.7 7.3-7.7 3.2 0 5.5 1.4 6.5 4" /><path d="M8.6 13.6c.4 2 2.1 2.8 3.8 2.3 2.1-.6 3.3-2.4 2.9-4.2-.4-1.7-2.1-2.5-4-2.2-1.9.3-3 1.3-2.8 2.5.2 1.3 1.8 1.8 3.4 1.5 2-.4 3.3-1.7 3.4-3.3" /></>}
      {props.provider === 'slack' && <><path d="M9 3.5a2 2 0 1 0 0 4h2V5.5a2 2 0 0 0-2-2Z" /><path d="M3.5 9a2 2 0 1 0 4 0V7h-2a2 2 0 0 0-2 2Z" /><path d="M15 20.5a2 2 0 1 0 0-4h-2v2a2 2 0 0 0 2 2Z" /><path d="M20.5 15a2 2 0 1 0-4 0v2h2a2 2 0 0 0 2-2Z" /><path d="M13 7h2a2 2 0 1 0 0-4" /><path d="M7 11v2a2 2 0 1 0 4 0v-2" /><path d="M11 17H9a2 2 0 1 0 0 4" /><path d="M17 13v-2a2 2 0 1 0-4 0v2" /></>}
      {props.provider === 'teams' && <><rect x="4" y="5" width="16" height="14" rx="2" /><path d="M8 9h8M12 9v7" /><circle cx="18" cy="5" r="2" /></>}
      {props.provider === 'discord' && <><path d="M7.5 7.2a13 13 0 0 1 9 0c1.2 1.7 1.8 3.8 1.8 6.6-1.3 1-2.6 1.6-4 2l-.9-1.2a6 6 0 0 1-2.8 0l-.9 1.2c-1.4-.4-2.7-1-4-2 0-2.8.6-4.9 1.8-6.6Z" /><circle cx="9.5" cy="11.5" r=".8" /><circle cx="14.5" cy="11.5" r=".8" /></>}
      {props.provider === 'linkedin' && <>
        <rect x="4" y="4" width="16" height="16" rx="2" fill="#0A66C2" stroke="none" />
        <path fill="#fff" stroke="none" d="M8 10h2v7H8v-7Zm1-3a1.2 1.2 0 1 1 0 2.4A1.2 1.2 0 0 1 9 7Zm3 3h1.9v1c.5-.8 1.3-1.3 2.5-1.3 2 0 2.6 1.3 2.6 3.3V17h-2v-3.6c0-1-.2-1.8-1.2-1.8s-1.8.7-1.8 2V17h-2v-7Z" />
      </>}
      {props.provider === 'x' && <><path d="M5 4h4.2l3 4.3L15.8 4H19l-5.3 6.1L19.5 20h-4.2l-3.6-5.1L7.2 20H4l5.3-6.9L5 4Z" /></>}
      {props.provider === 'sms' && <><path d="M5 5h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H11l-4 3v-3H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z" /><path d="M8 11h.01M12 11h.01M16 11h.01" /></>}
    </svg>
  )
}

export function supportProviderIcon(provider: SupportProvider): Component<LucideProps> {
  return (props) => <SupportProviderIcon {...props} provider={provider} />
}
