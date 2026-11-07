import { For } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { skillLintMessage, type SkillLintFinding } from '@/features/skills/skill-lint'

/**
 * The skill lint findings for one field, shown next to it (EP-11 S4). An error is why the save
 * was refused; a warning is a soft note on a skill that was saved.
 */
export function SkillLintNotes(props: { findings: SkillLintFinding[]; errorClass: string; noteClass: string }) {
  const i18n = useI18n()
  return (
    <For each={props.findings}>
      {(finding) => (
        <p class={finding.severity === 'error' ? props.errorClass : props.noteClass} data-lint-code={finding.code}>
          {skillLintMessage(finding.code, i18n.tr)}
        </p>
      )}
    </For>
  )
}
