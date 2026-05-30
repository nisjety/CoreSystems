/**
 * Analytics Service
 * Tracks user onboarding flow, completion rates, and drop-off points
 * Stores events locally and syncs to backend when possible
 */

interface OnboardingEvent {
  event_type: 'step_entered' | 'step_completed' | 'step_skipped' | 'step_error' | 'onboarding_started' | 'onboarding_completed'
  step_name: 'profile' | 'organization' | 'website' | 'connect' | 'team' | 'complete' | null
  user_id?: string
  org_id?: string
  timestamp: string
  duration_ms?: number
  error_message?: string
  metadata?: Record<string, unknown>
}

interface OnboardingMetrics {
  total_starts: number
  total_completions: number
  completion_rate: number
  average_time_per_step: Record<string, number>
  drop_off_by_step: Record<string, number>
  errors_by_step: Record<string, string[]>
}

class AnalyticsServiceImpl {
  private readonly STORAGE_KEY = 'onboarding_events'
  private readonly METRICS_KEY = 'onboarding_metrics'
  private sessionStartTime: number = Date.now()
  private stepStartTimes: Record<string, number> = {}

  /**
   * Track onboarding step entry
   */
  trackStepEntered(
    stepName: 'profile' | 'organization' | 'website' | 'connect' | 'team' | 'complete',
    userId?: string,
    orgId?: string
  ): void {
    if (typeof window === 'undefined') return

    const event: OnboardingEvent = {
      event_type: 'step_entered',
      step_name: stepName,
      user_id: userId,
      org_id: orgId,
      timestamp: new Date().toISOString(),
    }

    this.stepStartTimes[stepName] = Date.now()
    this._recordEvent(event)

    console.log(`📊 Analytics: Entered step "${stepName}"`)
  }

  /**
   * Track onboarding step completion
   */
  trackStepCompleted(
    stepName: 'profile' | 'organization' | 'website' | 'connect' | 'team' | 'complete',
    userId?: string,
    orgId?: string,
    metadata?: Record<string, unknown>
  ): void {
    if (typeof window === 'undefined') return

    const startTime = this.stepStartTimes[stepName] || Date.now()
    const duration = Date.now() - startTime

    const event: OnboardingEvent = {
      event_type: 'step_completed',
      step_name: stepName,
      user_id: userId,
      org_id: orgId,
      timestamp: new Date().toISOString(),
      duration_ms: duration,
      metadata,
    }

    this._recordEvent(event)
    this._updateMetrics('completion', stepName)

    console.log(`✅ Analytics: Completed step "${stepName}" in ${duration}ms`, metadata)
  }

  /**
   * Track step skip (optional steps like Connect, Team)
   */
  trackStepSkipped(
    stepName: 'connect' | 'team',
    userId?: string,
    orgId?: string,
    reason?: string
  ): void {
    if (typeof window === 'undefined') return

    const event: OnboardingEvent = {
      event_type: 'step_skipped',
      step_name: stepName,
      user_id: userId,
      org_id: orgId,
      timestamp: new Date().toISOString(),
      metadata: { reason },
    }

    this._recordEvent(event)
    this._updateMetrics('skip', stepName)

    console.log(`⏭️  Analytics: Skipped step "${stepName}"`, reason || '')
  }

  /**
   * Track step error / drop-off point
   */
  trackStepError(
    stepName: 'profile' | 'organization' | 'website' | 'connect' | 'team' | 'complete',
    errorMessage: string,
    userId?: string,
    orgId?: string,
    errorDetails?: Record<string, unknown>
  ): void {
    if (typeof window === 'undefined') return

    const event: OnboardingEvent = {
      event_type: 'step_error',
      step_name: stepName,
      user_id: userId,
      org_id: orgId,
      timestamp: new Date().toISOString(),
      error_message: errorMessage,
      metadata: errorDetails,
    }

    this._recordEvent(event)
    this._updateMetrics('error', stepName, errorMessage)

    console.error(`🚨 Analytics: Error in step "${stepName}": ${errorMessage}`, errorDetails)
  }

  /**
   * Track onboarding flow start (after OAuth, before Step 1)
   */
  trackOnboardingStarted(userId: string): void {
    if (typeof window === 'undefined') return

    this.sessionStartTime = Date.now()

    const event: OnboardingEvent = {
      event_type: 'onboarding_started',
      step_name: null,
      user_id: userId,
      timestamp: new Date().toISOString(),
    }

    this._recordEvent(event)

    console.log(`🚀 Analytics: Onboarding started for user ${userId}`)
  }

  /**
   * Track full onboarding completion
   */
  trackOnboardingCompleted(
    userId: string,
    orgId: string,
    metadata?: Record<string, unknown>
  ): void {
    if (typeof window === 'undefined') return

    const totalDuration = Date.now() - this.sessionStartTime

    const event: OnboardingEvent = {
      event_type: 'onboarding_completed',
      step_name: null,
      user_id: userId,
      org_id: orgId,
      timestamp: new Date().toISOString(),
      duration_ms: totalDuration,
      metadata,
    }

    this._recordEvent(event)

    console.log(`🎉 Analytics: Onboarding completed in ${totalDuration}ms`, metadata)
  }

  /**
   * Get current session metrics
   */
  getMetrics(): OnboardingMetrics {
    if (typeof window === 'undefined') {
      return this._emptyMetrics()
    }

    try {
      const stored = localStorage.getItem(this.METRICS_KEY)
      return stored ? JSON.parse(stored) : this._emptyMetrics()
    } catch {
      return this._emptyMetrics()
    }
  }

  /**
   * Get all recorded events
   */
  getEvents(): OnboardingEvent[] {
    if (typeof window === 'undefined') return []

    try {
      const stored = localStorage.getItem(this.STORAGE_KEY)
      return stored ? JSON.parse(stored) : []
    } catch {
      return []
    }
  }

  /**
   * Export analytics data (for backend sync or debugging)
   */
  exportAnalytics(): {
    events: OnboardingEvent[]
    metrics: OnboardingMetrics
    exported_at: string
  } {
    return {
      events: this.getEvents(),
      metrics: this.getMetrics(),
      exported_at: new Date().toISOString(),
    }
  }

  /**
   * Clear all analytics data
   */
  clearAnalytics(): void {
    if (typeof window === 'undefined') return

    try {
      localStorage.removeItem(this.STORAGE_KEY)
      localStorage.removeItem(this.METRICS_KEY)
      this.stepStartTimes = {}
      console.log('📊 Analytics: Data cleared')
    } catch (error) {
      console.error('Failed to clear analytics:', error)
    }
  }

  /**
   * Sync analytics to backend (optional, for persistent analytics)
   */
  async syncToBackend(endpoint: string = '/api/analytics/onboarding'): Promise<boolean> {
    if (typeof window === 'undefined') return false

    try {
      const data = this.exportAnalytics()

      const response = await fetch(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data),
      })

      if (response.ok) {
        console.log('📤 Analytics synced to backend')
        this.clearAnalytics()
        return true
      } else {
        console.warn('Analytics sync failed:', response.statusText)
        return false
      }
    } catch (error) {
      console.error('Analytics sync error:', error)
      return false
    }
  }

  /**
   * Print analytics summary to console
   */
  printSummary(): void {
    const metrics = this.getMetrics()
    const events = this.getEvents()

    console.group('📊 Onboarding Analytics Summary')
    console.log(`Total Events: ${events.length}`)
    console.log(`Starts: ${metrics.total_starts}`)
    console.log(`Completions: ${metrics.total_completions}`)
    console.log(`Completion Rate: ${(metrics.completion_rate * 100).toFixed(1)}%`)
    console.table(metrics.average_time_per_step)
    console.table(metrics.drop_off_by_step)
    console.groupEnd()
  }

  // === Private Methods ===

  private _recordEvent(event: OnboardingEvent): void {
    try {
      const events = this.getEvents()
      events.push(event)

      // Keep last 1000 events
      if (events.length > 1000) {
        events.splice(0, events.length - 1000)
      }

      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(events))
    } catch (error) {
      console.error('Failed to record analytics event:', error)
    }
  }

  private _updateMetrics(
    action: 'completion' | 'skip' | 'error',
    stepName: string,
    errorInfo?: string
  ): void {
    try {
      const metrics = this.getMetrics()

      if (action === 'completion') {
        // Update completion counts and time per step
        const events = this.getEvents()
        const completionEvents = events.filter(
          (e) => e.event_type === 'step_completed' && e.step_name === stepName
        )

        if (completionEvents.length > 0) {
          const totalTime = completionEvents.reduce((sum, e) => sum + (e.duration_ms || 0), 0)
          metrics.average_time_per_step[stepName] = totalTime / completionEvents.length
        }
      } else if (action === 'skip') {
        // Increment skip count per step
        if (!metrics.drop_off_by_step[`${stepName}_skipped`]) {
          metrics.drop_off_by_step[`${stepName}_skipped`] = 0
        }
        metrics.drop_off_by_step[`${stepName}_skipped`]++
      } else if (action === 'error') {
        // Track errors per step
        if (!metrics.errors_by_step[stepName]) {
          metrics.errors_by_step[stepName] = []
        }
        if (errorInfo && !metrics.errors_by_step[stepName].includes(errorInfo)) {
          metrics.errors_by_step[stepName].push(errorInfo)
        }

        // Increment drop-off count
        if (!metrics.drop_off_by_step[stepName]) {
          metrics.drop_off_by_step[stepName] = 0
        }
        metrics.drop_off_by_step[stepName]++
      }

      // Recalculate completion rate
      const events = this.getEvents()
      metrics.total_starts = events.filter((e) => e.event_type === 'onboarding_started').length
      metrics.total_completions = events.filter(
        (e) => e.event_type === 'onboarding_completed'
      ).length
      metrics.completion_rate = metrics.total_starts > 0
        ? metrics.total_completions / metrics.total_starts
        : 0

      localStorage.setItem(this.METRICS_KEY, JSON.stringify(metrics))
    } catch (error) {
      console.error('Failed to update metrics:', error)
    }
  }

  private _emptyMetrics(): OnboardingMetrics {
    return {
      total_starts: 0,
      total_completions: 0,
      completion_rate: 0,
      average_time_per_step: {},
      drop_off_by_step: {},
      errors_by_step: {},
    }
  }
}

export const analyticsService = new AnalyticsServiceImpl()
