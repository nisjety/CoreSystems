"use client";

import React, { useRef, useLayoutEffect, useState, useContext } from "react";
import Image from "next/image";
import Link from "next/link";
import { ArrowRight } from "lucide-react";
import { ThemeContext } from "./ThemeController";
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

gsap.registerPlugin(ScrollTrigger);

type Product = {
    id: string;
    navn: string;
    category: string;
    pris: string;
    href: string;
    bilde: string;
    alt: string;
    resolution: string;
};

const produkter: Product[] = [
    {
        id: "p1",
        navn: "Rocol Anti-Seize Compound",
        category: "Smøremiddel",
        pris: "Kontakt for pris",
        href: "/shop/14033P",
        bilde: "/images/products/rocol-compound.png",
        alt: "Rocol Anti-Seize Compound",
        resolution: "720 × 960",
    },
    {
        id: "p2",
        navn: "Aqua Trans Forvask Extra",
        category: "Rengjøring",
        pris: "Kontakt for pris",
        href: "/shop/FE682",
        bilde: "/images/products/aqua-trans-extra.png",
        alt: "Aqua Trans Forvask Extra",
        resolution: "720 × 960",
    },
    {
        id: "p3",
        navn: "Arrow Ecowash Autoshine",
        category: "Bilpleie",
        pris: "Kontakt for pris",
        href: "/shop/C568",
        bilde: "/images/products/arrow-ecowash-autoshine.png",
        alt: "Arrow Ecowash Autoshine",
        resolution: "720 × 960",
    },
    {
        id: "p4",
        navn: "Nielsen Gloss Shampoo",
        category: "Bilpleie",
        pris: "Kontakt for pris",
        href: "/shop/L900D",
        bilde: "/images/products/nielsen-gloss.png",
        alt: "Nielsen Gloss Shampoo",
        resolution: "720 × 960",
    },
];

/** SVG icon replicating the lovart "Image" label icon */
function ImageIcon() {
    return (
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width="13"
            height="13"
            fill="none"
            viewBox="0 0 13 13"
            className="text-current shrink-0"
        >
            <path
                fill="currentColor"
                d="M7.719 3.656a1.219 1.219 0 1 0 0 2.438 1.219 1.219 0 0 0 0-2.438"
            />
            <path
                fill="currentColor"
                fillRule="evenodd"
                d="M1.625 2.573c0-.524.424-.948.948-.948h7.854c.524 0 .948.424.948.948v7.854a.95.95 0 0 1-.948.948H2.573a.95.95 0 0 1-.948-.948zm8.938 7.415-2.5-2.5a.677.677 0 0 0-.958 0l-.51.508a.135.135 0 0 1-.19 0L4.805 6.4a.677.677 0 0 0-.942-.015L2.438 7.726V2.573c0-.075.06-.135.135-.135h7.854c.075 0 .136.06.136.135z"
                clipRule="evenodd"
            />
        </svg>
    );
}

/** A single product card styled exactly like lovart.ai posters */
function ProductCard({
    produkt,
    cardRef,
    imgRef,
    isBestseller = false,
}: {
    produkt: Product;
    cardRef: React.RefCallback<HTMLDivElement>;
    imgRef: React.RefCallback<HTMLImageElement>;
    isBestseller?: boolean;
}) {
    const [loaded, setLoaded] = useState(false);

    return (
        <div
            ref={cardRef}
            className="lovart-card will-change-[transform,filter,opacity]"
        >
            <Link
                href={produkt.href}
                className="product-card group relative flex flex-col gap-0 rounded-none bg-white transition-shadow duration-300 hover:shadow-xl border border-neutral-100 h-full"
            >
                {/* Image container — 3/4 aspect ratio */}
                <div className="relative w-full overflow-hidden bg-[var(--bg-premium-light)]" style={{ aspectRatio: "3/4" }}>
                    {/* Skeleton overlay */}
                    <div
                        className="skeleton-overlay absolute inset-0 z-10 pointer-events-none"
                        style={{
                            background: "linear-gradient(to bottom, #262625, #424140)",
                            opacity: loaded ? 0 : 1,
                            transition: "opacity 0.6s ease",
                        }}
                    >
                        <div className="absolute inset-0 animate-pulse bg-white/5" />
                    </div>

                    <Image
                        ref={imgRef}
                        src={produkt.bilde}
                        alt={produkt.alt}
                        fill
                        sizes="(max-width: 768px) 45vw, 22vw"
                        className="pointer-events-none select-none object-contain p-6 md:p-8 lg:p-12 mix-blend-multiply will-change-[transform,filter]"
                        onLoad={() => setLoaded(true)}
                    />

                    {/* Bestseller Badge */}
                    {isBestseller && (
                        <div className="absolute top-4 left-4 z-20">
                            <span className="bg-[#151F6D] text-white text-[10px] font-bold uppercase tracking-widest px-3 py-1.5 rounded-none shadow-sm">
                                Bestseller
                            </span>
                        </div>
                    )}

                    {/* Hover arrow */}
                    <div className="absolute bottom-4 right-4 translate-y-4 opacity-0 transition-all duration-300 group-hover:translate-y-0 group-hover:opacity-100 z-20">
                        <div className="flex h-10 w-10 items-center justify-center rounded-none bg-[#1a1d1d] text-white shadow-md">
                            <ArrowRight size={18} />
                        </div>
                    </div>
                </div>

                {/* Product info - styled like previous cards */}
                <div className="flex flex-col p-4 md:p-5">
                    <span className="mb-2 text-[10px] font-bold uppercase tracking-[0.2em] text-gray-400">
                        {produkt.category}
                    </span>
                    <h3 className="text-base md:text-xl lg:text-2xl font-light leading-tight text-[#1a1d1d] group-hover:text-[#151F6D] transition-colors">
                        {produkt.navn}
                    </h3>
                    <p className="mt-2 text-xs font-medium text-gray-400">{produkt.pris}</p>
                </div>
            </Link>
        </div>
    );
}

export function ProductSection() {
    const sectionRef = useRef<HTMLElement>(null);
    const cardsRef = useRef<(HTMLDivElement | null)[]>([]);
    const imgsRef = useRef<(HTMLImageElement | null)[]>([]);
    const { isDark } = useContext(ThemeContext);

    useLayoutEffect(() => {
        if (!sectionRef.current) return;

        const cards = cardsRef.current.filter(Boolean) as HTMLDivElement[];
        const imgs = imgsRef.current.filter(Boolean) as HTMLImageElement[];
        if (cards.length === 0) return;

        const ctx = gsap.context(() => {
            // Set initial states. 
            // The first card is the best seller and is fully visible from the start.
            const animatedCards = cards.slice(1);
            const animatedImgs = imgs.slice(1);

            gsap.set(cards[0], { opacity: 1, filter: "blur(0px)", scale: 1 });
            if (imgs[0]) gsap.set(imgs[0], { scale: 1, filter: "blur(0px)" });

            // The rest are blurred and slightly scaled up
            gsap.set(animatedCards, { opacity: 0, filter: "blur(20px)", scale: 1.1 });
            gsap.set(animatedImgs, { scale: 1.1, filter: "blur(20px)" });

            // Pin the section and run a scroll-linked timeline
            const tl = gsap.timeline({
                scrollTrigger: {
                    trigger: sectionRef.current,
                    start: "top top",
                    // Reduced from 3 to 1.5 so the animation completes faster relative to scrolling
                    end: `+=${window.innerHeight * 1.5}`,
                    pin: true,
                    scrub: 1.2,
                    anticipatePin: 1,
                },
            });

            // Staggered reveal for the remaining cards
            const segmentDuration = 0.3; // fraction of total timeline per card
            const staggerOffset = 0.15;  // slightly overlap the reveals

            animatedCards.forEach((card, i) => {
                const img = animatedImgs[i];
                const startAt = i * staggerOffset;

                tl.to(
                    card,
                    {
                        opacity: 1,
                        filter: "blur(0px)",
                        scale: 1,
                        ease: "power2.out",
                        duration: segmentDuration,
                    },
                    startAt
                );

                if (img) {
                    tl.to(
                        img,
                        {
                            scale: 1,
                            filter: "blur(0px)",
                            ease: "power2.out",
                            duration: segmentDuration,
                        },
                        startAt
                    );
                }
            });
        }, sectionRef);

        return () => ctx.revert();
    }, []);

    return (
        <section
            ref={sectionRef}
            className="relative w-full overflow-hidden bg-transparent transition-colors duration-700"
            style={{ height: "100svh" }}
        >
            {/* ---------- Static header ---------- */}
            <div className="absolute top-10 left-0 right-0 z-20 pointer-events-none px-6 md:px-10 xl:px-14">
                <div className="flex flex-col md:flex-row md:items-end justify-between gap-6 max-w-[1600px] mx-auto">
                    <div>
                        <span className={`block mb-5 text-[10px] font-bold tracking-[0.25em] uppercase transition-colors duration-700 ${isDark ? 'text-blue-300' : 'text-[#151F6D]/50'}`}>
                            01 / Butikken
                        </span>
                        <h2 className={`text-5xl md:text-7xl lg:text-[86px] font-thin tracking-tighter leading-[0.88] transition-colors duration-700 ${isDark ? 'text-white' : 'text-[#1a1d1d]'}`}>
                            Industriell <br className="hidden md:block" /> Kvalitet
                        </h2>
                    </div>
                    <div className="max-w-xs flex flex-col gap-5 pointer-events-auto">
                        <p className={`text-sm leading-relaxed transition-colors duration-700 ${isDark ? 'text-slate-300' : 'text-[#1a1d1d]/55'}`}>
                            Et utvalg av våre mest populære rengjørings- og vedlikeholdsprodukter — designet for de strengeste kravene innen mattrygghet og effektivitet.
                        </p>
                        <Link
                            href="/shop"
                            className={`group inline-flex items-center gap-3 text-xs font-bold uppercase tracking-widest transition-all hover:gap-5 ${isDark ? 'text-blue-300' : 'text-[#151F6D]'}`}
                        >
                            Se hele utvalget
                            <div className={`flex h-8 w-8 items-center justify-center transition-colors ${isDark ? 'bg-blue-500/20 group-hover:bg-blue-500 group-hover:text-white' : 'bg-[#151F6D]/10 group-hover:bg-[#151F6D] group-hover:text-white'}`}>
                                <ArrowRight className="h-4 w-4" />
                            </div>
                        </Link>
                    </div>
                </div>
            </div>

            {/* ---------- Cards grid ---------- */}
            {/*
        Mobile: 2-column grid  (like lovart mobile)
        md+:    horizontal flex row
      */}
            <div className="absolute inset-x-0 bottom-8 lg:bottom-16 flex items-end justify-center">
                <div
                    className="grid w-[min(55vh,100vw)] max-w-[520px] grid-cols-2 lg:items-end gap-[20px] px-6 md:mx-0 md:flex md:w-[85vw] md:max-w-[1500px] md:px-0 lg:gap-[30px] xl:w-[calc(100vw-280px)] xl:px-10 2xl:pb-[40px]"
                >
                    {produkter.map((produkt, index) => (
                        <ProductCard
                            key={produkt.id}
                            produkt={produkt}
                            cardRef={(el) => { cardsRef.current[index] = el; }}
                            imgRef={(el) => { imgsRef.current[index] = el; }}
                            isBestseller={index === 0}
                        />
                    ))}
                </div>
            </div>
        </section>
    );
}