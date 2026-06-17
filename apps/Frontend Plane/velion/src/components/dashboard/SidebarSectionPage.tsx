import Link from 'next/link';

export type SidebarSectionView = {
  id: string;
  label: string;
  description: string;
  href: string;
  status?: 'live' | 'coming-soon';
};

interface SidebarSectionPageProps {
  eyebrow: string;
  title: string;
  description: string;
  currentView: SidebarSectionView;
  views: SidebarSectionView[];
}

export function SidebarSectionPage({
  eyebrow,
  title,
  description,
  currentView,
  views,
}: SidebarSectionPageProps) {
  return (
    <div className="min-h-dvh bg-transparent text-[#23252F]">
      <div className="border-b border-[var(--linear-border)] bg-[var(--linear-panel-bg)]/85 px-6 py-8 backdrop-blur-[6px] md:px-10">
        <div className="mx-auto max-w-5xl">
          <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#A2A6B1]">
            {eyebrow}
          </div>
          <h1 className="mt-2 text-[34px] font-semibold tracking-[-0.04em] text-[#2F3138]">
            {title}
          </h1>
          <p className="mt-2 max-w-[56ch] text-[15px] leading-7 text-[#707480]">
            {description}
          </p>
        </div>
      </div>

      <div className="px-6 py-10 md:px-10">
        <div className="mx-auto grid max-w-5xl gap-6 lg:grid-cols-[1.2fr_0.8fr]">
          <section className="rounded-[28px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)] p-6 shadow-[0_12px_30px_rgba(33,38,52,0.04)]">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-[#A2A6B1]">
                  Current view
                </div>
                <h2 className="mt-2 text-[24px] font-semibold tracking-[-0.03em] text-[#2F3138]">
                  {currentView.label}
                </h2>
              </div>
              <span className="rounded-full border border-[var(--linear-border)] bg-[#F7F7F8] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-[#8A8E98]">
                {currentView.status === 'coming-soon' ? 'Coming soon' : 'Live'}
              </span>
            </div>

            <p className="mt-4 max-w-[52ch] text-[15px] leading-7 text-[#666B77]">
              {currentView.description}
            </p>

            <div className="mt-8 rounded-[22px] border border-dashed border-[var(--linear-border)] bg-[#FCFCFD] p-5">
              <div className="text-[12px] font-semibold uppercase tracking-[0.18em] text-[#A2A6B1]">
                Why this exists now
              </div>
              <p className="mt-3 text-[14px] leading-7 text-[#6E727D]">
                This section is ready for routing so the new sidebar can support the full product information architecture immediately. Live destinations can grow into richer workflows without changing the navigation model again.
              </p>
            </div>
          </section>

          <aside className="rounded-[28px] border border-[var(--linear-border)] bg-[var(--linear-panel-bg)] p-6 shadow-[0_12px_30px_rgba(33,38,52,0.04)]">
            <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-[#A2A6B1]">
              Available views
            </div>

            <div className="mt-4 space-y-2">
              {views.map((view) => {
                const isCurrent = view.href === currentView.href;

                return (
                  <Link
                    key={view.id}
                    href={view.href}
                    className={`block rounded-[18px] border px-4 py-3 transition-colors ${
                      isCurrent
                        ? 'border-[var(--linear-border)] bg-[#F7F7F8]'
                        : 'border-transparent bg-[#FCFCFD] hover:border-[var(--linear-border)] hover:bg-[#F7F7F8]'
                    }`}
                  >
                    <div className="flex items-center justify-between gap-3">
                      <div className="text-[15px] font-medium tracking-[-0.02em] text-[#2F3138]">
                        {view.label}
                      </div>
                      <span className="text-[11px] font-semibold uppercase tracking-[0.16em] text-[#9A9EAA]">
                        {view.status === 'coming-soon' ? 'Soon' : 'Open'}
                      </span>
                    </div>
                    <div className="mt-1 text-[13px] leading-6 text-[#7A7F8A]">
                      {view.description}
                    </div>
                  </Link>
                );
              })}
            </div>
          </aside>
        </div>
      </div>
    </div>
  );
}
