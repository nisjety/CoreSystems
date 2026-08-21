import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'
import { VerevonFooterLinks } from '@/shared/ui/verevon/VerevonFooterLinks'

type VerevonScreenProps = {
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

export function VerevonScreen(props: VerevonScreenProps) {
  return (
    <div class={cn(props.rootClass, props.visible && props.visibleClass)} style={props.rootStyle}>
      <div class={props.chromeClass} style={props.chromeStyle}>
        {props.children}
      </div>

      <VerevonFooterLinks links={props.footerLinks} class={props.footerClass} />
    </div>
  )
}
