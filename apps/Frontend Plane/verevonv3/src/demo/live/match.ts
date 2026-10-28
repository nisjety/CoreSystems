/**
 * Deterministic matching of a visitor's message to a scenario. No model and no
 * network: the exact suggestion text wins outright, otherwise the scenario
 * whose trigger groups the message contains most of, by whole words, and
 * nothing else. Free text that matches no case gets the fallback.
 */
import type { Scenario } from './scenario-types'

/** Score of a message that is exactly a scenario's suggestion. */
export const EXACT_SCORE = 100

/**
 * Lowercase ASCII words: æ, ø and å become ae, o and a, other diacritics go,
 * and every run of punctuation or spaces becomes one space. Both the message
 * and the trigger terms pass through it, so «oppfølging» matches «oppfolging».
 */
export function normalizeText(text: string): string {
  return text
    .toLowerCase()
    .replace(/æ/g, 'ae')
    .replace(/ø/g, 'o')
    .replace(/å/g, 'a')
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, ' ')
    .trim()
}

export type ScenarioMatch = { scenario: Scenario; score: number }

/**
 * The best scenario for a message, or null when none applies (the caller then
 * answers with the honest fallback). Ties keep the earlier scenario in the list.
 */
export function matchScenario(text: string, scenarios: readonly Scenario[]): ScenarioMatch | null {
  const normal = normalizeText(text)
  if (!normal) return null
  let best: ScenarioMatch | null = null
  for (const scenario of scenarios) {
    const score = normalizeText(scenario.suggestion.text) === normal
      ? EXACT_SCORE
      : scoreTriggers(normal, scenario.triggers)
    if (score > 0 && (!best || score > best.score)) best = { scenario, score }
  }
  return best
}

/**
 * One point per trigger group whose every term is in the message. A term is a
 * whole word of the message, or a stem when it ends with `*`: «stol*» takes
 * «stoler» and «stolene», never «bord». A substring never counts, so «bord» does
 * not fire on «bordplate». `normal` is a `normalizeText` result.
 */
export function scoreTriggers(normal: string, groups: readonly string[][]): number {
  const words = normal.split(' ').filter((word) => word.length > 0)
  let score = 0
  for (const group of groups) {
    if (group.length > 0 && group.every((term) => termMatches(term, words))) score += 1
  }
  return score
}

/** Whether a trigger term is one of the message's words, or starts one when it is a stem (see scoreTriggers). */
export function termMatches(term: string, words: readonly string[]): boolean {
  const stem = term.endsWith('*')
  const word = normalizeText(stem ? term.slice(0, -1) : term)
  if (!word) return false
  return stem ? words.some((item) => item.startsWith(word)) : words.includes(word)
}

/** A thread title from the first message: one line, at most 60 characters. */
export function titleFromQuestion(text: string): string {
  const clean = text.replace(/\s+/g, ' ').trim()
  if (!clean) return 'Ny samtale'
  return clean.length > 60 ? `${clean.slice(0, 59).trimEnd()}…` : clean
}
