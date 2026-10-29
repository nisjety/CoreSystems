"use client";

import {
  AnimatePresence,
  motion,
  useInView,
  useIsPresent,
} from "framer-motion";
import Image from "next/image";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import { useRef } from "react";
import { ArrowRight } from "@/components/icons";
import type { Locale } from "@/lib/i18n/locales";
import { portfolioCopy, type OwnedProduct } from "./portfolio-content";
import styles from "./product-orbit-visual.module.css";
import { useHoverVideo } from "./use-hover-video";

type ProductOrbitVisualProps = {
  locale: Locale;
  product: OwnedProduct;
  otherProduct: OwnedProduct;
};

const orbitPositions = [0, 1, 2, 3] as const;

export function ProductOrbitVisual({
  locale,
  product,
  otherProduct,
}: ProductOrbitVisualProps) {
  const stageRef = useRef<HTMLDivElement>(null);
  const isInView = useInView(stageRef, { margin: "150px 0px" });

  return (
    <div
      className={styles.stage}
      data-testid="product-orbit-stage"
      ref={stageRef}
    >
      <div aria-hidden="true" className={styles.orbit}>
        {orbitPositions.map((position) => (
          <div
            className={styles.trail}
            data-testid="product-orbit-trail"
            key={position}
            style={{
              animationDelay: `${position * -5}s`,
              animationPlayState: isInView ? "running" : "paused",
            }}
          >
            <Image
              alt=""
              className={styles.trailImage}
              fill
              sizes="(min-width: 1024px) 15vw, 25vw"
              src={position % 2 === 0 ? otherProduct.image : product.image}
            />
          </div>
        ))}
      </div>

      <AnimatePresence initial={false} mode="wait">
        <ProductOrbitCard
          key={product.title}
          locale={locale}
          product={product}
        />
      </AnimatePresence>
    </div>
  );
}

function ProductOrbitCard({
  locale,
  product,
}: {
  locale: Locale;
  product: OwnedProduct;
}) {
  const content = portfolioCopy[locale];
  const reduceMotion = usePrefersReducedMotion();
  const cardRef = useRef<HTMLAnchorElement>(null);
  const isInView = useInView(cardRef);
  const isPresent = useIsPresent();
  const { videoRef, playing, handlers, onVideoError } = useHoverVideo(
    Boolean(product.video) && !reduceMotion && isInView && isPresent,
  );

  return (
    <motion.a
      {...handlers}
      ref={cardRef}
      aria-label={`${product.title} ${content.viewProduct}`}
      className={styles.card}
      data-testid="product-orbit-card"
      href={product.href}
      initial={
        reduceMotion
          ? false
          : { opacity: 0, scale: 0.78, rotateY: 32, rotateZ: 8, y: 40 }
      }
      key={product.title}
      rel="noreferrer"
      target="_blank"
      animate={{
        opacity: 1,
        scale: 1,
        rotateY: -12,
        rotateX: 4,
        rotateZ: -4,
        y: 0,
      }}
      exit={
        reduceMotion
          ? { opacity: 0, transition: { duration: 0 } }
          : { opacity: 0, scale: 0.78, rotateY: -32, rotateZ: -8, y: -40 }
      }
      transition={
        reduceMotion
          ? { duration: 0 }
          : { duration: 0.65, ease: [0.22, 1, 0.36, 1] }
      }
      whileHover={
        reduceMotion
          ? undefined
          : { scale: 1.05, rotateX: 0, rotateY: -2, rotateZ: 0, y: -8 }
      }
    >
      <div className={styles.media} key="media">
        <Image
          alt={content.imageAlt(product.title)}
          className={styles.coverImage}
          fill
          sizes="(min-width: 1024px) 42vw, 78vw"
          src={product.image}
        />
        {product.video && (
          <video
            aria-hidden="true"
            className={styles.previewVideo}
            data-playing={playing}
            loop
            muted
            onError={onVideoError}
            playsInline
            poster={product.image}
            preload="none"
            ref={videoRef}
            src={product.video}
          />
        )}
      </div>
      <span
        className={styles.titleBadge}
        key="title"
        style={{
          backgroundColor: product.accentColor,
          color: product.accentTextColor,
        }}
      >
        {product.title}
      </span>
      <span className={styles.actionBadge} key="action">
        {content.viewProduct}
        <ArrowRight aria-hidden="true" className="h-4 w-4 -rotate-45" />
      </span>
    </motion.a>
  );
}
