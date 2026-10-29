import { Show } from 'solid-js'

import { Badge } from '@/shared/ui/Badge'
import { isGateOpen } from '@/shared/context/ownership-gate'

const LABELS: Record<'private' | 'org' | 'shared', string> = {
  private: 'Private',
  org: 'Organization',
  shared: 'Shared',
}

/**
 * Renders a visibility badge derived from the SAME `visibility` value the
 * documents-api filters on (PR-2 column on `LiveKnowledgeSource`) — never a
 * separate display flag. Renders ONLY when the honesty gate is open AND a
 * visibility is present; otherwise nothing (honest empty). If a future change
 * tries to show this without the gate, the PR-6 DoD test must fail.
 */
export function PrivacyBadge(props: { visibility?: 'private' | 'org' | 'shared' }) {
  return (
    <Show when={isGateOpen() && props.visibility}>
      {(visibility) => (
        <Badge tone={visibility() === 'private' ? 'accent' : 'neutral'}>
          {LABELS[visibility()]}
        </Badge>
      )}
    </Show>
  )
}
