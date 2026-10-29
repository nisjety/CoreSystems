import type { TenderProfile } from '@/shared/api/anbud-client'

/** The profile form while the user types: text, as typed. */
export type ProfileDraft = {
  orgNumber: string
  cpv: string
  nuts: string
  keywords: string
  valueMin: string
  valueMax: string
}

export const emptyProfileDraft: ProfileDraft = {
  orgNumber: '',
  cpv: '',
  nuts: '',
  keywords: '',
  valueMin: '',
  valueMax: '',
}

export type ProfileField = 'orgNumber' | 'cpv' | 'nuts' | 'keywords' | 'value'

export type ProfileProblem = {
  field: ProfileField
  message: { no: string; en: string }
}

export type ParsedProfile = { profile: TenderProfile } | { problems: ProfileProblem[] }

const MAX_KEYWORD_LENGTH = 80

/** Codes are separated by commas, semicolons or spaces. */
function codes(text: string): string[] {
  return text.split(/[\s,;]+/).filter(Boolean)
}

/** Keywords are separated by commas, since a keyword may hold a space. */
function keywords(text: string): string[] {
  return text
    .split(',')
    .map((word) => word.trim())
    .filter(Boolean)
}

/** A whole amount in kroner, undefined when the field is empty, and null when it is not a whole amount. */
function amount(text: string): number | undefined | null {
  const trimmed = text.trim()
  if (!trimmed) return undefined
  return /^\d+$/.test(trimmed) ? Number(trimmed) : null
}

/**
 * Reads the form into a profile, or says what is wrong with it. Only the shape is checked here: the server
 * still judges every code, and says so in its own words when one is not known.
 */
export function parseProfile(draft: ProfileDraft): ParsedProfile {
  const problems: ProfileProblem[] = []
  const orgNumber = draft.orgNumber.replace(/\s/g, '')
  const cpv = codes(draft.cpv)
  const nuts = codes(draft.nuts).map((code) => code.toUpperCase())
  const words = keywords(draft.keywords)
  const min = amount(draft.valueMin)
  const max = amount(draft.valueMax)

  if (orgNumber && !/^\d{9}$/.test(orgNumber)) {
    problems.push({
      field: 'orgNumber',
      message: { no: 'Organisasjonsnummeret har ni sifre.', en: 'The organisation number has nine digits.' },
    })
  }
  if (cpv.some((code) => !/^\d{8}(-\d)?$/.test(code))) {
    problems.push({
      field: 'cpv',
      message: {
        no: 'CPV-koder har åtte sifre, for eksempel 72212000.',
        en: 'CPV codes have eight digits, for example 72212000.',
      },
    })
  }
  if (nuts.some((code) => !/^[A-Z]{2}[A-Z0-9]{1,3}$/.test(code))) {
    problems.push({
      field: 'nuts',
      message: { no: 'Regionkoder ser ut som NO08 eller NO081.', en: 'Region codes look like NO08 or NO081.' },
    })
  }
  if (words.some((word) => word.length > MAX_KEYWORD_LENGTH)) {
    problems.push({
      field: 'keywords',
      message: {
        no: `Hvert nøkkelord er høyst ${MAX_KEYWORD_LENGTH} tegn.`,
        en: `Each keyword is at most ${MAX_KEYWORD_LENGTH} characters.`,
      },
    })
  }
  if (min === null || max === null || (min !== undefined && max !== undefined && min > max)) {
    problems.push({
      field: 'value',
      message: {
        no: 'Verdien er hele kroner, og fra er ikke større enn til.',
        en: 'The value is whole kroner, and "from" is not larger than "to".',
      },
    })
  }
  if (cpv.length === 0 && words.length === 0) {
    problems.push({
      field: 'cpv',
      message: { no: 'Legg inn minst én CPV-kode eller ett nøkkelord.', en: 'Add at least one CPV code or one keyword.' },
    })
  }
  if (problems.length > 0) return { problems }

  const hasValue = min !== undefined || max !== undefined
  return {
    profile: {
      orgNumber: orgNumber || undefined,
      cpv,
      nuts,
      keywords: words,
      valueRange: hasValue ? { currency: 'NOK', min: min ?? undefined, max: max ?? undefined } : undefined,
    },
  }
}

/** The words that say where a field of the profile came from, for the form to show beside the field. */
export function sourceLabel(source?: string): { no: string; en: string } | undefined {
  switch (source) {
    case 'organisation':
      return { no: 'Fra organisasjonen', en: 'From the organisation' }
    case 'wins':
      return { no: 'Foreslått fra vunne anbud', en: 'Suggested from tenders won' }
    case 'user':
      return { no: 'Lagt inn av deg', en: 'Entered by you' }
    default:
      return undefined
  }
}

/** A saved profile read back into the form: codes and keywords joined the way the form takes them. */
export function draftFromProfile(profile: TenderProfile): ProfileDraft {
  return {
    orgNumber: profile.orgNumber ?? '',
    cpv: profile.cpv.join(', '),
    nuts: profile.nuts.join(', '),
    keywords: profile.keywords.join(', '),
    valueMin: profile.valueRange?.min?.toString() ?? '',
    valueMax: profile.valueRange?.max?.toString() ?? '',
  }
}
