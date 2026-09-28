/**
 * A tiny store-only zip READER, used only by this directory's own tests to
 * assert on what `buildZip` (and, through it, `buildDocx`/`buildXlsx`/
 * `buildPptx`) actually produced. `zip.ts` itself stays write-only — see its
 * header comment — so this lives next to the tests instead, and only ever
 * needs to understand the one encoding this codebase's own writer emits:
 * store method (0), UTF-8 names (general-purpose bit 11 set).
 *
 * Not a general-purpose unzip: no DEFLATE, no ZIP64, no encrypted entries.
 * If a future writer here needs any of those, this reader — and the
 * assumption baked into `buildZip` that store-only is always enough — both
 * need revisiting together.
 */

export type UnzippedEntry = { name: string; data: Uint8Array }

const END_OF_CENTRAL_DIRECTORY_SIGNATURE = 0x06054b50
const CENTRAL_DIRECTORY_SIGNATURE = 0x02014b50

function readUint16(view: DataView, offset: number): number {
  return view.getUint16(offset, true)
}

function readUint32(view: DataView, offset: number): number {
  return view.getUint32(offset, true)
}

function findEndOfCentralDirectory(view: DataView): number {
  // The EOCD record is at the very end of the archive except for its
  // (unused, always-empty here) comment field, so scanning backward from the
  // tail is both correct and fast enough for test-sized archives.
  for (let offset = view.byteLength - 22; offset >= 0; offset -= 1) {
    if (readUint32(view, offset) === END_OF_CENTRAL_DIRECTORY_SIGNATURE) return offset
  }
  throw new Error('Not a valid zip archive: end-of-central-directory record not found.')
}

export function unzip(archive: Uint8Array): UnzippedEntry[] {
  const view = new DataView(archive.buffer, archive.byteOffset, archive.byteLength)
  const eocdOffset = findEndOfCentralDirectory(view)
  const entryCount = readUint16(view, eocdOffset + 10)
  const centralDirectoryOffset = readUint32(view, eocdOffset + 16)

  const entries: UnzippedEntry[] = []
  let cursor = centralDirectoryOffset
  const decoder = new TextDecoder('utf-8')

  for (let i = 0; i < entryCount; i += 1) {
    if (readUint32(view, cursor) !== CENTRAL_DIRECTORY_SIGNATURE) {
      throw new Error(`Malformed central directory entry at offset ${cursor}.`)
    }
    const compressionMethod = readUint16(view, cursor + 10)
    if (compressionMethod !== 0) {
      throw new Error(`unzip-for-tests only understands store (method 0); found method ${compressionMethod}.`)
    }
    const compressedSize = readUint32(view, cursor + 20)
    const nameLength = readUint16(view, cursor + 28)
    const extraLength = readUint16(view, cursor + 30)
    const commentLength = readUint16(view, cursor + 32)
    const localHeaderOffset = readUint32(view, cursor + 42)
    const name = decoder.decode(archive.subarray(cursor + 46, cursor + 46 + nameLength))

    const localNameLength = readUint16(view, localHeaderOffset + 26)
    const localExtraLength = readUint16(view, localHeaderOffset + 28)
    const dataStart = localHeaderOffset + 30 + localNameLength + localExtraLength
    const data = archive.slice(dataStart, dataStart + compressedSize)

    entries.push({ name, data })
    cursor += 46 + nameLength + extraLength + commentLength
  }

  return entries
}

/** Convenience for a test that only cares about one part's text. Throws if the archive has no entry at that exact path. */
export function readZipText(archive: Uint8Array, name: string): string {
  const entry = unzip(archive).find((candidate) => candidate.name === name)
  if (!entry) throw new Error(`No such entry in archive: ${name}`)
  return new TextDecoder('utf-8').decode(entry.data)
}
