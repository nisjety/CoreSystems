import { workspaceSettingsSections, type SectionDetail } from '@/features/settings/lib/settings-sections'

// Search ranks whole sections, because a section is the page a person jumps to.
// Queries and fields are folded the same way, so "læring", "laering" and "LARING"
// find the same section. Every word of a query must match somewhere in a section;
// the score adds the best match per word. A section's own name and id outrank a
// synonym that other sections also list, so "minne" opens Minne before Org security.

type MatchScores = { exact: number; word: number; part: number }

const nameScores: MatchScores = { exact: 100, word: 70, part: 30 }
const synonymScores: MatchScores = { exact: 90, word: 60, part: 20 }

// A fragment shorter than this matches only the start of a word, so one letter
// does not pull in every section that contains it.
const minPartLength = 3

export function searchSettingsSections(
  query: string,
  sections: readonly SectionDetail[] = workspaceSettingsSections,
): SectionDetail[] {
  const words = fold(query).split(' ').filter(Boolean)
  if (words.length === 0) return []

  return sections
    .map((section, order) => ({ section, order, score: scoreSection(section, words) }))
    .filter((hit) => hit.score > 0)
    .sort((a, b) => b.score - a.score || a.order - b.order)
    .map((hit) => hit.section)
}

function scoreSection(section: SectionDetail, words: readonly string[]): number {
  const names = [section.id, section.label, section.title].map(fold)
  const synonyms = section.keywords.map(fold)
  let score = 0
  for (const word of words) {
    const best = Math.max(
      0,
      ...names.map((name) => scoreField(name, word, nameScores)),
      ...synonyms.map((synonym) => scoreField(synonym, word, synonymScores)),
    )
    if (best === 0) return 0
    score += best
  }
  return score
}

function scoreField(field: string, word: string, scores: MatchScores): number {
  if (field === word) return scores.exact
  if (field.split(' ').some((part) => part.startsWith(word))) return scores.word
  if (word.length >= minPartLength && field.includes(word)) return scores.part
  return 0
}

// NFD removes the ring of å and the accents of other letters; ø and æ do not
// decompose, so they are folded by hand, and hyphens count as spaces.
function fold(value: string): string {
  return value
    .toLowerCase()
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .replace(/ø/g, 'o')
    .replace(/æ/g, 'ae')
    .replace(/[\s-]+/g, ' ')
    .trim()
}
