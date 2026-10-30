'use client';

import Image from 'next/image';
import AccentFlow from '@/components/intro/AccentFlow';

export default function HeroSection() {
  return (
    <section id="hero" className="flex h-dvh w-full flex-col overflow-hidden bg-transparent pt-16 pb-4 px-4 md:pt-24 md:pb-8 md:px-8 lg:px-12">
      <div className="relative flex w-full flex-1 flex-col overflow-hidden border border-border bg-transparent shadow-sm">
        <div className="pointer-events-none absolute inset-0 grid grid-cols-4 opacity-50">
          <div className="border-r border-border/40" />
          <div className="border-r border-border/40" />
          <div className="border-r border-border/40" />
          <div />
        </div>

        <div className="relative z-10 flex items-center justify-between px-6 py-6 text-[10px] font-medium uppercase tracking-[0.2em] text-muted-foreground md:px-10">
          <span>Trio dé Lab</span>
          <span className="hidden md:inline-block">Hjem</span>
          <span>digitalt studio</span>
        </div>

        <div className="relative z-10 flex flex-1 flex-col items-center justify-center p-6">
          <div className="relative aspect-video w-full max-w-[85%] md:max-w-3xl lg:max-w-5xl overflow-visible">
            <div className="relative h-full w-full overflow-hidden border border-border bg-muted shadow-2xl">
              <Image
                src="/imagens/arched-hallway-symmetry.jpeg"
                alt="Hovedvisual"
                fill
                sizes="(max-width: 768px) 90vw, (max-width: 1200px) 80vw, 1200px"
                className="object-cover opacity-90"
                priority
              />
            </div>

            <div className="pointer-events-none absolute inset-0 -bottom-[34%] z-10">
              <AccentFlow />
            </div>
          </div>

          <h1
            className="pointer-events-none absolute left-1/2 top-1/2 z-20 w-full -translate-x-1/2 -translate-y-1/2 text-center text-[5rem] font-light tracking-[0.25em] text-foreground mix-blend-normal md:text-[9rem] lg:text-[13rem] whitespace-nowrap"
            style={{
              fontFamily: 'var(--font-cormorant-garamond), serif',
              textShadow:
                '1px 1px 0px rgba(0,0,0,0.1), 2px 2px 0px rgba(0,0,0,0.05), 3px 3px 0px rgba(0,0,0,0.05), 4px 4px 8px rgba(0,0,0,0.1)',
            }}
          >
            Trio dé Lab
          </h1>
        </div>

        <div className="relative z-10 flex items-center justify-between px-6 py-6 text-[10px] font-medium uppercase tracking-[0.2em] text-muted-foreground md:px-10">
          <span>Utforsk</span>
          <span>Norway</span>
        </div>
      </div>
    </section>
  );
}
