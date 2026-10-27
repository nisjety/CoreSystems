import { createEffect, Errored } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { FailureCard } from '@/shared/ui/resilience/FailureCard'
import type { ClientRegion } from '@/shared/ui/resilience/classify-client-error'

export interface RegionBoundaryProps {
  readonly region: ClientRegion
  /**
   * When this value changes, a failed region is reset. Navigation or a Space
   * switch passes its key here so one bad page does not stay failed after the
   * user moves on.
   */
  readonly resetKey?: () => unknown
  readonly compact?: boolean
  readonly children: JSX.Element
}

/**
 * Error boundary with a classified failure card (WS07-S1). A throw inside the
 * region replaces only that region, so a page crash keeps navigation alive.
 */
export function RegionBoundary(props: RegionBoundaryProps) {
  let pendingReset: (() => void) | undefined
  let seeded = false

  createEffect(
    () => props.resetKey?.(),
    () => {
      // The first run only records the starting key; later runs mean it changed.
      if (!seeded) {
        seeded = true
        return
      }
      const reset = pendingReset
      pendingReset = undefined
      reset?.()
    },
  )

  return (
    <Errored
      fallback={(error: unknown, reset: () => void) => {
        pendingReset = reset
        console.error(`[region-boundary:${props.region}]`, error)
        return (
          <FailureCard
            error={error}
            region={props.region}
            compact={props.compact}
            onRetry={() => {
              pendingReset = undefined
              reset()
            }}
          />
        )
      }}
    >
      {props.children}
    </Errored>
  )
}
