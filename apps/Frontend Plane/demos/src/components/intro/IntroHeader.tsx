'use client';

import Link from 'next/link';
import { useEffect, useState } from 'react';

export const sections = [
  { id: 'hero', label: 'Hjem' },
  { id: 'about', label: 'Om' },
  { id: 'services', label: 'Tjenester' },
  { id: 'projects', label: 'Prosjekter (programmer)' },
  { id: 'structure', label: 'Struktur' },
  { id: 'contact', label: 'Kontakt oss' },
] as const;

export default function IntroHeader() {
  const [activeId, setActiveId] = useState('hero');

  useEffect(() => {
    const onScroll = () => {
      let current = 'hero';
      for (const section of sections) {
        const element = document.getElementById(section.id);
        if (!element) continue;
        const rect = element.getBoundingClientRect();
        if (rect.top <= 140) current = section.id;
      }
      setActiveId(current);
    };

    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  return (
    <header className="fixed left-0 right-0 top-4 z-50 px-3 md:px-6">
      <nav className="mx-auto flex h-14 w-full max-w-6xl items-center justify-between rounded-2xl border border-border/60 bg-background/55 px-4 shadow-lg backdrop-blur-xl supports-backdrop-filter:bg-background/45 md:h-16 md:px-6">
        <Link href="/" className="text-lg font-bold tracking-tight text-foreground/90 transition-opacity hover:opacity-70 md:text-xl">
          triodelab
        </Link>

        <ul className="hidden items-center gap-1 md:flex">
          {sections.map((section) => (
            <li key={section.id}>
              <a
                href={`#${section.id}`}
                className={`rounded-full px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.14em] transition-all ${
                  activeId === section.id
                    ? 'bg-foreground text-background'
                    : 'text-muted-foreground hover:text-foreground'
                }`}
              >
                {section.label}
              </a>
            </li>
          ))}
        </ul>

        <div className="md:hidden">
          <a
            href="#hero"
            className="rounded-full border border-border px-3 py-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-foreground/80"
          >
            Meny
          </a>
        </div>
      </nav>
    </header>
  );
}
