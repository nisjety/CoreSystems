// Optimalisert motion utilities for bedre ytelse
import { Variants } from 'framer-motion';

// Predefinerte easing curves - using proper Framer Motion format
export const easing = {
  perfect: [0.25, 1, 0.5, 1] as const,
  smooth: [0.4, 0, 0.2, 1] as const,
  bounce: [0.68, -0.55, 0.265, 1.55] as const,
  elastic: [0.175, 0.885, 0.32, 1.275] as const,
};

// Predefinerte animasjoner for å unngå duplikater
export const fadeInUp: Variants = {
  hidden: { opacity: 0, y: 40 },
  visible: { opacity: 1, y: 0 },
};

export const fadeInLeft: Variants = {
  hidden: { opacity: 0, x: -40 },
  visible: { opacity: 1, x: 0 },
};

export const fadeInRight: Variants = {
  hidden: { opacity: 0, x: 40 },
  visible: { opacity: 1, x: 0 },
};

export const scaleIn: Variants = {
  hidden: { opacity: 0, scale: 0.8 },
  visible: { opacity: 1, scale: 1 },
};

export const staggerContainer: Variants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: {
      staggerChildren: 0.1,
    },
  },
};

// Hover animasjoner
export const hoverScale = {
  scale: 1.05,
  transition: { duration: 0.2, ease: easing.smooth },
};

export const hoverLift = {
  y: -4,
  transition: { duration: 0.2, ease: easing.smooth },
};

// Viewport options
export const viewportOptions = {
  once: true,
  margin: "-100px",
} as const;

// Transition presets
export const transitions = {
  fast: { duration: 0.2, ease: easing.smooth },
  normal: { duration: 0.4, ease: easing.smooth },
  slow: { duration: 0.8, ease: easing.perfect },
} as const; 