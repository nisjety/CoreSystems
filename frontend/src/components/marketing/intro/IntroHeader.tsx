'use client';

import Link from 'next/link';
import { useEffect, useState, useContext } from 'react';
import { AuthContext } from '@/components/auth/hooks/use-auth';
import { Loader2 } from 'lucide-react';

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
  const ctx = useContext(AuthContext);
  const user = ctx?.user ?? null;
  const authLoading = ctx?.isLoading ?? false;
  const signOut = ctx?.signOut;

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
        <div className="flex items-center gap-3">
          {/* Auth-aware button */}
          {authLoading ? (
            <div className="flex items-center justify-center px-3 py-1.5">
              <Loader2 className="h-4 w-4 animate-spin text-foreground/80" />
            </div>
          ) : user ? (
            <>
              <Link
                href="/dashboard"
                className="rounded-full border border-border px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.08em] text-foreground/90 hover:opacity-80"
              >
                Dashboard
              </Link>
              <button
                onClick={() => signOut?.()}
                className="rounded-full border border-border px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.08em] text-foreground/70 hover:opacity-80"
              >
                Logg ut
              </button>
            </>
          ) : (
            <Link
              href="/sign-in"
              className="rounded-full border border-border px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.08em] text-foreground/90 hover:opacity-80"
            >
              Logg inn
            </Link>
          )}

          <div className="md:hidden">
            <a
              href="#hero"
              className="rounded-full border border-border px-3 py-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-foreground/80"
            >
              Meny
            </a>
          </div>
        </div>
      </nav>
    </header>
  );
}
