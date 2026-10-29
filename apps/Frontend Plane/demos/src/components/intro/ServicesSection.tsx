'use client';

import { useRef, useEffect } from 'react';
import { motion, useScroll, useTransform, useSpring, useMotionValueEvent } from 'framer-motion';
import Image from 'next/image';

const SERVICES = [
  {
    id: 0,
    image: '',
    alt: '',
    title: '',
    description: '',
    color: 'transparent'
  },
  {
    id: 1,
    image: '/imagens/curved-concrete-space.png',
    alt: 'Tjenester',
    title: 'Våre tjenester',
    description: 'Vi har allerede bevist vår verdi gjennom komplekse prosjekter i kommunal sektor. Vi kombinerer strategisk innsikt med teknisk ekspertise for å levere løsninger som gir målbare resultater.',
    color: '#D2D2D2'
  },
  {
    id: 2,
    image: '/imagens/arched-corridor-1.jpeg',
    alt: 'Strategi',
    title: 'Strategisk Rådgivning',
    description: 'Vi analyserer din virksomhet og utvikler en skreddersydd digital strategi som driver vekst og effektivitet.',
    color: '#32271F'
  },
  {
    id: 3,
    image: '/imagens/curved-interior-sculpture.png',
    alt: 'Implementering',
    title: 'Teknisk Implementering',
    description: 'Fra konsept til lansering – vi implementerer løsninger med fokus på kvalitet, sikkerhet og skalerbarhet.',
    color: '#623D28'
  },
  {
    id: 4,
    image: '/imagens/arched-corridor-1.jpeg',
    alt: 'Vekst',
    title: 'Digital Vekst',
    description: 'Vi hjelper deg å identifisere og realisere digitale muligheter som øker omsetning og kundetilfredshet.',
    color: '#32271F'
  },
  {
    id: 5,
    image: '/imagens/curved-concrete-space.png',
    alt: 'Support',
    title: 'Kontinuerlig Support',
    description: 'Vi er din pålitelige partner med løpende support, optimalisering og strategisk oppfølging.',
    color: '#D2D2D2'
  },
];

export default function ServicesSection({ onIndexChange }: { onIndexChange?: (index: number) => void }) {
  const targetRef = useRef<HTMLDivElement>(null);
  const { scrollYProgress } = useScroll({
    target: targetRef,
    offset: ["start start", "end end"]
  });

  const smoothProgress = useSpring(scrollYProgress, {
    stiffness: 40,
    damping: 20,
    restDelta: 0.001
  });

  useMotionValueEvent(scrollYProgress, "change", (latest) => {
    if (!onIndexChange) return;
    
    // Map scroll progress to service index (0-4 for 5 cards)
    if (latest < 0.25) onIndexChange(0);      // Våre tjenester
    else if (latest < 0.45) onIndexChange(1); // Strategisk Rådgivning
    else if (latest < 0.65) onIndexChange(2); // Teknisk Implementering
    else if (latest < 0.85) onIndexChange(3); // Digital Vekst
    else onIndexChange(4);                    // Kontinuerlig Support
  });

  // Create a non-linear mapping to "pause" or slow down when each item is centered.
  // We have 6 items (indices 0 to 5).
  // Total width: 60vw + 100vw + 100vw + 100vw + 100vw + 100vw = 560vw
  // Visible: 100vw
  // Max Scroll: 460vw
  // Max % = -460/560 = -82.14%

  const x = useTransform(
    smoothProgress,
    // Input Range (scroll progress)
    [0, 1],
    // Output Range (horizontal translate %)
    ["0%", "-82.14%"]
  );

  return (
    <section ref={targetRef} id="services" className="relative h-[400vh] bg-transparent text-foreground -mt-[100vh] z-10">
      <div className="sticky top-0 flex h-screen items-center overflow-hidden">
        
        {/* Horizontal Moving Container */}
        <motion.div style={{ x }} className="flex h-full w-[560vw]">
          {SERVICES.map((service) => (
            <div 
              key={service.id} 
              className={`relative h-screen flex-shrink-0 flex items-center justify-center p-6 md:p-20 ${
                service.id === 0 ? 'w-[60vw]' : 'w-screen'
              }`}
            >
              {/* Only render content if it's not the empty placeholder */}
              {service.id !== 0 && (
              <div className="flex w-full max-w-7xl flex-col-reverse md:flex-row items-center gap-12 md:gap-24">
                
                {/* Text Side - Centered */}
                <div className="flex-1 flex flex-col items-center text-center gap-6">
                  <div className="flex flex-col gap-2 items-center">
                    <span className="text-xs md:text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground/80">
                      {service.alt}
                    </span>
                    <h2 
                      className="text-[4rem] leading-[0.9] md:text-[8rem] lg:text-[10rem] font-light tracking-tight mix-blend-difference"
                      style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
                    >
                      {service.title}
                    </h2>
                  </div>
                  
                  <p className="max-w-md text-sm md:text-base leading-relaxed text-muted-foreground font-medium uppercase tracking-wide">
                    {service.description}
                  </p>

                  <div className="mt-4 flex items-center gap-2 text-xs font-bold uppercase tracking-widest border-b border-foreground pb-1 cursor-pointer hover:opacity-60 transition-opacity">
                    Utforsk
                  </div>
                </div>

                {/* Image Side */}
                <div className="flex-1 relative w-full aspect-[4/5] md:aspect-square flex items-center justify-center">
                  {/* Decorative parallax-style wrapper */}
                  <div className="relative w-full h-full md:w-[85%] md:h-[85%] overflow-hidden bg-muted shadow-2xl">
                     <Image
                        src={service.image}
                        alt={service.alt}
                        fill
                        className="object-cover transition-transform duration-700 hover:scale-105"
                        sizes="(max-width: 768px) 100vw, 50vw"
                        priority={service.id === 1}
                     />
                     {/* Overlay gradient for depth */}
                     <div className="absolute inset-0 bg-gradient-to-tr from-black/10 to-transparent pointer-events-none" />
                  </div>
                  
                  {/* Floating geometric accent (optional based on ref style) */}
                  <div 
                     className="absolute -z-10 -bottom-8 -right-8 w-2/3 h-2/3 opacity-20"
                     style={{ backgroundColor: service.color }}
                  />
                </div>

              </div>
              )}
            </div>
          ))}
        </motion.div>

        {/* Global Progress Indicator */}
        <div className="absolute bottom-8 left-8 md:bottom-12 md:left-12 flex gap-4 pointer-events-none mix-blend-difference z-10">
            <span className="text-xs font-mono uppercase tracking-widest text-background md:text-foreground">
                01 — 05
            </span>
        </div>

      </div>
    </section>
  );
}
