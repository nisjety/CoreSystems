import type { JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'
import { VelionFooterLinks } from '@/shared/ui/velion/VelionFooterLinks'

type VelionScreenProps = {
  rootClass: string
  chromeClass: string
  footerLinks: readonly string[]
  footerClass?: string
  rootStyle?: JSX.CSSProperties
  chromeStyle?: JSX.CSSProperties
  visible?: boolean
  visibleClass?: string
  children: JSX.Element
}

export function VelionScreen(props: VelionScreenProps) {
  return (
    <div class={cn(props.rootClass, props.visible && props.visibleClass)} style={props.rootStyle}>
      <div class={props.chromeClass} style={props.chromeStyle}>
        {props.children}
      </div>

      <VelionFooterLinks links={props.footerLinks} class={props.footerClass} />
    </div>
  )
}
