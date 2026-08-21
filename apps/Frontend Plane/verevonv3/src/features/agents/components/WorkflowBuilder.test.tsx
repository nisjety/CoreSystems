// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { WorkflowBuilder } from '@/features/agents/components/WorkflowBuilder'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'

describe('WorkflowBuilder', () => {
  it('renders the dedicated workflow surface and updates the inspector from canvas selection', async () => {
    window.history.pushState(null, '', '/agents?agent=workflow')

    render(() => (
      <AgentsProvider>
        <WorkflowBuilder />
      </AgentsProvider>
    ))

    expect(screen.getByRole('heading', { name: 'Generer innlegg til sosiale medier' })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: 'Arbeidsflyt-prompt' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Generate Caption' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Velg arbeidsflytnoden Update Status to DONE' }))
    flush()

    expect(screen.getByRole('heading', { name: 'Update Status' })).toBeTruthy()
    expect(screen.getAllByText('Google Sheets').length).toBeGreaterThan(0)
  })

  it('is labelled a design preview with no live execution/publish controls (PR-1 honesty)', () => {
    window.history.pushState(null, '', '/agents?agent=workflow')

    render(() => (
      <AgentsProvider>
        <WorkflowBuilder />
      </AgentsProvider>
    ))

    // Visible "Design preview" label is present (rendered in Norwegian by default).
    expect(screen.getByText('Designforhåndsvisning')).toBeTruthy()

    // The dead Test Run / Publish controls are gone (no backend implied).
    expect(screen.queryByRole('button', { name: 'Test Run' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Publish' })).toBeNull()

    // The prompt composer is inert: no working "Generate" affordance.
    expect((screen.getByRole('textbox', { name: 'Arbeidsflyt-prompt' }) as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: 'Generer arbeidsflyt' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
