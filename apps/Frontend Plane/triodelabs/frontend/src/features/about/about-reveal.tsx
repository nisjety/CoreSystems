"use client";

import { motion } from "framer-motion";
import type { ReactNode } from "react";
import { easing } from "@/lib/motion";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";

type AboutRevealProps = {
  children: ReactNode;
  className?: string;
  delay?: number;
};

export function AboutReveal({
  children,
  className,
  delay = 0,
}: AboutRevealProps) {
  const reducedMotion = usePrefersReducedMotion();

  return (
    <motion.div
      className={className}
      initial={reducedMotion ? false : { opacity: 0, y: 24 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-12%" }}
      transition={{
        duration: reducedMotion ? 0 : 0.72,
        delay: reducedMotion ? 0 : delay,
        ease: easing.perfect,
      }}
    >
      {children}
    </motion.div>
  );
}
