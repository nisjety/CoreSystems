'use client';

import { motion } from 'framer-motion';

export default function HeroSection() {
  return (
    <section id="hero" className="min-h-screen flex items-center justify-center px-6">
      <div className="max-w-4xl mx-auto text-center">
        <motion.h1
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="text-5xl md:text-7xl font-bold mb-6"
        >
          Teknisk Implementering
        </motion.h1>
        <motion.p
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="text-xl md:text-2xl text-gray-600 max-w-2xl mx-auto"
        >
          Vi bygger moderne, skalerbare løsninger med fokus på kvalitet, sikkerhet og ytelse. 
          Fra konsept til lansering.
        </motion.p>
      </div>
    </section>
  );
}
