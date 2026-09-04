import { describe, expect, it } from 'vitest'
import { createPreview, truncateText } from '@/features/chat/components/chat-media-markdown'

// Regression: both helpers sliced to `max - 1` and then appended "..." — three
// characters — so any over-long input came back `max + 2` long. The thread
// snapshot preview is built with `createPreview(content, 180)` and the gateway
// rejects previews over 180 characters ("preview must not exceed 180
// characters"), so every assistant reply longer than 180 characters made
// `PUT /api/v1/chat/threads/:id` fail with 400 and the thread title/preview
// silently never persisted.
describe('chat preview truncation', () => {
  const long = 'x'.repeat(400)

  it('createPreview never exceeds max, ellipsis included', () => {
    for (const max of [34, 48, 58, 96, 180]) {
      const preview = createPreview(long, max)
      expect(preview.length).toBeLessThanOrEqual(max)
      expect(preview.endsWith('...')).toBe(true)
    }
  })

  it('truncateText never exceeds max, ellipsis included', () => {
    for (const max of [34, 120, 260]) {
      expect(truncateText(long, max).length).toBeLessThanOrEqual(max)
    }
  })

  it('leaves short content untouched apart from whitespace collapsing', () => {
    expect(createPreview('  hei   verden ', 34)).toBe('hei verden')
    expect(truncateText('a  b', 10)).toBe('a b')
  })

  it('fits the gateway thread-preview contract for a realistic reply', () => {
    const reply = 'For å hente nøyaktige fraktpriser trenger jeg noen flere opplysninger: avsendernavn, mottakernavn og pakkens mål i cm. Du oppga allerede 2 kg som vekt, så resten kan vi fylle ut sammen nå.'
    expect(reply.length).toBeGreaterThan(180)
    expect(createPreview(reply, 180).length).toBeLessThanOrEqual(180)
  })
})
