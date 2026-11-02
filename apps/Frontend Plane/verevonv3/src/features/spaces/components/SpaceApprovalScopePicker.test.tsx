import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ApprovalScope } from '@/shared/api/space-approvals-client'

const { SpaceApprovalScopePicker } = await import('./SpaceApprovalScopePicker')

afterEach(() => {
  cleanup()
})

function renderPicker(options: readonly ApprovalScope[], value: ApprovalScope = 'once', extra = {}) {
  const onChange = vi.fn()
  const view = render(() => (
    <SpaceApprovalScopePicker
      approvalId="ap-1"
      options={options}
      value={value}
      onChange={onChange}
      {...extra}
    />
  ))
  return { onChange, view }
}

describe('SpaceApprovalScopePicker', () => {
  it('renders nothing when once is the only option', () => {
    const { view } = renderPicker(['once'])
    expect(view.container.innerHTML).toBe('')
    expect(screen.queryByRole('radio')).toBeNull()
    expect(screen.queryByRole('group')).toBeNull()
  })

  it('renders nothing for an empty option list too', () => {
    const { view } = renderPicker([])
    expect(view.container.innerHTML).toBe('')
  })

  it('draws a radio for once and one for always, once checked by default', () => {
    renderPicker(['once', 'always'])
    const radios = screen.getAllByRole('radio') as HTMLInputElement[]
    expect(radios.map((radio) => radio.value)).toEqual(['once', 'always'])
    expect(radios[0]?.checked).toBe(true)
    expect(radios[1]?.checked).toBe(false)
    // One group per approval so two cards never share a selection.
    expect(new Set(radios.map((radio) => radio.name))).toEqual(new Set(['approval-scope-ap-1']))
    expect(screen.getByText('Bare denne gangen')).toBeTruthy()
    expect(screen.getByText('Alltid for dette verktøyet i rommet')).toBeTruthy()
  })

  it('says what a standing rule means, but only while always is chosen', () => {
    const once = renderPicker(['once', 'always'], 'once')
    expect(screen.queryByText(/En fast regel lar agenten/)).toBeNull()
    once.view.unmount()
    renderPicker(['once', 'always'], 'always')
    expect(screen.getByText(/En fast regel lar agenten bruke dette verktøyet uten å spørre igjen/)).toBeTruthy()
    expect((screen.getAllByRole('radio')[1] as HTMLInputElement).checked).toBe(true)
  })

  it('reports the chosen scope', () => {
    const { onChange } = renderPicker(['once', 'always'])
    ;(screen.getAllByRole('radio')[1] as HTMLInputElement).click()
    expect(onChange).toHaveBeenCalledWith('always')
  })

  it('disables the whole group while a decision is being recorded', () => {
    renderPicker(['once', 'always'], 'once', { disabled: true })
    expect((screen.getByRole('group') as HTMLFieldSetElement).disabled).toBe(true)
  })

  it('words thread as a conversation scope if a caller ever offers it', () => {
    renderPicker(['once', 'thread'])
    expect(screen.getByText('I denne samtalen')).toBeTruthy()
  })
})
