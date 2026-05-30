'use client';

import { useEffect, useState, type ReactElement } from 'react';
import { ThumbsUp, ThumbsDown, Meh, Check } from 'lucide-react';
import type { PlaygroundMessage } from './hooks/useAgentPlayground';

type Rating = NonNullable<PlaygroundMessage['rating']>;

interface PlaygroundRatingChipsProps {
  message: PlaygroundMessage;
  onRate: (rating: Rating) => Promise<boolean>;
  /**
   * When true, this is the most recent assistant turn — keyboard
   * shortcuts G/A/P bind to it.
   */
  isLatest: boolean;
}

const OPTIONS: ReadonlyArray<{ value: Rating; label: string; key: string; icon: typeof ThumbsUp; tone: string }> = [
  {
    value: 'good',
    label: 'Good',
    key: 'G',
    icon: ThumbsUp,
    tone: 'text-emerald-700 bg-emerald-50 ring-emerald-200 hover:bg-emerald-100',
  },
  {
    value: 'acceptable',
    label: 'Acceptable',
    key: 'A',
    icon: Meh,
    tone: 'text-amber-700 bg-amber-50 ring-amber-200 hover:bg-amber-100',
  },
  {
    value: 'poor',
    label: 'Poor',
    key: 'P',
    icon: ThumbsDown,
    tone: 'text-red-700 bg-red-50 ring-red-200 hover:bg-red-100',
  },
];

/**
 * Wave 11 §5 — Intercom Fin "Evaluate answer" pattern (Mobbin
 * `a4875514…94ca`). Three chips per assistant reply: Good / Acceptable /
 * Poor, with keyboard shortcuts G / A / P on the most recent turn.
 *
 * Persistence: `rateMessage` POSTs to `/api/agents/runs/{runId}/rate`,
 * which writes to Convex `agentRuns.rating`. The orchestrator's
 * nightly job feeds aggregate ratings back into the re-ranker so
 * "Poor" docs get down-weighted.
 */
export function PlaygroundRatingChips({
  message,
  onRate,
  isLatest,
}: PlaygroundRatingChipsProps): ReactElement | null {
  const [busy, setBusy] = useState<Rating | null>(null);

  const handleRate = async (rating: Rating): Promise<void> => {
    if (busy) return;
    setBusy(rating);
    try {
      await onRate(rating);
    } finally {
      setBusy(null);
    }
  };

  useEffect(() => {
    if (!isLatest || !message.runId) return;
    const onKey = (event: KeyboardEvent): void => {
      // Don't hijack when the user is typing.
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.isContentEditable)
      ) {
        return;
      }
      const key = event.key.toLowerCase();
      const match = OPTIONS.find((opt) => opt.key.toLowerCase() === key);
      if (match) {
        event.preventDefault();
        void handleRate(match.value);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isLatest, message.runId]);

  // No run id means there's nothing to attach a rating to (welcome
  // message or errored turn).
  if (!message.runId) return null;

  return (
    <div className="mt-1 flex items-center gap-1">
      {OPTIONS.map((opt) => {
        const Icon = opt.icon;
        const active = message.rating === opt.value;
        const loading = busy === opt.value;
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => handleRate(opt.value)}
            disabled={Boolean(busy)}
            aria-pressed={active}
            aria-label={`Rate as ${opt.label}`}
            title={`${opt.label} (${opt.key})`}
            className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ring-1 ring-inset transition disabled:opacity-50 ${
              active
                ? `${opt.tone} ring-2`
                : 'bg-white text-[#6B7280] ring-[#E5E7EB] hover:bg-[#F9FAFB] hover:text-[#111827]'
            }`}
          >
            {active ? <Check className="size-3" /> : <Icon className="size-3" />}
            {opt.label}
            {isLatest ? (
              <span className="font-mono text-[9px] text-[#9CA3AF]">{opt.key}</span>
            ) : null}
            {loading ? <span className="ml-0.5 size-1 animate-pulse rounded-full bg-current" /> : null}
          </button>
        );
      })}
    </div>
  );
}
