/**
 * State palette definitions and animation parameters for each OrbAnimationState.
 *
 * Separated so the main component stays focused on lifecycle and rendering.
 */

import type { OrbAnimationState } from './ThreeJSOrb';
import { oklchVarToRGB } from './orb-helpers';

// ── Palette types ─────────────────────────────────────────────────────────

export interface OuterPalette {
  base: [number, number, number];
  accent1: [number, number, number];
  accent2: [number, number, number];
  core: [number, number, number];
  intensity: number;
  glow: number;
  opacity: number;
}

export interface InnerPalette {
  deform: number;
  opacity: number;
  color: [number, number, number];
}

export interface ParticlePalette {
  color: string | number;
  opacity: number;
}

export interface StatePalette {
  outer: OuterPalette;
  inner: InnerPalette;
  particle: ParticlePalette;
}

// ── Per-state palette factories ───────────────────────────────────────────

function waitingPalette(): StatePalette {
  const prim = oklchVarToRGB('--primary', 'oklch(0.292 0.132 269.8)');
  return {
    outer: {
      base: [0.70, 0.76, 1.00],
      accent1: [0.86, 0.90, 1.00],
      accent2: [0.92, 0.88, 1.00],
      core: [1.0, 1.0, 1.0],
      intensity: 0.18,
      glow: 0.85,
      opacity: 0.94,
    },
    inner: { deform: 0.20, opacity: 0.95, color: [prim.r, prim.g, prim.b] },
    particle: { color: '#cfd6ff', opacity: 0.55 },
  };
}

function listeningPalette(): StatePalette {
  return {
    outer: {
      base: [1.00, 0.78, 0.50],
      accent1: [1.00, 0.88, 0.63],
      accent2: [1.00, 0.92, 0.78],
      core: [1.0, 1.0, 1.0],
      intensity: 0.45,
      glow: 1.0,
      opacity: 0.96,
    },
    inner: { deform: 0.24, opacity: 0.94, color: [1.00, 0.90, 0.70] },
    particle: { color: '#ffd27a', opacity: 0.72 },
  };
}

function thinkingPalette(): StatePalette {
  return {
    outer: {
      base: [0.96, 0.97, 1.00],
      accent1: [0.98, 0.99, 1.00],
      accent2: [0.93, 0.95, 1.00],
      core: [1.0, 1.0, 1.0],
      intensity: 0.58,
      glow: 0.95,
      opacity: 0.97,
    },
    inner: { deform: 0.28, opacity: 0.9, color: [0.98, 0.99, 1.0] },
    particle: { color: 0x111111, opacity: 0.85 },
  };
}

function respondingPalette(): StatePalette {
  return {
    outer: {
      base: [0.55, 0.95, 0.70],
      accent1: [0.70, 1.00, 0.80],
      accent2: [0.82, 1.00, 0.90],
      core: [1.0, 1.0, 1.0],
      intensity: 0.78,
      glow: 1.15,
      opacity: 0.96,
    },
    inner: { deform: 0.32, opacity: 0.95, color: [0.85, 1.0, 0.9] },
    particle: { color: '#b6ffd6', opacity: 0.78 },
  };
}

const PALETTE_MAP: Record<OrbAnimationState, () => StatePalette> = {
  waiting: waitingPalette,
  listening: listeningPalette,
  thinking: thinkingPalette,
  responding: respondingPalette,
};

export function getPalette(state: OrbAnimationState): StatePalette {
  return PALETTE_MAP[state]();
}

// ── Animation dynamics per state ──────────────────────────────────────────

export interface AnimDynamics {
  baseSpin: number;
  spinX: number;
}

export function getAnimDynamics(state: OrbAnimationState): AnimDynamics {
  switch (state) {
    case 'thinking':   return { baseSpin: 1.2, spinX: 1.05 };
    case 'responding': return { baseSpin: 0.85, spinX: 0.65 };
    case 'listening':  return { baseSpin: 0.55, spinX: 0.40 };
    default:           return { baseSpin: 0.35, spinX: 0.18 };
  }
}

/** Compute breathing / pulse scale modulation for the outer orb. */
export function getScaleModulation(state: OrbAnimationState, t: number): number {
  switch (state) {
    case 'waiting':
      return Math.sin(t * 1.1) * 0.025 + Math.sin(t * 0.37) * 0.01;
    case 'listening': {
      const beat1 = Math.max(0, Math.sin(t * 3.2));
      const beat2 = Math.max(0, Math.sin(t * 3.2 + 1.2));
      return beat1 * 0.05 + beat2 * 0.035;
    }
    case 'thinking':
      return Math.sin(t * 2.4) * 0.018;
    case 'responding':
      return Math.sin(t * 4.0) * 0.035 + Math.sin(t * 1.3) * 0.025;
  }
}

/** Inner core micro-wobble delta rotation speed. */
export function getCoreWobble(state: OrbAnimationState, t: number): number {
  switch (state) {
    case 'waiting':    return Math.sin(t * 1.6) * 0.08;
    case 'thinking':   return Math.sin(t * 3.0) * 0.05;
    default:           return 0.03;
  }
}

/** Core crystal scale pulse. */
export function getCoreScalePulse(state: OrbAnimationState, t: number): number {
  let s = Math.sin(t * 1.25) * 0.012;
  if (state === 'responding') s += Math.sin(t * 5.2) * 0.018;
  if (state === 'listening') s += Math.max(0, Math.sin(t * 3.2)) * 0.02;
  return s;
}

/** Deform modulation for the inner core mesh. */
export function getDeformModulation(state: OrbAnimationState, t: number): number {
  switch (state) {
    case 'responding': return Math.sin(t * 4.5) * 0.08 + Math.sin(t * 2.1) * 0.04;
    case 'thinking':   return Math.sin(t * 2.2) * 0.05;
    case 'listening':  return Math.sin(t * 3.2) * 0.035;
    default:           return Math.sin(t * 1.4) * 0.02;
  }
}

/** Particle swirl speed per state. */
export function getParticleSwirl(state: OrbAnimationState): number {
  switch (state) {
    case 'thinking':   return 1.15;
    case 'responding': return 0.95;
    case 'listening':  return 0.62;
    default:           return 0.38;
  }
}

/** Radial push multiplier per state. */
export function getRadialPush(state: OrbAnimationState): number {
  switch (state) {
    case 'responding': return 2.3;
    case 'thinking':   return 1.8;
    case 'listening':  return 1.5;
    default:           return 1.0;
  }
}

/** Breathing intensity modulation for the outer uniform. */
export function getIntensityModulation(state: OrbAnimationState, t: number): number {
  switch (state) {
    case 'waiting':    return Math.sin(t * 0.8) * 0.04;
    case 'listening':  return Math.sin(t * 3.2) * 0.06;
    case 'thinking':   return Math.sin(t * 2.0) * 0.05;
    default:           return Math.sin(t * 4.0) * 0.08;
  }
}
