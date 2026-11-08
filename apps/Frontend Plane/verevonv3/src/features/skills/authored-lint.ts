import { skillLintMessage } from '@/features/skills/skill-lint'

type Translate = (noText: string, enText: string) => string

/**
 * What a lint finding means on a template or preset (EP-11 S6). The kind rules are new codes of the
 * same authored-behaviour lint; every other code reads as it does for a skill.
 */
export function authoredLintMessage(code: string, tr: Translate): string {
  switch (code) {
    case 'template_placeholder_invalid':
      return tr(
        'En mal kan bare ha plassholderen {{input}}.',
        'A template can only hold the placeholder {{input}}.',
      )
    case 'preset_settings_invalid':
      return tr(
        'Innstillingene er ugyldige: planlegging må være på, og høyst fire ferdigheter kan velges.',
        'The settings are invalid: planning must stay on, and at most four skills can be picked.',
      )
    case 'preset_unknown_key':
      return tr(
        'En forhåndsinnstilling kan bare sette modus, nettsøk, ferdigheter og mal.',
        'A preset can only set the mode, web search, skills and a template.',
      )
    default:
      return skillLintMessage(code, tr)
  }
}
