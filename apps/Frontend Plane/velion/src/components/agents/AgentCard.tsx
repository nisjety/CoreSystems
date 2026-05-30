'use client';

import Link from 'next/link';
import Image from 'next/image';
import { ArrowUpRight, Sparkles } from 'lucide-react';
import { cn } from '@/lib/utils';
import { USE_CASE_LABELS, type Agent } from './types';
import type { AgentWithMeta } from './data';

function formatCreatedDate(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const diffDays = Math.floor((now.getTime() - d.getTime()) / (1000 * 60 * 60 * 24));
  if (diffDays === 0) return 'Today';
  if (diffDays === 1) return 'Yesterday';
  if (diffDays < 7) return `${diffDays} days ago`;
  return d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
}

interface AgentCardProps {
  agent: Agent | AgentWithMeta;
}

export function AgentCard({ agent }: AgentCardProps) {
  const useCaseLabel = USE_CASE_LABELS[agent.useCase] ?? agent.useCase;
  const isActive = agent.status === 'active';
  const previewImage = 'previewImage' in agent ? agent.previewImage : '/imagens/arched-corridor-1.jpeg';
  const gradient = 'gradient' in agent
    ? agent.gradient
    : 'from-[#f3ded7] via-[#fbf6f2] to-[#d8ddea]';
  const badgeLabel = 'badgeLabel' in agent ? agent.badgeLabel : useCaseLabel;
  const toolCount = agent.tools.length;

  return (
    <Link href={`/agents/${agent.id}`} className="group block h-full">
      <div
        className={cn(
          'flex h-full flex-col overflow-hidden rounded-[28px] border border-black/10 bg-white/90 p-2.5 transition-all duration-300',
          'shadow-[0_8px_30px_rgba(19,25,39,0.06)] hover:-translate-y-0.5 hover:shadow-[0_20px_50px_rgba(19,25,39,0.12)]',
        )}
      >
        <div className={cn('relative overflow-hidden rounded-[24px] border border-black/8 bg-gradient-to-br', gradient)}>
          <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(255,255,255,0.75),transparent_38%),radial-gradient(circle_at_bottom_right,rgba(255,255,255,0.5),transparent_35%)]" />
          <div className="relative aspect-[1.45] overflow-hidden rounded-[24px]">
            <Image
              src={previewImage}
              alt={agent.name}
              fill
              sizes="(max-width: 768px) 100vw, (max-width: 1280px) 50vw, 30vw"
              className="object-cover opacity-55 saturate-[0.8] transition-transform duration-700 group-hover:scale-[1.03]"
            />
            <div className="absolute inset-0 bg-white/45" />
            <div className="absolute left-5 top-5 inline-flex items-center gap-2 rounded-full bg-white/85 px-3 py-1.5 text-[11px] font-medium text-[#2b2d33] shadow-[0_8px_18px_rgba(15,23,42,0.08)] backdrop-blur">
              <Sparkles className="size-3.5" strokeWidth={1.8} />
              {badgeLabel}
            </div>
            <div className="absolute inset-x-5 bottom-5 rounded-[22px] border border-white/70 bg-white/82 p-4 shadow-[0_18px_40px_rgba(17,24,39,0.10)] backdrop-blur">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="truncate text-[22px] font-semibold tracking-[-0.04em] text-[#202228]">
                    {agent.name}
                  </p>
                  <p className="mt-1 text-[13px] text-[#666a74]">{useCaseLabel}</p>
                </div>
                <span
                  className={cn(
                    'rounded-full px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.18em]',
                    isActive ? 'bg-[#edf7ef] text-[#24613b]' : 'bg-[#f3efe8] text-[#80634c]',
                  )}
                >
                  {isActive ? 'Live' : 'Draft'}
                </span>
              </div>
            </div>
          </div>
        </div>

        <div className="flex flex-1 flex-col px-3 pb-3 pt-4">
          <div className="flex items-center gap-2 text-[11px] uppercase tracking-[0.22em] text-[#9a8f80]">
            <span>{agent.model}</span>
            <span className="size-1 rounded-full bg-[#d7ccbd]" />
            <span>{toolCount} tools</span>
          </div>

          <div className="mt-3 flex-1">
            {agent.description ? (
              <p className="line-clamp-3 text-[14px] leading-6 text-[#666a74]">
                {agent.description}
              </p>
            ) : (
              <p className="text-[13px] text-[#A2A6B1]">No description</p>
            )}
          </div>

          <div className="mt-5 flex items-center justify-between border-t border-black/6 pt-4">
            <div>
              <p className="text-[11px] uppercase tracking-[0.18em] text-[#a2a6b1]">Updated</p>
              <p className="mt-1 text-[13px] text-[#2a2d34]">{formatCreatedDate(agent.createdAt)}</p>
            </div>
            <div className="flex items-center gap-2 rounded-full border border-black/10 bg-[#fcfaf7] px-3 py-2 text-[12px] font-medium text-[#2a2d34] transition-colors group-hover:bg-[#f6f1eb]">
              Open workspace
              <ArrowUpRight className="size-4" strokeWidth={1.8} />
            </div>
          </div>
        </div>
      </div>
    </Link>
  );
}
