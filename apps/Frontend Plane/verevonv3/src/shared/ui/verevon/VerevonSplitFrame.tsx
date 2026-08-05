import { Show, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonSplitFrameProps = {
  rootClass: string
  rootStyle?: JSX.CSSProperties
  leftPaneClass: string
  rightPaneClass: string
  leftHeightClass?: string
  leftHeight?: number
  leftContentClass?: string
  rightContentClass?: string
  leftContentRef?: (element: HTMLDivElement) => void
  leftLeavingClass?: string
  leftEnteringClass?: string
  rightLeavingClass?: string
  rightEnteringClass?: string
  phase?: 'idle' | 'leaving' | 'entering'
  scannerClass?: string
  showScanner?: boolean
  left: JSX.Element
  right: JSX.Element
}

export function VerevonSplitFrame(props: VerevonSplitFrameProps) {
  const leftContent = () => (
    <div
      ref={props.leftContentRef}
      class={cn(
        props.leftContentClass,
        props.phase === 'leaving' && props.leftLeavingClass,
        props.phase === 'entering' && props.leftEnteringClass,
      )}
    >
      {props.left}
    </div>
  )

  const rightContent = () => (
    <div
      class={cn(
        props.rightContentClass,
        props.phase === 'leaving' && props.rightLeavingClass,
        props.phase === 'entering' && props.rightEnteringClass,
      )}
    >
      {props.right}
    </div>
  )

  return (
    <div class={props.rootClass} style={props.rootStyle}>
      <div class={props.leftPaneClass}>
        <Show when={props.leftHeightClass} fallback={leftContent()}>
          {(heightClass) => (
            <div
              class={heightClass()}
              style={{ height: props.leftHeight ? `${props.leftHeight}px` : undefined }}
            >
              {leftContent()}
            </div>
          )}
        </Show>
      </div>

      <div class={props.rightPaneClass}>
        <Show when={props.showScanner && props.scannerClass}>
          <div class={props.scannerClass} />
        </Show>
        {rightContent()}
      </div>
    </div>
  )
}
