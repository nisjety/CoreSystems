/**
 * Deterministic, store-only (method 0) ZIP writer.
 *
 * OOXML containers (.docx/.xlsx/.pptx) are ZIP archives, and the three
 * builders in this directory are the only producers this module needs to
 * serve — none of them benefit from DEFLATE (a chat answer is at most a few
 * hundred KB of XML), and skipping compression entirely means this file has
 * no dependency on a compression library, pure or otherwise. Store-only also
 * makes the output byte-for-byte REPRODUCIBLE: every entry gets the same
 * fixed DOS timestamp (1980-01-01T00:00:00, the oldest date the format can
 * represent) instead of `Date.now()`, so exporting the same answer twice
 * produces the same bytes — which is what lets the tests next to this file
 * assert on exact archive contents instead of "some zip came out".
 *
 * Deliberately WRITE-ONLY: nothing here parses a zip back. A reader only
 * exists in this directory's tests, which need to unzip the very ZIPs this
 * file builds in order to assert on their contents — see
 * `unzip-for-tests.ts`.
 */

export type ZipEntry = {
  /** Path inside the archive, e.g. "word/document.xml". Forward slashes only, no leading slash. */
  name: string
  data: Uint8Array
}

// (1980 - 1980) << 9 | 1 << 5 | 1 -- 1980-01-01, the MS-DOS epoch, encoded as
// an MS-DOS date. Every entry in an archive built by this module carries
// this exact value, alongside an MS-DOS time of 0 (midnight): see the module
// doc comment for why a fixed timestamp matters here.
const FIXED_DOS_DATE = 0x21
const FIXED_DOS_TIME = 0

const LOCAL_FILE_HEADER_SIGNATURE = 0x04034b50
const CENTRAL_DIRECTORY_SIGNATURE = 0x02014b50
const END_OF_CENTRAL_DIRECTORY_SIGNATURE = 0x06054b50
/**
 * General-purpose bit flag bit 11 ("Language encoding flag", APPNOTE.TXT
 * §4.4.4): the filename (and comment, unused here) is UTF-8, not the legacy
 * CP437 codepage a reader would otherwise assume. Every part name this
 * module writes is ASCII, but a Norwegian answer's TITLE can end up in this
 * archive's metadata later, so the flag is set unconditionally rather than
 * only when a name happens to need it.
 */
const UTF8_NAME_FLAG = 0x0800
const VERSION_NEEDED = 20
const VERSION_MADE_BY = 20
const STORE_METHOD = 0

let crcTable: Uint32Array | null = null

function buildCrcTable(): Uint32Array {
  const table = new Uint32Array(256)
  for (let n = 0; n < 256; n += 1) {
    let value = n
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? (0xedb88320 ^ (value >>> 1)) : value >>> 1
    }
    table[n] = value >>> 0
  }
  return table
}

/** CRC-32 (IEEE 802.3 / PKZIP), the checksum every zip entry header carries. */
export function crc32(data: Uint8Array): number {
  crcTable ??= buildCrcTable()
  const table = crcTable
  let crc = 0xffffffff
  for (let i = 0; i < data.length; i += 1) {
    const byte = data[i] ?? 0
    crc = (table[(crc ^ byte) & 0xff] ?? 0) ^ (crc >>> 8)
  }
  return (crc ^ 0xffffffff) >>> 0
}

/** A growable little-endian byte buffer — everything a zip header needs, nothing more. */
class ByteWriter {
  private readonly chunks: Uint8Array[] = []
  private total = 0

  get size(): number {
    return this.total
  }

  bytes(data: Uint8Array): this {
    this.chunks.push(data)
    this.total += data.length
    return this
  }

  u16(value: number): this {
    return this.bytes(new Uint8Array([value & 0xff, (value >>> 8) & 0xff]))
  }

  u32(value: number): this {
    return this.bytes(
      new Uint8Array([value & 0xff, (value >>> 8) & 0xff, (value >>> 16) & 0xff, (value >>> 24) & 0xff]),
    )
  }

  build(): Uint8Array {
    const out = new Uint8Array(this.total)
    let offset = 0
    for (const chunk of this.chunks) {
      out.set(chunk, offset)
      offset += chunk.length
    }
    return out
  }
}

const nameEncoder = new TextEncoder()

/** Builds a complete, valid store-only ZIP archive from its entries, in the order given. */
export function buildZip(entries: readonly ZipEntry[]): Uint8Array {
  const local = new ByteWriter()
  const central = new ByteWriter()

  for (const entry of entries) {
    const nameBytes = nameEncoder.encode(entry.name)
    const checksum = crc32(entry.data)
    const localHeaderOffset = local.size

    local
      .u32(LOCAL_FILE_HEADER_SIGNATURE)
      .u16(VERSION_NEEDED)
      .u16(UTF8_NAME_FLAG)
      .u16(STORE_METHOD)
      .u16(FIXED_DOS_TIME)
      .u16(FIXED_DOS_DATE)
      .u32(checksum)
      .u32(entry.data.length)
      .u32(entry.data.length)
      .u16(nameBytes.length)
      .u16(0) // extra field length
      .bytes(nameBytes)
      .bytes(entry.data)

    central
      .u32(CENTRAL_DIRECTORY_SIGNATURE)
      .u16(VERSION_MADE_BY)
      .u16(VERSION_NEEDED)
      .u16(UTF8_NAME_FLAG)
      .u16(STORE_METHOD)
      .u16(FIXED_DOS_TIME)
      .u16(FIXED_DOS_DATE)
      .u32(checksum)
      .u32(entry.data.length)
      .u32(entry.data.length)
      .u16(nameBytes.length)
      .u16(0) // extra field length
      .u16(0) // file comment length
      .u16(0) // disk number start
      .u16(0) // internal file attributes
      .u32(0) // external file attributes
      .u32(localHeaderOffset)
      .bytes(nameBytes)
  }

  const localBytes = local.build()
  const centralBytes = central.build()

  const end = new ByteWriter()
  end
    .u32(END_OF_CENTRAL_DIRECTORY_SIGNATURE)
    .u16(0) // this disk
    .u16(0) // disk with central directory start
    .u16(entries.length) // entries on this disk
    .u16(entries.length) // total entries
    .u32(centralBytes.length)
    .u32(localBytes.length) // offset of central directory from start of archive
    .u16(0) // comment length
  const endBytes = end.build()

  const archive = new Uint8Array(localBytes.length + centralBytes.length + endBytes.length)
  archive.set(localBytes, 0)
  archive.set(centralBytes, localBytes.length)
  archive.set(endBytes, localBytes.length + centralBytes.length)
  return archive
}
