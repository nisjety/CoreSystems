// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PersonaSection } from '@/features/settings/components/PersonaSection'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function errorResponse(status: number, code: string, message = 'nope'): Response {
  return new Response(JSON.stringify({ error: { code, message } }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('PersonaSection', () => {
  it('renders the saved persona text and enabled state', async () => {
    vi.stubGlobal('fetch', vi.fn(async () =>
      jsonResponse({ data: { instructions: 'Svar alltid kort.', enabled: true } }),
    ))

    render(() => <PersonaSection />)

    expect(await screen.findByDisplayValue('Svar alltid kort.')).toBeTruthy()
    expect(screen.getByRole('switch', { name: /bruk persona i nye samtaler/i }).getAttribute('aria-checked')).toBe('true')
  })

  it('tracks a dirty draft and enables the save button only after an edit', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ data: { instructions: '', enabled: true } })))

    render(() => <PersonaSection />)
    await screen.findByRole('textbox', { name: /instrukser/i })

    const saveButton = screen.getByRole('button', { name: /lagre persona/i })
    expect(saveButton.hasAttribute('disabled')).toBe(true)

    fireEvent.input(screen.getByRole('textbox', { name: /instrukser/i }), {
      target: { value: 'Vær direkte.' },
    })
    flush()

    expect(saveButton.hasAttribute('disabled')).toBe(false)
  })

  it('submits the draft text and enabled switch through a PATCH', async () => {
    const fetchMock = vi.fn(async (_input: unknown, init?: RequestInit) => {
      if (init?.method === 'PATCH') {
        return jsonResponse({ data: { instructions: 'Vær direkte.', enabled: false } })
      }
      return jsonResponse({ data: { instructions: '', enabled: true } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <PersonaSection />)
    await screen.findByRole('textbox', { name: /instrukser/i })

    fireEvent.input(screen.getByRole('textbox', { name: /instrukser/i }), {
      target: { value: 'Vær direkte.' },
    })
    flush()
    fireEvent.click(screen.getByRole('switch', { name: /bruk persona i nye samtaler/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /lagre persona/i }))
    flush()

    await waitFor(() => {
      const patchCall = fetchMock.mock.calls.find(([, init]) => (init as RequestInit | undefined)?.method === 'PATCH')
      expect(patchCall).toBeTruthy()
      const [path, init] = patchCall as unknown as [string, RequestInit]
      expect(path).toBe('/api/v1/me/persona')
      expect(JSON.parse(String(init.body))).toEqual({ instructions: 'Vær direkte.', enabled: false })
    })
  })

  it('shows a translated error and keeps the draft when the save fails', async () => {
    const fetchMock = vi.fn(async (_input: unknown, init?: RequestInit) => {
      if (init?.method === 'PATCH') {
        return errorResponse(503, 'persona_unavailable')
      }
      return jsonResponse({ data: { instructions: '', enabled: true } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <PersonaSection />)
    await screen.findByRole('textbox', { name: /instrukser/i })

    fireEvent.input(screen.getByRole('textbox', { name: /instrukser/i }), {
      target: { value: 'Vær direkte.' },
    })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /lagre persona/i }))
    flush()

    expect(await screen.findByText('Kunne ikke lagre persona-instruksene dine.')).toBeTruthy()
    expect(screen.getByDisplayValue('Vær direkte.')).toBeTruthy()
  })

  it('shows a load error without blocking the form', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => errorResponse(503, 'persona_unavailable')))

    render(() => <PersonaSection />)

    expect(await screen.findByText('Kunne ikke laste persona-instruksene dine.')).toBeTruthy()
  })

  it('hides the tone selector unless user-core offers tone presets', async () => {
    const fetchMock = vi.fn(async (input: unknown) => {
      if (String(input).includes('/api/v1/settings/ai')) {
        return jsonResponse({ ...aiSettings(), tonePresetsAvailable: false })
      }
      return jsonResponse({ data: { instructions: '', enabled: true } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <PersonaSection />)
    await screen.findByRole('textbox', { name: /instrukser/i })
    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([input]) => String(input).includes('/api/v1/settings/ai'))).toBe(true)
    })
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(screen.queryByRole('radio', { name: /direkte/i })).toBeNull()
  })

  it('saves the chosen tone and leaves the other AI settings as they were', async () => {
    const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
      if (String(input).includes('/api/v1/settings/ai')) {
        if (init?.method === 'PUT') {
          return jsonResponse({ ...aiSettings(), tonePreset: 'direct' })
        }
        return jsonResponse({ ...aiSettings(), tonePresetsAvailable: true })
      }
      return jsonResponse({ data: { instructions: '', enabled: true } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <PersonaSection />)
    fireEvent.click(await screen.findByRole('radio', { name: /direkte/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /lagre tone/i }))
    flush()

    await waitFor(() => {
      const putCall = fetchMock.mock.calls.find(
        ([input, init]) =>
          String(input).includes('/api/v1/settings/ai') && (init as RequestInit | undefined)?.method === 'PUT',
      )
      expect(putCall).toBeTruthy()
      const [, init] = putCall as unknown as [string, RequestInit]
      expect(JSON.parse(String(init.body))).toEqual({ ...aiSettings(), tonePreset: 'direct' })
    })
  })

  it('keeps the chosen tone and shows a translated error when the save fails', async () => {
    const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
      if (String(input).includes('/api/v1/settings/ai')) {
        if (init?.method === 'PUT') {
          return errorResponse(503, 'tone_unavailable')
        }
        return jsonResponse({ ...aiSettings(), tonePresetsAvailable: true })
      }
      return jsonResponse({ data: { instructions: '', enabled: true } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <PersonaSection />)
    fireEvent.click(await screen.findByRole('radio', { name: /varm/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /lagre tone/i }))
    flush()

    expect(await screen.findByText('Kunne ikke lagre tonen.')).toBeTruthy()
    expect((screen.getByRole('radio', { name: /varm/i }) as HTMLInputElement).checked).toBe(true)
  })
})

function aiSettings() {
  return {
    aiEnabled: true,
    modelPreference: 'auto',
    dataCollectionEnabled: false,
    personalizationEnabled: true,
    tonePreset: 'default',
  }
}
