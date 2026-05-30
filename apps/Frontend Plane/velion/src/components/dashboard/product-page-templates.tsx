'use client';

import type { ReactNode } from 'react';
import Link from 'next/link';
import { ArrowRight, ChevronRight } from 'lucide-react';

import { cn } from '@/lib/utils';

export type ProductMetric = {
  label: string;
  value: string;
  trend?: string;
  tone?: 'default' | 'accent' | 'warning' | 'success';
};

export type ProductAction = {
  label: string;
  href: string;
};

export type ProductListItem = {
  id: string;
  title: string;
  subtitle?: string;
  meta?: string;
  tone?: 'default' | 'accent' | 'warning' | 'success';
};

export function ProductPageShell({
  eyebrow,
  title,
  description,
  actions,
  children,
  headerAside,
  contentClassName,
}: {
  eyebrow: string;
  title: string;
  description: string;
  actions?: ProductAction[];
  children: ReactNode;
  headerAside?: ReactNode;
  contentClassName?: string;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto text-[#23252F]">
      <div className="px-4 py-6 md:px-6 md:py-7 xl:px-7">
        <div className={cn('mx-auto max-w-[1440px] space-y-6', contentClassName)}>
          <section className="rounded-[30px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/96 px-6 py-6 shadow-[0_22px_48px_rgba(22,20,17,0.04)] backdrop-blur-[6px] md:px-7 md:py-7">
            <div className="flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between">
              <div className="max-w-[760px]">
                <div className="text-[11px] font-semibold uppercase tracking-[0.28em] text-[var(--linear-accent)]">
                  {eyebrow}
                </div>
                <h1 className="mt-3 text-[34px] font-semibold tracking-[-0.05em] text-[#1F2229] md:text-[42px]">
                  {title}
                </h1>
                <p className="mt-3 max-w-[64ch] text-[14px] leading-7 text-[#666A73] md:text-[15px]">
                  {description}
                </p>
              </div>

              {headerAside ? <div className="lg:min-w-[240px]">{headerAside}</div> : null}
            </div>

            {actions?.length ? (
              <div className="mt-6 flex flex-wrap gap-3">
                {actions.map((action) => (
                  <Link
                    key={action.href}
                    href={action.href}
                    className="inline-flex items-center gap-2 rounded-full border border-[var(--linear-border)] bg-[var(--linear-panel-bg)] px-4 py-2.5 text-[13px] font-medium text-[#2E3445] shadow-[0_8px_18px_rgba(0,0,0,0.04)] transition-colors hover:border-[#D8D8DC] hover:bg-[#FBFBFC]"
                  >
                    {action.label}
                    <ArrowRight size={14} strokeWidth={2} />
                  </Link>
                ))}
              </div>
            ) : null}
          </section>

          {children}
        </div>
      </div>
    </div>
  );
}

export function MetricStrip({ metrics }: { metrics: ProductMetric[] }) {
  return (
    <section className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
      {metrics.map((metric) => (
        <article
          key={metric.label}
          className="rounded-[24px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/92 px-5 py-5 shadow-[0_18px_40px_rgba(22,20,17,0.04)]"
        >
          <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#9E978B]">
            {metric.label}
          </div>
          <div className="mt-3 flex items-end justify-between gap-4">
            <div className="text-[30px] font-semibold tracking-[-0.04em] text-[#1F2229]">
              {metric.value}
            </div>
            {metric.trend ? (
              <span
                className={cn(
                  'rounded-full px-2.5 py-1 text-[11px] font-semibold',
                  metric.tone === 'warning'
                    ? 'bg-[#FFF5E9] text-[#B96618]'
                    : metric.tone === 'success'
                      ? 'bg-[#EEF8F1] text-[#2D7A46]'
                      : metric.tone === 'accent'
                        ? 'bg-[#EEF2FF] text-[var(--linear-accent)]'
                        : 'bg-[#F5F5F7] text-[#6D675F]',
                )}
              >
                {metric.trend}
              </span>
            ) : null}
          </div>
        </article>
      ))}
    </section>
  );
}

export function SurfaceCard({
  title,
  eyebrow,
  description,
  children,
  action,
  className,
}: {
  title: string;
  eyebrow?: string;
  description?: string;
  children?: ReactNode;
  action?: ProductAction;
  className?: string;
}) {
  return (
      <section
        className={cn(
        'rounded-[28px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/96 px-6 py-6 shadow-[0_18px_40px_rgba(22,20,17,0.04)]',
        className,
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          {eyebrow ? (
            <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8C93A3]">
              {eyebrow}
            </div>
          ) : null}
          <h2 className="mt-2 text-[22px] font-semibold tracking-[-0.03em] text-[#1F2229]">
            {title}
          </h2>
          {description ? (
            <p className="mt-2 max-w-[62ch] text-[14px] leading-7 text-[#666A73]">
              {description}
            </p>
          ) : null}
        </div>
        {action ? (
          <Link
            href={action.href}
            className="inline-flex items-center gap-2 rounded-full border border-[var(--linear-border)] bg-[var(--linear-panel-bg)] px-4 py-2 text-[12px] font-medium text-[#2E3445] transition-colors hover:border-[#D8D8DC]"
          >
            {action.label}
            <ArrowRight size={14} strokeWidth={2} />
          </Link>
        ) : null}
      </div>

      {children ? <div className="mt-6">{children}</div> : null}
    </section>
  );
}

export function ListTable({
  items,
  columns,
}: {
  items: ProductListItem[];
  columns?: Array<{ key: 'subtitle' | 'meta'; label: string }>;
}) {
  return (
    <div className="overflow-hidden rounded-[24px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/92">
      <div className="grid grid-cols-[minmax(0,1.25fr)_minmax(120px,0.8fr)_auto] gap-3 border-b border-[var(--linear-border)] px-5 py-3 text-[11px] font-semibold uppercase tracking-[0.22em] text-[#A39B90]">
        <div>Title</div>
        <div>{columns?.[0]?.label ?? 'Details'}</div>
        <div>{columns?.[1]?.label ?? 'Status'}</div>
      </div>

      {items.map((item, index) => (
        <div
          key={item.id}
          className={cn(
            'grid grid-cols-[minmax(0,1.25fr)_minmax(120px,0.8fr)_auto] items-center gap-3 px-5 py-4',
            index > 0 && 'border-t border-[var(--linear-border)]',
          )}
        >
          <div className="min-w-0">
            <div className="truncate text-[14px] font-semibold text-[#1F2229]">{item.title}</div>
            {item.subtitle ? (
              <div className="mt-1 truncate text-[12px] text-[#7B776F]">{item.subtitle}</div>
            ) : null}
          </div>
          <div className="truncate text-[12px] text-[#666A73]">{item.meta ?? 'Active'}</div>
          <div className="justify-self-end">
            <span
              className={cn(
                'inline-flex items-center rounded-full px-2.5 py-1 text-[11px] font-semibold',
                item.tone === 'warning'
                  ? 'bg-[#FFF5E9] text-[#B96618]'
                  : item.tone === 'success'
                    ? 'bg-[#EEF8F1] text-[#2D7A46]'
                    : item.tone === 'accent'
                      ? 'bg-[#EEF2FF] text-[var(--linear-accent)]'
                      : 'bg-[#F5F5F7] text-[#6D675F]',
              )}
            >
              {item.tone === 'warning'
                ? 'Needs attention'
                : item.tone === 'success'
                  ? 'Healthy'
                  : item.tone === 'accent'
                    ? 'Live'
                    : 'Open'}
            </span>
          </div>
        </div>
      ))}
    </div>
  );
}

export function SplitWorkspace({
  left,
  center,
  right,
}: {
  left: ReactNode;
  center: ReactNode;
  right: ReactNode;
}) {
  return (
    <section className="grid gap-4 xl:grid-cols-[280px_minmax(0,1fr)_320px]">
      <div className="rounded-[28px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/96 shadow-[0_18px_40px_rgba(22,20,17,0.04)]">
        {left}
      </div>
      <div className="rounded-[28px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/96 shadow-[0_18px_40px_rgba(22,20,17,0.04)]">
        {center}
      </div>
      <div className="rounded-[28px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/96 shadow-[0_18px_40px_rgba(22,20,17,0.04)]">
        {right}
      </div>
    </section>
  );
}

export function SectionNavList({
  title,
  items,
}: {
  title: string;
  items: Array<{ label: string; value?: string; active?: boolean; hint?: string }>;
}) {
  return (
    <div className="p-5">
      <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#9E978B]">
        {title}
      </div>
      <div className="mt-4 space-y-2">
        {items.map((item) => (
          <div
            key={item.label}
            className={cn(
              'flex items-center justify-between rounded-[18px] border px-4 py-3',
              item.active
                ? 'border-[#E6D5C4] bg-[#FFF8EF]'
                : 'border-transparent bg-white/72',
            )}
          >
            <div>
              <div className="text-[13px] font-medium text-[#1F2229]">{item.label}</div>
              {item.hint ? <div className="mt-1 text-[11px] text-[#7B776F]">{item.hint}</div> : null}
            </div>
            {item.value ? (
              <span className="rounded-full bg-[#F4F1EA] px-2.5 py-1 text-[11px] font-semibold text-[#6D675F]">
                {item.value}
              </span>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}

export function ContextStack({
  title,
  sections,
}: {
  title: string;
  sections: Array<{ title: string; rows: Array<{ label: string; value: string }> }>;
}) {
  return (
    <div className="p-5">
      <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#9E978B]">
        {title}
      </div>
      <div className="mt-4 space-y-4">
        {sections.map((section) => (
          <div key={section.title} className="rounded-[20px] border border-[#EEE8DC] bg-white/92 p-4">
            <div className="text-[13px] font-semibold text-[#1F2229]">{section.title}</div>
            <div className="mt-3 space-y-2">
              {section.rows.map((row) => (
                <div key={row.label} className="flex items-start justify-between gap-4 text-[12px]">
                  <span className="text-[#8D877D]">{row.label}</span>
                  <span className="text-right font-medium text-[#2E3445]">{row.value}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function ActivityFeed({
  items,
}: {
  items: Array<{ id: string; title: string; description: string; meta: string }>;
}) {
  return (
    <div className="space-y-3">
      {items.map((item) => (
        <article key={item.id} className="rounded-[20px] border border-[#EEE8DC] bg-white/92 p-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h3 className="text-[14px] font-semibold text-[#1F2229]">{item.title}</h3>
              <p className="mt-1 text-[13px] leading-6 text-[#666A73]">{item.description}</p>
            </div>
            <span className="shrink-0 text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9E978B]">
              {item.meta}
            </span>
          </div>
        </article>
      ))}
    </div>
  );
}

export function InlineStatPills({
  items,
}: {
  items: Array<{ label: string; value: string }>;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {items.map((item) => (
        <div
          key={item.label}
          className="inline-flex items-center gap-2 rounded-full border border-[#E6E0D4] bg-white/90 px-3 py-2 text-[12px] text-[#615B52]"
        >
          <span className="font-medium">{item.label}</span>
          <span className="text-[#1F2229]">{item.value}</span>
        </div>
      ))}
    </div>
  );
}

export function LinkGrid({
  items,
}: {
  items: Array<{ title: string; description: string; href: string }>;
}) {
  return (
    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      {items.map((item) => (
        <Link
          key={item.href}
          href={item.href}
          className="group rounded-[24px] border border-black/8 bg-white/92 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)] transition-colors hover:bg-[#FFFDFC]"
        >
          <div className="flex items-start justify-between gap-3">
            <div>
              <h3 className="text-[16px] font-semibold tracking-[-0.03em] text-[#1F2229]">
                {item.title}
              </h3>
              <p className="mt-2 text-[13px] leading-6 text-[#666A73]">{item.description}</p>
            </div>
            <ChevronRight className="mt-1 h-4 w-4 shrink-0 text-[#A39B90] transition-transform group-hover:translate-x-0.5 group-hover:text-[#1F2229]" />
          </div>
        </Link>
      ))}
    </div>
  );
}
