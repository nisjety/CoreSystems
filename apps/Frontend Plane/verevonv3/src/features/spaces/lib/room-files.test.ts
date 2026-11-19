import { afterEach, describe, expect, it, vi } from 'vitest'

import { viewerFor } from '@/shared/artifacts/viewer-registry'

import {
  ROOM_FILE_TEXT_KIND,
  canAddRoomFiles,
  fileExtension,
  formatBytes,
  roomFileTextPreviewable,
  roomFilesDurableEnabled,
  roomFilesFlag,
  roomFilesFromChips,
  roomFilesLimits,
  roomFilesTabEnabled,
  vetRoomFiles,
  type RoomFilesLimits,
} from './room-files'
import { primeSpaceRoomFeatures } from './space-room-features'

// WS14-S9: the durable room file library as the SPA reads it. Presentation helpers only; the gateway decides who may
// upload, read or delete. These tests pin the bounds the SPA falls back to, the flags it reads from the nested block,
// the client-side vetting that spares a certain refusal, and the viewer the library registers.

const english = (_no: string, en: string) => en

function file(name: string, size: number, type = ''): File {
  return new File([new Uint8Array(size)], name, { type })
}

afterEach(() => {
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
  primeSpaceRoomFeatures(undefined)
})

describe('roomFilesLimits (the bounds the context states, or the documented defaults)', () => {
  it('uses the documented defaults when the context states no bounds', () => {
    const defaults = {
      maxFiles: 10,
      maxFileBytes: 25 * 1024 * 1024,
      allowedExtensions: ['pdf', 'docx', 'xlsx', 'pptx', 'txt', 'md', 'csv', 'json'],
    }
    expect(roomFilesLimits(undefined)).toEqual(defaults)
    expect(roomFilesLimits(null)).toEqual(defaults)
    expect(roomFilesLimits({})).toEqual(defaults)
  })

  it('reads the stated bounds, lower-cases the types and drops blank or non-text entries', () => {
    expect(
      roomFilesLimits({ max_files: 3, max_file_bytes: 1024, allowed_extensions: ['PDF', '', 'Md', 7] }),
    ).toEqual({ maxFiles: 3, maxFileBytes: 1024, allowedExtensions: ['pdf', 'md'] })
  })

  it('falls back for a bound that is not a positive number, and floors a fraction', () => {
    const limits = roomFilesLimits({ max_files: 0, max_file_bytes: -5, allowed_extensions: [] })
    expect(limits.maxFiles).toBe(10)
    expect(limits.maxFileBytes).toBe(25 * 1024 * 1024)
    expect(limits.allowedExtensions).toContain('pdf')
    expect(roomFilesLimits({ max_files: 2.9 }).maxFiles).toBe(2)
    expect(roomFilesLimits({ max_files: 'lots' }).maxFiles).toBe(10)
  })
})

describe('canAddRoomFiles', () => {
  it('lets only a member who can write in the room offer to add files', () => {
    expect(canAddRoomFiles('editor')).toBe(true)
    expect(canAddRoomFiles('manager')).toBe(true)
    expect(canAddRoomFiles('owner')).toBe(true)
    expect(canAddRoomFiles('viewer')).toBe(false)
    expect(canAddRoomFiles('')).toBe(false)
    expect(canAddRoomFiles(undefined)).toBe(false)
  })
})

describe('fileExtension and formatBytes', () => {
  it('takes the final extension in lower case, and none for a name without one', () => {
    expect(fileExtension('Report.PDF')).toBe('pdf')
    expect(fileExtension('archive.tar.gz')).toBe('gz')
    expect(fileExtension('README')).toBe('')
  })

  it('states a size in bytes, kilobytes or megabytes, and nothing for a size that is not one', () => {
    expect(formatBytes(812)).toBe('812 B')
    expect(formatBytes(14 * 1024)).toBe('14 KB')
    expect(formatBytes(1.2 * 1024 * 1024)).toBe('1.2 MB')
    expect(formatBytes(null)).toBe('')
    expect(formatBytes(Number.NaN)).toBe('')
    expect(formatBytes(-1)).toBe('')
  })
})

describe('vetRoomFiles (the refusals the SPA can see before any request)', () => {
  const limits: RoomFilesLimits = { maxFiles: 2, maxFileBytes: 10, allowedExtensions: ['txt', 'pdf'] }

  it('turns away a file type the library does not keep, and names the allowed types', () => {
    const verdict = vetRoomFiles([file('setup.exe', 3)], limits, english)
    expect(verdict.accepted).toEqual([])
    expect(verdict.messages).toEqual([
      'setup.exe: this file type cannot be kept in the room. Allowed: txt, pdf.',
    ])
  })

  it('turns away an empty file and one larger than the bound', () => {
    const verdict = vetRoomFiles([file('empty.txt', 0), file('big.txt', 11)], limits, english)
    expect(verdict.accepted).toEqual([])
    expect(verdict.messages).toEqual([
      'empty.txt: the file is empty.',
      'big.txt: the file is larger than 10 B.',
    ])
  })

  it('keeps at most the allowed count and names each file held back, in the order picked', () => {
    const one = file('one.txt', 3)
    const two = file('two.pdf', 4)
    const three = file('three.txt', 5)
    const verdict = vetRoomFiles([one, two, three], limits, english)
    expect(verdict.accepted).toEqual([one, two])
    expect(verdict.messages).toEqual(['three.txt: at most 2 files at a time.'])
  })

  it('matches the type regardless of case, and sends only the files it accepts', () => {
    const upper = file('REPORT.TXT', 4)
    const verdict = vetRoomFiles([upper, file('image.png', 4)], limits, english)
    expect(verdict.accepted).toEqual([upper])
    expect(verdict.messages).toHaveLength(1)
  })
})

describe('the nested room_files flags (WS14-S9)', () => {
  it('reads one member of the room_files block, and is off when the room has not been read', () => {
    expect(roomFilesFlag('files_tab')).toBe(false)
    primeSpaceRoomFeatures({ room_files: { files_tab: true, durable: false } })
    expect(roomFilesFlag('files_tab')).toBe(true)
    expect(roomFilesFlag('durable')).toBe(false)
    expect(roomFilesTabEnabled()).toBe(true)
    expect(roomFilesDurableEnabled()).toBe(false)
  })

  it('counts only the literal true inside the block, and a flag in the wrong place', () => {
    primeSpaceRoomFeatures({ room_files: { files_tab: 'true' } })
    expect(roomFilesFlag('files_tab')).toBe(false)
    primeSpaceRoomFeatures({ room_files: true })
    expect(roomFilesFlag('files_tab')).toBe(false)
    primeSpaceRoomFeatures({ files_tab: true })
    expect(roomFilesTabEnabled()).toBe(false)
  })

  it('lets a dev build force the library on or off, and a production build reads only the BFF', () => {
    primeSpaceRoomFeatures({ room_files: { files_tab: true, durable: true } })
    vi.stubEnv('DEV', true)
    vi.stubEnv('VITE_SPACE_ROOM_FILES', 'false')
    expect(roomFilesTabEnabled()).toBe(false)
    expect(roomFilesDurableEnabled()).toBe(false)

    vi.stubEnv('DEV', false)
    expect(roomFilesTabEnabled()).toBe(true)
    expect(roomFilesDurableEnabled()).toBe(true)
  })
})

describe('roomFilesFromChips (the composer chips read back as files)', () => {
  it('keeps the chips of a type the library accepts, skips the rest, and caps the count', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url === 'blob:gone') throw new TypeError('revoked')
        return new Response('tekst', { headers: { 'Content-Type': 'text/plain' } })
      }),
    )
    const limits = roomFilesLimits(undefined)
    const chips = [
      { name: 'notes.txt', url: 'blob:1', type: 'text/plain' },
      { name: 'tool.exe', url: 'blob:2', type: '' },
      { name: 'gone.md', url: 'blob:gone', type: 'text/markdown' },
      { name: 'no-url.md', url: '', type: '' },
      { name: 'b.md', url: 'blob:3', type: '' },
    ]

    const kept = await roomFilesFromChips(chips, limits)
    expect(kept.map((item) => item.name)).toEqual(['notes.txt', 'b.md'])
    expect(kept[0]?.type).toBe('text/plain')

    const capped = await roomFilesFromChips(chips, { ...limits, maxFiles: 1 })
    expect(capped.map((item) => item.name)).toEqual(['notes.txt'])
  })
})

describe('the room_file_text viewer (WS14-S9)', () => {
  it('is registered for the room surface as inert text, and not for chat', () => {
    expect(ROOM_FILE_TEXT_KIND).toBe('room_file_text')
    expect(roomFileTextPreviewable()).toBe(true)
    expect(viewerFor(ROOM_FILE_TEXT_KIND, 'room')?.trust).toBe('text')
    expect(viewerFor(ROOM_FILE_TEXT_KIND, 'chat')).toBeUndefined()
  })
})
