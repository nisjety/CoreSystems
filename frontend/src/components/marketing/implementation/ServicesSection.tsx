'use client';

import { useMemo, useState, useRef, useLayoutEffect, type ReactNode } from 'react';
import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { ScrollToPlugin } from 'gsap/ScrollToPlugin';
import { Code2, Server, Cloud, Shield } from 'lucide-react';

gsap.registerPlugin(ScrollTrigger, ScrollToPlugin);

type Service = {
  key: string;
  title: string;
  subtitle: string;
  cardDesc: string;
  heading: string;
  features: string[];
  icon: ReactNode;
};

const services: Service[] = [
  {
    key: 'web',
    title: 'Webutvikling',
    subtitle: 'Moderne webapplikasjoner med React, Next.js og TypeScript',
    cardDesc: 'Moderne webapplikasjoner',
    heading: 'Moderne webapplikasjoner med React, Next.js og TypeScript',
    features: ['Responsive design', 'SEO-optimalisert', 'Høy ytelse', 'Skalerbar arkitektur'],
    icon: <Code2 className="h-6 w-6" />,
  },
  {
    key: 'backend',
    title: 'Backend & API',
    subtitle: 'Robuste backend-løsninger og RESTful APIs',
    cardDesc: 'Robuste backend-løsninger',
    heading: 'Robuste backend-løsninger og RESTful APIs',
    features: ['Node.js / Python', 'Database design', 'API-dokumentasjon', 'Sikkerhet'],
    icon: <Server className="h-6 w-6" />,
  },
  {
    key: 'cloud',
    title: 'Cloud & DevOps',
    subtitle: 'Cloud-migrering og CI/CD pipelines',
    cardDesc: 'Cloud-migrering og CI/CD',
    heading: 'Cloud-migrering og CI/CD pipelines',
    features: ['AWS / Azure', 'Docker & Kubernetes', 'Automatisering', 'Monitoring'],
    icon: <Cloud className="h-6 w-6" />,
  },
  {
    key: 'security',
    title: 'Sikkerhet & Testing',
    subtitle: 'Omfattende testing og sikkerhetsimplementering',
    cardDesc: 'Testing og sikkerhetsimplementering',
    heading: 'Omfattende testing og sikkerhetsimplementering',
    features: ['Unit testing', 'Integration testing', 'Sikkerhetsaudit', 'Code review'],
    icon: <Shield className="h-6 w-6" />,
  },
];

function getCellBorders(idx: number, total: number) {
  const isTopRow = idx < 2;
  const isLeftCol = idx % 2 === 0;
  return `
    ${isTopRow ? 'border-b border-gray-200' : ''} 
    ${isLeftCol ? 'md:border-r border-gray-200' : ''}
  `;
}

export default function ServicesSection() {
  const [active, setActive] = useState<string>(services[0]!.key);
  const sectionRef = useRef<HTMLElement>(null);
  const scrollTriggerRef = useRef<ScrollTrigger | null>(null);

  useLayoutEffect(() => {
    const ctx = gsap.context(() => {
      const triggerEl = sectionRef.current;
      if (!triggerEl) return;
      const totalCards = services.length;
      const st = ScrollTrigger.create({
        trigger: triggerEl,
        start: 'top top',
        end: `+=${totalCards * 200}`,
        pin: true,
        pinSpacing: true,
        anticipatePin: 1,
        onUpdate: (self) => {
          const index = Math.floor(self.progress * totalCards);
          const safeIndex = Math.min(index, totalCards - 1);
          setActive(services[safeIndex].key);
        },
      });
      scrollTriggerRef.current = st;
      ScrollTrigger.refresh();
    }, sectionRef);
    return () => {
      scrollTriggerRef.current = null;
      ctx.revert();
    };
  }, []);

  const handleCardClick = (key: string) => {
    const cardIndex = services.findIndex((s) => s.key === key);
    if (cardIndex === -1 || !scrollTriggerRef.current) return;
    const targetProgress = (cardIndex + 0.5) / services.length;
    const start = scrollTriggerRef.current.start;
    const end = scrollTriggerRef.current.end;
    const targetScroll = start + (end - start) * targetProgress;
    window.scrollTo({ top: targetScroll, behavior: 'instant' });
    setActive(key);
  };

  const current = useMemo(() => services.find((s) => s.key === active) ?? services[0]!, [active]);

  return (
    <section
      ref={sectionRef}
      id="services"
      className="relative flex lg:min-h-screen items-center px-4 py-8 lg:py-16 lg:px-8 xl:px-16 bg-white/30"
    >
      <div className="mx-auto w-full max-w-[1400px]">
        
        {/* Main Grid: Single column on mobile, 2 columns on desktop */}
        <div className="flex flex-col lg:grid lg:grid-cols-2 gap-6 lg:gap-24 lg:items-stretch">
          
          {/* --- LEFT COLUMN: Text Content --- */}
          <div className="flex flex-col gap-4 lg:gap-8">
            
            {/* TOP: Text Content */}
            <div className="pt-2">
              <div className="flex items-center gap-3 mb-2 lg:mb-8">
                <span className="text-[10px] text-gray-400">◆</span>
                <span className="font-mono text-[10px] font-bold uppercase tracking-[0.25em] text-gray-500">
                  Våre tjenester
                </span>
              </div>

              <h2 
                className="text-xl lg:text-[52px] font-medium tracking-tighter text-black mb-2 lg:mb-8"
                style={{ lineHeight: 1.05 }}
              >
                {current.heading}
              </h2>

              <div className="text-[12px] lg:text-[15px] leading-[1.4] lg:leading-7 text-gray-600 max-w-105 mb-4 lg:mb-6">
                {current.subtitle}
              </div>

              {/* Features List */}
              <div className="grid grid-cols-2 gap-3 lg:gap-4">
                {current.features.map((feature) => (
                  <div key={feature} className="flex items-center gap-2 text-[11px] lg:text-[13px]">
                    <div className="w-1.5 h-1.5 bg-black rounded-full shrink-0" />
                    <span className="text-gray-700">{feature}</span>
                  </div>
                ))}
              </div>
            </div>

            {/* Icon Display */}
            <div className="w-full max-w-[240px] mx-auto lg:mx-0 lg:max-w-xs lg:mt-auto">
              <div className="relative aspect-square w-full overflow-hidden rounded-xl lg:rounded-3xl bg-white shadow-md flex items-center justify-center border border-gray-100">
                <div className="text-gray-800 scale-[3]">
                  {current.icon}
                </div>
              </div>
            </div>

          </div>

          {/* --- RIGHT COLUMN: The Grid --- */}
          <div className="flex flex-col mt-4 lg:mt-0">
            <div className="grid grid-cols-2 gap-0 bg-white rounded-xl lg:rounded-[24px] overflow-hidden shadow-sm border border-gray-200">
              {services.map((s, idx) => {
                const selected = s.key === active;

                return (
                  <div
                    key={s.key}
                    onClick={() => handleCardClick(s.key)}
                    role="button"
                    tabIndex={0}
                    className={[
                      'group flex flex-col p-4 lg:p-10 transition-all duration-300 cursor-pointer outline-none relative',
                      getCellBorders(idx, services.length),
                      'min-h-50 lg:min-h-85',
                      selected 
                        ? 'bg-gray-50 z-10' 
                        : 'hover:bg-gray-50 bg-white',
                    ].join(' ')}
                  >
                    
                    {/* Top Content: Icon + Heading */}
                    <div className="mb-3 lg:mb-4">
                      <div className="flex items-center gap-2 lg:gap-3 mb-2 lg:mb-3">
                        {/* Icon */}
                        <div className={`
                          w-8 h-8 lg:w-10 lg:h-10 rounded-lg flex items-center justify-center transition-colors duration-300 shrink-0
                          ${selected ? 'bg-black text-white' : 'bg-gray-100 text-gray-600 group-hover:bg-gray-200'}
                        `}>
                          {s.icon}
                        </div>
                        
                        <h3 className="text-[14px] lg:text-[18px] font-medium tracking-tight text-black leading-tight">
                          {s.title}
                        </h3>
                      </div>
                      
                      <p className="text-[11px] lg:text-[13px] leading-relaxed text-gray-500">
                        {s.cardDesc}
                      </p>
                    </div>

                    {/* Bottom Content: Action */}
                    <div className="mt-auto pt-3 lg:pt-4">
                      <div className="inline-block border-b border-black pb-0.5">
                        <span className="text-[11px] lg:text-[12px] font-medium tracking-wide text-black">
                          Detaljer
                        </span>
                      </div>
                    </div>

                  </div>
                );
              })}
            </div>
          </div>

        </div>
      </div>
    </section>
  );
}
