"use client";

import type Lenis from "@studio-freight/lenis";
import { type ReactNode, useEffect } from "react";
import { smoothScrollEasing } from "./scroll-easing";

type SmoothScrollProps = {
  children: ReactNode;
};

export function SmoothScroll({ children }: SmoothScrollProps) {
  useEffect(() => {
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const coarsePointer = window.matchMedia("(pointer: coarse)");
    let animationFrame = 0;
    let disposed = false;
    let lenis: Lenis | null = null;

    const stop = () => {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = 0;
      lenis?.destroy();
      lenis = null;
    };

    const start = async () => {
      if (disposed || reducedMotion.matches || coarsePointer.matches || lenis) {
        return;
      }

      const { default: Lenis } = await import("@studio-freight/lenis");

      if (disposed || reducedMotion.matches || coarsePointer.matches || lenis) {
        return;
      }

      lenis = new Lenis({
        duration: 0.75,
        easing: smoothScrollEasing,
        smoothWheel: true,
        wheelMultiplier: 1.2,
      });

      const update = (time: number) => {
        lenis?.raf(time);
        animationFrame = window.requestAnimationFrame(update);
      };

      animationFrame = window.requestAnimationFrame(update);
    };

    const syncScrollStrategy = () => {
      if (reducedMotion.matches || coarsePointer.matches) {
        stop();
        return;
      }

      void start();
    };

    syncScrollStrategy();
    reducedMotion.addEventListener("change", syncScrollStrategy);
    coarsePointer.addEventListener("change", syncScrollStrategy);

    return () => {
      disposed = true;
      reducedMotion.removeEventListener("change", syncScrollStrategy);
      coarsePointer.removeEventListener("change", syncScrollStrategy);
      stop();
    };
  }, []);

  return children;
}
