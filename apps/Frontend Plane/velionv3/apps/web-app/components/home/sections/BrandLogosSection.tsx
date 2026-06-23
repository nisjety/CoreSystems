"use client";

import { animate, motion, useMotionValue } from "motion/react";
import type { CSSProperties, ReactNode } from "react";
import { useEffect, useState } from "react";
import useMeasure from "react-use-measure";

type InfiniteSliderProps = {
  children: ReactNode;
  gap?: number;
  speed?: number;
  speedOnHover?: number;
  direction?: "horizontal" | "vertical";
  reverse?: boolean;
  className?: string;
};

function InfiniteSlider({
  children,
  gap = 16,
  speed = 100,
  speedOnHover,
  direction = "horizontal",
  reverse = false,
  className,
}: InfiniteSliderProps) {
  const [currentSpeed, setCurrentSpeed] = useState(speed);
  const [ref, { width, height }] = useMeasure();
  const translation = useMotionValue(0);
  const [isTransitioning, setIsTransitioning] = useState(false);
  const [key, setKey] = useState(0);

  useEffect(() => {
    const size = direction === "horizontal" ? width : height;

    if (size === 0) {
      return undefined;
    }

    const contentSize = size + gap;
    const from = reverse ? -contentSize / 2 : 0;
    const to = reverse ? 0 : -contentSize / 2;
    const distanceToTravel = Math.abs(to - from);
    const duration = distanceToTravel / currentSpeed;

    const controls = isTransitioning
      ? animate(translation, [translation.get(), to], {
          duration: Math.abs(translation.get() - to) / currentSpeed,
          ease: "linear",
          onComplete: () => {
            setIsTransitioning(false);
            setKey((prevKey) => prevKey + 1);
          },
        })
      : animate(translation, [from, to], {
          duration,
          ease: "linear",
          repeat: Infinity,
          repeatDelay: 0,
          repeatType: "loop",
          onRepeat: () => {
            translation.set(from);
          },
        });

    return () => controls.stop();
  }, [key, translation, currentSpeed, width, height, gap, isTransitioning, direction, reverse]);

  const handleHoverStart = () => {
    if (!speedOnHover) {
      return;
    }

    setIsTransitioning(true);
    setCurrentSpeed(speedOnHover);
  };

  const handleHoverEnd = () => {
    if (!speedOnHover) {
      return;
    }

    setIsTransitioning(true);
    setCurrentSpeed(speed);
  };

  return (
    <div className={className}>
      <motion.div
        className="velion-logo-carousel__slider"
        onHoverEnd={handleHoverEnd}
        onHoverStart={handleHoverStart}
        ref={ref}
        style={{
          ...(direction === "horizontal" ? { x: translation } : { y: translation }),
          flexDirection: direction === "horizontal" ? "row" : "column",
          gap: `${gap}px`,
        }}
      >
        {children}
        {children}
      </motion.div>
    </div>
  );
}

type BlurredInfiniteSliderProps = InfiniteSliderProps & {
  containerClassName?: string;
  fadeWidth?: number;
};

function BlurredInfiniteSlider({
  children,
  containerClassName,
  fadeWidth = 80,
  ...sliderProps
}: BlurredInfiniteSliderProps) {
  const maskStyle: CSSProperties = {
    WebkitMaskImage: `linear-gradient(to right, transparent, black ${fadeWidth}px, black calc(100% - ${fadeWidth}px), transparent)`,
    maskImage: `linear-gradient(to right, transparent, black ${fadeWidth}px, black calc(100% - ${fadeWidth}px), transparent)`,
  };

  return (
    <div className={containerClassName} style={maskStyle}>
      <InfiniteSlider {...sliderProps}>{children}</InfiniteSlider>
    </div>
  );
}

const logoMarks = [
  "Shopify",
  "Zendesk",
  "Gorgias",
  "Klaviyo",
  "Meta",
  "Slack",
  "Teams",
  "Gmail",
  "Brreg",
  "Visma",
  "Tripletex",
  "HubSpot",
];

export function BrandLogosSection() {
  return (
    <section className="velion-logo-carousel" aria-label="Teams powered by Velion">
      <p className="velion-logo-carousel__label">
        <span>Powering</span>
        <span>the best teams</span>
      </p>

      <BlurredInfiniteSlider
        containerClassName="velion-logo-carousel__viewport"
        fadeWidth={80}
        gap={112}
        speed={40}
        speedOnHover={20}
      >
        {logoMarks.map((logo) => (
          <span className="velion-logo-carousel__mark" key={logo}>
            {logo}
          </span>
        ))}
      </BlurredInfiniteSlider>
    </section>
  );
}

export default BrandLogosSection;
