import { ApiError } from '@/shared/api/api-error'

/**
 * The skill lint (EP-11 S4) as the editor shows it. capability-core refuses a write that breaks
 * a rule with 422 `skill_lint_failed` and `details.findings`, and adds `warnings` to the body of
 * a write that went ahead. A finding is a closed rule code and the field it is about, never the
 * text, so the words the person reads are written here.
 */
export interface SkillLintFinding {
  code: string
  field: string
  severity: 'error' | 'warn'
}

export const SKILL_LINT_FAILED = 'skill_lint_failed'

type Translate = (noText: string, enText: string) => string

function asFindings(value: unknown): SkillLintFinding[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((entry) => {
    if (!entry || typeof entry !== 'object') return []
    const { code, field, severity } = entry as Record<string, unknown>
    if (typeof code !== 'string' || typeof field !== 'string') return []
    if (severity !== 'error' && severity !== 'warn') return []
    return [{ code, field, severity }]
  })
}

/** The findings of a write the lint refused; empty for any other failure. */
export function skillLintRefusal(err: unknown): SkillLintFinding[] {
  if (!(err instanceof ApiError) || err.code !== SKILL_LINT_FAILED || Array.isArray(err.details)) return []
  return asFindings(err.details?.findings)
}

/** The findings a write that went ahead carried back; empty while the lint is off. */
export function skillLintWarnings(body: unknown): SkillLintFinding[] {
  if (!body || typeof body !== 'object') return []
  return asFindings((body as { warnings?: unknown }).warnings)
}

/** The findings about one field, so each is shown next to it. */
export function findingsFor(findings: SkillLintFinding[], field: string): SkillLintFinding[] {
  return findings.filter((finding) => finding.field === field)
}

/** What a finding means to the person writing the skill. */
export function skillLintMessage(code: string, tr: Translate): string {
  switch (code) {
    case 'name_empty':
      return tr('Navnet kan ikke være tomt.', 'The name cannot be empty.')
    case 'name_too_long':
      return tr('Navnet kan ha høyst 64 tegn.', 'The name can have at most 64 characters.')
    case 'name_charset':
      return tr(
        'Navnet kan ikke starte eller slutte med mellomrom eller ha kontrolltegn.',
        'The name cannot start or end with a space or hold control characters.',
      )
    case 'description_missing':
      return tr(
        'En kort beskrivelse gjør ferdigheten lettere å finne igjen.',
        'A short description makes the skill easier to find again.',
      )
    case 'description_too_long':
      return tr('Beskrivelsen kan ha høyst 300 tegn.', 'The description can have at most 300 characters.')
    case 'content_empty':
      return tr('Innholdet kan ikke være tomt.', 'The content cannot be empty.')
    case 'content_too_long':
      return tr('Innholdet kan ha høyst 32 000 tegn.', 'The content can have at most 32,000 characters.')
    case 'content_exceeds_budget':
      return tr(
        'Innhold over 8 000 tegn kuttes når ferdigheten brukes i en samtale.',
        'Content over 8,000 characters is cut when the skill is used in a chat.',
      )
    case 'transcript_shape':
      return tr(
        'Dette ser ut som en samtalelogg. Skriv lærdommen, ikke loggen.',
        'This looks like a chat log. Write the lesson, not the log.',
      )
    case 'trigger_keyword_count':
      return tr('En ferdighet kan ha høyst 20 utløser-nøkkelord.', 'A skill can have at most 20 trigger keywords.')
    case 'trigger_keyword_too_long':
      return tr(
        'Hvert utløser-nøkkelord kan ha høyst 40 tegn.',
        'Each trigger keyword can have at most 40 characters.',
      )
    case 'tool_restriction_invalid':
      return tr('Hver verktøybegrensning må være ett verktøynavn.', 'Each tool restriction must be one tool name.')
    case 'frontmatter_name_mismatch':
      return tr(
        'Navnet i SKILL.md må være det samme som ferdighetens navn.',
        'The name in SKILL.md must match the skill name.',
      )
    default:
      return tr('Ferdigheten består ikke kontrollen.', 'The skill does not pass the check.')
  }
}
