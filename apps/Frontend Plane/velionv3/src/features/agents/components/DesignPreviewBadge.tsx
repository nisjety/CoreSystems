import { Eye } from 'lucide-solid'
import { cn } from '@/shared/lib/cn'

const PREVIEW_TITLE = 'Design preview — a visual concept. This surface is not yet connected to a backend, so its actions are inactive.'

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
  return (
    <span
      role="note"
      title={props.title ?? PREVIEW_TITLE}
      class={cn(
        'inline-flex items-center gap-1.5 rounded-full border border-[#E3D8B8] bg-[#FBF4DD] px-3 py-1 text-[11px] font-semibold text-[#7A6410] dark:border-[#4A411F] dark:bg-[#2A2614] dark:text-[#E4D08A]',
        props.class,
      )}
    >
      <Eye class="size-3.5" strokeWidth={2} />
      {props.label ?? 'Design preview'}
    </span>
  )
}

/** Honest tooltip for the agents operating-model (Blueprint) surface (A5). */
export const BLUEPRINT_BADGE_TITLE =
  'Blueprint — a reference operating model. This agent is not yet configured for this org, so its activation controls are inactive.'
