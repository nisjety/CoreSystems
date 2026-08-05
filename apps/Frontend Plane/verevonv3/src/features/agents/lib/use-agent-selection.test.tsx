// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { AgentsProvider, useAgentSelection } from '@/features/agents/lib/use-agent-selection'

function SelectionProbe() {
  const [selection] = useAgentSelection()
  return <output data-testid="agent-selection">{selection()}</output>
}

describe('AgentsProvider', () => {
  it('clears a selected agent when the app router navigates to a bare non-agent route', () => {
    window.history.pushState(null, '', '/agents?agent=service')

    function Harness() {
      const [route, setRoute] = createSignal({ pathname: '/agents', search: '?agent=service' })

      return (
        <AgentsProvider routeLocation={route()}>
          <SelectionProbe />
          <button type="button" onClick={() => setRoute({ pathname: '/support', search: '' })}>
            Open Support
          </button>
        </AgentsProvider>
      )
    }

    render(() => <Harness />)

    expect(screen.getByTestId('agent-selection').textContent).toBe('service')

    fireEvent.click(screen.getByRole('button', { name: 'Open Support' }))

    expect(screen.getByTestId('agent-selection').textContent).toBe('all')
  })
})
