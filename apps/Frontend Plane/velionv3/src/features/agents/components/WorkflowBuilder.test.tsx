// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
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

    expect(screen.getByRole('heading', { name: 'Generate Social Media Post' })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: 'Workflow prompt' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Generate Caption' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Select Update Status to DONE workflow node' }))

    expect(screen.getByRole('heading', { name: 'Update Status' })).toBeTruthy()
    expect(screen.getAllByText('Google Sheets').length).toBeGreaterThan(0)
  })
})
