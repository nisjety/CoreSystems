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
 */
export function DesignPreviewBadge(props: { class?: string; label?: string }) {
  return (
    <span
      role="note"
      title={PREVIEW_TITLE}
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
