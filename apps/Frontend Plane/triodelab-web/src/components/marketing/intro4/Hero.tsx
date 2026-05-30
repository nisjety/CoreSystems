"use client";

import { useRef } from "react";
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { useGSAP } from "@gsap/react";

if (typeof window !== "undefined") {
    gsap.registerPlugin(ScrollTrigger, useGSAP);
}

export function Hero() {
    const containerRef = useRef<HTMLElement>(null);

    useGSAP(() => {
        // Timeline for the load animation
        const tl = gsap.timeline({ delay: 0.2 });

        // Premium Mask Reveal: slide up from behind the overflow-hidden wrapper
        tl.from(".hero-text-line", {
            yPercent: 120, // start pushed completely outside the wrapper
            duration: 1.4,
            stagger: 0.1,
            ease: "power4.out",
        })
            // Fade in the supporting text and button
            .from(".hero-fade-element", {
                y: 20,
                opacity: 0,
                duration: 1,
                stagger: 0.15,
                ease: "power3.out",
            }, "-=1.0"); // Start slightly before the mask reveal finishes

        // Pin the hero section so the next section slides over it (card stacking effect),
        // and scrub the text upwards while it is pinned
        const scrubTl = gsap.timeline({
            scrollTrigger: {
                trigger: containerRef.current,
                start: "top top",
                end: "bottom top",
                pin: true,
                pinSpacing: false,
                scrub: true, // Smoothly scrub the timeline based on scroll position
            }
        });

        // Add the parallax translation to the scrubbed timeline
        scrubTl.to(".hero-content-wrapper", {
            y: "-30vh", // Float upwards as user scrolls
            ease: "none",
        });

    }, { scope: containerRef });

    return (
        <section ref={containerRef} className="relative w-full h-screen bg-[#E7E7E6] flex flex-col items-center justify-center overflow-hidden selection:bg-[#282A22] selection:text-[#E7E7E6] z-0">

            {/* Massive, lightweight typography typical of Stingray */}
            <div className="hero-content-wrapper w-full max-w-[1600px] px-6 md:px-12 lg:px-24 z-10 flex flex-col items-start gap-8">
                <div className="flex flex-col">
                    <div className="overflow-hidden pb-2">
                        <h1 className="hero-text-line text-[12vw] sm:text-[10vw] md:text-[8vw] lg:text-[7vw] font-light leading-[0.9] text-[#282A22] tracking-tight">
                            Intelligent
                        </h1>
                    </div>
                    <div className="overflow-hidden pb-4">
                        <h1 className="hero-text-line text-[12vw] sm:text-[10vw] md:text-[8vw] lg:text-[7vw] font-light leading-[0.9] text-[#282A22] tracking-tight ml-0 sm:ml-12 md:ml-32">
                            Systems
                        </h1>
                    </div>
                </div>

                <div className="hero-fade-element mt-8 md:mt-16 max-w-xl text-[#282A22]">
                    <p className="text-xl md:text-2xl font-light leading-relaxed opacity-80">
                        Pioneering the next generation of algorithmic automation and agentic intelligence for sustainable business growth.
                    </p>
                </div>

                <a
                    href="#explore"
                    className="hero-fade-element mt-12 group inline-flex items-center gap-4 border border-[#282A22]/20 rounded-full px-8 py-4 uppercase text-xs tracking-[0.2em] font-medium hover:border-[#282A22] transition-colors"
                >
                    Utforsk teknologien
                    <span className="group-hover:translate-x-1 transition-transform">→</span>
                </a>
            </div>

        </section>
    );
}
