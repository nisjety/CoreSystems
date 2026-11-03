// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DocumentReadPanel } from '@/features/ingestions/components/DocumentReadPanel'

const READ_BUTTON = /Les dokument|Read document/

const reading = {
  markdown: '# Faktura\n\nBeløp 100 kr',
  kind: 'pdf',
  bytes: 1599,
  structure: 'plain',
  truncated: false,
  pages: 2,
  warnings: [{ code: 'some_pages_have_no_text_layer', message: 'Some pages have no text layer (1 of 2).' }],
}

function reply(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } })
}

/** Pick a file in the page's file input, then wait until the read button can press. */
async function pick(container: HTMLElement, file: File): Promise<HTMLButtonElement> {
  const input = container.querySelector('input[type="file"]') as HTMLInputElement
  fireEvent.change(input, { target: { files: [file] } })
  flush()
  const button = screen.getByRole('button', { name: READ_BUTTON }) as HTMLButtonElement
  return button
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('DocumentReadPanel', () => {
  it('reads a PDF, sends its bytes under the PDF type, and shows the text with its warning above it', async () => {
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => reply({ data: reading, meta: { request_id: 'r' } }))
    vi.stubGlobal('fetch', fetchMock)
    const { container } = render(() => <DocumentReadPanel />)
    const file = new File(['%PDF-1.4'], 'faktura.pdf', { type: 'application/pdf' })

    const button = await pick(container, file)
    await waitFor(() => expect(button.disabled).toBe(false))
    fireEvent.click(button)

    await waitFor(() => expect(container.querySelector('.ingestions-doc__text')?.textContent).toContain('Beløp 100 kr'))
    expect(container.querySelector('.ingestions-doc__warnings')?.textContent?.length ?? 0).toBeGreaterThan(0)
    const [url, init] = fetchMock.mock.calls[0] ?? []
    expect(url).toMatch(/\/api\/ingestions\/parse$/)
    expect(new Headers(init?.headers).get('Content-Type')).toBe('application/pdf')
    expect(init?.body).toBe(file)
  })

  it('refuses a file that is neither a PDF nor a DOCX, without sending anything', async () => {
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => reply({ data: reading }))
    vi.stubGlobal('fetch', fetchMock)
    const { container } = render(() => <DocumentReadPanel />)

    await pick(container, new File(['x'], 'bilde.png', { type: 'image/png' }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('names the switch when Quarry has document reading off', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async (_url: string, _init?: RequestInit) =>
          reply(
            { error: { code: 'UNSUPPORTED', message: 'off', details: { reason: 'parse_route_disabled', env: 'QUARRY_EDGE__PARSE_ROUTE' } } },
            501,
          ),
      ),
    )
    const { container } = render(() => <DocumentReadPanel />)

    const button = await pick(container, new File(['%PDF-1.4'], 'faktura.pdf'))
    await waitFor(() => expect(button.disabled).toBe(false))
    fireEvent.click(button)

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('QUARRY_EDGE__PARSE_ROUTE')
  })

  it('holds the read button for the wait Quarry asked for after a busy answer', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async (_url: string, _init?: RequestInit) =>
          reply({ error: { code: 'RATE_LIMITED', message: 'busy' } }, 429, { 'Retry-After': '7' }),
      ),
    )
    const { container } = render(() => <DocumentReadPanel />)

    const button = await pick(container, new File(['%PDF-1.4'], 'faktura.pdf'))
    await waitFor(() => expect(button.disabled).toBe(false))
    fireEvent.click(button)

    await waitFor(() => expect(screen.getByRole('button', { name: /7 s/ }).hasAttribute('disabled')).toBe(true))
    expect(screen.getByRole('alert').textContent).toContain('7')
  })
})
