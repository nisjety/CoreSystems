"use client";

import React, { useRef, useEffect } from "react";
import Image from "next/image";
import { ArrowRight } from "lucide-react";
import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

gsap.registerPlugin(ScrollTrigger);

const TECHNOLOGIES = [
	{
		title: "Rengjøringssystemer",
		desc: "Avanserte lavtrykks- og høytrykkssystemer for optimal hygiene.",
		category: "Systemer",
	},
	{
		title: "Clean-in-Place (CIP)",
		desc: "Automatiserte rengjøringsprosesser for lukkede systemer.",
		category: "Automasjon",
	},
	{
		title: "Kjemi & Desinfeksjon",
		desc: "Spesialutviklede kjemikalier for næringsmiddelindustrien.",
		category: "Kjemi",
	},
	{
		title: "Inspeksjon & Kontroll",
		desc: "Systemer for validering og dokumentasjon av renhold.",
		category: "Kvalitet",
	},
];

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
						04 / Teknologi
					</div>

					{/* Col 2+3 — intro heading */}
					<h2 className="md:col-span-2 explore-header-text text-2xl md:text-3xl lg:text-[2rem] font-light leading-snug tracking-wide text-[#282A22]">
						Vi kombinerer banebrytende teknologi med dyp
						bransjekunnskap for å levere systemer som setter nye
						standarder for mattrygghet og effektivitet i
						produksjonen.
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
							src="/images/CIP/sveise.png"
							alt="Sveise"
							fill
							className="explore-media-img object-cover opacity-40 scale-110"
						/>
						<div className="absolute inset-0 bg-[#282A22]/60" />
					</div>
					<span className="relative z-10 text-[10px] font-bold tracking-[0.25em] uppercase text-[#F2F2F2]/40">
						Teknologi &amp; Innovasjon
					</span>

					<div className="relative z-10 flex flex-col gap-0">
						{TECHNOLOGIES.map((tech, idx) => (
							<div
								key={idx}
								className="flex items-start gap-6 border-t border-[#F2F2F2]/10 py-6"
							>
								<span className="text-[11px] font-semibold tracking-widest text-[#F2F2F2]/30 mt-1 shrink-0 w-6">
									0{idx + 1}
								</span>
								<div className="flex flex-col gap-1">
									<p className="text-lg md:text-xl font-light tracking-wide text-[#F2F2F2]">
										{tech.title}
									</p>
									<p className="text-[13px] font-light text-[#F2F2F2]/50 leading-relaxed">
										{tech.desc}
									</p>
								</div>
							</div>
						))}
					</div>
				</div>

				{/* Right — accent block + heading + text + CTA */}
				<div className="flex flex-col">
					{/* Accent block with image */}
					<div
						className="explore-media-wrap relative w-full overflow-hidden flex items-end p-8"
						style={{ aspectRatio: "4/3" }}
					>
						<Image
							src="/images/about/aquatiqOneMac2.png"
							alt="Aquatiq One"
							fill
							className="explore-media-img object-contain object-center scale-110"
						/>
						<div className="absolute inset-0 bg-[#282A22]/10" />
						<span className="relative z-10 text-[10px] font-bold tracking-[0.25em] uppercase text-[#282A22]/60">
							Utforsk løsninger
						</span>
					</div>

					{/* Text & CTA */}
					<div className="pt-10 md:pt-14 pb-10 md:pb-0 flex flex-col gap-6">
						<h3 className="text-3xl md:text-4xl lg:text-5xl font-light tracking-tight text-[#282A22]">
							Redefiner renheten
						</h3>

						<p className="text-[15px] md:text-base font-light leading-relaxed text-[#282A22]/70">
							Vår reise begynner med en grundig analyse av din
							produksjon. Vi spesifiserer, leverer og støtter
							systemer skreddersydd til dine behov.
						</p>

						<a
							href="/teknologi"
							className="inline-flex items-center justify-center self-start gap-2 px-7 py-3 bg-[#282A22] text-[#F2F2F2] text-sm font-light tracking-widest uppercase hover:bg-black transition-colors duration-300"
						>
							Se teknologi <ArrowRight className="w-4 h-4" />
						</a>
					</div>
				</div>
			</div>
		</section>
	);
}
a;
