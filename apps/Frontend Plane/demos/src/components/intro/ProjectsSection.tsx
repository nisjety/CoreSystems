'use client';

import { useRef, useState, useEffect, useCallback } from 'react';
import { motion, useScroll, useTransform, useSpring, AnimatePresence, useInView } from 'framer-motion';
import Image from 'next/image';

const PROJECTS = [
  { id: 1, number: '1', title: 'Møt den heteste stilen', category: 'Se', image: '/imagens/arched-corridor-1.jpeg', colStart: 'md:col-start-2', sideText: 'Supers møter sommer 2016', side: 'left' },
  { id: 2, number: '2', title: 'Andros vår', category: 'Vår/sommer 2016', image: '/imagens/curved-concrete-space.png', colStart: 'md:col-start-9', sideText: 'Andros vår vår/sommer 2016', side: 'right' },
  { id: 3, number: '3', title: 'Australsk frihet', category: 'Klassisk', image: '/imagens/curved-interior-sculpture.png', colStart: 'md:col-start-2', sideText: 'Januar 2016', side: 'left' },
  { id: 4, number: '4', title: 'Noe spesielt', category: 'Utstilling', image: '/imagens/arched-hallway-symmetry.jpeg', colStart: 'md:col-start-9', sideText: 'Andros vår vår/sommer 2016', side: 'right' },
];

function Card({ project, onSelect, onVisible }: { 
  project: typeof PROJECTS[0], 
  onSelect: (p: typeof PROJECTS[0]) => void,
  onVisible?: () => void 
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const isInView = useInView(cardRef, { margin: "-40% 0px -40% 0px" });

  useEffect(() => {
    if (isInView && onVisible) onVisible();
  }, [isInView, onVisible]);

  const { scrollYProgress } = useScroll({
    target: cardRef,
    offset: ["start end", "end start"]
  });

  // Sharp individual fade: Card is only fully visible in the center 10% of the viewport
  const opacity = useTransform(scrollYProgress, [0, 0.45, 0.55, 1], [0, 1, 1, 0]);
  const yMove = useTransform(scrollYProgress, [0, 1], ["15vh", "-15vh"]);
  const y = useSpring(yMove, { stiffness: 40, damping: 25 });

  return (
    <div ref={cardRef} className="grid grid-cols-1 md:grid-cols-12 w-full min-h-[90vh] items-center">
      <motion.div 
        layoutId={`container-${project.id}`}
        style={{ y, opacity }} 
        className={`relative col-span-1 md:col-span-5 ${project.colStart} flex gap-6 items-start group cursor-pointer`}
        onClick={() => onSelect(project)}
      >
        <motion.span 
          layoutId={`number-${project.id}`} 
          className="absolute -top-12 -left-8 text-7xl font-medium z-10"
          style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
        >
          {project.number}
        </motion.span>

        <motion.div layoutId={`image-${project.id}`} className="relative aspect-[4/5] w-full bg-[#f4f4f4] overflow-hidden">
          <Image src={project.image} alt={project.title} fill className="object-cover grayscale transition-all duration-1000 group-hover:grayscale-0" />
        </motion.div>

        {/* Individual Card side metadata */}
        <div className="pt-16">
            <p className="text-[9px] tracking-[0.4em] uppercase opacity-60 font-semibold whitespace-nowrap" style={{ writingMode: 'vertical-rl' }}>
                {project.sideText}
            </p>
        </div>

        <div className="absolute -bottom-16 left-0 flex flex-col gap-1">
          <h3 className="text-[11px] font-bold uppercase tracking-widest">{project.title}</h3>
          <p className="text-[10px] uppercase tracking-[0.2em] opacity-40">{project.category}</p>
        </div>
      </motion.div>
    </div>
  );
}

export default function ProjectsSection({ onIndexChange }: { onIndexChange?: (index: number) => void }) {
  const [selected, setSelected] = useState<typeof PROJECTS[0] | null>(null);

  useEffect(() => {
    if (selected) document.body.style.overflow = 'hidden';
    else document.body.style.overflow = 'unset';
  }, [selected]);

  // Handle navigation between projects
  const handleNext = useCallback(() => {
    if (!selected) return;
    const currentIndex = PROJECTS.findIndex(p => p.id === selected.id);
    const nextIndex = (currentIndex + 1) % PROJECTS.length;
    setSelected(PROJECTS[nextIndex]);
  }, [selected]);

  const handlePrev = useCallback(() => {
    if (!selected) return;
    const currentIndex = PROJECTS.findIndex(p => p.id === selected.id);
    const prevIndex = (currentIndex - 1 + PROJECTS.length) % PROJECTS.length;
    setSelected(PROJECTS[prevIndex]);
  }, [selected]);

  // Keyboard navigation
  useEffect(() => {
    if (!selected) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelected(null);
      if (e.key === 'ArrowRight') handleNext();
      if (e.key === 'ArrowLeft') handlePrev();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selected, handleNext, handlePrev]);

  return (
    <section className="relative text-[#1A1A1A] w-full min-h-screen">
      
      {/* 2. STAGGERED GRID: Alternating rows ensure cards appear one-by-one */}
      <div className="max-w-[1440px] mx-auto px-12 pt-[20vh] relative z-10 flex flex-col gap-y-40 pb-[20vh]">
        {PROJECTS.map((project, i) => (
          <Card 
            key={project.id} 
            project={project} 
            onSelect={setSelected} 
            onVisible={() => onIndexChange?.(i)} 
          />
        ))}
      </div>

      {/* 3. DETAIL VIEW: Expands from the card's current side */}
      <AnimatePresence>
        {selected && (
          <motion.div 
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
            className={`fixed inset-0 z-50 bg-white flex flex-col ${selected.side === 'right' ? 'md:flex-row-reverse' : 'md:flex-row'}`}
          >
            <button onClick={() => setSelected(null)} className="absolute top-10 right-10 z-60 text-[10px] uppercase tracking-[0.3em] font-bold">Lukk</button>
            
            {/* Shared Element Image Transition */}
            <motion.div layoutId={`image-${selected.id}`} className="relative h-[50vh] md:h-full md:w-1/2">
              <Image src={selected.image} alt={selected.title} fill className="object-cover" priority />
            </motion.div>

            <motion.div 
              initial={{ opacity: 0, x: selected.side === 'right' ? -20 : 20 }} 
              animate={{ opacity: 1, x: 0 }} 
              transition={{ delay: 0.3 }} 
              className="p-10 md:p-32 md:w-1/2 flex flex-col justify-center gap-12"
            >
              <header className="flex justify-between items-start">
                <h2 
                  className="text-5xl md:text-8xl leading-[0.85] text-black/5 uppercase"
                  style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
                >
                    Australsk<br/>frihet i<br/>klassisk stil
                </h2>
                <span className="text-[10px] opacity-40 uppercase tracking-widest pt-4">22. mai 2016</span>
              </header>

              <div className="max-w-md space-y-8">
                <p 
                  className="text-sm italic leading-relaxed text-black/80"
                  style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
                >
                    Begynnelsen av forrige tiår markerte overgangen fra nyutdannet designer til redaksjonelt arbeid, før rollen som hoveddesigner i ditt første merke.
                </p>
                <p className="text-[13px] leading-loose opacity-50">
                    Du har alltid søkt kreative måter å forme hverdagen på. Erfaringen fra en familiebedrift med lang historie har gitt et tydelig blikk for detaljer, kvalitet og helhet.
                </p>
              </div>

              <footer className="mt-auto flex justify-between items-center text-[9px] uppercase tracking-[0.3em] font-bold border-t border-black/5 pt-12">
                <span className="opacity-30">Mary Greger · Supera redaktør</span>
                <div className="flex gap-12">
                  <span>{selected.number} / 04</span>
                  <div className="flex gap-6 opacity-60">
                    <button onClick={handlePrev} className="hover:opacity-100">Forrige</button>
                    <button onClick={handleNext} className="hover:opacity-100">Neste</button>
                  </div>
                </div>
              </footer>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}