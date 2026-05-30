'use client';

import { useState } from 'react';
import Link from 'next/link';
import { BotIcon, Loader2, Plus, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface AgentsEmptyStateProps {
  onSeedDemo?: () => Promise<void>;
}

export function AgentsEmptyState({ onSeedDemo }: AgentsEmptyStateProps) {
  const [seeding, setSeeding] = useState(false);
  return (
    <div className="flex min-h-[34rem] items-center justify-center">
      <div className="w-full max-w-[680px] overflow-hidden rounded-[36px] border border-black/10 bg-white/88 shadow-[0_24px_70px_rgba(33,38,52,0.10)] backdrop-blur">
        <div className="relative flex h-[240px] items-center justify-center overflow-hidden bg-[linear-gradient(135deg,#f3dfd8,#fbf8f4_50%,#dee4ef)]">
          <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(255,255,255,0.85),transparent_34%),radial-gradient(circle_at_bottom_right,rgba(255,255,255,0.45),transparent_30%)]" />
          <div className="relative grid size-24 place-items-center rounded-[28px] border border-white/80 bg-white/88 shadow-[0_18px_40px_rgba(33,38,52,0.08)]">
            <BotIcon className="size-11 text-[#7b7f87]" strokeWidth={1.2} />
          </div>
        </div>

        <div className="flex flex-col items-center gap-4 px-10 py-12 text-center">
          <div className="inline-flex items-center gap-2 rounded-full bg-[#f5f1eb] px-4 py-1.5 text-[11px] font-semibold uppercase tracking-[0.2em] text-[#9a8c79]">
            <Sparkles className="size-3.5" />
            Agent studio
          </div>
          <h2
            className="max-w-[12ch] text-[48px] leading-[0.96] tracking-[-0.05em] text-[#24262d]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Build the first agent for this workspace
          </h2>
          <p className="max-w-[52ch] text-[15px] leading-7 text-[#666b76]">
            Start with a prebuilt role, then tailor training, testing, and deployment from a dedicated workspace designed to match the rest of the dashboard.
          </p>
          <div className="flex items-center gap-3">
            <Button
              asChild
              className="h-12 rounded-full bg-[#111318] px-6 text-white hover:bg-[#090b0f]"
            >
              <Link href="/agents/create">
                <Plus className="size-4" />
                Create your first agent
              </Link>
            </Button>
            {onSeedDemo && (
              <Button
                variant="outline"
                disabled={seeding}
                onClick={async () => {
                  setSeeding(true);
                  try { await onSeedDemo(); } finally { setSeeding(false); }
                }}
                className="h-12 rounded-full border-black/12 px-6 text-[#444]"
              >
                {seeding ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Sparkles className="size-4" />
                )}
                Load demo agents
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
