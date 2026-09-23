import { describe, expect, it } from 'vitest'
import { appendInlinedAttachments, contentWithPreparedSources, messageToTurn, splitInlinedAttachments, transcriptTurnToChatTurn } from './chat-normalizers'

describe('attachment restoration from durable thread content', () => {
  const prompt = 'Lag et svar om FF-1042.'
  const content = appendInlinedAttachments(prompt, [{ name: 'kildepakke.md', content: 'K3: 9 pakket, 3 venter.' }])
  it('shows the original prompt and restores the readable source separately', () => {
    const turn = messageToTurn({ id: 'one', role: 'user', content, createdAt: '' })
    expect(turn.content).toBe(prompt)
    expect(turn.attachments).toHaveLength(1)
    expect(turn.attachments[0]).toMatchObject({ name: 'kildepakke.md', extractedText: 'K3: 9 pakket, 3 venter.' })
    expect(contentWithPreparedSources(turn.content, turn.attachments)).toBe(content)
    expect(contentWithPreparedSources(content, turn.attachments)).toBe(content)
    expect(contentWithPreparedSources('Et kortere svar.', turn.attachments)).toContain('K3: 9 pakket, 3 venter.')
  })
  it('restores server transcript sources when local metadata has been cleared', () => {
    expect(transcriptTurnToChatTurn({ id: 'one', role: 'user', content, createdAt: '' }).attachments).toHaveLength(1)
  })
  it('labels a PDF text copy honestly when no original binary is retained', () => {
    const turn = messageToTurn({ id: 'one', role: 'user', content: appendInlinedAttachments(prompt, [{ name: 'source.pdf', content: 'Full extracted text' }]), createdAt: '' })
    expect(turn.attachments[0]).toMatchObject({ name: 'source.pdf.txt', type: 'text/plain' })
  })
  it('never hides incomplete or unrecognized suffix text', () => {
    for (const value of [content.replace('SLUTT PÅ', 'SLUTT'), `${content}\nImportant extra instruction`]) {
      expect(splitInlinedAttachments(value)).toEqual({ text: value, attachments: [] })
    }
  })
})
