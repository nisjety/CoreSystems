"use client";

import { useLayoutEffect, useRef } from "react";
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { MessageSquare } from "lucide-react";
import Image from "next/image";

gsap.registerPlugin(ScrollTrigger);

export function HeroSection() {
    const sectionRef = useRef<HTMLElement>(null);
    const tlRef = useRef<HTMLDivElement>(null);
    const blRef = useRef<HTMLDivElement>(null);
    const trRef = useRef<HTMLDivElement>(null);
    const brRef = useRef<HTMLDivElement>(null);
    const centerRef = useRef<HTMLDivElement>(null);

    useLayoutEffect(() => {
        let ctx = gsap.context(() => {
            const tl = gsap.timeline({
                scrollTrigger: {
                    trigger: sectionRef.current,
                    start: "top top",
                    end: "+=1500", // Scroll duration for the effect to complete
                    pin: true,
                    scrub: 1, // Smooth scrubbing
                    anticipatePin: 1
                }
            });

            // Center image expansion to full viewport
            tl.to(centerRef.current, {
                width: "100vw",
                height: "100vh",
                borderRadius: "0px",
                ease: "power2.inOut",
            }, 0);

            // Peripherals moving outwards, scaling down, and fading
            tl.to(tlRef.current, {
                xPercent: -80,
                yPercent: -80,
                opacity: 0,
                scale: 0.8,
                ease: "power2.inOut",
            }, 0);

            tl.to(blRef.current, {
                xPercent: -80,
                yPercent: 80,
                opacity: 0,
                scale: 0.8,
                ease: "power2.inOut",
            }, 0);

            tl.to(trRef.current, {
                xPercent: 80,
                yPercent: -80,
                opacity: 0,
                scale: 0.8,
                ease: "power2.inOut",
            }, 0);

            tl.to(brRef.current, {
                xPercent: 80,
                yPercent: 80,
                opacity: 0,
                scale: 0.8,
                ease: "power2.inOut",
            }, 0);

        }, sectionRef);

        return () => ctx.revert();
    }, []);

    return (
        <section
            ref={sectionRef}
            className="relative w-full h-[100svh] bg-transparent flex items-center justify-center overflow-hidden"
        >
            {/* Peripheral Image - Top Left */}
            <div ref={tlRef} className="absolute left-[-2vw] top-[10vh] w-[35vw] h-[45vh] md:w-[25vw] md:h-[35vh] rounded-[24px] overflow-hidden shadow-lg will-change-transform z-10">
                <Image
                    src="https://images.unsplash.com/photo-1542332213-31f87348057f?q=80&w=2940&auto=format&fit=crop"
                    alt="Lake" fill className="object-cover"
                />
            </div>

            {/* Peripheral Image - Bottom Left */}
            <div ref={blRef} className="absolute left-[5vw] top-[55vh] w-[30vw] h-[35vh] md:w-[20vw] md:h-[25vh] rounded-[24px] overflow-hidden shadow-lg will-change-transform z-10">
                <Image
                    src="https://images.unsplash.com/photo-1473580044384-7ba9967e16a0?q=80&w=2940&auto=format&fit=crop"
                    alt="Desert" fill className="object-cover"
                />
            </div>

            {/* Peripheral Image - Top Right */}
            <div ref={trRef} className="absolute right-[5vw] top-[25vh] w-[35vw] h-[30vh] md:w-[25vw] md:h-[25vh] rounded-[24px] overflow-hidden shadow-lg will-change-transform z-10">
                <Image
                    src="https://images.unsplash.com/photo-1495616811223-4d98c6e9c869?q=80&w=2872&auto=format&fit=crop"
                    alt="Sunset" fill className="object-cover"
                />
            </div>

            {/* Peripheral Image - Bottom Right */}
            <div ref={brRef} className="absolute right-[-2vw] top-[60vh] w-[40vw] h-[35vh] md:w-[30vw] md:h-[30vh] rounded-[24px] overflow-hidden shadow-lg will-change-transform z-10">
                <Image
                    src="https://images.unsplash.com/photo-1464822759023-fed622ff2c3b?q=80&w=2940&auto=format&fit=crop"
                    alt="Mountains" fill className="object-cover"
                />
            </div>

            {/* Center Main Container - The Expanding Mask */}
            <div
                ref={centerRef}
                className="absolute z-20 w-[85vw] h-[60vh] md:w-[45vw] md:h-[70vh] rounded-[2rem] overflow-hidden shadow-2xl will-change-[width,height,border-radius]"
            >
                <Image
                    src="https://images.unsplash.com/photo-1523987355523-c7b5b0dd90a7?q=80&w=2940&auto=format&fit=crop"
                    alt="RV in nature"
                    fill
                    priority
                    className="object-cover"
                />

                {/* Text Mask Container wrapper - fixed to the viewport size, centered in the expanding container */}
                <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[100vw] h-[100vh] flex items-end justify-center pb-[5vh] md:pb-[10vh] pointer-events-none">
                    <h1 className="text-[22vw] md:text-[19vw] leading-[0.75] font-semibold tracking-tighter text-white m-0 p-0 text-center mix-blend-normal">
                        Go Further
                    </h1>
                </div>
            </div>

            {/* Chat Icon - Fixed to bottom right, appearing like in the reference */}
            <div className="absolute bottom-6 right-8 z-30">
                <div className="flex items-center justify-center w-14 h-14 bg-black text-white outline outline-4 outline-transparent rounded-full hover:scale-105 transition-transform cursor-pointer shadow-xl">
                    <MessageSquare className="w-6 h-6" />
                </div>
            </div>

        </section>
    );
}
