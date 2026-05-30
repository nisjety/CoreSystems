'use client';

import React, { useState, useEffect } from 'react';
import Link from 'next/link';
import { usePathname, useRouter } from 'next/navigation';
import { Fingerprint, Lock, ShieldCheck } from 'lucide-react';
import { onboardingService } from '@/components/onboarding/services/onboarding-service';
import { CrawlProgressProvider } from '../CrawlProgressContext';
import { CrawlProgressModal } from '../core/CrawlProgressModal';

const STEPS = [
  // G45 (velion-gap.md §8.31 / Slice F): the wizard is now 5 steps instead
  // of 6. The legacy `/onboarding/connect` step is deferred to a
  // post-first-value `<ConnectorConsentPrompt />` on the dashboard. The
  // route file `/onboarding/connect/page.tsx` redirects forward for any
  // session that still has the old step in localStorage.
  {
    number: 1,
    path: '/onboarding/profile',
    title: 'Din Profil',
    description: 'Fortell oss litt om deg selv.',
  },
  {
    number: 2,
    path: '/onboarding/organization',
    title: 'Din Organisasjon',
    description: 'Sett opp eller bli med i en organisasjon.',
  },
  {
    number: 3,
    path: '/onboarding/website',
    title: 'Koble til nettsiden',
    description: 'La oss lære om selskapet ditt.',
  },
  {
    number: 4,
    path: '/onboarding/team',
    title: 'Inviter Team',
    description: 'Legg til kollegaer i arbeidsområdet ditt.',
  },
  {
    number: 5,
    path: '/onboarding/complete',
    title: 'Alt Klart!',
    description: 'Kontoen din er klar til bruk.',
  },
];

interface OnboardingPageProps {
  children: React.ReactNode;
}

export function OnboardingPage({ children }: OnboardingPageProps) {
  const pathname = usePathname();
  const router = useRouter();
  const [isVisible, setIsVisible] = useState(false);
  const [viewportHeight, setViewportHeight] = useState<number | null>(null);
  const [isCancelling, setIsCancelling] = useState(false);

  const handleCancel = async () => {
    setIsCancelling(true);
    try {
      await onboardingService.cancelOnboarding();
      router.push('/login');
    } catch (err) {
      console.error('Failed to cancel onboarding:', err);
      setIsCancelling(false);
    }
  };

  const currentStep = STEPS.find((s) => pathname?.includes(s.path)) ?? STEPS[0];

  useEffect(() => {
    const update = () => setViewportHeight(window.innerHeight);
    update();
    window.addEventListener('resize', update);
    return () => window.removeEventListener('resize', update);
  }, []);

  useEffect(() => {
    const frame = requestAnimationFrame(() => setIsVisible(true));
    return () => cancelAnimationFrame(frame);
  }, []);

  const cardScale = viewportHeight
    ? Math.min(1, Math.max(0.52, (viewportHeight - 18) / 980))
    : 1;

  return (
    <CrawlProgressProvider>
    <div
      className={`relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 transition-opacity duration-700 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10 ${
        isVisible ? 'opacity-100' : 'opacity-0'
      }`}
      style={
        {
          '--primary': '#111111',
          '--primary-foreground': '#ffffff',
          '--ring': '#111111',
        } as React.CSSProperties
      }
    >
      <div
        className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]"
        style={{ transform: `scale(${cardScale})`, transformOrigin: 'center center' }}
      >
        {/* ── Left panel ── */}
        <div className="flex items-start justify-center rounded-l-[24px] bg-white px-5 py-6 sm:px-7 sm:py-7 md:px-8 md:py-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
          <div className="w-full max-w-[21rem] sm:max-w-[22rem] lg:max-w-[22.75rem] xl:max-w-[25rem]">

            {/* Top bar */}
            <div className="mb-6 flex items-center justify-between xl:mb-7">
              <button
                onClick={handleCancel}
                disabled={isCancelling}
                className="text-xs font-bold tracking-tight text-[#111111] transition-colors hover:text-[#FF2E63] disabled:opacity-50"
              >
                {isCancelling ? '...' : '← TILBAKE'}
              </button>
              <span
                className="inline-block h-1.5 w-1.5 rounded-full bg-[#FF2E63]/70"
                aria-hidden="true"
              />
            </div>

            {/* Step indicator dots */}
            <div className="mb-5 flex items-center gap-1.5 xl:mb-6">
              {STEPS.map((step, index) => (
                <React.Fragment key={step.number}>
                  <div
                    className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold transition-all duration-300 ${
                      step.number < currentStep.number
                        ? 'bg-[#111111] text-white'
                        : step.number === currentStep.number
                        ? 'bg-[#FF2E63] text-white'
                        : 'bg-[#E8E4DF] text-[#A09890]'
                    }`}
                  >
                    {step.number < currentStep.number ? (
                      <svg className="h-3 w-3" fill="currentColor" viewBox="0 0 20 20">
                        <path
                          fillRule="evenodd"
                          d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
                          clipRule="evenodd"
                        />
                      </svg>
                    ) : (
                      step.number
                    )}
                  </div>
                  {index < STEPS.length - 1 && (
                    <div
                      className={`h-px flex-1 transition-colors duration-300 ${
                        step.number < currentStep.number ? 'bg-[#111111]' : 'bg-[#E0DBD5]'
                      }`}
                    />
                  )}
                </React.Fragment>
              ))}
            </div>

            {/* Step heading */}
            <div className="mb-5 text-left xl:mb-6">
              <p className="mb-1 font-inter text-[11px] font-semibold uppercase tracking-[0.12em] text-[#FF2E63]">
                Steg {currentStep.number} av {STEPS.length}
              </p>
              <h1
                className="text-[clamp(36px,4.5vw,64px)] font-normal leading-[1.1] tracking-[-0.01em] text-[#1C1C1C]"
                style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
              >
                {currentStep.title}
              </h1>
              <p className="mt-2 font-inter text-[14px] leading-[1.6] text-[#66615B] xl:mt-3">
                {currentStep.description}
              </p>
            </div>

            {/* Step content */}
            <div className="w-full">{children}</div>

          </div>
        </div>

        {/* ── Right panel ── */}
        <div className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
          <div
            className="absolute inset-0 bg-cover bg-center"
            style={{ backgroundImage: "url('/imagens/curved-interior-sculpture.png')" }}
          />
          {/* Red scanner line */}
          <div className="pointer-events-none absolute bottom-8 left-5 top-8 z-20">
            <div className="absolute inset-y-0 left-0 w-px bg-[#FF2E63]/90" />
            <div className="absolute inset-y-0 -left-[3px] w-[8px] bg-[#FF3B5C]/35 blur-[7px]" />
            <div className="scanner-dot absolute -left-[4px] top-0 h-[20px] w-[9px] rounded-lg bg-gradient-to-b from-[#FF3B5C]/15 via-[#FF3B5C]/40 to-[#FF3B5C]/15 shadow-[0_0_8px_rgba(255,59,92,0.5),0_0_16px_rgba(255,59,92,0.3),0_0_32px_rgba(255,59,92,0.15)]" />
          </div>
          {/* Side icons */}
          <div className="absolute left-4 top-1/2 z-30 -translate-y-1/2 xl:left-5">
            <div className="flex flex-col items-center gap-10 text-white/90 lg:gap-11 xl:gap-12">
              <ShieldCheck strokeWidth={1.4} className="h-4 w-4 text-[#10B981] lg:h-4 lg:w-4 xl:h-5 xl:w-5" />
              <Lock strokeWidth={1.4} className="h-4 w-4 lg:h-4 lg:w-4 xl:h-5 xl:w-5" />
              <Fingerprint strokeWidth={1.4} className="h-4 w-4 text-[#FF2E63]/80 lg:h-4 lg:w-4 xl:h-5 xl:w-5" />
            </div>
          </div>
        </div>
      </div>

      {/* Footer */}
      <div className="pointer-events-none absolute bottom-3 left-0 right-0 hidden text-center md:block">
        <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 font-inter text-xs tracking-[0.02em] text-[#6A655F]">
          <button className="transition-colors hover:text-[#1C1C1C]">Om oss</button>
          <button className="transition-colors hover:text-[#1C1C1C]">Personvern</button>
          <button className="transition-colors hover:text-[#1C1C1C]">Opphavsrett</button>
        </div>
      </div>

      <style>{`
        .scanner-dot {
          animation: scannerMove 5.5s ease-in-out infinite alternate;
        }
        @keyframes scannerMove {
          0% { top: 0; }
          100% { top: calc(100% - 20px); }
        }
      `}</style>
      <CrawlProgressModal />
    </div>
    </CrawlProgressProvider>
  );
}
