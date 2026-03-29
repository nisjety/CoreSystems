"use client";

import { useRef } from "react";
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { useGSAP } from "@gsap/react";
import { AnimatedArrowButton, AnimatedArrowLink } from "./AnimatedArrow";

if (typeof window !== "undefined") {
    gsap.registerPlugin(ScrollTrigger, useGSAP);
}

const pillStyle = {
    boxShadow: '0 8px 32px rgba(234, 160, 80, 0.55), 0 2px 8px rgba(234, 160, 80, 0.3)',
};

const notchClip = (size: number) =>
    `polygon(${size}px 0%, 100% 0%, 100% 100%, 0% 100%, 0% ${size}px)`;

export function MediaCards() {
    const containerRef = useRef<HTMLElement>(null);

    useGSAP(() => {
        const parallaxImages = gsap.utils.toArray('.media-img');
        parallaxImages.forEach((img: any) => {
            gsap.fromTo(img,
                { y: "-10%", scale: 1.1 },
                {
                    y: "10%",
                    ease: "none",
                    scrollTrigger: {
                        trigger: img.parentElement,
                        start: "top bottom",
                        end: "bottom top",
                        scrub: true,
                    }
                }
            );
        });

        ScrollTrigger.create({
            trigger: ".split-media-trigger",
            start: "top 80%",
            animation: gsap.from(".split-text-element", {
                y: 30,
                opacity: 0,
                duration: 1,
                stagger: 0.15,
                ease: "power3.out"
            }),
            toggleActions: "play reverse play reverse"
        });

        ScrollTrigger.create({
            trigger: ".first-card",
            start: "top top",
            end: "+=100%",
            pin: true,
            pinSpacing: false,
        });

    }, { scope: containerRef });

    return (
        <section ref={containerRef} className="relative w-full z-10 selection:bg-[#282A22] selection:text-[#E7E7E6]">

            {/* Large Card (Pins) */}
            <div className="first-card relative w-full h-screen bg-[#E7E7E6] flex items-center justify-center pt-24 pb-12 z-10">
                <div className="w-full h-full px-6 md:px-12 lg:px-24 max-w-[1800px] mx-auto pb-12">
                    <div className="relative w-full h-full bg-white rounded-4xl shadow-xl overflow-hidden">
                        {/* Image with top-left notch */}
                        <div
                            className="absolute inset-0"
                            style={{ clipPath: notchClip(44) }}
                        >
                            <img
                                src="https://images.unsplash.com/photo-1518770660439-4636190af475?auto=format&fit=crop&q=80&w=2940"
                                alt="Triodelab Technology"
                                className="media-img absolute inset-0 w-full h-full object-cover opacity-80 scale-110"
                            />
                        </div>

                        {/* Bottom gradient overlay */}
                        <div className="absolute inset-x-0 bottom-0 h-2/5 bg-linear-to-t from-black/65 via-black/35 to-transparent pointer-events-none rounded-b-4xl" />

                        {/* Content */}
                        <div className="absolute inset-0 p-8 md:p-12 flex flex-col justify-between">
                            {/* Top-right play button */}
                            <div className="self-end">
                                <button className="w-14 h-14 rounded-full bg-white/10 backdrop-blur-md flex items-center justify-center hover:bg-white/20 transition-colors border border-white/20 group">
                                    <div className="w-0 h-0 border-t-[6px] border-t-transparent border-l-10 border-l-white border-b-[6px] border-b-transparent ml-1 group-hover:scale-110 transition-transform" />
                                </button>
                            </div>

                            {/* Bottom: text left, pill CTA right */}
                            <div className="flex items-end justify-between gap-8">
                                <div className="max-w-xl">
                                    <h3 className="text-white text-3xl md:text-5xl font-light leading-tight mb-3">
                                        Precision at scale.
                                    </h3>
                                    <p className="text-white/70 font-light text-lg">
                                        Discover how our agentic frameworks integrate seamlessly into your existing operations.
                                    </p>
                                </div>

                                <div className="shrink-0">
                                    <AnimatedArrowButton
                                        className="px-8 py-4 bg-white/90 rounded-full text-[#282A22] text-xs uppercase tracking-widest font-medium"
                                        style={pillStyle}
                                    >
                                        Les mer
                                    </AnimatedArrowButton>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            {/* Split Media Block */}
            <div className="split-media-trigger relative w-full min-h-screen bg-[#E7E7E6] py-24 md:py-48 flex items-center justify-center z-20 shadow-[0_-20px_50px_rgba(0,0,0,0.05)]">
                <div className="w-full px-6 md:px-12 lg:px-24 max-w-[1600px] mx-auto grid grid-cols-1 md:grid-cols-2 gap-16 md:gap-24 items-center">
                    <div className="flex flex-col gap-8 order-2 md:order-1">
                        <p className="split-text-element text-xs uppercase tracking-[0.2em] text-[#282A22]/50 font-bold">
                            Bærekraftig Innovasjon
                        </p>
                        <h2 className="split-text-element text-3xl md:text-5xl font-light text-[#282A22] leading-tight max-w-md">
                            Data-driven decisions for a complex world.
                        </h2>
                        <p className="split-text-element text-[#282A22]/70 font-light text-lg max-w-md leading-relaxed">
                            By leveraging advanced machine learning models, we provide unprecedented visibility into structural inefficiencies, allowing companies to adapt faster than ever before.
                        </p>
                        <AnimatedArrowLink
                            href="#"
                            className="split-text-element mt-4 text-sm uppercase tracking-widest font-medium text-[#282A22]"
                        >
                            Les mer
                        </AnimatedArrowLink>
                    </div>

                    {/* Profile card style image */}
                    <div className="relative order-1 md:order-2">
                        <div className="relative aspect-4/5 bg-white rounded-4xl shadow-xl overflow-hidden">
                            {/* Image with top-left notch */}
                            <div
                                className="absolute inset-0"
                                style={{ clipPath: notchClip(36) }}
                            >
                                <img
                                    src="https://images.unsplash.com/photo-1550751827-4bd374c3f58b?auto=format&fit=crop&q=80&w=2940"
                                    alt="Data Analytics"
                                    className="media-img absolute inset-0 w-full h-full object-cover grayscale-20 scale-110"
                                />
                            </div>

                            {/* Bottom gradient overlay */}
                            <div className="absolute inset-x-0 bottom-0 h-2/5 bg-linear-to-t from-black/60 via-black/25 to-transparent pointer-events-none rounded-b-4xl" />

                            {/* Bottom: pill CTA */}
                            <div className="absolute bottom-6 right-6">
                                <AnimatedArrowButton
                                    className="px-7 py-3.5 bg-white/90 rounded-full text-[#282A22] text-xs uppercase tracking-widest font-medium"
                                    style={pillStyle}
                                >
                                    Les mer
                                </AnimatedArrowButton>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

        </section>
    );
}
