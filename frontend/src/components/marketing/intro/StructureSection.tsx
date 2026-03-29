'use client';

import React from 'react';

const TIMELINE_STEPS = [
  { id: '00', title: 'Oppstart', description: 'Bli kjent\nForventninger\nKontrakt' },
  { id: '01', title: 'Analysere', description: 'Behovsanalyse\nTeknisk gjennomgang\nMulighetsstudie' },
  { id: '02', title: 'Designe', description: 'Prototyping\nBrukervennlighet\nVisuell identitet' },
  { id: '03', title: 'Utvikle', description: 'Koding\nIterasjon\nTesting' },
  { id: '04', title: 'Levere', description: 'Lansering\nOpplæring\nDokumentasjon' },
  { id: '05', title: 'Drifte', description: 'Overvåking\nSupport\nVedlikehold' },
  { id: '06', title: 'Vokse', description: 'Skalering\nNye funksjoner\nOptimalisering' },
];

export default function StructureSection() {
  return (
    <section id="structure" className="relative min-h-screen bg-transparent py-32 px-6 overflow-hidden flex flex-col items-center justify-center">
      
      <div className="relative z-10 w-full max-w-6xl mx-auto">
        {/* Header */}
        <div className="text-center mb-20 md:mb-32">
          <h2 className="text-4xl md:text-5xl font-serif text-[#1a1a1a] mb-6">
            Tidslinje
          </h2>
        </div>

        {/* Desktop Timeline (Snake Layout) */}
        <div className="hidden md:block relative h-[600px]">
          {/* SVG Path joining the points */}
          <svg className="absolute inset-0 w-full h-full pointer-events-none" viewBox="0 0 1000 500" fill="none">
             {/* 
                Path logic updated for 7 items (00-06):
                00 (Start left alone?) 
                Actually, let's put 00, 01, 02, 03 on top row? Or 00 is start point.
                The image shows 00 as a start node. 
                Let's arrange 00-03 on top, 04-06 on bottom.
                Or keep 00 separate.
                Let's fit 4 on top (00, 01, 02, 03) and 3 on bottom (06, 05, 04).
                Path: M 100,120 L 900,120 ...
             */}
             <path 
                d="M 100,120 L 900,120 A 80,80 0 0 1 980,200 L 980,320 A 80,80 0 0 1 900,400 L 300,400"
                stroke="#1a1a1a"
                strokeWidth="1"
                vectorEffect="non-scaling-stroke"
             />
          </svg>

          {/* Top Row (00, 01, 02, 03) */}
          <div className="absolute top-[80px] left-[50px] w-[900px] flex justify-between">
             {TIMELINE_STEPS.slice(0, 4).map((step) => (
               <div key={step.id} className="relative flex flex-col items-center group">
                  <div className="w-16 h-16 rounded-full bg-[#1a1a1a] text-white flex items-center justify-center text-xl font-serif z-10 mb-6 transition-transform group-hover:scale-110 shadow-lg border-4 border-white">
                     {step.id}
                  </div>
                  <div className="absolute top-24 text-center w-40">
                     <h3 className="font-bold uppercase tracking-wider text-sm mb-2">{step.title}</h3>
                     <p className="text-xs text-gray-500 whitespace-pre-line leading-relaxed">{step.description}</p>
                  </div>
               </div>
             ))}
          </div>

          {/* Bottom Row (06, 05, 04) - Reversed order visually in code to match right-to-left flow */}
          <div className="absolute top-[360px] left-[310px] w-[640px] flex justify-between flex-row-reverse">
             {TIMELINE_STEPS.slice(4, 7).map((step) => (
               <div key={step.id} className="relative flex flex-col items-center group">
                  <div className="w-16 h-16 rounded-full bg-[#1a1a1a] text-white flex items-center justify-center text-xl font-serif z-10 mb-6 transition-transform group-hover:scale-110 shadow-lg border-4 border-white">
                     {step.id}
                  </div>
                  <div className="absolute top-24 text-center w-40">
                     <h3 className="font-bold uppercase tracking-wider text-sm mb-2">{step.title}</h3>
                     <p className="text-xs text-gray-500 whitespace-pre-line leading-relaxed">{step.description}</p>
                  </div>
               </div>
             ))}
          </div>
        </div>

        {/* Mobile Timeline (Vertical) */}
        <div className="md:hidden flex flex-col gap-12 relative pl-8 border-l border-[#1a1a1a]/20 ml-6">
           {TIMELINE_STEPS.map((step) => (
              <div key={step.id} className="relative pl-8">
                 <div className="absolute -left-[45px] top-0 w-10 h-10 rounded-full bg-[#1a1a1a] text-white flex items-center justify-center text-sm font-serif border-4 border-white shadow-sm">
                    {step.id}
                 </div>
                 <h3 className="font-bold uppercase tracking-wider text-sm mb-1">{step.title}</h3>
                 <p className="text-xs text-gray-500 whitespace-pre-line leading-relaxed">{step.description}</p>
              </div>
           ))}
        </div>

        
        {/* Footer Text */}
        <div className="mt-20 md:mt-32 max-w-sm ml-auto mr-20">
            <p className="font-serif italic text-3xl md:text-4xl text-[#1a1a1a] leading-tight">
                Kanskje din reise <br /> ser slik ut...
            </p>
            <div className="flex gap-4 mt-8">
               <button className="text-[10px] font-bold uppercase tracking-[0.2em] border-b border-black pb-1 hover:opacity-50 transition-opacity">
                  Liker
               </button>
               <button className="text-[10px] font-bold uppercase tracking-[0.2em] border-b border-black pb-1 hover:opacity-50 transition-opacity">
                  Del
               </button>
               <button className="text-[10px] font-bold uppercase tracking-[0.2em] border-b border-black pb-1 hover:opacity-50 transition-opacity">
                  Lagre
               </button>
            </div>
        </div>
      </div>
    </section>
  );
}
