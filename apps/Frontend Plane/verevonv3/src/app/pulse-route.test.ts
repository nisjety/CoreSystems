import { describe, expect, it } from 'vitest'
import app from './App.tsx?raw'

describe('authenticated Visibility route', () => {
  it('loads the shared feature lazily instead of the demo entry point', () => {
    expect(app).toContain("const VisibilityPage = lazy(() => import('@/features/visibility/components/VisibilityPage'))")
  })

  it('registers Visibility inside the authenticated route group', () => {
    const guarded = app.slice(
      app.indexOf('component={(props: RouteSectionProps) => <RequireAuth>'),
      app.indexOf('<Route path="*404"'),
    )
    expect(guarded).toContain('<Route path="/engagement/visibility" component={VisibilityPage} />')
    for (const view of ['pages', 'groups', 'links', 'competitors']) expect(guarded).toContain(`<Route path="/engagement/visibility/${view}" component={VisibilityPage} />`)
    expect(guarded).toContain('<Route path="/engagement" component={EngagementPage} />')
    expect(guarded).toContain('<Route path="/studio/pulse" component={() => <Navigate href="/engagement/visibility" />} />')
    const publicRoutes = app.slice(app.indexOf('<Router root={AppShell}>'), app.indexOf('<RequireOnboarding>'))
    expect(publicRoutes).not.toContain('/engagement/visibility')
  })
})
