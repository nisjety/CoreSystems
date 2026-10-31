import { describe, expect, it } from 'vitest'
import app from './App.tsx?raw'

describe('authenticated Visibility route', () => {
  it('loads the shared feature lazily instead of the demo entry point', () => {
    expect(app).toContain("const VisibilityPage = lazy(() => import('@/features/visibility/components/VisibilityPage'))")
  })

  it('registers Visibility inside the authenticated route group', () => {
    // Routes are declared as createRouter config objects; the authenticated
    // group is the '/' route whose component wraps its children in RequireAuth.
    const guarded = app.slice(
      app.indexOf('<RequireAuth>{props.children}</RequireAuth>'),
      app.indexOf("{ path: '*404'"),
    )
    expect(guarded.length).toBeGreaterThan(0)
    expect(guarded).toContain("{ path: '/engagement/visibility', component: VisibilityPage }")
    for (const view of ['pages', 'groups', 'links', 'competitors']) expect(guarded).toContain(`{ path: '/engagement/visibility/${view}', component: VisibilityPage }`)
    expect(guarded).toContain("{ path: '/engagement', component: EngagementPage }")
    expect(guarded).toContain(`{ path: '/studio/pulse', component: () => <RedirectTo href="/engagement/visibility" /> }`)
    const publicRoutes = app.slice(app.indexOf('routes: ['), app.indexOf('<RequireAuth>{props.children}</RequireAuth>'))
    expect(publicRoutes).not.toContain('/engagement/visibility')
  })
})
