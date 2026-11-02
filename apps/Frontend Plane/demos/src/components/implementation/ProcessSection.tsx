'use client';

import { motion } from 'framer-motion';

const process = [
  {
    step: '1',
    duration: '1 uke',
    title: 'Teknisk Analyse',
    description: 'Vi analyserer dine tekniske behov og eksisterende systemer for å lage en optimal løsning.',
  },
  {
    step: '2',
    duration: '1-2 uker',
    title: 'Arkitektur & Design',
    description: 'Vi designer systemarkitekturen og brukergrensesnittet basert på dine krav og behov.',
  },
  {
    step: '3',
    duration: '4-12 uker',
    title: 'Utvikling',
    description: 'Vi bygger løsningen med moderne teknologier og beste praksis for kodekvalitet.',
  },
  {
    step: '4',
    duration: '1-2 uker',
    title: 'Testing & Lansering',
    description: 'Vi tester grundig og lanserer løsningen med kontinuerlig overvåking og støtte.',
  },
];

export default function ProcessSection() {
  return (
    <section id="process" className="min-h-screen flex items-center py-20 px-6 bg-white/50">
      <div className="max-w-5xl mx-auto w-full">
        <div className="mb-16 text-center">
          <h2 className="text-4xl md:text-5xl font-bold mb-4">Vår prosess</h2>
          <p className="text-lg text-gray-600 max-w-2xl mx-auto">
            Vi følger en strukturert tilnærming for å sikre at vi leverer løsninger av høy kvalitet.
          </p>
        </div>
        
        <div className="grid md:grid-cols-2 gap-8">
          {process.map((item, index) => (
            <motion.div
              key={item.step}
              initial={{ opacity: 0, x: index % 2 === 0 ? -20 : 20 }}
              whileInView={{ opacity: 1, x: 0 }}
              viewport={{ once: true }}
              transition={{ delay: index * 0.1 }}
              className="relative"
            >
              <div className="flex gap-6">
                <div className="flex flex-col items-center">
                  <div className="w-12 h-12 rounded-full bg-black text-white flex items-center justify-center font-bold text-lg">
                    {item.step}
                  </div>
                  {index < process.length - 1 && index % 2 === 0 && (
                    <div className="w-px h-full bg-gray-200 mt-4" />
                  )}
                </div>
                <div className="pb-12">
                  <div className="text-sm text-gray-500 mb-2">{item.duration}</div>
                  <h3 className="text-xl font-bold mb-2">{item.title}</h3>
                  <p className="text-gray-600">{item.description}</p>
                </div>
              </div>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}
