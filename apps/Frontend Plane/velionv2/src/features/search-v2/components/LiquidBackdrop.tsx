"use client";

/**
 * LiquidBackdrop — a pure-presentational animated "liquid glass" gradient.
 *
 * Renders an airy, near-white cream surface with a few pale warm colour blobs
 * (coral + apricot + soft-accent, from the Velion brand palette) that drift
 * slowly behind the content. Two visual variants:
 *
 *  - "idle"   → barely-there pale warm drift for the empty search prompt.
 *  - "answer" → a gentle cream → soft apricot wash for the answer view.
 *
 * The motion is GPU-friendly (only `transform` + `opacity` on absolutely
 * positioned layers, `will-change: transform`) and is driven by CSS keyframes
 * defined in globals.css. Those keyframes are neutralised by the global
 * `prefers-reduced-motion` rule, so the backdrop renders as a calm static
 * gradient when reduced motion is requested — we additionally hard-stop the
 * animation here via `useReducedMotion` for belt-and-suspenders.
 *
 * It is non-interactive (`pointer-events-none`, `aria-hidden`) and is meant to
 * be placed as the first child of a `relative` container, with the real
 * content layered above it.
 */

import { useReducedMotion } from "framer-motion";

type LiquidBackdropProps = {
  /** Visual intensity. Defaults to "idle". */
  variant?: "idle" | "answer";
  /** Extra classes for the positioning wrapper. */
  className?: string;
};

export function LiquidBackdrop({ variant = "idle", className }: LiquidBackdropProps) {
  const reduceMotion = useReducedMotion();
  const animate = !reduceMotion;

  return (
    <div
      aria-hidden="true"
      data-variant={variant}
      className={[
        "velion-liquid-backdrop pointer-events-none absolute inset-0 overflow-hidden",
        className ?? "",
      ].join(" ")}
    >
      {/* Base wash — static gradient, no motion. */}
      <div className="velion-liquid-base absolute inset-0" />

      {/* Drifting colour blobs. Each is a soft radial gradient on its own
          GPU layer; they orbit on slightly different cadences so the field
          never visibly loops. */}
      <div className={`velion-liquid-blob velion-liquid-blob-1 ${animate ? "is-animated" : ""}`} />
      <div className={`velion-liquid-blob velion-liquid-blob-2 ${animate ? "is-animated" : ""}`} />
      <div className={`velion-liquid-blob velion-liquid-blob-3 ${animate ? "is-animated" : ""}`} />
    </div>
  );
}
