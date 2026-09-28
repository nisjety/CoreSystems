import { describe, expect, it } from 'vitest'
import { buildZip, crc32 } from './zip'
import { unzip, readZipText } from './unzip-for-tests'

describe('crc32', () => {
  it('matches the well-known CRC-32 of "123456789"', () => {
    // The standard check value for the CRC-32/ISO-HDLC (PKZIP) algorithm.
    expect(crc32(new TextEncoder().encode('123456789'))).toBe(0xcbf43926)
  })

  it('is 0 for an empty buffer', () => {
    expect(crc32(new Uint8Array(0))).toBe(0)
  })
})

describe('buildZip', () => {
  it('round-trips entry names and contents', () => {
    const archive = buildZip([
      { name: 'a.txt', data: new TextEncoder().encode('hello') },
      { name: 'dir/b.txt', data: new TextEncoder().encode('world') },
    ])
    const entries = unzip(archive)
    expect(entries.map((entry) => entry.name)).toEqual(['a.txt', 'dir/b.txt'])
    expect(readZipText(archive, 'a.txt')).toBe('hello')
    expect(readZipText(archive, 'dir/b.txt')).toBe('world')
  })

  it('round-trips UTF-8 (Norwegian) filenames and contents byte-for-byte', () => {
    const archive = buildZip([{ name: 'ærlig-svar.xml', data: new TextEncoder().encode('Blåbær og økonomi') }])
    const entries = unzip(archive)
    expect(entries[0]?.name).toBe('ærlig-svar.xml')
    expect(readZipText(archive, 'ærlig-svar.xml')).toBe('Blåbær og økonomi')
  })

  it('is deterministic: the same input always produces the same bytes', () => {
    const build = () => buildZip([{ name: 'x.xml', data: new TextEncoder().encode('same every time') }])
    const first = build()
    const second = build()
    expect([...first]).toEqual([...second])
  })

  it('produces an archive with no entries for an empty input', () => {
    const archive = buildZip([])
    expect(unzip(archive)).toEqual([])
  })

  it('sets the UTF-8 filename flag (general-purpose bit 11) on every local file header', () => {
    const archive = buildZip([{ name: 'plain.xml', data: new Uint8Array([1, 2, 3]) }])
    const view = new DataView(archive.buffer)
    const flags = view.getUint16(6, true) // local file header: signature(4) + version(2) + flags(2)
    expect(flags & 0x0800).toBe(0x0800)
  })

  it('uses the store method (0) for every entry', () => {
    const archive = buildZip([{ name: 'plain.xml', data: new Uint8Array([1, 2, 3, 4, 5]) }])
    const view = new DataView(archive.buffer)
    const method = view.getUint16(8, true) // signature(4) + version(2) + flags(2) + method(2)
    expect(method).toBe(0)
  })
})
