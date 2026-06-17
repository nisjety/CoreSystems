'use client'

import { useEffect, useRef } from 'react'
import { usePathname } from 'next/navigation'
import { useReportWebVitals } from 'next/web-vitals'
import type { NextWebVitalsMetric } from 'next/app'

import { emit } from '@/lib/telemetry/client'

type PerformanceEntryWithAttribution = PerformanceEntry & {
  processingStart?: number
  startTime: number
  duration: number
  name: string
}

export function WebVitalsTelemetry() {
  const pathname = usePathname()
  const routeRef = useRef(pathname)
  useEffect(() => {
    routeRef.current = pathname
  }, [pathname])

  useReportWebVitals((metric: NextWebVitalsMetric) => {
    // Next's `NextWebVitalsMetric` type omits `rating`/`navigationType`, but the
    // object Next forwards from the `web-vitals` library carries them at runtime.
    const m = metric as NextWebVitalsMetric & {
      rating?: 'good' | 'needs-improvement' | 'poor'
      navigationType?: string
    }
    emit('web_vital.recorded', {
      props: {
        route: routeRef.current,
        name: metric.name,
        value: metric.value,
        rating: m.rating,
        id: metric.id,
        navigation_type: m.navigationType,
      },
    })
  })

  useEffect(() => {
    const startedAt = performance.now()
    emit('route.viewed', {
      props: {
        route: pathname,
      },
    })

    return () => {
      emit('route.left', {
        durationMs: Math.round(performance.now() - startedAt),
        props: {
          route: pathname,
        },
      })
    }
  }, [pathname])

  useEffect(() => {
    if (typeof PerformanceObserver === 'undefined') {
      return
    }

    const observers: PerformanceObserver[] = []

    const observe = (type: string, callback: (entry: PerformanceEntryWithAttribution) => void) => {
      try {
        const observer = new PerformanceObserver((list) => {
          for (const entry of list.getEntries() as PerformanceEntryWithAttribution[]) {
            callback(entry)
          }
        })
        observer.observe({ type, buffered: true })
        observers.push(observer)
      } catch {
        // Unsupported entry type in this browser.
      }
    }

    observe('longtask', (entry) => {
      if (entry.duration < 80) return
      emit('performance.long_task', {
        durationMs: Math.round(entry.duration),
        props: {
          route: routeRef.current,
          name: entry.name,
          start_time: Math.round(entry.startTime),
        },
      })
    })

    observe('event', (entry) => {
      if (entry.duration < 120) return
      emit('performance.event_timing', {
        durationMs: Math.round(entry.duration),
        props: {
          route: routeRef.current,
          name: entry.name,
          processing_start: Math.round(entry.processingStart ?? entry.startTime),
        },
      })
    })

    return () => {
      observers.forEach((observer) => observer.disconnect())
    }
  }, [])

  return null
}
