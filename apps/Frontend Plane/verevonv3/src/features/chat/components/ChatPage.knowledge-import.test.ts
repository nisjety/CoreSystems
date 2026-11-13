/**
 * The knowledge import the composer starts from the page (R18). The page has no cheap unit harness, so its
 * refusals are pinned as source text: each one is a LocalizedError with Norwegian and English copy, so the
 * composer's notice shows it in the reader's language and never a Norwegian-only sentence.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

describe('ChatPage knowledge import refusals', () => {
  it('throws LocalizedError with both languages, never a plain Norwegian Error', async () => {
    const page = await read('src/features/chat/components/ChatPage.tsx')
    const start = page.indexOf('const importAttachmentsToKnowledge = async')
    const end = page.indexOf('const voiceReply = createMemo(', start)
    expect(start).toBeGreaterThan(-1)
    expect(end).toBeGreaterThan(start)
    const body = page.slice(start, end)

    expect(body).not.toContain('new Error(')
    expect(body.match(/throw new LocalizedError\(\{ no: /g)).toHaveLength(4)
    expect(body).toContain("throw new LocalizedError({ no: 'Ingen aktiv organisasjon er tilgjengelig for import.', en: 'No active organization is available for import.' })")
    expect(body).toContain("throw new LocalizedError({ no: 'Midlertidig chat kan ikke lagre filer i kunnskapsbasen.', en: 'Temporary chat cannot save files to the knowledge base.' })")
    expect(body).toContain('throw new LocalizedError({ no: `Kunne ikke lese ${attachment.name}.`, en: `Could not read ${attachment.name}.` })')
    expect(body).toContain("throw new LocalizedError({ no: 'Ingen filer er valgt.', en: 'No files are selected.' })")
  })
})
