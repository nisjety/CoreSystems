'use client';

import React, { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';
import LoadingPencil from './LoadingPencil';

interface NavigationButton {
  label: string;
  sublabel: string;
  href: string;
}

const navigationButtons: NavigationButton[] = [
  { label: 'Trio dé Lab', sublabel: 'Intro Page', href: '/intro' },
  { label: 'Agenci', sublabel: 'Chatbot', href: '/agenci' },
  { label: 'Qualai', sublabel: 'Web Management', href: '/qualai' },
  { label: 'Intro 2', sublabel: 'Second Intro', href: '/intro2' }
];
const introTitle = 'Digital transformasjon som faktisk fungerer';

export default function LoadingPage() {
  const router = useRouter();
  const [revealCards, setRevealCards] = useState(false);
  const [animatingTo, setAnimatingTo] = useState<string | null>(null);

  // Single seamless transition from logo-only to logo+cards
  useEffect(() => {
    const timer = setTimeout(() => {
      setRevealCards(true);
    }, 1000);

    return () => {
      clearTimeout(timer);
    };
  }, []);
  const [introPhase, setIntroPhase] = useState<'idle' | 'zoom' | 'black' | 'title' | 'reveal'>('idle');

  const handleNavigation = (href: string) => {
    setAnimatingTo(href);
    if (href === '/intro') {
      setIntroPhase('zoom');

      setTimeout(() => {
        setIntroPhase('black');
      }, 2200);

      setTimeout(() => {
        setIntroPhase('title');
      }, 2500);

      setTimeout(() => {
        setIntroPhase('reveal');
      }, 6200);

      setTimeout(() => {
        router.push(href);
      }, 7600);

      return;
    }

    // Non-intro routes keep the short transition
    setTimeout(() => {
      router.push(href);
    }, 2100);
  };

  return (
    <div className="relative flex min-h-screen items-center justify-center overflow-hidden">
      {/* Animated Line on Navigation */}
      {animatingTo && (
        <LoadingPencil
          rotateStroke={
            animatingTo === '/intro' &&
            (introPhase === 'zoom' || introPhase === 'black' || introPhase === 'title' || introPhase === 'reveal')
          }
        />
      )}

      <div className="relative z-10 flex w-full items-center justify-center px-4">
        <div
          className={`relative flex w-full max-w-[980px] flex-col items-center transition-opacity duration-500 ${
            introPhase === 'zoom' || introPhase === 'black' || introPhase === 'title' || introPhase === 'reveal'
              ? 'intro-wrapper-motion'
              : ''
          } ${introPhase === 'black' || introPhase === 'title' || introPhase === 'reveal' ? 'opacity-0' : 'opacity-100'}`}
        >
          {/* Logo Text - centered first, then smoothly moves up */}
          <div
            className={`relative z-20 transition-all duration-900 ease-out ${
              revealCards ? '-translate-y-[68%] md:-translate-y-[72%]' : '-translate-y-1/2'
            }`}
          >
            <h1 className="text-center text-6xl font-light tracking-wide text-[#111111] md:text-7xl lg:text-8xl" style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}>
              Trio dé Lab
            </h1>
          </div>

          {/* Navigation Buttons - always mounted, one shared drop+fade animation */}
          <div
            className={`pointer-events-none absolute left-1/2 top-full z-30 mt-8 flex w-full -translate-x-1/2 flex-col items-center gap-6 transition-all duration-900 ease-out md:mt-10 md:gap-8 ${
              revealCards ? '-translate-y-2 opacity-100' : '-translate-y-8 opacity-0'
            }`}
          >
            {/* First row - buttons 0 and 1 */}
            <div className="flex w-full flex-col items-center gap-6 md:flex-row md:justify-center md:gap-10">
              {navigationButtons.slice(0, 2).map((button) => (
                <button
                  key={button.href}
                  onClick={() => handleNavigation(button.href)}
                  disabled={!revealCards || !!animatingTo}
                  className="pointer-events-auto group relative overflow-hidden transition-all duration-300 hover:scale-105 disabled:pointer-events-none"
                >
                  <div className="flex min-w-[280px] flex-col items-center justify-center border border-[#D8D2C6] bg-white px-8 py-6 transition-all duration-300 hover:border-[#FF2E63] hover:shadow-lg md:min-w-[340px] md:px-10 md:py-7">
                    <span className="text-2xl font-light tracking-wide text-[#111111] md:text-3xl" style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}>
                      {button.label}
                    </span>
                    <span className="mt-1 text-xs tracking-wider text-[#6A655F] md:text-sm" style={{ fontFamily: 'Inter, system-ui, -apple-system, sans-serif' }}>
                      {button.sublabel}
                    </span>
                  </div>
                  <div className="absolute bottom-0 left-0 h-0.5 w-0 bg-[#FF2E63] transition-all duration-300 group-hover:w-full" />
                </button>
              ))}
            </div>

            {/* Second row - buttons 2 and 3 */}
            {navigationButtons.length > 2 && (
              <div className="flex w-full flex-col items-center gap-6 md:flex-row md:justify-center md:gap-10">
                {navigationButtons.slice(2, 4).map((button) => (
                  <button
                    key={button.href}
                    onClick={() => handleNavigation(button.href)}
                    disabled={!revealCards || !!animatingTo}
                    className="pointer-events-auto group relative overflow-hidden transition-all duration-300 hover:scale-105 disabled:pointer-events-none"
                  >
                    <div className="flex min-w-[280px] flex-col items-center justify-center border border-[#D8D2C6] bg-white px-8 py-6 transition-all duration-300 hover:border-[#FF2E63] hover:shadow-lg md:min-w-[340px] md:px-10 md:py-7">
                      <span className="text-2xl font-light tracking-wide text-[#111111] md:text-3xl" style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}>
                        {button.label}
                      </span>
                      <span className="mt-1 text-xs tracking-wider text-[#6A655F] md:text-sm" style={{ fontFamily: 'Inter, system-ui, -apple-system, sans-serif' }}>
                        {button.sublabel}
                      </span>
                    </div>
                    <div className="absolute bottom-0 left-0 h-0.5 w-0 bg-[#FF2E63] transition-all duration-300 group-hover:w-full" />
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      <div
        className={`pointer-events-none fixed inset-0 z-40 transition-opacity duration-900 ${
          introPhase === 'black' || introPhase === 'title' || introPhase === 'reveal' ? 'opacity-100' : 'opacity-0'
        } ${introPhase === 'reveal' ? 'intro-black-reveal' : ''}`}
      >
        <div className="absolute inset-0 bg-[#111111]" />

        <div className="absolute inset-0 flex items-center justify-center px-8 text-center">
          <h2
            className={`noise-text noise-text-animated text-3xl font-semibold tracking-[0.04em] md:text-5xl lg:text-6xl ${
              introPhase === 'title' || introPhase === 'reveal' ? 'opacity-100' : 'opacity-0'
            }`}
            style={{ fontFamily: 'var(--font-cormorant-garamond), serif', fontWeight: 800 }}
            aria-hidden={introPhase === 'idle' || introPhase === 'zoom'}
          >
            {introTitle.split('').map((char, index) => (
              <span
                key={`${char}-${index}`}
                className={`inline-block transition-all duration-700 ease-out ${
                  introPhase === 'title' || introPhase === 'reveal'
                    ? 'translate-y-0 scale-100 opacity-100'
                    : 'translate-y-6 scale-95 opacity-0'
                } ${introPhase === 'reveal' ? 'intro-title-expand' : ''}`}
                style={{ transitionDelay: `${index * 32}ms` }}
              >
                {char === ' ' ? '\u00A0' : char}
              </span>
            ))}
          </h2>
        </div>
      </div>

      <style jsx>{`
        .intro-wrapper-motion {
          animation: introWarp 1.25s cubic-bezier(0.22, 0.61, 0.36, 1) forwards;
          transform-origin: 50% 50%;
        }

        .intro-black-reveal {
          animation: blackLift 1.25s ease-out forwards;
          animation-delay: 0.4s;
        }

        .intro-title-expand {
          animation: titleGrow 1.35s ease-out forwards;
          animation-delay: 0.22s;
        }

        @keyframes introWarp {
          from {
            transform: translateY(0) scale(1);
            opacity: 1;
          }
          to {
            transform: translateY(-6vh) scale(1.22);
            opacity: 0;
          }
        }

        @keyframes blackLift {
          from {
            clip-path: circle(140% at 50% 50%);
          }
          to {
            clip-path: circle(68% at 50% 50%);
          }
        }

        @keyframes titleGrow {
          from {
            transform: scale(1);
            letter-spacing: 0.04em;
          }
          to {
            transform: scale(1.18);
            letter-spacing: 0.1em;
          }
        }
      `}</style>
    </div>
  );
}
