"use client";

import { motion, AnimatePresence } from "framer-motion";

interface SectionWatermarkProps {
  section: string;
}

const SECTION_LABELS: Record<string, string> = {
  hero: "Hjem",
  about: "Om",
  services: "Tjenester",
  projects: "Prosjekter",
  structure: "Struktur",
  contact: "Kontakt",
};

export default function SectionWatermark({ section }: SectionWatermarkProps) {
  const label = SECTION_LABELS[section] || "";

  return (
    <div className="fixed inset-0 flex items-center justify-center pointer-events-none z-0 overflow-hidden">
      <AnimatePresence mode="wait">
        <motion.h2
          key={section}
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -20 }}
          transition={{ duration: 0.5, ease: "easeOut" }}
          className="text-[15vw] md:text-[20vh] font-light italic select-none text-black/[0.03] whitespace-nowrap"
          style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
        >
          {label}
        </motion.h2>
      </AnimatePresence>
    </div>
  );
}
