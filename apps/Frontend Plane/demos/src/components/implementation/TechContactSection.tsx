'use client';

import { motion } from 'framer-motion';
import { ArrowRight } from 'lucide-react';
import Link from 'next/link';

const technologies = [
  'React',
  'Next.js',
  'TypeScript',
  'Node.js',
  'Python',
  'PostgreSQL',
  'MongoDB',
  'AWS',
  'Docker',
  'Kubernetes',
];

export default function TechContactSection() {
  return (
    <section id="tech-contact" className="min-h-screen flex items-center py-20 px-6 bg-white/30">
      <div className="max-w-5xl mx-auto w-full">
        {/* Technologies */}
        <div className="mb-20">
          <div className="text-center mb-12">
            <h2 className="text-4xl md:text-5xl font-bold mb-4">Teknologier vi bruker</h2>
            <p className="text-lg text-gray-600 max-w-2xl mx-auto">
              Vi jobber med moderne teknologier og verktøy for å bygge beste løsninger.
            </p>
          </div>
          
          <div className="flex flex-wrap justify-center gap-4">
            {technologies.map((tech, index) => (
              <motion.div
                key={tech}
                initial={{ opacity: 0, scale: 0.8 }}
                whileInView={{ opacity: 1, scale: 1 }}
                viewport={{ once: true }}
                transition={{ delay: index * 0.05 }}
                className="px-6 py-3 bg-white/80 backdrop-blur-sm rounded-full border border-gray-200 font-medium hover:bg-black hover:text-white transition-colors cursor-default"
              >
                {tech}
              </motion.div>
            ))}
          </div>
        </div>

        {/* Ideas Section */}
        <div className="mb-0">
          <div className="bg-white/60 backdrop-blur-sm rounded-3xl p-8 md:p-12 border border-gray-200">
            <h3 className="text-3xl font-bold mb-6 text-center">Har du en idé?</h3>
            <p className="text-lg text-gray-600 text-center max-w-2xl mx-auto mb-8">
              Vi elsker å jobbe med innovative løsninger. Enten du har en klar visjon eller bare en gryende idé, 
              kan vi hjelpe deg med å forme den til virkelighet.
            </p>
            <div className="flex flex-wrap justify-center gap-4">
              <div className="px-6 py-3 bg-black text-white rounded-full text-sm font-medium">Konsept til produkt</div>
              <div className="px-6 py-3 bg-black text-white rounded-full text-sm font-medium">Rask prototype</div>
              <div className="px-6 py-3 bg-black text-white rounded-full text-sm font-medium">MVP utvikling</div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
