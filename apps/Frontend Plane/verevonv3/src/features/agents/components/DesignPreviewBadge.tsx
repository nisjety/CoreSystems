import { Eye } from '@/shared/icons'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

/**
 * Non-interactive badge marking a surface as a not-yet-wired design preview.
 *
 * Honesty contract (Phase 3 PR-1): wherever this badge appears, the
 * surface's action-implying controls (run / publish / activate / generate)
 * MUST be removed or disabled in the same change — the badge and the
 * neutralized controls ship together, never the badge alone.
 *
 * Phase 4 PR-3 (A5): the agent operating-model surface reuses this badge with
 * `label="Blueprint"` and a Blueprint-specific `title`, so it reads "Blueprint /
 * not yet configured for this org" instead of a generic design preview. There is
 * no per-org agent-config store yet (deferred to Phase 5), so the agent surface
 * carries no Active/Private/Live badge.
 */
export function DesignPreviewBadge(props: { class?: string; label?: string; title?: string }) {
  const i18n = useI18n()
  const defaultTitle = i18n.tr(
    'Designforhåndsvisning — et visuelt konsept. Denne flaten er ikke koblet til en backend ennå, så handlingene er inaktive.',
    'Design preview — a visual concept. This surface is not yet connected to a backend, so its actions are inactive.',
  )

  return (
    <span
      role="note"
      title={props.title ?? defaultTitle}
      class={cn(
        'inline-flex items-center gap-1.5 rounded-full border border-[#E3D8B8] bg-[#FBF4DD] px-3 py-1 text-[11px] font-semibold text-[#7A6410] dark:border-[#4A411F] dark:bg-[#2A2614] dark:text-[#E4D08A]',
        props.class,
      )}
    >
      <Eye class="size-3.5" strokeWidth={2} />
      {props.label ?? i18n.tr('Designforhåndsvisning', 'Design preview')}
    </span>
  )
}

/** Honest tooltip for the agents operating-model (Blueprint) surface (A5). */
export function blueprintBadgeTitle(tr: (noText: string, enText: string) => string): string {
  return tr(
    'Blueprint — en referansemodell for drift. Denne agenten er ikke konfigurert for denne organisasjonen ennå, så aktiveringskontrollene er inaktive.',
    'Blueprint — a reference operating model. This agent is not yet configured for this org, so its activation controls are inactive.',
  )
}
