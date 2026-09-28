// Strips the markdown decoration a stored assistant reply carries (**bold**,
// `code`, #headings, list/quote markers) so a one-line preview reads as plain
// prose instead of showing literal punctuation. The full reply still renders
// as real markdown wherever it's read in full.
//
// Shared: the Space room timeline uses it for thread previews, and governed
// read-aloud (`speakableText` in shared/api/speech-client) builds on it.
export function stripMarkdownPreview(content: string): string {
  return content
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/^[-*+]\s+/gm, '')
    .replace(/^>\s?/gm, '')
    .replace(/(\*\*|__)(.*?)\1/g, '$2')
    .replace(/(\*|_)(.*?)\1/g, '$2')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/\s+/g, ' ')
    .trim()
}
