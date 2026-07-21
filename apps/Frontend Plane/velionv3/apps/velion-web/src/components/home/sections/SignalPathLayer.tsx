"use client";

import { useEffect, useRef } from "react";

/**
 * SignalPathLayer — the hero's one literal nod to "Fra signal til handling."
 *
 * A single thin line draws in once (Cinematic tier, same load moment as the
 * headline), then a small coral dot rides it back and forth on loop — the
 * codebase's own "signal dot" token (DESIGN.md: coral is a scalpel, one job
 * per view) given motion instead of a new color. The line itself stays
 * white-at-low-opacity, matching the existing VelionMarkOutline treatment,
 * so coral still only does the one job.
 *
 * Desktop-only (matches ProblemSection's galaxyImages precedent — no room
 * for scattered ambient detail once the layout stacks on mobile).
 */

const PATH_D =
  "M 1180 60 L 1180 190 A 46 46 0 0 1 1134 236 L 840 236 A 46 46 0 0 0 794 282 L 794 470 A 34 34 0 0 1 754 502 L 560 604";

export function SignalPathLayer() {
  const pathRef = useRef<SVGPathElement>(null);
  const dotRef = useRef<SVGCircleElement>(null);

  useEffect(() => {
    let disposed = false;
    let cleanupContext: { revert: () => void } | undefined;

    async function setup() {
      const path = pathRef.current;
      const dot = dotRef.current;

      if (!path || !dot) {
        return;
      }

      const { default: gsap } = await import("gsap");

      if (disposed) {
        return;
      }

      const length = path.getTotalLength();
      const reduceMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)",
      ).matches;

      if (reduceMotion) {
        gsap.set(path, { strokeDasharray: length, strokeDashoffset: 0 });
        const end = path.getPointAtLength(length);
        gsap.set(dot, { x: end.x, y: end.y, opacity: 1 });
        return;
      }

      gsap.set(path, { strokeDasharray: length, strokeDashoffset: length });
      gsap.set(dot, { x: 0, y: 0, opacity: 0 });

      const context = gsap.context(() => {
        const travel = { progress: 0 };
        const timeline = gsap.timeline({ delay: 0.5 });

        timeline
          .to(path, {
            strokeDashoffset: 0,
            duration: 1.8,
            ease: "power2.inOut",
          })
          .to(
            dot,
            {
              opacity: 1,
              duration: 0.4,
            },
            "-=0.5",
          )
          .to(travel, {
            progress: 1,
            duration: 3.4,
            ease: "power1.inOut",
            repeat: -1,
            yoyo: true,
            onUpdate: () => {
              const point = path.getPointAtLength(travel.progress * length);
              gsap.set(dot, { x: point.x, y: point.y });
            },
          });
      });

      cleanupContext = context;
    }

    setup();

    return () => {
      disposed = true;
      cleanupContext?.revert();
    };
  }, []);

  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 z-[1] max-[760px]:hidden"
    >
      <svg
        className="size-full"
        preserveAspectRatio="xMidYMid slice"
        viewBox="0 0 1600 900"
      >
        <path
          d={PATH_D}
          fill="none"
          ref={pathRef}
          stroke="color-mix(in srgb, var(--velion-c-white) 22%, transparent)"
          strokeLinecap="round"
          strokeWidth={1}
        />
        <circle
          fill="var(--velion-coral)"
          r={3.5}
          ref={dotRef}
          style={{ filter: "drop-shadow(0 0 6px var(--velion-coral))" }}
        />
      </svg>
    </div>
  );
}

export default SignalPathLayer;
