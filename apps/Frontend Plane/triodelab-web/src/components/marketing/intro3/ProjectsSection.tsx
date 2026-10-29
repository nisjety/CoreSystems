"use client";

import Link from 'next/link';
import { useRef, useState } from "react";
import Image from "next/image";
import { m, useInView } from "framer-motion";

const PROJECTS = [
    {
        name: "Vibrant",
        desc: "Unified branding and website for a health and longevity practice",
        img: "https://cdn.prod.website-files.com/68c9282d38c8df82b6d5f66f/68f18f4b6b559a3de54cd024_poster_vibrant.avif",
        targetRotate: -12,
        targetX: -580,
    },
    {
        name: "Just Salad",
        desc: "Playful web experience bringing BBMG's iconic branding to life",
        img: "https://cdn.prod.website-files.com/68c9282d38c8df82b6d5f66f/68f203ae968ed54a69acd0d4_68ea1c11b2a7949815ff6e11_Just%20Salad.avif",
        targetRotate: -8,
        targetX: -372,
    },
    {
        name: "Eppright Homes",
        desc: "Luxury homebuilder site with flexible Webflow components",
        img: "https://cdn.prod.website-files.com/68c9282d38c8df82b6d5f66f/68f640e40214dc395259a529_poster_eppright.avif",
        targetRotate: -4,
        targetX: -158,
    },
    {
        name: "PBS - North Carolina",
        desc: "Interactive website for a storied regional broadcaster",
        img: "https://cdn.prod.website-files.com/68c9282d38c8df82b6d5f66f/68fc038558af080667a5143c_poster_pbsnc.avif",
        targetRotate: 4,
        targetX: 38,
    },
    {
        name: "Titan",
        desc: "Webflow site to support a modern RIA's growth strategy",
        img: "https://cdn.prod.website-files.com/68c9282d38c8df82b6d5f66f/6921eb82b88ca105a9621ecb_poster_titan-compressed.webp",
        targetRotate: 8,
        targetX: 233,
    },
    {
        name: "Betterment",
        desc: "Product-grade design system for a leading fintech platform",
        img: "https://cdn.prod.website-files.com/68c9282d38c8df82b6d5f66f/69654fe6539a93e8684d3f5d_poster_betterment-compressed.jpg",
        targetRotate: 12,
        targetX: 443,
    }
];

export function ProjectsSection() {
    const containerRef = useRef(null);
    const isInView = useInView(containerRef, { once: true, margin: "-100px 0px" });
    const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);

    return (
        <section className="w-full bg-transparent py-32 overflow-hidden relative">
            <div className="max-w-7xl mx-auto px-6 mb-24 flex flex-col md:flex-row md:items-end justify-between gap-8">
                <h2 className="text-4xl md:text-5xl lg:text-[64px] font-light text-[#282A22] tracking-wide uppercase leading-[1]">
                    SELECTED CLIENT<br />PROJECTS
                </h2>
                <Link
                    href="/projects"
                    className="group inline-flex items-center gap-4 bg-transparent border border-[#282A22]/20 text-[#282A22] px-8 py-4 rounded-full font-light uppercase tracking-wider text-sm hover:border-[#282A22] transition-colors"
                >
                    View all projects
                    <svg width="17" height="16" viewBox="0 0 17 16" fill="none" xmlns="http://www.w3.org/2000/svg" className="w-5 h-5 group-hover:translate-x-1 transition-transform stroke-[1.5]">
                        <path d="M-3.0598e-07 8L16 8M16 8L8.61538 1M16 8L8.61539 15" stroke="currentColor" strokeWidth="2"></path>
                    </svg>
                </Link>
            </div>

            {/* Cards Fan Container */}
            <div
                ref={containerRef}
                className="relative w-full max-w-7xl mx-auto h-[500px] flex items-center justify-center -translate-x-[40px]"
            >
                {PROJECTS.map((proj, idx) => {
                    const isHovered = hoveredIndex === idx;
                    const isNotHoveredButSomethingIs = hoveredIndex !== null && hoveredIndex !== idx;

                    return (
                        <m.div
                            key={proj.name}
                            onMouseEnter={() => setHoveredIndex(idx)}
                            onMouseLeave={() => setHoveredIndex(null)}
                            initial={{ x: 0, rotate: 0, scale: 0.9, opacity: 0 }}
                            animate={{
                                x: isInView ? proj.targetX : 0,
                                rotate: isInView ? (isHovered ? 0 : proj.targetRotate) : 0,
                                scale: isHovered ? 1.05 : (isNotHoveredButSomethingIs ? 0.95 : 1),
                                opacity: isInView ? (isNotHoveredButSomethingIs ? 0.6 : 1) : 0,
                                zIndex: isHovered ? 50 : idx,
                            }}
                            transition={{
                                type: "spring",
                                damping: 25,
                                stiffness: 150,
                                duration: 0.6
                            }}
                            className="absolute mt-20 w-[240px] md:w-[320px] aspect-[4/5] rounded-[32px] overflow-hidden shadow-[0_20px_40px_rgba(0,0,0,0.15)] cursor-pointer origin-bottom bg-slate-200 will-change-transform"
                        >
                            <Image
                                src={proj.img}
                                alt={proj.name}
                                className="w-full h-full object-cover"
                                fill
                                sizes="(max-width: 768px) 240px, 320px"
                            />
                            {/* Papertiger link overlay overlay */}
                            <a aria-label={`View ${proj.name}`} href={`/projects/${proj.name.toLowerCase().replace(/[\s-]/g, '')}`} className="absolute inset-0 z-10" />
                        </m.div>
                    );
                })}
            </div>

            {/* Descriptions Container */}
            <div className="max-w-7xl mx-auto mt-24 px-6 flex justify-center relative min-h-[100px]">
                <div className="text-center w-full max-w-lg">
                    {/* Initially visible overall description or default. 
                        In Papertiger, active class changes opacity, but let's emulate it by mapping state */}
                    <div className="relative">
                        {PROJECTS.map((proj, idx) => (
                            <m.div
                                key={proj.name}
                                initial={false}
                                animate={{
                                    opacity: hoveredIndex === null ? (idx === 0 ? 1 : 0) : hoveredIndex === idx ? 1 : 0,
                                    y: hoveredIndex === null ? (idx === 0 ? 0 : 20) : hoveredIndex === idx ? 0 : 20,
                                }}
                                transition={{ duration: 0.3 }}
                                className="absolute inset-x-0 top-0 flex flex-col items-center justify-start pointer-events-none"
                            >
                                <h3 className="text-2xl font-light text-[#282A22] mb-2">{proj.name}</h3>
                                <p className="text-[#282A22]/60 font-light">{proj.desc}</p>
                            </m.div>
                        ))}
                    </div>
                </div>
            </div>

            <div className="h-20 w-full" />
        </section>
    );
}
