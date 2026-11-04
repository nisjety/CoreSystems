// @vitest-environment jsdom

// EP-11 S6: a run preset is a goal text and the visible controls it sets. The editor sends only
// those controls with plan mode on, and shows a refusal of the closed settings schema as the server
// named it.

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/authored-kinds-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/authored-kinds-client')>()
  return { ...actual, createPreset: vi.fn() }
})

import { createPreset } from '@/shared/api/authored-kinds-client'
import { ApiError } from '@/shared/api/api-error'
import { PresetEditor, presetSettingsFromControls } from './PresetEditor'

const mockedCreate = vi.mocked(createPreset)

afterEach(() => {
  vi.clearAllMocks()
})

function fill(getByLabelText: (text: string) => HTMLElement) {
  fireEvent.input(getByLabelText('Navn'), { target: { value: 'Konkurrenter' } })
  fireEvent.input(getByLabelText('Mål'), { target: { value: 'Sammenlign prisene til konkurrentene.' } })
}

describe('PresetEditor', () => {
  it('sends only the visible controls with plan mode on', async () => {
    mockedCreate.mockResolvedValue({ id: 'preset_1' })
    const onSaved = vi.fn()
    const { getByLabelText, getByRole, unmount } = render(() => <PresetEditor onSaved={onSaved} />)
    fill(getByLabelText)
    fireEvent.click(getByRole('switch'))
    fireEvent.submit(getByRole('form'))

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1))
    expect(mockedCreate.mock.calls[0]?.[0]).toEqual({
      name: 'Konkurrenter',
      description: undefined,
      content: 'Sammenlign prisene til konkurrentene.',
      settings: { browseWeb: true, planMode: true },
    })
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith('preset_1'))
    unmount()
  })

  it('shows a settings refusal as the server named it and stays open', async () => {
    mockedCreate.mockRejectedValue(
      new ApiError('The skill does not pass the skill lint.', 422, 'skill_lint_failed', {
        details: { findings: [{ code: 'preset_settings_invalid', field: 'settings', severity: 'error' }] },
      }),
    )
    const onSaved = vi.fn()
    const { getByLabelText, getByRole, container, unmount } = render(() => <PresetEditor onSaved={onSaved} />)
    fill(getByLabelText)
    fireEvent.submit(getByRole('form'))

    await waitFor(() => expect(container.querySelector('[data-lint-code="preset_settings_invalid"]')).not.toBeNull())
    expect(onSaved).not.toHaveBeenCalled()
    unmount()
  })

  it('never sends without a name or a goal', () => {
    const { getByRole, getByText, unmount } = render(() => <PresetEditor />)
    fireEvent.submit(getByRole('form'))
    expect(mockedCreate).not.toHaveBeenCalled()
    expect(getByText('Navnet må ha 1–64 tegn, og målet må ha 1–32 000 tegn.')).toBeTruthy()
    unmount()
  })

  it('keeps plan mode on and names a mode only when one is picked', () => {
    expect(presetSettingsFromControls({ modeId: '', browseWeb: false })).toEqual({ browseWeb: false, planMode: true })
    expect(presetSettingsFromControls({ modeId: 'research', browseWeb: true })).toEqual({
      modeId: 'research',
      browseWeb: true,
      planMode: true,
    })
  })
})
