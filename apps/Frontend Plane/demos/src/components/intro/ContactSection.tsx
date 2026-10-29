'use client';

import { useState } from 'react';
import Image from 'next/image';
import { motion, AnimatePresence } from 'framer-motion';
import { X } from 'lucide-react';

export default function ContactSection() {
  const [isModalOpen, setIsModalOpen] = useState(false);

  return (
    <section id="contact" className="relative h-screen w-full overflow-hidden text-[#1a1a1a]">
      {/* Rotated Background Image */}
      <div className="absolute inset-0 z-0 flex items-center justify-center opacity-40 md:opacity-100">
         <div className="relative w-[100vw] h-[100vh] md:w-[120vw] md:h-[120vw]">
            <Image
              src="/imagens/handtransparent.png"
              alt="Background Hand"
              fill
              className="object-contain rotate-0 scale-125"
              priority
            />
         </div>
      </div>

      {/* Content */}
      <div className="relative z-10 h-full w-full max-w-[1440px] mx-auto px-6 md:px-20 flex items-center">
        <div className="max-w-2xl bg-white/0 backdrop-blur-sm md:backdrop-blur-none p-8 md:p-0 rounded-2xl md:rounded-none">
          <p className="text-xs font-bold uppercase tracking-[0.25em] mb-6 opacity-60">
            Kontakt Oss
          </p>
          
          <h2 
            className="text-5xl md:text-7xl font-light leading-[1.1] mb-8"
            style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
          >
            La oss skape<br />
            noe varig sammen.
          </h2>

          <p className="text-lg md:text-xl font-light opacity-80 mb-12 max-w-md leading-relaxed">
             Har du et prosjekt i tankene? Vi hjelper deg med struktur, design og teknisk gjennomføring fra start til mål.
          </p>

          <button 
            onClick={() => setIsModalOpen(true)}
            className="group relative px-8 py-4 bg-[#1a1a1a] text-white overflow-hidden transition-all hover:bg-[#333]"
          >
             <span className="relative z-10 text-xs font-bold uppercase tracking-[0.2em]">Start et prosjekt</span>
          </button>
        </div>
      </div>

      {/* Modal Overlay */}
      <AnimatePresence>
        {isModalOpen && (
          <motion.div 
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-4"
            onClick={() => setIsModalOpen(false)}
          >
            {/* Modal Content */}
            <motion.div 
              initial={{ scale: 0.9, opacity: 0, y: 20 }}
              animate={{ scale: 1, opacity: 1, y: 0 }}
              exit={{ scale: 0.9, opacity: 0, y: 20 }}
              transition={{ type: "spring", duration: 0.5 }}
              onClick={(e) => e.stopPropagation()}
              className="relative w-full max-w-lg bg-white p-8 md:p-12 shadow-2xl"
            >
              <button 
                onClick={() => setIsModalOpen(false)}
                className="absolute top-6 right-6 p-2 opacity-50 hover:opacity-100 transition-opacity"
              >
                <X className="w-6 h-6" />
              </button>

              <h3 className="text-3xl font-serif mb-2">Hei på deg!</h3>
              <p className="text-sm text-gray-500 mb-8">Fortell oss litt om hva du trenger hjelp til.</p>

              <form className="flex flex-col gap-6" onSubmit={(e) => e.preventDefault()}>
                <div className="flex flex-col gap-2">
                   <label className="text-xs font-bold uppercase tracking-wider text-gray-400">Navn</label>
                   <input type="text" className="border-b border-gray-300 py-2 outline-none focus:border-black transition-colors bg-transparent placeholder:text-gray-300" placeholder="Ola Nordmann" />
                </div>
                
                <div className="flex flex-col gap-2">
                   <label className="text-xs font-bold uppercase tracking-wider text-gray-400">E-post</label>
                   <input type="email" className="border-b border-gray-300 py-2 outline-none focus:border-black transition-colors bg-transparent placeholder:text-gray-300" placeholder="ola@bedrift.no" />
                </div>

                <div className="flex flex-col gap-2">
                   <label className="text-xs font-bold uppercase tracking-wider text-gray-400">Hva handler det om?</label>
                   <textarea rows={3} className="border-b border-gray-300 py-2 outline-none focus:border-black transition-colors bg-transparent resize-none placeholder:text-gray-300" placeholder="Kort om prosjektet..." />
                </div>

                <button className="mt-4 bg-black text-white py-4 text-xs font-bold uppercase tracking-[0.2em] hover:opacity-80 transition-opacity">
                  Send melding
                </button>
              </form>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}
