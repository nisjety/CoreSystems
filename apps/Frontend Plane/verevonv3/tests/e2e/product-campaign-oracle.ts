// Extraction of the customer-facing copy from one campaign piece (a LinkedIn
// post or the e-mail body) for the journey-03 oracle in product-scenarios.spec.ts.
//
// The model shows which product information a text builds on, as the brief
// asks. It puts that note either BELOW the copy or, as in Q16g take 1, in a
// metadata block ABOVE it:
//
//   **Status:** Utkast til gjennomgang
//   **Kanal:** LinkedIn
//   **Foreslått dato:** 21. september 2026
//   **Vinkel:** ...
//   **Produktinformasjon teksten bygger på:** ...
//
//   Når dere setter opp en ny arbeidsplass, ...
//
// The oracle used to cut the piece at the first source note, which is right
// for a note below the copy and throws the whole copy away for a note above
// it: the "piece" then held only the metadata lines it did not recognise
// (7 words, "piece 1: 7 words" against a 60-word minimum) while the stored
// document was complete. A metadata block at the top of a piece is now removed
// first; a note below the copy still ends the copy as before.

/** The label of a metadata line: `**Kanal:** LinkedIn`, `Vinkel: ...`, `**Kanal**: ...`. */
const METADATA_LABEL = /^\s*(?:#{1,6}\s*)?\*{0,2}(?:Status|Kanal|Foreslått dato|Dato|Vinkel|Avsender|Rekkevidde(?:\s*\([^)]*\))?|Produktinformasjon(?:[^:\n]*)?|Kildenoter|Kildegrunnlag|Faktagrunnlag|Bygger på)\s*\*{0,2}\s*:/i
/** A label whose value follows as a list on the next lines. */
const LABEL_WITHOUT_VALUE = /^\s*[^:\n]*:\s*\*{0,2}\s*$/
const LIST_ITEM = /^\s*(?:[-*•]|\d+[.)])\s+/

function withoutLeadingMetadata(text: string) {
  const lines = text.split(/\r?\n/)
  let index = 0
  let listFollows = false
  while (index < lines.length) {
    const line = lines[index]
    if (line.trim() === '') { index++; continue }
    if (METADATA_LABEL.test(line)) { listFollows = LABEL_WITHOUT_VALUE.test(line); index++; continue }
    if (listFollows && LIST_ITEM.test(line)) { index++; continue }
    break
  }
  return lines.slice(index).join('\n')
}

// A post can carry a sub-section under its text, `### Kilder`, that shows which
// product information the post builds on (the brief asks for it). A revision
// of one post must not delete another post's note, nor that post's own: Q16h
// (section-only output) lost the note of the revised post in 3 of 3 takes that
// had one, and the copy checks passed each of them.
const POST_DAYS = [21, 23, 25]

/** The sub-heading titles (lower case) under each post whose heading names its day, by day. */
export function postNotes(document: string) {
  const headings = document.split(/\r?\n/)
    .map(line => /^(#{1,6})\s+(.+?)\s*$/.exec(line))
    .filter((match): match is RegExpExecArray => match !== null)
  const notes = new Map<number, string[]>()
  headings.forEach((heading, at) => {
    const title = heading[2]
    const day = POST_DAYS.find(day => new RegExp(`(?:LinkedIn|innlegg).*\\b${day}\\b|^${day}\\.`, 'i').test(title))
    if (day === undefined || notes.has(day)) return
    const children: string[] = []
    for (const next of headings.slice(at + 1)) {
      if (next[1].length <= heading[1].length) break
      children.push(next[2].toLowerCase())
    }
    notes.set(day, children)
  })
  return notes
}

/** The days whose post had a sub-section in `before` and has none in `after`. */
export function lostPostNotes(before: string, after: string) {
  const kept = postNotes(after)
  return [...postNotes(before)]
    .filter(([day, notes]) => notes.length > 0 && (kept.get(day)?.length ?? 0) === 0)
    .map(([day]) => day)
}

export function copyBody(text: string) {
  return text.split(/\r?\n/).filter(line => !/^\s*(?:#{1,6}\s|---+$|\*?\*?(?:Rapportdato|Dato|Vinkel|Status|(?:Antall )?ord(?:antall|telling|tall)?(?:\s+brødtekst)?(?:\s*\([^)]*\))?|Maks emneknagger|Rekkevidde(?:\s*\([^)]*\))?|Kilde(?:r|grunnlag)?|Produktinformasjon|Bygger på)\s*\*?\*?\s*:)/i.test(line))
    .join('\n').replace(/[*`]/g, '').trim()
}

export function campaignCopy(text: string) {
  const piece = withoutLeadingMetadata(text)
  // A source note may sit below each piece instead of in one final appendix.
  // Its explicit label separates internal evidence from customer-facing copy.
  const sourceNote = /^\s*(?:(?:#{1,6}\s*)?(?:\*\*)?(?:Produktinformasjon(?:\s+[^\n]*)?|Kildenoter|Kildegrunnlag|Faktagrunnlag|Bygger på)[^\n]*|#{1,6}\s+(?:Kilder|Sources)(?:[\s/][^\n]*)?)$/im.exec(piece)
  return copyBody(sourceNote ? piece.slice(0, sourceNote.index) : piece)
    .split(/\r?\n/).filter(line => !/^\s*Produktgrunnlag(?:\s*\([^)]*\))?\s*:|^\s*\(?\d+\s+ord\)?\s*$/i.test(line)).join('\n').trim()
}
