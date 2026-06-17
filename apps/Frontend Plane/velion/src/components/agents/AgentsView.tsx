'use client';

import Image from 'next/image';
import Link from 'next/link';
import { Plus, Sparkles, WandSparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useAgents } from './hooks/useAgents';
import { AgentCard } from './AgentCard';
import { AgentsEmptyState } from './AgentsEmptyState';
import { AGENT_ROLE_CARDS } from './data';

function RoleShowcaseCard({
  role,
}: {
  role: (typeof AGENT_ROLE_CARDS)[number];
}) {
  const Icon = role.icon;

  return (
    <article className="group overflow-hidden rounded-[18px] border border-black/10 bg-white p-2.5 shadow-[0_2px_10px_rgba(0,0,0,0.05)] transition-all duration-300 hover:shadow-[0_4px_16px_rgba(0,0,0,0.08)]">
      <div className={`relative overflow-hidden rounded-[15px] bg-gradient-to-br ${role.accentClassName}`}>
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(255,255,255,0.8),transparent_35%),radial-gradient(circle_at_bottom_right,rgba(255,255,255,0.3),transparent_30%)]" />
        <div className="relative aspect-[1.42] overflow-hidden rounded-[15px]">
          <Image
            src={role.image}
            alt={role.name}
            fill
            sizes="(max-width: 1024px) 100vw, 24vw"
            className="object-cover opacity-70 transition-transform duration-700 group-hover:scale-[1.04]"
          />
          <div className="absolute inset-0 bg-white/45" />
          <div className="absolute inset-x-3 top-3 flex items-start justify-between gap-2">
            <div className="inline-flex items-center gap-1.5 rounded-full bg-white/85 px-2.5 py-1 text-[11px] font-medium text-[#24272d] shadow-[0_2px_8px_rgba(17,24,39,0.08)] backdrop-blur">
              <Icon className="size-3.5" strokeWidth={1.8} />
              {role.name}
            </div>
            {role.status === 'coming-soon' ? (
              <span className="rounded-full bg-[#111318] px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-white">
                Soon
              </span>
            ) : null}
          </div>
        </div>
      </div>

      <div className="px-2.5 pb-2.5 pt-3">
        <h3 className="text-[15px] font-semibold leading-snug text-[#1f2229]">
          {role.name}
        </h3>
        <p className="mt-1 text-[12px] text-[#2a2d34]">{role.strapline}</p>
        <p className="mt-1.5 text-[12px] leading-5 text-[#666b75]">{role.description}</p>

        <div className="my-3 h-px border-t border-dashed border-[#ddd6ca]" />

        <ul className="space-y-1 text-[12px] leading-5 text-[#2d3037]">
          {role.bullets.map((bullet) => (
            <li key={bullet} className="flex items-start gap-1.5">
              <span className="mt-[8px] size-1 rounded-full bg-[#2d3037]" />
              <span>{bullet}</span>
            </li>
          ))}
        </ul>

        <div className="mt-4">
          <button
            type="button"
            disabled={role.status === 'coming-soon'}
            className="rounded-full bg-[#22252c] px-3.5 py-2 text-[12px] font-semibold text-white transition-colors disabled:cursor-not-allowed disabled:bg-[#efebe4] disabled:text-[#8e938d]"
          >
            {role.ctaLabel}
          </button>
        </div>
      </div>
    </article>
  );
}

export function AgentsView() {
  const { data: agents, isLoading, refetch } = useAgents();
  const hasAgents = !!agents && agents.length > 0;

  async function handleSeedDemo() {
    await fetch('/api/agents/seed', { method: 'POST' });
    refetch();
  }

  return (
    <div className="relative h-full overflow-y-auto text-[#23252F]">
      <div className="relative px-4 py-6 md:px-6 md:py-7 xl:px-7">
        <div className="mx-auto max-w-[1460px]">
          <div className="mb-6 flex items-start justify-between gap-4">
            <div>
              <div className="inline-flex items-center gap-2 rounded-full border border-black/8 bg-[#FCFBF8]/96 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.22em] text-[#B96618] shadow-[0_2px_8px_rgba(17,24,39,0.04)]">
                <WandSparkles className="size-3" />
                Agent studio
              </div>
              <h1 className="mt-3 text-[28px] font-semibold leading-tight tracking-[-0.03em] text-[#1f2229] md:text-[34px]">
                Agent studio
              </h1>
              <p className="mt-1.5 max-w-[56ch] text-[13px] leading-6 text-[#666a74]">
                Prebuilt roles, custom agents, and tailored workspaces — all in one place.
              </p>
            </div>

            <Button
              asChild
              className="mt-1 shrink-0 rounded-full bg-[#111318] px-4 py-2 text-[13px] text-white hover:bg-[#090b0f]"
            >
              <Link href="/agents/create">
                <Plus className="size-3.5" />
                New agent
              </Link>
            </Button>
          </div>

          {!hasAgents && !isLoading ? (
            <AgentsEmptyState onSeedDemo={handleSeedDemo} />
          ) : (
            <div className="space-y-5">
              <section className="rounded-[24px] border border-black/8 bg-[#FCFBF8]/96 px-5 py-6 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
                <div className="mb-5 flex items-center gap-2">
                  <div className="inline-flex items-center gap-1.5 rounded-full bg-[#f5f1eb] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9a8d7a]">
                    <Sparkles className="size-3" />
                    Prebuilt roles
                  </div>
                  <h2 className="text-[16px] font-semibold tracking-[-0.02em] text-[#1f2229]">
                    Start from a role, then tailor it to your brand
                  </h2>
                </div>

                <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                  {AGENT_ROLE_CARDS.map((role) => (
                    <RoleShowcaseCard key={role.id} role={role} />
                  ))}
                </div>
              </section>

              <section className="rounded-[24px] border border-black/8 bg-[#FCFBF8]/96 px-5 py-6 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
                <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-[#9ea4ae]">
                      Configured agents
                    </div>
                    <h3 className="mt-1 text-[18px] font-semibold tracking-[-0.02em] text-[#1f2229]">
                      Your live and draft workspaces
                    </h3>
                  </div>
                  <p className="max-w-[48ch] text-[12px] leading-5 text-[#6c707b]">
                    Open any card to enter its tailored workspace with a role-specific sidebar and live preview canvas.
                  </p>
                </div>

                <ul className="grid gap-4 xl:grid-cols-3" role="list" aria-label="Agents">
                  {agents.map((agent) => (
                    <li key={agent.id}>
                      <AgentCard agent={agent} />
                    </li>
                  ))}
                </ul>
              </section>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
