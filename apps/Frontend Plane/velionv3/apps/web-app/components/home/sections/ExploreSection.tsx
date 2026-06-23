"use client";

import React, { useRef, useEffect } from "react";
import Image from "next/image";
import { ArrowRight } from "lucide-react";
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

gsap.registerPlugin(ScrollTrigger);

export default function ExploreSection() {
	const containerRef = useRef<HTMLElement>(null);

	useEffect(() => {
		const ctx = gsap.context(() => {
			// Deep, premium scrubbed image parallax
			const parallaxImages =
				gsap.utils.toArray<HTMLElement>(".explore-media-img");
			parallaxImages.forEach((img) => {
				gsap.fromTo(
					img,
					{ y: "-15%", scale: 1.15 },
					{
						y: "15%",
						ease: "none",
						scrollTrigger: {
							trigger: img.closest(
								".explore-media-wrap",
							) as Element,
							start: "top bottom",
							end: "bottom top",
							scrub: true,
						},
					},
				);
			});

			// Intro heading reveal — triggered when the header enters viewport
			gsap.from(".explore-header-text", {
				y: 40,
				opacity: 0,
				duration: 1.2,
				ease: "power3.out",
				immediateRender: false,
				scrollTrigger: {
					trigger: ".explore-header-trigger",
					start: "top 80%",
					toggleActions: "play none none none",
				},
			});
		}, containerRef);

		return () => ctx.revert();
	}, []);

	return (
		<section
			ref={containerRef}
			className="w-full bg-transparent text-[#282A22] overflow-hidden py-24 lg:py-32"
		>
			{/* Header grid: label + intro heading */}
			<div className="px-[6vw] explore-header-trigger">
				<div className="grid grid-cols-1 md:grid-cols-3 gap-y-6 gap-x-8">
					{/* Col 1 — label */}
					<div className="text-[10px] font-bold tracking-[0.25em] text-[#282A22]/60 uppercase pt-1">
						01 / Product
					</div>

					{/* Col 2+3 — intro heading */}
					<h2 className="md:col-span-2 explore-header-text text-2xl md:text-3xl lg:text-[2rem] font-light leading-snug tracking-wide text-[#282A22]">
						Velion is the operating layer for customer experience.
						Every autonomous action has a manual equivalent, and
						every risky step can require human approval.
					</h2>
				</div>
			</div>

			<div className="mb-16 md:mb-24" />

			{/* Content grid — 10vw outer inset, 40% column gap */}
			<div
				className="grid grid-cols-1 md:grid-cols-[5fr_2fr] items-start px-[6vw] split-media-trigger"
				style={{ columnGap: "30%" }}
			>
				{/* Left — large feature card styled like a portrait image */}
				<div
					className="explore-media-wrap relative w-full bg-[#282A22] text-[#F2F2F2] flex flex-col justify-between p-10 md:p-14 overflow-hidden"
					style={{ aspectRatio: "3/4" }}
				>
					{/* Background image with parallax */}
					<div className="absolute inset-0 z-0">
						<Image
							src="/velion-vibe/signal-ridge.png"
							alt="Abstract signal field for Velion's operating layer"
							fill
							sizes="(min-width: 768px) 45vw, 88vw"
							className="explore-media-img object-cover opacity-40 scale-110"
						/>
						<div className="absolute inset-0 bg-[#282A22]/60" />
					</div>
					<span className="relative z-10 text-[10px] font-bold tracking-[0.25em] uppercase text-[#F2F2F2]/40">
						Knowledge, workflows, approval
					</span>
				</div>

				{/* Right — accent block + heading + text + CTA */}
				<div className="flex flex-col">
					{/* Accent block with image */}
					<div
						className="explore-media-wrap relative w-full overflow-hidden flex items-end p-8"
						style={{ aspectRatio: "4/3" }}
					>
						<Image
							src="/velion-vibe/glass-edge.png"
							alt="Soft abstract interface surface for Velion"
							fill
							sizes="(min-width: 768px) 24vw, 88vw"
							className="explore-media-img object-cover object-center scale-110"
						/>
						<div className="absolute inset-0 bg-[#282A22]/10" />
						<span className="relative z-10 text-[10px] font-bold tracking-[0.25em] uppercase text-[#282A22]/60">
							Controlled AI work
						</span>
					</div>

					{/* Text & CTA */}
					<div className="pt-10 md:pt-14 pb-10 md:pb-0 flex flex-col gap-6">
						<h3 className="text-3xl md:text-4xl lg:text-5xl font-light tracking-tight text-[#282A22]">
							An AI teammate, not a helpdesk add-on.
						</h3>

						<p className="text-[15px] md:text-base font-light leading-relaxed text-[#282A22]/70">
							Velion is the operating layer for customer
							experience: chatbot creation, support drafts,
							source connections, knowledge inspection,
							workflows, and governed next actions.
						</p>

						<a
							href="#product-loop"
							className="inline-flex items-center justify-center self-start gap-2 px-7 py-3 bg-[#282A22] text-[#F2F2F2] text-sm font-light tracking-widest uppercase hover:bg-black transition-colors duration-300"
						>
							Follow the work loop <ArrowRight className="w-4 h-4" />
						</a>
					</div>
				</div>
			</div>
		</section>
	);
}
