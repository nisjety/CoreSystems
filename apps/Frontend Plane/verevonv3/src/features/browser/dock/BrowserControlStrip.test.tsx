// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { BrowserControlStrip } from './BrowserControlStrip'

function strip(): HTMLElement {
  return document.querySelector('.browser-control-strip') as HTMLElement
}

function action(): HTMLButtonElement {
  return document.querySelector('.browser-control-strip__action') as HTMLButtonElement
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('BrowserControlStrip (WS9.I)', () => {
  it('when the agent has control, Take over sends the epoch the strip rendered', () => {
    const onChangeMode = vi.fn()
    render(() => (
      <BrowserControlStrip controlMode="agent_control" controlEpoch={4} onChangeMode={onChangeMode} />
    ))

    expect(strip().getAttribute('data-state')).toBe('agent')
    fireEvent.click(action())
    expect(onChangeMode).toHaveBeenCalledWith('human_takeover', 4)
  })

  it('when the person has control, Give back sends the epoch and the agent is paused', () => {
    const onChangeMode = vi.fn()
    render(() => (
      <BrowserControlStrip controlMode="human_takeover" controlEpoch={5} onChangeMode={onChangeMode} />
    ))

    expect(strip().getAttribute('data-state')).toBe('human')
    fireEvent.click(action())
    expect(onChangeMode).toHaveBeenCalledWith('agent_control', 5)
  })

  it('shows the agent-working state only while the agent is running and still has control', () => {
    render(() => (
      <BrowserControlStrip controlMode="agent_control" controlEpoch={0} agentWorking onChangeMode={vi.fn()} />
    ))
    expect(strip().getAttribute('data-state')).toBe('agent_working')
  })

  it('a person in control is never shown as the agent working, even if a loop is still flagged', () => {
    render(() => (
      <BrowserControlStrip controlMode="human_takeover" controlEpoch={1} agentWorking onChangeMode={vi.fn()} />
    ))
    expect(strip().getAttribute('data-state')).toBe('human')
  })

  it('the button waits while a request is busy and sends nothing', () => {
    const onChangeMode = vi.fn()
    render(() => (
      <BrowserControlStrip controlMode="agent_control" controlEpoch={2} busy onChangeMode={onChangeMode} />
    ))

    expect(action().disabled).toBe(true)
    expect(action().getAttribute('aria-busy')).toBe('true')
    fireEvent.click(action())
    expect(onChangeMode).not.toHaveBeenCalled()
  })

  it('is disabled when the session cannot be controlled', () => {
    const onChangeMode = vi.fn()
    render(() => (
      <BrowserControlStrip controlMode="agent_control" controlEpoch={2} disabled onChangeMode={onChangeMode} />
    ))

    expect(action().disabled).toBe(true)
    fireEvent.click(action())
    expect(onChangeMode).not.toHaveBeenCalled()
  })

  it('sends no epoch when the projection carried none, so the change stays unconditional', () => {
    const onChangeMode = vi.fn()
    render(() => <BrowserControlStrip controlMode="agent_control" onChangeMode={onChangeMode} />)

    fireEvent.click(action())
    expect(onChangeMode).toHaveBeenCalledWith('human_takeover', null)
  })

  it('shows the Zero Data Retention copy for a ZDR session and not otherwise', () => {
    render(() => (
      <BrowserControlStrip controlMode="agent_control" controlEpoch={0} zdr onChangeMode={vi.fn()} />
    ))
    expect(document.querySelector('.browser-control-strip__zdr')).not.toBeNull()
    expect(document.querySelector('.browser-control-strip__zdr')?.textContent ?? '').toMatch(/Zero Data Retention/)
  })

  it('has no ZDR line for an ordinary session', () => {
    render(() => (
      <BrowserControlStrip controlMode="agent_control" controlEpoch={0} zdr={false} onChangeMode={vi.fn()} />
    ))
    expect(document.querySelector('.browser-control-strip__zdr')).toBeNull()
  })
})
