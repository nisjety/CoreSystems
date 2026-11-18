import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

// WS14-S9: the room's Files tab. The files client is mocked, so no request leaves the test. The
// gateway's refusals arrive as ApiError values, the way the real client delivers them.

const client = vi.hoisted(() => ({
  listSpaceFiles: vi.fn(),
  uploadSpaceFiles: vi.fn(),
  getSpaceFileUpload: vi.fn(),
  readSpaceFileText: vi.fn(),
  deleteSpaceFile: vi.fn(),
  newUploadIdempotencyKey: vi.fn(),
}))
vi.mock('@/shared/api/space-files-client', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ...client,
}))

const { SpaceFilesPanel } = await import('./SpaceFilesPanel')
const { ActiveSpaceTabContext } = await import('./SpaceCockpit')

type TabId = 'chat' | 'filer'

// Fixtures in the shapes the client returns (it maps the wire into these).
const fileRow = (over: Record<string, unknown> = {}) => ({
  document_id: 'doc-1',
  file_name: 'Plan.txt',
  mime_type: 'text/plain',
  size_bytes: 2048,
  sha256: null,
  extension: 'txt',
  status: 'ready',
  text_chars: 1200,
  uploaded_by: 'Kari',
  uploaded_at: '2026-11-18T10:00:00Z',
  ...over,
})
const page = (over: Record<string, unknown> = {}) => ({ files: [], nextCursor: null, unavailable: [], ...over })
const accepted = (names: string[]) => ({
  uploadId: 'up-1',
  deduplicated: false,
  files: names.map((fileName) => ({ fileName, status: 'queued' })),
})
const progress = (over: Record<string, unknown> = {}) => ({
  uploadId: 'up-1',
  status: 'running',
  total: 1,
  processed: 0,
  failed: 0,
  items: [],
  ...over,
})
const fileText = (over: Record<string, unknown> = {}) => ({
  documentId: 'doc-1',
  fileName: 'Plan.txt',
  text: 'Innkjøpsplan for våren.',
  textChars: 24,
  truncated: false,
  ...over,
})

function mount(
  options: { initial?: TabId; role?: string; archived?: boolean; subject?: string; limits?: Record<string, unknown> } = {},
) {
  const [active, setActive] = createSignal<TabId>(options.initial ?? 'chat')
  const view = render(() => (
    <ActiveSpaceTabContext value={active as never}>
      <SpaceFilesPanel
        spaceRef="room-a"
        viewerSubjectId={() => options.subject ?? 'Kari'}
        role={() => options.role ?? 'editor'}
        archived={options.archived === undefined ? undefined : () => options.archived === true}
        limits={options.limits === undefined ? undefined : () => options.limits}
      />
    </ActiveSpaceTabContext>
  ))
  const select = (id: TabId) => {
    setActive(id)
    flush()
  }
  return { ...view, select }
}

// Real timers: lets the pending client promise settle, then applies Solid's queued writes.
async function settle(ms = 0) {
  await new Promise((resolve) => window.setTimeout(resolve, ms))
  flush()
}

function pickFiles(files: File[]) {
  const input = document.querySelector<HTMLInputElement>('input[type="file"]')
  if (!input) throw new Error('no file picker is offered to this reader')
  fireEvent.change(input, { target: { files } })
  flush()
}

function rowFor(name: string): HTMLElement {
  const row = screen.getByRole('button', { name: `Forhåndsvis ${name}` }).closest('li')
  if (!row) throw new Error(`no row for ${name}`)
  return row
}

// Remove asks in place: the first press arms the button, the second press confirms.
function removeFile(name: string) {
  fireEvent.click(within(rowFor(name)).getByRole('button', { name: 'Fjern' }))
  flush()
  fireEvent.click(screen.getByRole('button', { name: `Bekreft fjerning av ${name}` }))
  flush()
}

beforeEach(() => {
  for (const mock of Object.values(client)) mock.mockReset()
  client.listSpaceFiles.mockResolvedValue(page())
  client.newUploadIdempotencyKey.mockReturnValue('upload-key-0001')
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('SpaceFilesPanel (WS14-S9)', () => {
  describe('lazy loading and the states', () => {
    it('reads the library only while the Files tab is the active one, and again on each re-entry', async () => {
      const { select } = mount({ initial: 'chat' })
      await settle()
      expect(client.listSpaceFiles).not.toHaveBeenCalled()

      select('filer')
      await waitFor(() => expect(client.listSpaceFiles).toHaveBeenCalledTimes(1))
      expect(client.listSpaceFiles).toHaveBeenLastCalledWith('room-a')

      select('chat')
      await settle()
      expect(client.listSpaceFiles).toHaveBeenCalledTimes(1)

      select('filer')
      await waitFor(() => expect(client.listSpaceFiles).toHaveBeenCalledTimes(2))
    })

    it('shows the loading copy while the first read is in flight', async () => {
      let answer: (value: unknown) => void = () => {}
      client.listSpaceFiles.mockReturnValueOnce(
        new Promise((resolve) => {
          answer = resolve
        }),
      )
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByText('Henter filer i rommet …')).toBeTruthy())
      answer(page())
      await waitFor(() => expect(screen.getByRole('heading', { name: 'Ingen filer ennå' })).toBeTruthy())
      expect(screen.queryByText('Henter filer i rommet …')).toBeNull()
    })

    it('empty: says no files are there yet, and offers the add control to a writer', async () => {
      mount({ initial: 'filer', role: 'editor' })
      await waitFor(() => expect(screen.getByRole('heading', { name: 'Ingen filer ennå' })).toBeTruthy())
      expect(screen.getByRole('button', { name: 'Legg til filer' })).toBeTruthy()
      expect(document.querySelector('.verevon-space-files__list')).toBeNull()
    })

    it('restricted (403): says the reader may not read the files, and Try again loads them', async () => {
      client.listSpaceFiles
        .mockRejectedValueOnce(new ApiError('forbidden', 403, 'files_read_not_authorized'))
        .mockResolvedValueOnce(page({ files: [fileRow()] }))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByText('Du har ikke tilgang til å lese filene i dette rommet.')).toBeTruthy())
      expect(document.querySelector('.verevon-space-files__row')).toBeNull()
      expect(screen.queryByRole('heading', { name: 'Ingen filer lagres' })).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())
      expect(client.listSpaceFiles).toHaveBeenCalledTimes(2)
    })

    it('disabled (404 room_files_disabled): says the library is off here, and another 404 is a plain error', async () => {
      client.listSpaceFiles.mockRejectedValueOnce(new ApiError('not found', 404, 'room_files_disabled'))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByText('Romfiler er ikke slått på for dette miljøet.')).toBeTruthy())
      expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()

      cleanup()
      client.listSpaceFiles.mockRejectedValueOnce(new ApiError('not found', 404, 'space_not_found'))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByText('Vi fikk ikke hentet filene i rommet. Ingenting er endret.')).toBeTruthy())
      expect(screen.queryByText('Romfiler er ikke slått på for dette miljøet.')).toBeNull()
    })

    it('zdr (412): shows the no-files copy, lists nothing, stores nothing, and makes no further list request', async () => {
      client.listSpaceFiles.mockRejectedValueOnce(new ApiError('zero retention', 412, 'zdr_durable_mutation_forbidden'))
      mount({ initial: 'filer', role: 'editor' })
      await waitFor(() => expect(screen.getByRole('heading', { name: 'Ingen filer lagres' })).toBeTruthy())
      expect(
        screen.getByText('Organisasjoner med null datalagring beholder ikke filer i rom. Ingenting er listet.'),
      ).toBeTruthy()
      expect(document.querySelector('.verevon-space-files__list')).toBeNull()
      expect(document.querySelector('.verevon-space-files__row')).toBeNull()
      expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
      expect(screen.queryByRole('button', { name: 'Legg til filer' })).toBeNull()

      await settle(50)
      expect(client.listSpaceFiles).toHaveBeenCalledTimes(1)
      expect(window.localStorage.length).toBe(0)
      expect(window.sessionStorage.length).toBe(0)
    })

    it('error: a failed read is not an empty library, and Try again loads the files', async () => {
      client.listSpaceFiles
        .mockRejectedValueOnce(new ApiError('server', 500, 'internal_error'))
        .mockResolvedValueOnce(page({ files: [fileRow()] }))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByText('Vi fikk ikke hentet filene i rommet. Ingenting er endret.')).toBeTruthy())
      expect(screen.queryByRole('heading', { name: 'Ingen filer ennå' })).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())
      expect(screen.queryByText('Vi fikk ikke hentet filene i rommet. Ingenting er endret.')).toBeNull()
      expect(client.listSpaceFiles).toHaveBeenCalledTimes(2)
    })

    it('gap entries: each section the gateway could not list is stated, and no empty copy claims there are no files', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(
        page({
          unavailable: [
            { section: 'files', code: 'files_binding_unavailable', reason: '' },
            { section: 'files', code: 'files_upstream_unavailable', reason: '' },
            { section: 'files', code: 'partial_list', reason: 'Delt liste mangler.' },
          ],
        }),
      )
      mount({ initial: 'filer' })
      await waitFor(() =>
        expect(screen.getByText('Rommet er ikke koblet til et kunnskapsarkiv ennå, så filer kan ikke listes.')).toBeTruthy(),
      )
      expect(screen.getByText('Filene i rommet kunne ikke hentes akkurat nå.')).toBeTruthy()
      expect(screen.getByText('Delt liste mangler.')).toBeTruthy()
      expect(screen.queryByRole('heading', { name: 'Ingen filer ennå' })).toBeNull()
    })

    it('a gap is stated beside the files that did list', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(
        page({ files: [fileRow()], unavailable: [{ section: 'files', code: 'files_upstream_unavailable', reason: '' }] }),
      )
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())
      expect(screen.getByText('Filene i rommet kunne ikke hentes akkurat nå.')).toBeTruthy()
    })
  })

  describe('adding files', () => {
    it('a reader who cannot add files gets no picker or drop zone, and is told why', async () => {
      mount({ initial: 'filer', role: 'viewer' })
      await waitFor(() => expect(screen.getByText('Rollen din i rommet tillater ikke å legge til filer.')).toBeTruthy())
      expect(screen.queryByRole('button', { name: 'Legg til filer' })).toBeNull()
      expect(document.querySelector('input[type="file"]')).toBeNull()
      expect(document.querySelector('.verevon-space-files__drop')).toBeNull()

      cleanup()
      mount({ initial: 'filer', role: 'editor', archived: true })
      await waitFor(() => expect(screen.getByText('Rommet er arkivert, så filer kan ikke legges til.')).toBeTruthy())
      expect(document.querySelector('input[type="file"]')).toBeNull()
    })

    it('turns away a file type the library does not keep, before any request; the picker offers only the allowed types', async () => {
      mount({ initial: 'filer', role: 'editor' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Legg til filer' })).toBeTruthy())
      expect(document.querySelector('input[type="file"]')?.getAttribute('accept')).toBe(
        '.pdf,.docx,.xlsx,.pptx,.txt,.md,.csv,.json',
      )

      pickFiles([new File(['MZ'], 'evil.exe', { type: 'application/octet-stream' })])
      await settle()
      expect(screen.getByRole('alert').textContent).toContain('evil.exe: denne filtypen kan ikke lagres i rommet.')
      expect(client.uploadSpaceFiles).not.toHaveBeenCalled()
    })

    it('turns away an empty file and an oversized one, and sends only the files it accepts', async () => {
      client.uploadSpaceFiles.mockResolvedValueOnce(accepted(['a.txt']))
      mount({ initial: 'filer', role: 'editor', limits: { max_file_bytes: 1024 } })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Legg til filer' })).toBeTruthy())

      pickFiles([new File(['a'], 'a.txt'), new File([new Uint8Array(2048)], 'big.txt'), new File([], 'tom.txt')])
      await settle()
      const notice = screen.getByRole('alert').textContent ?? ''
      expect(notice).toContain('big.txt: filen er større enn 1 KB.')
      expect(notice).toContain('tom.txt: filen er tom.')

      expect(client.uploadSpaceFiles).toHaveBeenCalledTimes(1)
      const [spaceRef, sent, key] = client.uploadSpaceFiles.mock.calls[0] ?? []
      expect(spaceRef).toBe('room-a')
      expect((sent as File[]).map((file) => file.name)).toEqual(['a.txt'])
      expect(key).toBe('upload-key-0001')
    })

    it('keeps at most the allowed number of files in one upload, and names the ones held back', async () => {
      client.uploadSpaceFiles.mockResolvedValueOnce(accepted(['a.txt', 'b.txt']))
      mount({ initial: 'filer', role: 'editor', limits: { max_files: 2 } })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Legg til filer' })).toBeTruthy())

      pickFiles([new File(['a'], 'a.txt'), new File(['b'], 'b.txt'), new File(['c'], 'c.txt')])
      await settle()
      expect(screen.getByRole('alert').textContent).toContain('c.txt: maks 2 filer om gangen.')
      expect(client.uploadSpaceFiles).toHaveBeenCalledTimes(1)
      const [, sent] = client.uploadSpaceFiles.mock.calls[0] ?? []
      expect((sent as File[]).map((file) => file.name)).toEqual(['a.txt', 'b.txt'])
    })

    it('polls the upload until it is done, then reloads the list and says the files were added', async () => {
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      client.listSpaceFiles
        .mockResolvedValueOnce(page())
        .mockResolvedValueOnce(page({ files: [fileRow({ document_id: 'doc-2', file_name: 'Notat.md', extension: 'md' })] }))
      client.uploadSpaceFiles.mockResolvedValueOnce(accepted(['Notat.md']))
      client.getSpaceFileUpload
        .mockResolvedValueOnce(progress({ status: 'running' }))
        .mockResolvedValueOnce(
          progress({ status: 'completed', processed: 1, items: [{ sourceName: 'Notat.md', status: 'completed', documentId: 'doc-2' }] }),
        )
      mount({ initial: 'filer', role: 'editor' })
      await vi.advanceTimersByTimeAsync(0)
      flush()
      expect(screen.getByRole('button', { name: 'Legg til filer' })).toBeTruthy()

      pickFiles([new File(['Notat'], 'Notat.md')])
      await vi.advanceTimersByTimeAsync(0)
      flush()
      expect(screen.getByText('Behandler 1 fil(er) …')).toBeTruthy()

      await vi.advanceTimersByTimeAsync(2_000)
      flush()
      expect(client.getSpaceFileUpload).toHaveBeenCalledTimes(1)
      expect(screen.getByText('Behandler 1 fil(er) …')).toBeTruthy()

      await vi.advanceTimersByTimeAsync(2_000)
      await vi.advanceTimersByTimeAsync(0)
      flush()
      expect(client.getSpaceFileUpload).toHaveBeenCalledTimes(2)
      expect(client.getSpaceFileUpload).toHaveBeenLastCalledWith('room-a', 'up-1')
      expect(screen.getByText('Filene er lagt til i rommet.')).toBeTruthy()
      expect(client.listSpaceFiles).toHaveBeenCalledTimes(2)
      expect(screen.getByRole('button', { name: 'Forhåndsvis Notat.md' })).toBeTruthy()
    })

    it('shows the error_code copy for each file that failed, and not the done line', async () => {
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      client.uploadSpaceFiles.mockResolvedValueOnce(accepted(['Plan.txt', 'Notat.md', 'Rapport.pdf']))
      client.getSpaceFileUpload.mockResolvedValueOnce(
        progress({
          status: 'completed_with_errors',
          total: 3,
          processed: 3,
          failed: 2,
          items: [
            { sourceName: 'Plan.txt', status: 'failed', errorCode: 'room_file_too_large' },
            { sourceName: 'Notat.md', status: 'failed', errorCode: 'room_file_unlisted_code' },
            { sourceName: 'Rapport.pdf', status: 'completed', documentId: 'doc-3' },
          ],
        }),
      )
      mount({ initial: 'filer', role: 'editor' })
      await vi.advanceTimersByTimeAsync(0)
      flush()
      pickFiles([new File(['a'], 'Plan.txt'), new File(['b'], 'Notat.md'), new File(['c'], 'Rapport.pdf')])
      await vi.advanceTimersByTimeAsync(0)
      flush()
      await vi.advanceTimersByTimeAsync(2_000)
      await vi.advanceTimersByTimeAsync(0)
      flush()

      const run = document.querySelector<HTMLElement>('.verevon-space-files__run')
      expect(run?.textContent).toContain('Noen filer ble ikke lagt til:')
      expect(Array.from(run?.querySelectorAll('li') ?? []).map((item) => item.textContent)).toEqual([
        'Plan.txt Filen er for stor til å lagres i rommet.',
        'Notat.md Filen kunne ikke behandles.',
      ])
      expect(run?.textContent).not.toContain('Rapport.pdf')
      expect(run?.textContent).not.toContain('Filene er lagt til i rommet.')
    })
  })

  describe('removing files', () => {
    it('offers Remove only on the files the reader added', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(
        page({
          files: [
            fileRow({ document_id: 'doc-1', file_name: 'Plan.txt', uploaded_by: 'Kari' }),
            fileRow({ document_id: 'doc-2', file_name: 'Notat.md', extension: 'md', uploaded_by: 'Ola' }),
          ],
        }),
      )
      mount({ initial: 'filer', subject: 'Kari' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Notat.md' })).toBeTruthy())
      expect(within(rowFor('Plan.txt')).getByRole('button', { name: 'Fjern' })).toBeTruthy()
      expect(within(rowFor('Plan.txt')).getByText('Lagt til av deg')).toBeTruthy()
      expect(within(rowFor('Notat.md')).queryByRole('button', { name: 'Fjern' })).toBeNull()
      expect(within(rowFor('Notat.md')).queryByText('Lagt til av deg')).toBeNull()
    })

    it('removes an own file only on the second press, and the row goes', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(page({ files: [fileRow()] }))
      client.deleteSpaceFile.mockResolvedValueOnce(undefined)
      mount({ initial: 'filer', subject: 'Kari' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())

      fireEvent.click(within(rowFor('Plan.txt')).getByRole('button', { name: 'Fjern' }))
      flush()
      expect(screen.getByText('Filen fjernes for alle i rommet og kan ikke gjenopprettes.')).toBeTruthy()
      expect(client.deleteSpaceFile).not.toHaveBeenCalled()

      fireEvent.click(screen.getByRole('button', { name: 'Bekreft fjerning av Plan.txt' }))
      await waitFor(() => expect(client.deleteSpaceFile).toHaveBeenCalledWith('room-a', 'doc-1'))
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeNull())
    })

    it('a 404 on remove says the file is gone, and reloads the list', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(page({ files: [fileRow()] })).mockResolvedValueOnce(page())
      client.deleteSpaceFile.mockRejectedValueOnce(new ApiError('not found', 404, 'file_not_found'))
      mount({ initial: 'filer', subject: 'Kari' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())
      removeFile('Plan.txt')

      await waitFor(() => expect(screen.getByText('Fant ikke filen, eller du har ikke tilgang til den.')).toBeTruthy())
      await waitFor(() => expect(client.listSpaceFiles).toHaveBeenCalledTimes(2))
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeNull())
    })

    it('a failed remove that is not a 404 keeps the row, says so, and does not reload', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(page({ files: [fileRow()] }))
      client.deleteSpaceFile.mockRejectedValueOnce(new ApiError('server', 500, 'internal_error'))
      mount({ initial: 'filer', subject: 'Kari' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())
      removeFile('Plan.txt')

      await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
      await settle(20)
      expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy()
      expect(client.listSpaceFiles).toHaveBeenCalledTimes(1)
    })
  })

  describe('preview', () => {
    it('opens the extracted text as inert text: markup in a file shows as characters, never as elements', async () => {
      const body = '<img src="x" onerror="window.__roomFilePwned = true"><b>fet</b> Tekst'
      client.listSpaceFiles.mockResolvedValueOnce(page({ files: [fileRow()] }))
      client.readSpaceFileText.mockResolvedValueOnce(fileText({ text: body, textChars: body.length }))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())

      fireEvent.click(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' }))
      await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy())
      expect(client.readSpaceFileText).toHaveBeenCalledWith('room-a', 'doc-1', { maxChars: 100_000 })
      const dialog = screen.getByRole('dialog')
      expect(dialog.querySelector('pre')?.textContent).toBe(body)
      expect(dialog.querySelector('img')).toBeNull()
      expect(dialog.querySelector('b')).toBeNull()
      expect((window as unknown as { __roomFilePwned?: boolean }).__roomFilePwned).toBeUndefined()
    })

    it('says the text was cut off when it was, and says nothing of the kind when it was not', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(
        page({ files: [fileRow(), fileRow({ document_id: 'doc-2', file_name: 'Notat.md', extension: 'md' })] }),
      )
      client.readSpaceFileText
        .mockResolvedValueOnce(fileText({ text: 'Starten av teksten', truncated: true }))
        .mockResolvedValueOnce(fileText({ documentId: 'doc-2', fileName: 'Notat.md', text: 'Hele notatet' }))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Notat.md' })).toBeTruthy())

      fireEvent.click(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' }))
      await waitFor(() => expect(screen.getByText('Teksten er kuttet; bare starten vises.')).toBeTruthy())
      fireEvent.click(screen.getByRole('button', { name: 'Lukk' }))
      flush()

      fireEvent.click(screen.getByRole('button', { name: 'Forhåndsvis Notat.md' }))
      await waitFor(() => expect(screen.getByText('Hele notatet')).toBeTruthy())
      expect(screen.queryByText('Teksten er kuttet; bare starten vises.')).toBeNull()
    })

    it('a 404 on open says the file is gone, and reloads the list', async () => {
      client.listSpaceFiles.mockResolvedValueOnce(page({ files: [fileRow()] })).mockResolvedValueOnce(page())
      client.readSpaceFileText.mockRejectedValueOnce(new ApiError('not found', 404, 'file_not_found'))
      mount({ initial: 'filer' })
      await waitFor(() => expect(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' })).toBeTruthy())

      fireEvent.click(screen.getByRole('button', { name: 'Forhåndsvis Plan.txt' }))
      await waitFor(() => expect(screen.getByText('Fant ikke filen, eller du har ikke tilgang til den.')).toBeTruthy())
      await waitFor(() => expect(client.listSpaceFiles).toHaveBeenCalledTimes(2))
    })
  })
})
