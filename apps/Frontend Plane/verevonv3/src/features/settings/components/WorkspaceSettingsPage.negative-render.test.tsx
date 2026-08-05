// @vitest-environment jsdom
//
// Phase 4 PR-2 negative-render guard. Renders VerevonWorkspaceSettingsPage for
// every workspace settings section and asserts that NONE of the known
// fabricated-posture strings (the removed security toggles / status cards /
// webhook rows) appear in the DOM. Fails if any concrete-false posture string
// is reintroduced without a backing live resource.
import { Route, Router } from '@solidjs/router'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, render, screen } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { workspaceSettingsSections } from '@/features/settings/lib/settings-sections'

// Concrete-false strings that previously rendered a fabricated security /
// integration posture. Any of these appearing without a live producer is a
// GDPR/SOC2 misrepresentation (the PR-2 breach).
// Unique posture strings only — generic labels that legitimately appear
// elsewhere (e.g. TrustCenterSection's "Connected apps" card title) are
// excluded so this guards the removed fabrications, not real content.
const FABRICATED_POSTURE = [
  'aquatiq.no is ready',
  'All new workspace data is stored in EU infrastructure',
  'Default inbox ownership is assigned',
  'Connected apps 2 / 4',
  'Zendesk and Slack are connected',
  'Last workspace sync finished 8 minutes ago',
  'No failed deliveries in the last 24 hours',
  '8 min ago',
  '14 min ago',
  '200 OK',
]

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function renderSection(component: () => JSX.Element) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(() => (
    <QueryClientProvider client={queryClient}>
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/*all" component={component} />
      </Router>
    </QueryClientProvider>
  ))
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('WorkspaceSettingsPage — no fabricated posture renders (Phase 4 PR-2)', () => {
  for (const section of workspaceSettingsSections) {
    it(`renders no fabricated security/integration posture on the "${section.id}" section`, () => {
      // Every endpoint returns an empty object: no live data, so any posture
      // string that still rendered would be a hardcoded fabrication.
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => new Response('{}', { headers: { 'Content-Type': 'application/json' }, status: 200 })),
      )

      renderSection(() => <VerevonWorkspaceSettingsPage section={section.id} />)

      for (const fabricated of FABRICATED_POSTURE) {
        expect(screen.queryAllByText(new RegExp(escapeRegExp(fabricated), 'i'))).toHaveLength(0)
      }
    })
  }
})
